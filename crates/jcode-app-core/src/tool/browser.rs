use super::{Tool, ToolContext, ToolOutput};
use anyhow::{Context, Result};
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, tungstenite::Message};

pub struct BrowserTool;

static FIREFOX_PROVIDER: FirefoxBridgeProvider = FirefoxBridgeProvider;
static CHROMIUM_PROVIDER: ChromiumCdpProvider = ChromiumCdpProvider;
static OBSCURA_PROVIDER: ObscuraCdpProvider = ObscuraCdpProvider;

impl BrowserTool {
    pub fn new() -> Self {
        Self
    }
}

fn browser_tool_description_text() -> &'static str {
    "Control the browser. Check action='status' first; run setup only if not ready."
}

#[derive(Debug, Deserialize)]
struct BrowserInput {
    action: String,
    #[serde(default)]
    browser: Option<String>,
    #[serde(default)]
    provider_action: Option<String>,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    params: Option<Value>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    tab_id: Option<i64>,
    #[serde(default)]
    window_id: Option<i64>,
    #[serde(default)]
    frame_id: Option<i64>,
    #[serde(default)]
    all_frames: Option<bool>,
    #[serde(default)]
    selector: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    contains: Option<String>,
    #[serde(default)]
    script: Option<String>,
    #[serde(default)]
    key: Option<String>,
    #[serde(default)]
    x: Option<f64>,
    #[serde(default)]
    y: Option<f64>,
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    wait: Option<bool>,
    #[serde(default)]
    new_tab: Option<bool>,
    #[serde(default)]
    focus: Option<bool>,
    #[serde(default)]
    clear: Option<bool>,
    #[serde(default)]
    submit: Option<bool>,
    #[serde(default)]
    page_world: Option<bool>,
    #[serde(default)]
    position: Option<String>,
    #[serde(default)]
    behavior: Option<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    fields: Option<Vec<BrowserField>>,
    #[serde(default)]
    scroll_to: Option<ScrollTo>,
}

impl BrowserInput {
    fn provider_method(&self) -> Option<&str> {
        self.provider_action
            .as_deref()
            .or_else(|| self.method.as_deref())
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct BrowserField {
    selector: String,
    #[serde(default)]
    value: Option<String>,
    #[serde(default)]
    checked: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct ScrollTo {
    #[serde(default)]
    x: Option<f64>,
    #[serde(default)]
    y: Option<f64>,
}

#[async_trait]
trait BrowserProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn supported_browsers(&self) -> &'static [&'static str];

    async fn status(&self, ctx: &ToolContext) -> Result<ToolOutput>;
    async fn setup(&self) -> Result<ToolOutput>;
    async fn ensure_ready(&self) -> Result<Option<String>>;
    async fn execute(
        &self,
        action: &str,
        input: &BrowserInput,
        ctx: &ToolContext,
    ) -> Result<ToolOutput>;
}

struct FirefoxBridgeProvider;

struct ChromiumCdpProvider;

struct ObscuraCdpProvider;

struct ObscuraCdpSession {
    ws: WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    next_id: i64,
    session_id: String,
    target_id: String,
    url: String,
}

static OBSCURA_CDP_SESSION: OnceLock<tokio::sync::Mutex<Option<ObscuraCdpSession>>> =
    OnceLock::new();

#[derive(Clone, Copy)]
enum CdpBackend {
    Obscura,
    Chromium,
}

fn obscura_target_value(target_id: &str, url: &str) -> Value {
    json!({
        "description": "",
        "devtoolsFrontendUrl": "",
        "id": target_id,
        "title": "",
        "type": "page",
        "url": url,
        "webSocketDebuggerUrl": format!("ws://127.0.0.1:9333/devtools/page/{target_id}"),
    })
}

impl CdpBackend {
    fn id(self) -> &'static str {
        match self {
            CdpBackend::Obscura => "obscura_cdp",
            CdpBackend::Chromium => "chromium_cdp",
        }
    }

    fn browser(self) -> &'static str {
        match self {
            CdpBackend::Obscura => "obscura",
            CdpBackend::Chromium => "chromium",
        }
    }

    fn display_name(self) -> &'static str {
        match self {
            CdpBackend::Obscura => "Obscura CDP",
            CdpBackend::Chromium => "Controlled Chromium/CDP",
        }
    }

    fn base_url(self) -> String {
        match self {
            CdpBackend::Obscura => cdp_url_from_env("OBSCURA_CDP", "OBSCURA_PORT", "9333"),
            CdpBackend::Chromium => {
                cdp_url_from_env("AGENT_BROWSER_CDP", "AGENT_BROWSER_PORT", "9222")
            }
        }
    }

    fn executable(self) -> String {
        match self {
            CdpBackend::Obscura => std::env::var("OBSCURA_EXECUTABLE")
                .unwrap_or_else(|_| "/home/maarten/.local/bin/obscura".to_string()),
            CdpBackend::Chromium => {
                std::env::var("AGENT_BROWSER_EXECUTABLE_PATH").unwrap_or_default()
            }
        }
    }

    fn profile(self) -> String {
        match self {
            CdpBackend::Obscura => std::env::var("OBSCURA_STORAGE_DIR").unwrap_or_default(),
            CdpBackend::Chromium => std::env::var("AGENT_BROWSER_PROFILE").unwrap_or_default(),
        }
    }
}

#[async_trait]
impl BrowserProvider for FirefoxBridgeProvider {
    fn id(&self) -> &'static str {
        "firefox_agent_bridge"
    }

    fn supported_browsers(&self) -> &'static [&'static str] {
        &["auto", "firefox"]
    }

    async fn status(&self, ctx: &ToolContext) -> Result<ToolOutput> {
        Ok(attach_browser_metadata(
            firefox_status(self, ctx).await?,
            self.id(),
            "firefox",
        ))
    }

    async fn setup(&self) -> Result<ToolOutput> {
        Ok(attach_browser_metadata(
            firefox_setup(self).await?,
            self.id(),
            "firefox",
        ))
    }

    async fn ensure_ready(&self) -> Result<Option<String>> {
        ensure_firefox_ready().await
    }

    async fn execute(
        &self,
        action: &str,
        input: &BrowserInput,
        ctx: &ToolContext,
    ) -> Result<ToolOutput> {
        Ok(attach_browser_metadata(
            execute_firefox_action(self, action, input, ctx).await?,
            self.id(),
            "firefox",
        ))
    }
}

#[async_trait]
impl BrowserProvider for ChromiumCdpProvider {
    fn id(&self) -> &'static str {
        "chromium_cdp"
    }

    fn supported_browsers(&self) -> &'static [&'static str] {
        &["chrome", "chromium"]
    }

    async fn status(&self, _ctx: &ToolContext) -> Result<ToolOutput> {
        Ok(attach_browser_metadata(
            cdp_status(CdpBackend::Chromium).await?,
            self.id(),
            "chromium",
        ))
    }

    async fn setup(&self) -> Result<ToolOutput> {
        cdp_start(CdpBackend::Chromium).await?;
        Ok(attach_browser_metadata(
            cdp_status(CdpBackend::Chromium).await?,
            self.id(),
            "chromium",
        ))
    }

    async fn ensure_ready(&self) -> Result<Option<String>> {
        if cdp_is_ready(CdpBackend::Chromium).await {
            return Ok(None);
        }
        cdp_start(CdpBackend::Chromium).await?;
        if cdp_is_ready(CdpBackend::Chromium).await {
            return Ok(Some(
                "Started controlled Chromium for browser automation.".to_string(),
            ));
        }
        anyhow::bail!(
            "Controlled Chromium/CDP is not responding. Check AGENT_BROWSER_CDP, AGENT_BROWSER_EXECUTABLE_PATH, or run the configured controlled Chrome launcher."
        )
    }

    async fn execute(
        &self,
        action: &str,
        input: &BrowserInput,
        ctx: &ToolContext,
    ) -> Result<ToolOutput> {
        Ok(attach_browser_metadata(
            execute_cdp_action(CdpBackend::Chromium, action, input, ctx).await?,
            self.id(),
            "chromium",
        ))
    }
}

#[async_trait]
impl BrowserProvider for ObscuraCdpProvider {
    fn id(&self) -> &'static str {
        "obscura_cdp"
    }

    fn supported_browsers(&self) -> &'static [&'static str] {
        &["auto", "obscura"]
    }

    async fn status(&self, _ctx: &ToolContext) -> Result<ToolOutput> {
        Ok(attach_browser_metadata(
            cdp_status(CdpBackend::Obscura).await?,
            self.id(),
            "obscura",
        ))
    }

    async fn setup(&self) -> Result<ToolOutput> {
        cdp_start(CdpBackend::Obscura).await?;
        Ok(attach_browser_metadata(
            cdp_status(CdpBackend::Obscura).await?,
            self.id(),
            "obscura",
        ))
    }

    async fn ensure_ready(&self) -> Result<Option<String>> {
        if cdp_is_ready(CdpBackend::Obscura).await {
            return Ok(None);
        }
        cdp_start(CdpBackend::Obscura).await?;
        if cdp_is_ready(CdpBackend::Obscura).await {
            return Ok(Some(
                "Started Obscura CDP server for browser automation.".to_string(),
            ));
        }
        anyhow::bail!(
            "Obscura CDP is not responding. Check OBSCURA_CDP/OBSCURA_PORT or run `obscura serve --port 9333`."
        )
    }

    async fn execute(
        &self,
        action: &str,
        input: &BrowserInput,
        ctx: &ToolContext,
    ) -> Result<ToolOutput> {
        Ok(attach_browser_metadata(
            execute_cdp_action(CdpBackend::Obscura, action, input, ctx).await?,
            self.id(),
            "obscura",
        ))
    }
}

#[async_trait]
impl Tool for BrowserTool {
    fn name(&self) -> &str {
        "browser"
    }

    fn description(&self) -> &str {
        browser_tool_description_text()
    }

    fn parameters_schema(&self) -> Value {
        let mut properties = Map::new();
        properties.insert("intent".into(), super::intent_schema_property());
        properties.insert(
            "action".into(),
            json!({
                "type": "string",
                "enum": [
                    "status", "setup", "list_tabs", "new_tab", "select_tab", "get_active_tab",
                    "list_frames", "open", "snapshot", "get_content", "interactables", "click", "type",
                    "fill_form", "select", "wait", "screenshot", "eval", "scroll", "upload",
                    "press", "provider_command", "batch"
                ],
                "description": "Action. Check 'status' first; run 'setup' only when the bridge is not ready."
            }),
        );
        properties.insert(
            "browser".into(),
            json!({
                "type": "string",
                "enum": ["auto", "obscura", "firefox", "chrome", "chromium", "safari", "edge"],
                "description": "Browser. auto uses Obscura/CDP on port 9333. chrome/chromium are explicit fallbacks only."
            }),
        );
        properties.insert(
            "provider_action".into(),
            json!({
                "type": "string",
                "description": "Provider command name. For CDP backends this is the CDP method. Alias: method."
            }),
        );
        properties.insert(
            "method".into(),
            json!({
                "type": "string",
                "description": "Alias for provider_action, intended for CDP provider_command calls."
            }),
        );
        properties.insert(
            "params".into(),
            json!({
                "type": "object",
                "description": "Raw provider params."
            }),
        );
        for (name, schema) in [
            ("url", json!({"type": "string"})),
            ("tab_id", json!({"type": "integer"})),
            (
                "window_id",
                json!({"type": "integer", "description": "Scope the action to one browser window when multiple agents share the browser."}),
            ),
            ("frame_id", json!({"type": "integer"})),
            ("all_frames", json!({"type": "boolean"})),
            ("selector", json!({"type": "string"})),
            ("text", json!({"type": "string"})),
            ("contains", json!({"type": "string"})),
            ("script", json!({"type": "string"})),
            ("key", json!({"type": "string"})),
            ("x", json!({"type": "number"})),
            ("y", json!({"type": "number"})),
            ("wait", json!({"type": "boolean"})),
            ("new_tab", json!({"type": "boolean"})),
            ("focus", json!({"type": "boolean"})),
            ("clear", json!({"type": "boolean"})),
            ("submit", json!({"type": "boolean"})),
            ("page_world", json!({"type": "boolean"})),
            ("position", json!({"type": "string"})),
            ("behavior", json!({"type": "string"})),
            ("timeout_ms", json!({"type": "integer"})),
            ("path", json!({"type": "string"})),
        ] {
            properties.insert(name.into(), schema);
        }
        properties.insert(
            "format".into(),
            json!({
                "type": "string",
                "enum": ["annotated", "text", "textFast", "html", "title"],
                "description": "Format."
            }),
        );
        properties.insert(
            "fields".into(),
            json!({
                "type": "array",
                "description": "Form fields.",
                "items": {
                    "type": "object",
                    "required": ["selector"],
                    "properties": {
                        "selector": { "type": "string" },
                        "value": { "type": "string" },
                        "checked": { "type": "boolean" }
                    }
                }
            }),
        );
        properties.insert(
            "scroll_to".into(),
            json!({
                "type": "object",
                "properties": {
                    "x": { "type": "number" },
                    "y": { "type": "number" }
                }
            }),
        );
        Value::Object(Map::from_iter([
            ("type".into(), json!("object")),
            ("required".into(), json!(["action"])),
            ("properties".into(), Value::Object(properties)),
        ]))
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let params: BrowserInput = serde_json::from_value(input)?;
        let provider = resolve_provider(params.browser.as_deref())?;

        match params.action.as_str() {
            "status" => provider.status(&ctx).await,
            "setup" => provider.setup().await,
            "batch" => {
                let setup_message = provider.ensure_ready().await?;
                let output = execute_browser_batch(provider, &params, &ctx).await?;
                Ok(match setup_message {
                    Some(message) if !message.is_empty() => prepend_setup_message(output, &message),
                    _ => output,
                })
            }
            other => {
                let setup_message = provider.ensure_ready().await?;
                let output = provider.execute(other, &params, &ctx).await?;
                Ok(match setup_message {
                    Some(message) if !message.is_empty() => prepend_setup_message(output, &message),
                    _ => output,
                })
            }
        }
    }
}

async fn execute_browser_batch(
    provider: &'static dyn BrowserProvider,
    input: &BrowserInput,
    ctx: &ToolContext,
) -> Result<ToolOutput> {
    let steps = input
        .params
        .as_ref()
        .and_then(|params| params.get("steps"))
        .and_then(|steps| steps.as_array())
        .ok_or_else(|| anyhow::anyhow!("params.steps array is required for batch"))?;
    let mut outputs = Vec::new();
    for step in steps {
        let mut step_input: BrowserInput = serde_json::from_value(step.clone())?;
        if step_input.browser.is_none() {
            step_input.browser = input.browser.clone();
        }
        let action = step_input.action.clone();
        let output = match action.as_str() {
            "status" => provider.status(ctx).await?,
            "setup" => provider.setup().await?,
            "batch" => anyhow::bail!("Nested browser batch actions are not supported"),
            other => provider.execute(other, &step_input, ctx).await?,
        };
        outputs.push(json!({
            "action": action,
            "title": output.title,
            "output": output.output,
            "metadata": output.metadata,
            "images": output.images.len(),
        }));
    }
    Ok(
        ToolOutput::new(format!("Ran {} browser batch step(s).", outputs.len()))
            .with_title("browser batch")
            .with_metadata(json!({"steps": outputs})),
    )
}

pub(crate) async fn run_cli_action(action: &str) -> Result<ToolOutput> {
    run_cli_value(json!({ "action": action })).await
}

pub(crate) async fn run_cli_value(input: Value) -> Result<ToolOutput> {
    let ctx = ToolContext {
        session_id: "browser-cli".to_string(),
        message_id: "browser-cli".to_string(),
        tool_call_id: "browser-cli".to_string(),
        working_dir: std::env::current_dir().ok(),
        stdin_request_tx: None,
        graceful_shutdown_signal: None,
        execution_mode: super::ToolExecutionMode::Direct,
    };
    BrowserTool::new().execute(input, ctx).await
}

fn prepend_setup_message(mut output: ToolOutput, message: &str) -> ToolOutput {
    output.output = format!("{}\n\n{}", message, output.output);
    if output.title.is_none() {
        output.title = Some("browser".to_string());
    }

    let mut metadata = match output.metadata.take() {
        Some(Value::Object(map)) => map,
        Some(other) => {
            let mut map = Map::new();
            map.insert("result".into(), other);
            map
        }
        None => Map::new(),
    };
    metadata.insert("setup_ran".into(), json!(true));
    output.metadata = Some(Value::Object(metadata));
    output
}

fn attach_browser_metadata(
    mut output: ToolOutput,
    backend: &'static str,
    browser: &'static str,
) -> ToolOutput {
    let mut metadata = match output.metadata.take() {
        Some(Value::Object(map)) => map,
        Some(other) => {
            let mut map = Map::new();
            map.insert("result".into(), other);
            map
        }
        None => Map::new(),
    };
    metadata.insert("backend".into(), json!(backend));
    metadata.insert("browser".into(), json!(browser));
    output.metadata = Some(Value::Object(metadata));
    output
}

fn resolve_provider(browser: Option<&str>) -> Result<&'static dyn BrowserProvider> {
    let browser = browser.unwrap_or("auto");
    if OBSCURA_PROVIDER.supported_browsers().contains(&browser) {
        return Ok(&OBSCURA_PROVIDER);
    }
    if CHROMIUM_PROVIDER.supported_browsers().contains(&browser) {
        return Ok(&CHROMIUM_PROVIDER);
    }
    if FIREFOX_PROVIDER.supported_browsers().contains(&browser) {
        return Ok(&FIREFOX_PROVIDER);
    }

    anyhow::bail!(
        "Browser backend '{}' is not wired into the built-in browser tool yet. Use auto/obscura, chromium/chrome, or firefox.",
        browser
    )
}

fn cdp_url_from_env(url_env: &str, port_env: &str, default_port: &str) -> String {
    let raw = std::env::var(url_env)
        .or_else(|_| std::env::var(port_env))
        .unwrap_or_else(|_| default_port.to_string());
    if raw.starts_with("http://") || raw.starts_with("https://") {
        raw.trim_end_matches('/').to_string()
    } else if raw.contains(':') {
        format!("http://{}", raw.trim_end_matches('/'))
    } else {
        format!("http://127.0.0.1:{}", raw.trim())
    }
}

async fn cdp_is_ready(backend: CdpBackend) -> bool {
    reqwest::get(format!("{}/json/version", backend.base_url()))
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

async fn cdp_status(backend: CdpBackend) -> Result<ToolOutput> {
    let base = backend.base_url();
    let version_resp = reqwest::get(format!("{}/json/version", base)).await;
    let ready = version_resp
        .as_ref()
        .map(|r| r.status().is_success())
        .unwrap_or(false);
    let version_json = match version_resp {
        Ok(resp) if resp.status().is_success() => resp.json::<Value>().await.unwrap_or(Value::Null),
        _ => Value::Null,
    };
    let profile = backend.profile();
    let executable = backend.executable();
    let tabs = if ready {
        cdp_tabs(backend).await.unwrap_or_default()
    } else {
        Vec::new()
    };

    let body = if ready {
        format!(
            "{} is responding at {}.\nTabs: {}\nStorage/Profile: {}\nExecutable: {}",
            backend.display_name(),
            base,
            tabs.len(),
            if profile.is_empty() {
                "unknown"
            } else {
                &profile
            },
            if executable.is_empty() {
                "unknown"
            } else {
                &executable
            },
        )
    } else {
        format!(
            "{} is not responding at {}. Use action='setup' to start it.",
            backend.display_name(),
            base,
        )
    };

    Ok(ToolOutput::new(body).with_title("browser status").with_metadata(json!({
        "ready": ready,
        "responding": ready,
        "setup_complete": ready,
        "binary_installed": !executable.is_empty() && std::path::Path::new(&executable).exists(),
        "compatible": ready,
        "backend": backend.id(),
        "browser": backend.browser(),
        "cdp_base_url": base,
        "profile": profile,
        "executable": executable,
        "version": version_json,
        "tabs": tabs,
    })))
}

async fn cdp_start(backend: CdpBackend) -> Result<()> {
    if cdp_is_ready(backend).await {
        return Ok(());
    }
    let mut command = match backend {
        CdpBackend::Obscura => {
            let executable = backend.executable();
            if !std::path::Path::new(&executable).exists() {
                anyhow::bail!("Obscura executable not found: {}", executable);
            }
            let mut command = tokio::process::Command::new(executable);
            command
                .arg("serve")
                .arg("--host")
                .arg("127.0.0.1")
                .arg("--port")
                .arg("9333")
                .arg("--quiet")
                .arg("--allow-file-access");
            if let Ok(storage_dir) = std::env::var("OBSCURA_STORAGE_DIR") {
                if !storage_dir.is_empty() {
                    command.arg("--storage-dir").arg(storage_dir);
                }
            }
            command
        }
        CdpBackend::Chromium => {
            let script = std::env::var("AGENT_BROWSER_LAUNCHER").unwrap_or_else(|_| {
                "/data/projects/systeembeheer/chrome-agent-browser/start-controlled-chrome.sh"
                    .to_string()
            });
            if !std::path::Path::new(&script).exists() {
                anyhow::bail!("Controlled Chromium launcher not found: {}", script);
            }
            let mut command = tokio::process::Command::new(script);
            command.arg("about:blank");
            command
        }
    };
    command.stdin(std::process::Stdio::null());
    command.stdout(std::process::Stdio::null());
    command.stderr(std::process::Stdio::null());
    let _child = command
        .spawn()
        .with_context(|| format!("Failed to start {}", backend.display_name()))?;
    for _ in 0..40 {
        if cdp_is_ready(backend).await {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    Ok(())
}

async fn cdp_tabs(backend: CdpBackend) -> Result<Vec<Value>> {
    if matches!(backend, CdpBackend::Obscura) {
        if let Some(mutex) = OBSCURA_CDP_SESSION.get() {
            let guard = mutex.lock().await;
            if let Some(session) = guard.as_ref() {
                return Ok(vec![obscura_target_value(&session.target_id, &session.url)]);
            }
        }
        let browser_ws = cdp_browser_ws_url(backend).await?;
        let (mut ws, _) = tokio_tungstenite::connect_async(browser_ws).await?;
        let mut next_id = 1_i64;
        let result =
            cdp_call_on(&mut ws, &mut next_id, None, "Target.getTargets", json!({})).await?;
        let infos = result
            .get("targetInfos")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        return Ok(infos
            .into_iter()
            .filter(|tab| tab.get("type").and_then(|v| v.as_str()) == Some("page"))
            .map(|tab| {
                let target_id = tab
                    .get("targetId")
                    .and_then(|v| v.as_str())
                    .unwrap_or("page-1");
                json!({
                    "description": "",
                    "devtoolsFrontendUrl": "",
                    "id": target_id,
                    "title": tab.get("title").cloned().unwrap_or(Value::Null),
                    "type": "page",
                    "url": tab.get("url").cloned().unwrap_or(Value::Null),
                    "webSocketDebuggerUrl": format!("ws://127.0.0.1:9333/devtools/page/{target_id}"),
                })
            })
            .collect());
    }

    let tabs: Vec<Value> = reqwest::get(format!("{}/json/list", backend.base_url()))
        .await?
        .json()
        .await?;
    Ok(tabs
        .into_iter()
        .filter(|tab| tab.get("type").and_then(|v| v.as_str()) == Some("page"))
        .collect())
}

async fn cdp_target(backend: CdpBackend, input: &BrowserInput) -> Result<Value> {
    if matches!(backend, CdpBackend::Obscura) {
        if let Some(mutex) = OBSCURA_CDP_SESSION.get() {
            let guard = mutex.lock().await;
            if let Some(session) = guard.as_ref() {
                return Ok(obscura_target_value(&session.target_id, &session.url));
            }
        }
    }
    let tabs = cdp_tabs(backend).await?;
    if tabs.is_empty() {
        if matches!(backend, CdpBackend::Obscura) {
            return cdp_new_tab(backend, Some("about:blank")).await;
        }
        anyhow::bail!("No {} page targets are available", backend.display_name());
    }
    let idx = input.tab_id.unwrap_or(0).max(0) as usize;
    tabs.get(idx)
        .cloned()
        .or_else(|| tabs.first().cloned())
        .ok_or_else(|| anyhow::anyhow!("No {} page targets are available", backend.display_name()))
}

async fn cdp_new_tab(backend: CdpBackend, url: Option<&str>) -> Result<Value> {
    if matches!(backend, CdpBackend::Obscura) {
        let target_url = url.unwrap_or("about:blank");
        return obscura_navigate(target_url).await;
    }

    let target_url = url.unwrap_or("about:blank");
    let encoded = urlencoding::encode(target_url);
    let client = reqwest::Client::new();
    let resp = client
        .put(format!("{}/json/new?{}", backend.base_url(), encoded))
        .send()
        .await?;
    if !resp.status().is_success() {
        anyhow::bail!(
            "Failed to create {} tab: HTTP {}",
            backend.browser(),
            resp.status()
        );
    }
    Ok(resp.json::<Value>().await?)
}

async fn cdp_activate(backend: CdpBackend, id: &str) -> Result<()> {
    let _ = reqwest::get(format!("{}/json/activate/{}", backend.base_url(), id)).await?;
    Ok(())
}

async fn cdp_call_on(
    ws: &mut WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    next_id: &mut i64,
    session_id: Option<&str>,
    method: &str,
    params: Value,
) -> Result<Value> {
    let id = *next_id;
    *next_id += 1;
    let mut message = json!({"id": id, "method": method, "params": params});
    if let Some(session_id) = session_id {
        message["sessionId"] = json!(session_id);
    }
    ws.send(Message::Text(message.to_string())).await?;
    while let Some(msg) = ws.next().await {
        let msg = msg?;
        if !msg.is_text() {
            continue;
        }
        let value: Value = serde_json::from_str(msg.to_text()?)?;
        if value.get("id").and_then(|v| v.as_i64()) == Some(id) {
            if let Some(error) = value.get("error") {
                anyhow::bail!("CDP {} failed: {}", method, error);
            }
            return Ok(value.get("result").cloned().unwrap_or(Value::Null));
        }
    }
    anyhow::bail!("CDP {} returned no response", method)
}

async fn cdp_browser_ws_url(backend: CdpBackend) -> Result<String> {
    let version: Value = reqwest::get(format!("{}/json/version", backend.base_url()))
        .await?
        .json()
        .await?;
    version
        .get("webSocketDebuggerUrl")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "{} has no browser webSocketDebuggerUrl",
                backend.display_name()
            )
        })
}

async fn obscura_new_session(url: &str) -> Result<ObscuraCdpSession> {
    let browser_ws = cdp_browser_ws_url(CdpBackend::Obscura).await?;
    let (mut ws, _) = tokio_tungstenite::connect_async(browser_ws).await?;
    let mut next_id = 1_i64;
    let created = cdp_call_on(
        &mut ws,
        &mut next_id,
        None,
        "Target.createTarget",
        json!({"url": url}),
    )
    .await?;
    let target_id = created
        .get("targetId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Target.createTarget returned no targetId"))?
        .to_string();
    let attached = cdp_call_on(
        &mut ws,
        &mut next_id,
        None,
        "Target.attachToTarget",
        json!({"targetId": target_id, "flatten": true}),
    )
    .await?;
    let session_id = attached
        .get("sessionId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Target.attachToTarget returned no sessionId"))?
        .to_string();
    Ok(ObscuraCdpSession {
        ws,
        next_id,
        session_id,
        target_id,
        url: url.to_string(),
    })
}

async fn obscura_session_call(method: &str, params: Value) -> Result<Value> {
    let mutex = OBSCURA_CDP_SESSION.get_or_init(|| tokio::sync::Mutex::new(None));
    let mut guard = mutex.lock().await;
    if guard.is_none() {
        *guard = Some(obscura_new_session("about:blank").await?);
    }
    let session = guard.as_mut().expect("session was initialized");
    match cdp_call_on(
        &mut session.ws,
        &mut session.next_id,
        Some(&session.session_id),
        method,
        params.clone(),
    )
    .await
    {
        Ok(value) => Ok(value),
        Err(first_error) => {
            *guard = Some(obscura_new_session("about:blank").await?);
            let session = guard.as_mut().expect("session was reinitialized");
            cdp_call_on(
                &mut session.ws,
                &mut session.next_id,
                Some(&session.session_id),
                method,
                params,
            )
            .await
            .with_context(|| {
                format!("retry after recreating Obscura session; first error: {first_error}")
            })
        }
    }
}

async fn obscura_navigate(url: &str) -> Result<Value> {
    let mutex = OBSCURA_CDP_SESSION.get_or_init(|| tokio::sync::Mutex::new(None));
    let mut guard = mutex.lock().await;
    if guard.is_none() {
        *guard = Some(obscura_new_session(url).await?);
    } else if let Some(session) = guard.as_mut() {
        cdp_call_on(
            &mut session.ws,
            &mut session.next_id,
            Some(&session.session_id),
            "Page.navigate",
            json!({"url": url}),
        )
        .await?;
        session.url = url.to_string();
    }
    let target_id = guard
        .as_ref()
        .map(|session| session.target_id.clone())
        .unwrap_or_else(|| "page-1".to_string());
    Ok(obscura_target_value(&target_id, url))
}

async fn cdp_connect_to_target(
    backend: CdpBackend,
    target: &Value,
) -> Result<(
    WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    i64,
    Option<String>,
)> {
    let ws_url = match backend {
        CdpBackend::Obscura => cdp_browser_ws_url(backend).await?,
        CdpBackend::Chromium => cdp_ws_url(target)?.to_string(),
    };
    let (mut ws, _) = tokio_tungstenite::connect_async(ws_url).await?;
    let mut next_id = 1_i64;
    let session_id = if matches!(backend, CdpBackend::Obscura) {
        let target_id = target
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("{} target has no id", backend.display_name()))?;
        let attached = cdp_call_on(
            &mut ws,
            &mut next_id,
            None,
            "Target.attachToTarget",
            json!({"targetId": target_id, "flatten": true}),
        )
        .await?;
        Some(
            attached
                .get("sessionId")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Target.attachToTarget returned no sessionId"))?
                .to_string(),
        )
    } else {
        None
    };
    Ok((ws, next_id, session_id))
}

async fn cdp_call_target(
    backend: CdpBackend,
    target: &Value,
    method: &str,
    params: Value,
) -> Result<Value> {
    if matches!(backend, CdpBackend::Obscura) {
        return obscura_session_call(method, params).await;
    }

    let (mut ws, mut next_id, session_id) = cdp_connect_to_target(backend, target).await?;
    cdp_call_on(&mut ws, &mut next_id, session_id.as_deref(), method, params).await
}

async fn cdp_eval(
    backend: CdpBackend,
    target: &Value,
    expression: String,
    await_promise: bool,
) -> Result<Value> {
    let result = cdp_call_target(
        backend,
        target,
        "Runtime.evaluate",
        json!({
            "expression": expression,
            "awaitPromise": await_promise,
            "returnByValue": true,
        }),
    )
    .await?;
    let remote = result.get("result").cloned().unwrap_or(Value::Null);
    if let Some(exception) = result.get("exceptionDetails") {
        anyhow::bail!("JavaScript evaluation failed: {}", exception);
    }
    Ok(json!({
        "result": remote.get("value").cloned().unwrap_or(Value::Null),
        "type": remote.get("type").cloned().unwrap_or(Value::Null),
    }))
}

fn js_string(value: &str) -> Result<String> {
    Ok(serde_json::to_string(value)?)
}

fn cdp_ws_url(target: &Value) -> Result<&str> {
    target
        .get("webSocketDebuggerUrl")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("CDP target has no webSocketDebuggerUrl"))
}

async fn execute_cdp_action(
    backend: CdpBackend,
    action: &str,
    input: &BrowserInput,
    _ctx: &ToolContext,
) -> Result<ToolOutput> {
    match action {
        "list_tabs" => {
            let tabs = cdp_tabs(backend).await?;
            return Ok(ToolOutput::new(serde_json::to_string_pretty(&tabs)?)
                .with_title("browser list_tabs")
                .with_metadata(json!({"tabs": tabs})));
        }
        "new_tab" => {
            let tab = cdp_new_tab(backend, input.url.as_deref()).await?;
            return Ok(ToolOutput::new(serde_json::to_string_pretty(&tab)?)
                .with_title("browser new_tab")
                .with_metadata(tab));
        }
        "select_tab" => {
            let target = cdp_target(backend, input).await?;
            if let Some(id) = target.get("id").and_then(|v| v.as_str()) {
                cdp_activate(backend, id).await?;
            }
            return Ok(ToolOutput::new(serde_json::to_string_pretty(&target)?)
                .with_title("browser select_tab")
                .with_metadata(target));
        }
        "get_active_tab" => {
            let target = cdp_target(backend, input).await?;
            return Ok(ToolOutput::new(serde_json::to_string_pretty(&target)?)
                .with_title("browser get_active_tab")
                .with_metadata(target));
        }
        _ => {}
    }

    let target = cdp_target(backend, input).await?;
    let title = format!("browser {}", action);
    let result = match action {
        "open" => {
            if input.new_tab.unwrap_or(false) {
                cdp_new_tab(backend, input.url.as_deref()).await?
            } else {
                let url = input
                    .url
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("url is required for open"))?;
                cdp_call_target(backend, &target, "Page.navigate", json!({"url": url})).await?;
                if input.wait.unwrap_or(true) {
                    let timeout = input.timeout_ms.unwrap_or(30_000);
                    let expr = format!(
                        r#"new Promise((resolve, reject) => {{ const deadline = Date.now() + {}; const tick = () => {{ if (document.readyState === 'complete') resolve(true); else if (Date.now() > deadline) reject(new Error('navigation timeout')); else setTimeout(tick, 100); }}; tick(); }})"#,
                        timeout
                    );
                    cdp_eval(backend, &target, expr, true).await?;
                }
                json!({"ok": true, "url": url})
            }
        }
        "snapshot" | "content" | "get_content" => {
            let fmt = input.format.as_deref().unwrap_or(if action == "snapshot" {
                "annotated"
            } else {
                "text"
            });
            let script = match fmt {
                "html" => "document.documentElement.outerHTML",
                "title" => "document.title + '\\n' + location.href",
                _ => {
                    "document.title + '\\n' + location.href + '\\n\\n' + (document.body ? document.body.innerText : '')"
                }
            };
            let eval = cdp_eval(backend, &target, script.to_string(), false).await?;
            json!({"content": eval.get("result").cloned().unwrap_or(Value::Null)})
        }
        "interactables" => {
            let script = r#"Array.from(document.querySelectorAll('a,button,input,textarea,select,[role=button],[tabindex]')).slice(0,100).map((el, i) => ({index:i, tag:el.tagName, type:el.getAttribute('role') || el.type || 'element', text:(el.innerText || el.value || el.ariaLabel || el.name || el.id || '').trim().slice(0,120), selector: el.id ? '#' + CSS.escape(el.id) : el.tagName.toLowerCase() + ':nth-of-type(' + (Array.from(el.parentElement ? el.parentElement.children : []).filter(e => e.tagName === el.tagName).indexOf(el) + 1) + ')'}))"#;
            let eval = cdp_eval(backend, &target, script.to_string(), false).await?;
            json!({"elements": eval.get("result").cloned().unwrap_or(Value::Null)})
        }
        "click" => {
            let expr = if let Some(selector) = &input.selector {
                format!(
                    "(() => {{ const el = document.querySelector({}); if (!el) throw new Error('selector not found'); el.click(); return true; }})()",
                    js_string(selector)?
                )
            } else if let Some(text) = &input.text {
                format!(
                    "(() => {{ const needle = {}; const el = Array.from(document.querySelectorAll('a,button,input,[role=button]')).find(e => (e.innerText || e.value || '').includes(needle)); if (!el) throw new Error('text not found'); el.click(); return true; }})()",
                    js_string(text)?
                )
            } else {
                format!(
                    "(() => {{ const el = document.elementFromPoint({}, {}); if (!el) throw new Error('point not found'); el.click(); return true; }})()",
                    input.x.unwrap_or(0.0),
                    input.y.unwrap_or(0.0)
                )
            };
            cdp_eval(backend, &target, expr, false).await?
        }
        "type" => {
            let text = input
                .text
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("text is required for type"))?;
            let selector = input.selector.as_deref().unwrap_or(":focus");
            let expr = format!(
                "(() => {{ const el = document.querySelector({}); if (!el) throw new Error('selector not found'); el.focus(); if ({}) el.value = ''; el.value = (el.value || '') + {}; el.dispatchEvent(new Event('input', {{bubbles:true}})); if ({}) {{ el.form ? el.form.requestSubmit() : el.dispatchEvent(new KeyboardEvent('keydown', {{key:'Enter', bubbles:true}})); }} return true; }})()",
                js_string(selector)?,
                input.clear.unwrap_or(false),
                js_string(text)?,
                input.submit.unwrap_or(false)
            );
            cdp_eval(backend, &target, expr, false).await?
        }
        "fill_form" => {
            let fields = serde_json::to_string(
                input
                    .fields
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("fields are required for fill_form"))?,
            )?;
            let expr = format!(
                "(() => {{ const fields = {}; for (const f of fields) {{ const el = document.querySelector(f.selector); if (!el) throw new Error('selector not found: ' + f.selector); if (typeof f.checked === 'boolean') el.checked = f.checked; if (f.value !== undefined) el.value = f.value; el.dispatchEvent(new Event('input', {{bubbles:true}})); el.dispatchEvent(new Event('change', {{bubbles:true}})); }} return true; }})()",
                fields
            );
            cdp_eval(backend, &target, expr, false).await?
        }
        "select" => {
            let selector = input
                .selector
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("selector is required for select"))?;
            let value = input
                .text
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("text is required for select"))?;
            let expr = format!(
                "(() => {{ const el = document.querySelector({}); if (!el) throw new Error('selector not found'); el.value = {}; el.dispatchEvent(new Event('change', {{bubbles:true}})); return true; }})()",
                js_string(selector)?,
                js_string(value)?
            );
            cdp_eval(backend, &target, expr, false).await?
        }
        "wait" => {
            let timeout = input.timeout_ms.unwrap_or(10_000);
            let selector = input.selector.as_deref().unwrap_or("");
            let text = input
                .text
                .as_deref()
                .or(input.contains.as_deref())
                .unwrap_or("");
            let expr = format!(
                r#"new Promise((resolve, reject) => {{ const deadline = Date.now() + {}; const selector = {}; const text = {}; const tick = () => {{ const ok = (selector && document.querySelector(selector)) || (text && document.body && document.body.innerText.includes(text)); if (ok) resolve(true); else if (Date.now() > deadline) reject(new Error('wait timeout')); else setTimeout(tick, 100); }}; tick(); }})"#,
                timeout,
                js_string(selector)?,
                js_string(text)?
            );
            cdp_eval(backend, &target, expr, true).await?
        }
        "eval" => {
            let script = input
                .script
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("script is required for eval"))?;
            cdp_eval(backend, &target, script.to_string(), true).await?
        }
        "scroll" => {
            let x = input.x.unwrap_or(0.0);
            let y = input.y.unwrap_or(800.0);
            let expr = if let Some(selector) = &input.selector {
                format!(
                    "(() => {{ const el = document.querySelector({}); if (!el) throw new Error('selector not found'); el.scrollIntoView({{behavior:{}, block:'center'}}); return true; }})()",
                    js_string(selector)?,
                    js_string(input.behavior.as_deref().unwrap_or("auto"))?
                )
            } else {
                format!("window.scrollBy({}, {}); true", x, y)
            };
            cdp_eval(backend, &target, expr, false).await?
        }
        "press" => {
            let key = input
                .key
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("key is required for press"))?;
            let selector_literal = input
                .selector
                .as_deref()
                .map(serde_json::to_string)
                .transpose()?;
            let selector_expr = selector_literal
                .map(|s| format!("document.querySelector({})", s))
                .unwrap_or_else(|| "null".to_string());
            let key_literal = serde_json::to_string(key)?;
            let script = format!(
                r#"(() => {{
  const target = {selector_expr} || document.activeElement || document.body;
  if (!target) throw new Error('No target available for key press');
  if (typeof target.focus === 'function') target.focus();
  const key = {key_literal};
  const eventInit = {{ key, bubbles: true, cancelable: true }};
  target.dispatchEvent(new KeyboardEvent('keydown', eventInit));
  target.dispatchEvent(new KeyboardEvent('keypress', eventInit));
  if (key === 'Enter' && target.form && typeof target.form.requestSubmit === 'function') {{
    target.form.requestSubmit();
  }}
  target.dispatchEvent(new KeyboardEvent('keyup', eventInit));
  return {{ pressed: true, key, tag: target.tagName || null }};
}})()"#
            );
            cdp_eval(backend, &target, script, false).await?
        }
        "screenshot" => {
            let capture = cdp_call_target(
                backend,
                &target,
                "Page.captureScreenshot",
                json!({"format":"png", "fromSurface": true}),
            )
            .await?;
            let data = capture.get("data").and_then(|v| v.as_str()).unwrap_or("");
            let mut out = ToolOutput::new(format!(
                "Captured browser screenshot from {}.",
                backend.display_name()
            ))
            .with_title(title)
            .with_metadata(capture.clone());
            if !data.is_empty() {
                out = out.with_labeled_image(
                    "image/png",
                    data.to_string(),
                    "browser screenshot".to_string(),
                );
            }
            return Ok(out);
        }
        "list_frames" => cdp_call_target(backend, &target, "Page.getFrameTree", json!({})).await?,
        "upload" => {
            let selector = input
                .selector
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("selector is required for upload"))?;
            let path = input
                .path
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("path is required for upload"))?;
            if matches!(backend, CdpBackend::Obscura) {
                let mutex = OBSCURA_CDP_SESSION.get_or_init(|| tokio::sync::Mutex::new(None));
                let mut guard = mutex.lock().await;
                if guard.is_none() {
                    *guard = Some(obscura_new_session("about:blank").await?);
                }
                let session = guard.as_mut().expect("session was initialized");
                cdp_call_on(
                    &mut session.ws,
                    &mut session.next_id,
                    Some(&session.session_id),
                    "DOM.enable",
                    json!({}),
                )
                .await?;
                let document = cdp_call_on(
                    &mut session.ws,
                    &mut session.next_id,
                    Some(&session.session_id),
                    "DOM.getDocument",
                    json!({}),
                )
                .await?;
                let root_node_id = document
                    .get("root")
                    .and_then(|root| root.get("nodeId"))
                    .and_then(|v| v.as_i64())
                    .ok_or_else(|| {
                        anyhow::anyhow!("CDP DOM.getDocument did not return a root node")
                    })?;
                let queried = cdp_call_on(
                    &mut session.ws,
                    &mut session.next_id,
                    Some(&session.session_id),
                    "DOM.querySelector",
                    json!({"nodeId": root_node_id, "selector": selector}),
                )
                .await?;
                let node_id = queried
                    .get("nodeId")
                    .and_then(|v| v.as_i64())
                    .filter(|id| *id != 0)
                    .ok_or_else(|| anyhow::anyhow!("upload selector not found: {}", selector))?;
                cdp_call_on(
                    &mut session.ws,
                    &mut session.next_id,
                    Some(&session.session_id),
                    "DOM.setFileInputFiles",
                    json!({"nodeId": node_id, "files": [path]}),
                )
                .await?;
                return Ok(render_browser_output(
                    action,
                    title,
                    json!({"ok": true, "selector": selector, "path": path}),
                ));
            }
            let (mut ws_conn, mut next_id, session_id) =
                cdp_connect_to_target(backend, &target).await?;
            cdp_call_on(
                &mut ws_conn,
                &mut next_id,
                session_id.as_deref(),
                "DOM.enable",
                json!({}),
            )
            .await?;
            let document = cdp_call_on(
                &mut ws_conn,
                &mut next_id,
                session_id.as_deref(),
                "DOM.getDocument",
                json!({}),
            )
            .await?;
            let root_node_id = document
                .get("root")
                .and_then(|root| root.get("nodeId"))
                .and_then(|v| v.as_i64())
                .ok_or_else(|| anyhow::anyhow!("CDP DOM.getDocument did not return a root node"))?;
            let queried = cdp_call_on(
                &mut ws_conn,
                &mut next_id,
                session_id.as_deref(),
                "DOM.querySelector",
                json!({"nodeId": root_node_id, "selector": selector}),
            )
            .await?;
            let node_id = queried
                .get("nodeId")
                .and_then(|v| v.as_i64())
                .filter(|id| *id != 0)
                .ok_or_else(|| anyhow::anyhow!("upload selector not found: {}", selector))?;
            cdp_call_on(
                &mut ws_conn,
                &mut next_id,
                session_id.as_deref(),
                "DOM.setFileInputFiles",
                json!({"nodeId": node_id, "files": [path]}),
            )
            .await?;
            json!({"ok": true, "selector": selector, "path": path})
        }
        "provider_command" => {
            let method = input.provider_method().ok_or_else(|| {
                anyhow::anyhow!(
                    "provider_action or method is required and is used as the CDP method"
                )
            })?;
            cdp_call_target(
                backend,
                &target,
                method,
                input.params.clone().unwrap_or_else(|| json!({})),
            )
            .await?
        }
        other => anyhow::bail!(
            "Unsupported browser action for {}: {}",
            backend.display_name(),
            other
        ),
    };

    Ok(render_browser_output(action, title, result))
}

async fn firefox_status(
    provider: &FirefoxBridgeProvider,
    _ctx: &ToolContext,
) -> Result<ToolOutput> {
    let status = crate::browser::ensure_browser_ready_noninteractive().await?;
    let mut metadata = json!({
        "setup_complete": status.setup_complete,
        "binary_installed": status.binary_installed,
        "responding": status.responding,
        "compatible": status.compatible,
        "missing_actions": status.missing_actions,
        "ready": status.ready,
        "backend": if status.binary_installed || status.setup_complete || status.ready {
            provider.id()
        } else {
            "unconfigured"
        },
        "browser": "firefox",
    });

    if status.ready {
        return Ok(
            ToolOutput::new("Browser bridge is installed and responding.")
                .with_title("browser status")
                .with_metadata(metadata),
        );
    }

    if status.responding && !status.compatible {
        let missing = if status.missing_actions.is_empty() {
            "unknown required actions".to_string()
        } else {
            status.missing_actions.join(", ")
        };
        return Ok(ToolOutput::new(format!(
            "Browser bridge is connected, but the live Firefox extension is out of date and does not support required actions: {}. Use action='setup' only to repair or update the existing install. You do not need to run setup before every browser task.",
            missing
        ))
        .with_title("browser status")
        .with_metadata(metadata));
    }

    if status.binary_installed {
        return Ok(ToolOutput::new(
            "Browser bridge binaries are installed, but the live bridge is not responding. Use action='setup' only if you want to repair the existing install. You do not need to run setup before every browser task.",
        )
        .with_title("browser status")
        .with_metadata(metadata));
    }

    metadata["backend"] = json!("unconfigured");
    Ok(ToolOutput::new(
        "Browser bridge is not installed yet. Use action='setup' only for first-time install or repair. You do not need to run setup before every browser task.",
    )
    .with_title("browser status")
    .with_metadata(metadata))
}

async fn firefox_setup(provider: &FirefoxBridgeProvider) -> Result<ToolOutput> {
    let log = crate::browser::ensure_browser_setup().await?;
    let status = crate::browser::ensure_browser_ready_noninteractive().await?;
    let title = if status.ready {
        "browser setup"
    } else {
        "browser setup (incomplete)"
    };
    Ok(ToolOutput::new(log).with_title(title).with_metadata(json!({
        "setup_complete": status.setup_complete,
        "binary_installed": status.binary_installed,
        "responding": status.responding,
        "compatible": status.compatible,
        "missing_actions": status.missing_actions,
        "ready": status.ready,
        "backend": provider.id(),
        "browser": "firefox"
    })))
}

async fn ensure_firefox_ready() -> Result<Option<String>> {
    // A setup marker only proves that installation once completed. Always
    // verify the live bridge before launching an action because Firefox or the
    // extension may have stopped or become incompatible since then.
    let status = crate::browser::ensure_browser_ready_noninteractive().await?;
    if status.ready {
        return Ok(None);
    }

    let mut message = String::from(
        "Browser automation is not ready yet. Use the browser tool with action='status' to confirm current state. Only run action='setup' or `jcode browser setup` for first-time install or repair when the bridge is not already ready.\n",
    );
    if !status.binary_installed {
        message.push_str("Browser bridge binary is not installed yet.\n");
    } else if status.responding && !status.compatible {
        message.push_str("Browser bridge is connected, but the live Firefox extension is missing required actions.");
        if !status.missing_actions.is_empty() {
            message.push_str(&format!(
                " Missing actions: {}.",
                status.missing_actions.join(", ")
            ));
        }
        message.push('\n');
    } else {
        message.push_str("Browser bridge binaries are installed, but the live Firefox bridge is not responding.\n");
    }
    message.push_str(
        "Normal browser tool calls will not reopen the installer automatically anymore. Do not retry browser actions until status reports ready. Continue with another available capability; if the goal requires an external capability unavailable in this session, use capability discovery.",
    );
    anyhow::bail!(message)
}

async fn execute_firefox_action(
    _provider: &FirefoxBridgeProvider,
    action: &str,
    input: &BrowserInput,
    ctx: &ToolContext,
) -> Result<ToolOutput> {
    let (bridge_action, bridge_params, title) = bridge_request(action, input)?;

    if bridge_action == "screenshot" {
        return screenshot_via_bridge(&bridge_params, title, ctx).await;
    }

    let result = firefox_run_bridge_command(&bridge_action, bridge_params, ctx).await?;
    Ok(render_browser_output(action, title, result))
}

fn bridge_request(action: &str, input: &BrowserInput) -> Result<(String, Value, String)> {
    let bridge_action = match action {
        "list_tabs" => "listTabs",
        "new_tab" => "newSession",
        "select_tab" => "setActiveTab",
        "get_active_tab" => "getActiveTab",
        "list_frames" => "listFrames",
        "open" => "navigate",
        "snapshot" => "getContent",
        "content" => "getContent",
        "get_content" => "getContent",
        "interactables" => "getInteractables",
        "click" => "click",
        "type" => "type",
        "fill_form" => "fillForm",
        "select" => "fillForm",
        "wait" => "waitFor",
        "screenshot" => "screenshot",
        "eval" => "evaluate",
        "scroll" => "scroll",
        "upload" => "uploadFile",
        "press" => "evaluate",
        "provider_command" => input.provider_method().ok_or_else(|| {
            anyhow::anyhow!("provider_action or method is required when action='provider_command'")
        })?,
        other => anyhow::bail!("Unsupported browser action: {}", other),
    }
    .to_string();

    let mut params = Map::new();
    apply_common_targeting(&mut params, input);

    match action {
        "new_tab" => {
            if let Some(url) = &input.url {
                params.insert("url".into(), json!(url));
            }
            if let Some(timeout_ms) = input.timeout_ms {
                params.insert("timeoutMs".into(), json!(timeout_ms));
            }
        }
        "select_tab" => {
            let tab_id = input
                .tab_id
                .ok_or_else(|| anyhow::anyhow!("tab_id is required for select_tab"))?;
            params.insert("tabId".into(), json!(tab_id));
            if let Some(focus) = input.focus {
                params.insert("focus".into(), json!(focus));
            }
        }
        "open" => {
            let url = input
                .url
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("url is required for open"))?;
            params.insert("url".into(), json!(url));
            params.insert("wait".into(), json!(input.wait.unwrap_or(true)));
            if let Some(new_tab) = input.new_tab {
                params.insert("newTab".into(), json!(new_tab));
            }
            if let Some(timeout_ms) = input.timeout_ms {
                params.insert("timeoutMs".into(), json!(timeout_ms));
            }
        }
        "snapshot" => {
            params.insert("format".into(), json!("annotated"));
        }
        "get_content" => {
            params.insert(
                "format".into(),
                json!(input.format.as_deref().unwrap_or("text")),
            );
        }
        "interactables" => {}
        "click" => {
            if input.selector.is_none()
                && input.text.is_none()
                && input.x.is_none()
                && input.y.is_none()
            {
                anyhow::bail!("click requires selector, text, or x/y coordinates");
            }
            if let Some(x) = input.x {
                params.insert("x".into(), json!(x));
            }
            if let Some(y) = input.y {
                params.insert("y".into(), json!(y));
            }
        }
        "type" => {
            let text = input
                .text
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("text is required for type"))?;
            params.insert("text".into(), json!(text));
            if let Some(clear) = input.clear {
                params.insert("clear".into(), json!(clear));
            }
            if let Some(submit) = input.submit {
                params.insert("submit".into(), json!(submit));
            }
        }
        "fill_form" => {
            let fields = input
                .fields
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("fields are required for fill_form"))?;
            let mapped: Vec<Value> = fields
                .iter()
                .map(|field| {
                    let mut obj = Map::new();
                    obj.insert("selector".into(), json!(field.selector));
                    if let Some(value) = &field.value {
                        obj.insert("value".into(), json!(value));
                    }
                    if let Some(checked) = field.checked {
                        obj.insert("checked".into(), json!(checked));
                    }
                    Value::Object(obj)
                })
                .collect();
            params.insert("fields".into(), Value::Array(mapped));
        }
        "select" => {
            let selector = input
                .selector
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("selector is required for select"))?;
            let value = input.text.as_deref().ok_or_else(|| {
                anyhow::anyhow!("text is required for select and is used as the option value")
            })?;
            params.insert(
                "fields".into(),
                json!([{ "selector": selector, "value": value }]),
            );
        }
        "wait" => {
            if input.selector.is_none() && input.text.is_none() && input.contains.is_none() {
                anyhow::bail!("wait requires selector, text, or contains");
            }
            if let Some(timeout_ms) = input.timeout_ms {
                params.insert("timeout".into(), json!(timeout_ms));
            }
            if let Some(contains) = &input.contains {
                params.insert("contains".into(), json!(contains));
            }
        }
        "screenshot" => {}
        "eval" => {
            let script = input
                .script
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("script is required for eval"))?;
            params.insert("script".into(), json!(script));
            if let Some(page_world) = input.page_world {
                params.insert("pageWorld".into(), json!(page_world));
            }
        }
        "scroll" => {
            if let Some(x) = input.x {
                params.insert("x".into(), json!(x));
            }
            if let Some(y) = input.y {
                params.insert("y".into(), json!(y));
            }
            if let Some(position) = &input.position {
                params.insert("position".into(), json!(position));
            }
            if let Some(behavior) = &input.behavior {
                params.insert("behavior".into(), json!(behavior));
            }
            if let Some(scroll_to) = &input.scroll_to {
                let mut target = Map::new();
                if let Some(x) = scroll_to.x {
                    target.insert("x".into(), json!(x));
                }
                if let Some(y) = scroll_to.y {
                    target.insert("y".into(), json!(y));
                }
                params.insert("scrollTo".into(), Value::Object(target));
            }
            if !params.contains_key("x")
                && !params.contains_key("y")
                && !params.contains_key("selector")
                && !params.contains_key("position")
                && !params.contains_key("scrollTo")
            {
                anyhow::bail!("scroll requires x/y, selector, position, or scroll_to");
            }
        }
        "upload" => {
            let path = input
                .path
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("path is required for upload"))?;
            // The native messaging host reads the file from `filePath`, base64-encodes
            // it, and forwards it to the content script. It also accepts an optional
            // `fileName` override. (Previously this sent `path`, which the host ignored,
            // producing a "Missing filePath" error.)
            params.insert("filePath".into(), json!(path));
            if let Some(file_name) = std::path::Path::new(path)
                .file_name()
                .and_then(|name| name.to_str())
            {
                params.insert("fileName".into(), json!(file_name));
            }
        }
        "press" => {
            let script = build_press_script(input.key.as_deref(), input.selector.as_deref())?;
            params.insert("script".into(), json!(script));
            params.insert("pageWorld".into(), json!(true));
        }
        "provider_command" => {
            if let Some(raw) = &input.params {
                return Ok((bridge_action, raw.clone(), format!("browser {}", action)));
            }
        }
        _ => {}
    }

    Ok((
        bridge_action,
        Value::Object(params),
        format!("browser {}", action),
    ))
}

fn apply_common_targeting(params: &mut Map<String, Value>, input: &BrowserInput) {
    if let Some(tab_id) = input.tab_id {
        params.insert("tabId".into(), json!(tab_id));
    }
    if let Some(window_id) = input.window_id {
        params.insert("windowId".into(), json!(window_id));
    }
    if let Some(frame_id) = input.frame_id {
        params.insert("frameId".into(), json!(frame_id));
    }
    if let Some(all_frames) = input.all_frames {
        params.insert("allFrames".into(), json!(all_frames));
    }
    if let Some(selector) = &input.selector {
        params.insert("selector".into(), json!(selector));
    }
    if let Some(text) = &input.text {
        params.insert("text".into(), json!(text));
    }
}

fn build_press_script(key: Option<&str>, selector: Option<&str>) -> Result<String> {
    let key = key.ok_or_else(|| anyhow::anyhow!("key is required for press"))?;
    let selector_literal = selector.map(serde_json::to_string).transpose()?;
    let selector_expr = selector_literal
        .map(|s| format!("document.querySelector({})", s))
        .unwrap_or_else(|| "null".to_string());
    let key_literal = serde_json::to_string(key)?;
    Ok(format!(
        r#"return (() => {{
  const target = {selector_expr} || document.activeElement || document.body;
  if (!target) throw new Error('No target available for key press');
  if (typeof target.focus === 'function') target.focus();
  const key = {key_literal};
  const eventInit = {{ key, bubbles: true, cancelable: true }};
  target.dispatchEvent(new KeyboardEvent('keydown', eventInit));
  target.dispatchEvent(new KeyboardEvent('keypress', eventInit));
  if (key === 'Enter' && target.form && typeof target.form.submit === 'function') {{
    target.form.submit();
  }}
  target.dispatchEvent(new KeyboardEvent('keyup', eventInit));
  return {{ pressed: true, key, tag: target.tagName || null }};
}})();"#
    ))
}

async fn firefox_run_bridge_command(
    action: &str,
    params: Value,
    _ctx: &ToolContext,
) -> Result<Value> {
    let bin = crate::browser::browser_binary_path();
    if !bin.exists() {
        anyhow::bail!(
            "Browser bridge binary is not installed yet. Use action='status' to confirm readiness, then run action='setup' only for first-time install or repair."
        );
    }

    let params_json = serde_json::to_string(&params)?;
    let mut command = tokio::process::Command::new(&bin);
    command.arg(action).arg(&params_json);
    command.stdin(std::process::Stdio::null());
    command.stdout(std::process::Stdio::piped());
    command.stderr(std::process::Stdio::piped());

    #[cfg(not(windows))]
    if std::env::var("BROWSER_SESSION").is_err()
        && let Some(session_name) = crate::browser::ensure_browser_session(&_ctx.session_id)
    {
        command.env("BROWSER_SESSION", session_name);
    }

    let output = command
        .output()
        .await
        .with_context(|| format!("Failed to run browser bridge action '{}'.", action))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if !output.status.success() {
        let details = if stderr.is_empty() {
            stdout
        } else if stdout.is_empty() {
            stderr
        } else {
            format!("{}\n{}", stderr, stdout)
        };
        if details.contains("Unknown action:") {
            anyhow::bail!(
                "The connected Firefox browser bridge is missing required support for action '{}'. This usually means the installed extension is older than the browser CLI expected by jcode. Use browser action='status' to confirm, then action='setup' to repair or update the extension.\n\nOriginal bridge error: {}",
                action,
                details
            );
        }
        anyhow::bail!(details);
    }

    if stdout.is_empty() {
        return Ok(json!({ "ok": true }));
    }

    serde_json::from_str(&stdout).or_else(|_| Ok(json!({ "raw": stdout })))
}

async fn screenshot_via_bridge(
    params: &Value,
    title: String,
    ctx: &ToolContext,
) -> Result<ToolOutput> {
    let filename = temp_screenshot_path();
    let mut screenshot_params = params.clone();
    if let Some(map) = screenshot_params.as_object_mut() {
        map.insert(
            "filename".into(),
            json!(filename.to_string_lossy().to_string()),
        );
    }

    let result = firefox_run_bridge_command("screenshot", screenshot_params, ctx).await?;
    let saved = result
        .get("saved")
        .and_then(|v| v.as_str())
        .map(PathBuf::from)
        .unwrap_or(filename);

    let mut output = ToolOutput::new(format!(
        "Captured browser screenshot to {}.",
        saved.display()
    ))
    .with_title(title)
    .with_metadata(result.clone());

    if let Ok(bytes) = tokio::fs::read(&saved).await {
        output = output.with_labeled_image(
            "image/png",
            STANDARD.encode(&bytes),
            format!("browser screenshot: {}", saved.display()),
        );
        let _ = tokio::fs::remove_file(&saved).await;
    }

    Ok(output)
}

fn temp_screenshot_path() -> PathBuf {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("jcode-browser-{}.png", ts))
}

fn render_browser_output(action: &str, title: String, result: Value) -> ToolOutput {
    let body = match action {
        "snapshot" => result
            .get("content")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| serde_json::to_string_pretty(&result).unwrap_or_default()),
        "get_content" => format_content_result(&result),
        "interactables" => format_interactables_result(&result),
        "eval" => format_eval_result(&result),
        _ => serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string()),
    };

    ToolOutput::new(body)
        .with_title(title)
        .with_metadata(result)
}

fn format_content_result(result: &Value) -> String {
    if let Some(content) = result.get("content").and_then(|v| v.as_str()) {
        return content.to_string();
    }
    if let Some(text) = result.get("text").and_then(|v| v.as_str()) {
        return text.to_string();
    }
    if let Some(html) = result.get("html").and_then(|v| v.as_str()) {
        return html.to_string();
    }
    if let Some(title) = result.get("title").and_then(|v| v.as_str()) {
        if let Some(url) = result.get("url").and_then(|v| v.as_str()) {
            return format!("{}\n{}", title, url);
        }
        return title.to_string();
    }
    serde_json::to_string_pretty(result).unwrap_or_default()
}

fn format_eval_result(result: &Value) -> String {
    let value = result.get("result").cloned().unwrap_or(Value::Null);
    let rendered = if let Some(s) = value.as_str() {
        s.to_string()
    } else {
        serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string())
    };

    match result.get("type").and_then(|v| v.as_str()) {
        Some(kind) => format!("{}\n\n(type: {})", rendered, kind),
        None => rendered,
    }
}

fn format_interactables_result(result: &Value) -> String {
    let Some(elements) = result.get("elements").and_then(|v| v.as_array()) else {
        return serde_json::to_string_pretty(result).unwrap_or_default();
    };

    if elements.is_empty() {
        return "No interactable elements found.".to_string();
    }

    let mut lines = Vec::new();
    for (idx, element) in elements.iter().enumerate() {
        let kind = element
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("element");
        let tag = element.get("tag").and_then(|v| v.as_str()).unwrap_or("?");
        let text = element
            .get("text")
            .or_else(|| element.get("label"))
            .or_else(|| element.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let selector = element
            .get("selector")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        lines.push(format!(
            "{}. [{}] <{}> {} | selector: {}",
            idx + 1,
            kind,
            tag.to_lowercase(),
            text,
            selector
        ));
    }

    lines.join("\n")
}

#[cfg(test)]
#[path = "browser_tests.rs"]
mod browser_tests;
