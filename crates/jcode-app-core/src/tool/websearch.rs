use super::{Tool, ToolContext, ToolOutput};
use crate::config::{WebSearchConfig, WebSearchEngine};
use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

/// Web search using Exa, DuckDuckGo, Bing (HTML scraping, optional Bing API) or SearXNG.
pub struct WebSearchTool {
    client: reqwest::Client,
}

impl WebSearchTool {
    pub fn new() -> Self {
        Self {
            client: crate::provider::shared_http_client(),
        }
    }
}

#[derive(Deserialize)]
struct WebSearchInput {
    query: String,
    #[serde(default)]
    num_results: Option<usize>,
    #[serde(default)]
    engine: Option<WebSearchEngine>,
    #[serde(default)]
    bing_market: Option<String>,
}

#[derive(Debug)]
struct SearchResult {
    title: String,
    url: String,
    snippet: String,
}

/// Results returned by one engine, together with the provenance the caller
/// reports back to the model (which engine answered, and any request id the
/// provider returned).
#[derive(Debug)]
struct EngineSearch {
    engine: WebSearchEngine,
    request_id: Option<String>,
    results: Vec<SearchResult>,
}

impl EngineSearch {
    fn new(engine: WebSearchEngine, results: Vec<SearchResult>) -> Self {
        Self {
            engine,
            request_id: None,
            results,
        }
    }
}

#[derive(Clone, Copy)]
struct BingSearchOptions<'a> {
    market: &'a str,
    configured_api_key: Option<&'a str>,
    api_key_env: &'a str,
}

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "websearch"
    }

    fn description(&self) -> &str {
        "Search the web."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["query"],
            "properties": {
                "intent": super::intent_schema_property(),
                "query": {
                    "type": "string",
                    "description": "Search query."
                },
                "num_results": {
                    "type": "integer",
                    "description": "Max results."
                },
                "engine": {
                    "type": "string",
                    "enum": ["exa", "duckduckgo", "bing", "searxng"],
                    "description": "Engine. Defaults to exa; exa uses JCODE_EXA_API_KEY/EXA_API_KEY, bing uses JCODE_BING_API_KEY, searxng uses JCODE_SEARXNG_URL."
                },
                "bing_market": {
                    "type": "string",
                    "description": "Optional Bing market, e.g. en-US or zh-CN. Defaults to JCODE_BING_MARKET or en-US."
                }
            }
        })
    }

    async fn execute(&self, input: Value, _ctx: ToolContext) -> Result<ToolOutput> {
        let params: WebSearchInput = serde_json::from_value(input)?;
        let num_results = params.num_results.unwrap_or(8).min(20);

        let config = crate::config::config();
        // A missing API key on the *primary* engine is a configuration error, not
        // a transient engine failure: fail loudly instead of silently answering
        // from a fallback engine.
        let primary_engine = params.engine.unwrap_or(config.websearch.engine);
        if primary_engine == WebSearchEngine::Exa {
            resolve_exa_api_key(&config.websearch)?;
        }

        let mut engines = Vec::new();
        engines.push(primary_engine);
        engines.extend(config.websearch.fallback_engines.iter().copied());
        engines.dedup();

        let market = params
            .bing_market
            .as_deref()
            .unwrap_or(&config.websearch.bing_market);
        let mut last_error = None;
        let mut search = None;
        for (index, engine) in engines.into_iter().enumerate() {
            let allow_bing_api = index == 0;
            match self
                .search_with_engine(
                    engine,
                    &params.query,
                    num_results,
                    BingSearchOptions {
                        market,
                        configured_api_key: config.websearch.bing_api_key.as_deref(),
                        api_key_env: &config.websearch.bing_api_key_env,
                    },
                    allow_bing_api,
                )
                .await
            {
                Ok(found) => {
                    if !found.results.is_empty() {
                        search = Some(found);
                        break;
                    }
                }
                Err(err) => last_error = Some(err),
            }
        }

        let Some(search) = search else {
            if let Some(err) = last_error {
                return Err(err);
            }
            return Ok(ToolOutput::new(format!(
                "No results found for: {}\n\n\
                 If results are consistently empty on this machine, the keyless \
                 DuckDuckGo/Bing engines may be blocked here by TLS fingerprinting \
                 or IP reputation (common on Linux/servers). Workarounds:\n\
                 - Configure an Exa API key (EXA_API_KEY) and use engine \"exa\".\n\
                 - Point at a SearXNG instance: set `websearch.searxng_url` (or \
                 JCODE_SEARXNG_URL) and use engine \"searxng\".\n\
                 - Or provide a Bing Search API key via JCODE_BING_API_KEY.",
                params.query
            )));
        };

        let mut output = format!("Search results for: {}\n\n", params.query);

        for (i, result) in search.results.iter().enumerate() {
            output.push_str(&format!(
                "{}. **{}**\n   {}\n   {}\n\n",
                i + 1,
                result.title,
                result.url,
                result.snippet
            ));
        }

        output.push_str(&format!("provider: {}", search.engine));
        if let Some(request_id) = search.request_id.as_deref() {
            output.push_str(&format!(" (requestId {request_id})"));
        }
        output.push('\n');

        Ok(ToolOutput::new(output))
    }
}

impl WebSearchTool {
    async fn search_with_engine(
        &self,
        engine: WebSearchEngine,
        query: &str,
        num_results: usize,
        bing: BingSearchOptions<'_>,
        allow_bing_api: bool,
    ) -> Result<EngineSearch> {
        match engine {
            WebSearchEngine::Duckduckgo => Ok(EngineSearch::new(
                engine,
                self.search_duckduckgo(query, num_results).await?,
            )),
            WebSearchEngine::Bing => Ok(EngineSearch::new(
                engine,
                self.search_bing(query, num_results, bing, allow_bing_api)
                    .await?,
            )),
            WebSearchEngine::Searxng => Ok(EngineSearch::new(
                engine,
                self.search_searxng(query, num_results).await?,
            )),
            WebSearchEngine::Exa => self.search_exa(query, num_results).await,
        }
    }

    async fn search_duckduckgo(
        &self,
        query: &str,
        num_results: usize,
    ) -> Result<Vec<SearchResult>> {
        // DuckDuckGo's HTML endpoint now serves an anti-bot "anomaly" challenge
        // (HTTP 202, no results) for plain GET requests. Submitting the query as
        // a POST form, the same way the real HTML page does, still returns the
        // standard results markup with a 200.
        let response = self
            .client
            .post("https://html.duckduckgo.com/html/")
            .header(
                reqwest::header::USER_AGENT,
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
            )
            .header(reqwest::header::ACCEPT, "text/html,application/xhtml+xml")
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .form(&[("q", query), ("kl", "us-en")])
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Search failed with status: {}",
                response.status()
            ));
        }

        let body = response.text().await?;
        let results = parse_ddg_results(&body, num_results);
        if results.is_empty()
            && let Some(reason) = detect_anti_bot_page(&body)
        {
            return Err(anyhow::anyhow!(
                "DuckDuckGo served an anti-bot challenge page ({reason}) instead of \
                 results. This is commonly caused by TLS fingerprinting or IP \
                 reputation on Linux. Falling back to another engine if configured."
            ));
        }

        Ok(results)
    }

    async fn search_bing(
        &self,
        query: &str,
        num_results: usize,
        options: BingSearchOptions<'_>,
        allow_api: bool,
    ) -> Result<Vec<SearchResult>> {
        if allow_api {
            if let Some(api_key) = options
                .configured_api_key
                .filter(|key| !key.trim().is_empty())
            {
                return self
                    .search_bing_api(query, num_results, options.market, api_key)
                    .await;
            }
            if let Ok(api_key) = std::env::var(options.api_key_env)
                && !api_key.trim().is_empty()
            {
                return self
                    .search_bing_api(query, num_results, options.market, &api_key)
                    .await;
            }
        }

        self.search_bing_html(query, num_results, options.market)
            .await
    }

    async fn search_bing_api(
        &self,
        query: &str,
        num_results: usize,
        market: &str,
        api_key: &str,
    ) -> Result<Vec<SearchResult>> {
        let response = self
            .client
            .get("https://api.bing.microsoft.com/v7.0/search")
            .query(&[
                ("q", query),
                ("count", &num_results.to_string()),
                ("mkt", market),
            ])
            .header("Ocp-Apim-Subscription-Key", api_key)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Bing API search failed with status: {}",
                response.status()
            ));
        }

        Ok(parse_bing_api_results(response.json().await?, num_results))
    }

    async fn search_bing_html(
        &self,
        query: &str,
        num_results: usize,
        market: &str,
    ) -> Result<Vec<SearchResult>> {
        let url = format!(
            "https://www.bing.com/search?q={}&mkt={}",
            urlencoding::encode(query),
            urlencoding::encode(market)
        );

        let response = self
            .client
            .get(&url)
            .header(
                reqwest::header::USER_AGENT,
                "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36",
            )
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Bing search failed with status: {}",
                response.status()
            ));
        }

        let body = response.text().await?;
        let results = parse_bing_html_results(&body, num_results);
        if results.is_empty()
            && let Some(reason) = detect_anti_bot_page(&body)
        {
            return Err(anyhow::anyhow!(
                "Bing served an anti-bot challenge page ({reason}) instead of results."
            ));
        }

        Ok(results)
    }

    /// Query a user-configured SearXNG instance via its JSON API. SearXNG is a
    /// self-hostable metasearch engine; because the request goes to an instance
    /// the user controls (or a public one they trust), it sidesteps the TLS
    /// fingerprinting / IP-reputation blocks that DuckDuckGo and Bing apply to
    /// scraped requests on some hosts (see issue #270).
    async fn search_searxng(&self, query: &str, num_results: usize) -> Result<Vec<SearchResult>> {
        let config = crate::config::config();
        let base = config
            .websearch
            .searxng_url
            .as_deref()
            .filter(|u| !u.trim().is_empty())
            .map(|u| u.to_string())
            .or_else(|| {
                std::env::var(&config.websearch.searxng_url_env)
                    .ok()
                    .filter(|u| !u.trim().is_empty())
            })
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "SearXNG engine selected but no instance URL configured. Set \
                     `websearch.searxng_url` in your config or the {} environment \
                     variable to a SearXNG base URL (e.g. https://searx.example.org).",
                    config.websearch.searxng_url_env
                )
            })?;

        let endpoint = format!("{}/search", base.trim_end_matches('/'));
        let response = self
            .client
            .get(&endpoint)
            .query(&[("q", query), ("format", "json")])
            .header(
                reqwest::header::USER_AGENT,
                "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36",
            )
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "SearXNG search failed with status {} (endpoint: {endpoint}). \
                 Ensure the instance has the JSON format enabled in its settings.",
                response.status()
            ));
        }

        let parsed: SearxngResponse = response.json().await.map_err(|err| {
            anyhow::anyhow!(
                "SearXNG returned a non-JSON response ({err}). The instance may have \
                 the JSON format disabled; enable `formats: [html, json]` in its settings."
            )
        })?;

        Ok(parse_searxng_results(parsed, num_results))
    }

    /// Query the Exa search API (https://api.exa.ai/search). Exa is a hosted
    /// semantic search API: it needs an API key, but in exchange it is not
    /// subject to the scraping blocks that hit the keyless HTML engines.
    async fn search_exa(&self, query: &str, num_results: usize) -> Result<EngineSearch> {
        let config = crate::config::config();
        let api_key = resolve_exa_api_key(&config.websearch)?;

        let response = self
            .client
            .post(EXA_SEARCH_ENDPOINT)
            .header("x-api-key", api_key)
            .json(&exa_request_body(query, num_results))
            .send()
            .await?;

        let status = response.status().as_u16();
        let body = response.text().await.map_err(|err| {
            anyhow::anyhow!(
                "Exa search failed with status {status}: could not read the response body ({err})"
            )
        })?;
        parse_exa_http_response(status, &body, num_results)
    }
}

const EXA_SEARCH_ENDPOINT: &str = "https://api.exa.ai/search";

/// Resolve the Exa API key: the explicit config value wins, then the configured
/// environment variable (default `EXA_API_KEY`), then the `exa.env` credential
/// file — the same lookup providers use. The key is never logged.
fn resolve_exa_api_key(config: &WebSearchConfig) -> Result<String> {
    if let Some(key) = config
        .exa_api_key
        .as_deref()
        .map(str::trim)
        .filter(|key| !key.is_empty())
    {
        return Ok(key.to_string());
    }

    crate::provider_catalog::load_api_key_from_env_or_config(&config.exa_api_key_env, "exa.env")
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Exa engine selected but no API key is available. Set `websearch.exa_api_key` \
                 in your config, or export the {} environment variable (the PC001 launcher \
                 fetches it from the Doppler `infra/all` secret).",
                config.exa_api_key_env
            )
        })
}

/// Request body for `POST /search`. `type: "auto"` lets Exa pick the search
/// mode; `contents.highlights` asks for the query-relevant snippets we show as
/// result text.
fn exa_request_body(query: &str, num_results: usize) -> Value {
    json!({
        "query": query,
        "type": "auto",
        "numResults": num_results,
        "contents": { "highlights": true }
    })
}

/// Turn a raw Exa HTTP response into an [`EngineSearch`]. Kept separate from the
/// request so the success, error-status and malformed-body paths are all
/// unit-testable without touching the network.
fn parse_exa_http_response(status: u16, body: &str, max_results: usize) -> Result<EngineSearch> {
    if !(200..300).contains(&status) {
        let detail = truncate_error_detail(body);
        let detail = if detail.is_empty() {
            String::new()
        } else {
            format!(": {}", jcode_base::message::redact_secrets(&detail))
        };
        return Err(anyhow::anyhow!(
            "Exa search failed with status {status}{detail}"
        ));
    }

    let mut parsed: ExaResponse = serde_json::from_str(body)
        .map_err(|err| anyhow::anyhow!("Exa returned a non-JSON search response ({err})"))?;

    let request_id = parsed
        .request_id
        .take()
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty());

    Ok(EngineSearch {
        engine: WebSearchEngine::Exa,
        request_id,
        results: parse_exa_results(parsed, max_results),
    })
}

/// Keep provider error bodies short: they are surfaced to the user (and the
/// model) and are usually a one-line JSON error or an HTML page.
fn truncate_error_detail(body: &str) -> String {
    const MAX: usize = 300;
    let trimmed = body.trim();
    match trimmed.char_indices().nth(MAX) {
        Some((index, _)) => format!("{}…", &trimmed[..index]),
        None => trimmed.to_string(),
    }
}

/// Map a parsed Exa response to `SearchResult`s: the snippet is the joined
/// highlights, entries without a URL are dropped, and the list is capped.
fn parse_exa_results(response: ExaResponse, max_results: usize) -> Vec<SearchResult> {
    response
        .results
        .into_iter()
        .filter(|result| !result.url.trim().is_empty())
        .take(max_results)
        .map(|result| {
            let snippet = result
                .highlights
                .into_iter()
                .map(|highlight| highlight.trim().to_string())
                .filter(|highlight| !highlight.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            let title = if result.title.trim().is_empty() {
                result.url.clone()
            } else {
                result.title
            };
            SearchResult {
                title,
                url: result.url,
                snippet: truncate_snippet(&snippet),
            }
        })
        .collect()
}

/// Exa highlights are whole excerpts and can run to several kilobytes each
/// (a documentation page easily yields 4 KB per highlight). Tool output lands
/// in the model's context, so keep snippets comparable to the HTML engines'.
fn truncate_snippet(snippet: &str) -> String {
    const MAX_SNIPPET_CHARS: usize = 1200;
    match snippet.char_indices().nth(MAX_SNIPPET_CHARS) {
        Some((index, _)) => format!("{}…", &snippet[..index]),
        None => snippet.to_string(),
    }
}

/// Map a parsed SearXNG JSON response to `SearchResult`s, dropping entries with
/// empty URLs and capping to `num_results`.
fn parse_searxng_results(response: SearxngResponse, num_results: usize) -> Vec<SearchResult> {
    response
        .results
        .into_iter()
        .filter(|r| !r.url.trim().is_empty())
        .take(num_results)
        .map(|r| SearchResult {
            title: if r.title.trim().is_empty() {
                r.url.clone()
            } else {
                r.title
            },
            url: r.url,
            snippet: r.content.unwrap_or_default(),
        })
        .collect()
}

mod search_regex {
    use regex::Regex;
    use std::sync::OnceLock;

    fn compile_regex(pattern: &str, label: &str) -> Option<Regex> {
        match Regex::new(pattern) {
            Ok(regex) => Some(regex),
            Err(err) => {
                crate::logging::warn(&format!(
                    "websearch: failed to compile static regex {label}: {}",
                    err
                ));
                None
            }
        }
    }

    macro_rules! static_regex {
        ($name:ident, $pat:expr_2021) => {
            pub fn $name() -> Option<&'static Regex> {
                static RE: OnceLock<Option<Regex>> = OnceLock::new();
                RE.get_or_init(|| compile_regex($pat, stringify!($name)))
                    .as_ref()
            }
        };
    }

    static_regex!(
        result_link,
        r#"(?s)<a[^>]*class="result__a"[^>]*href="([^"]*)"[^>]*>(.*?)</a>"#
    );
    static_regex!(
        result_snippet,
        r#"(?s)<a[^>]*class="result__snippet"[^>]*>(.*?)</a>"#
    );
    static_regex!(tag, r"<[^>]+>");
    static_regex!(
        bing_result_block,
        r#"(?s)<li[^>]*class="[^"]*\bb_algo\b[^"]*"[^>]*>(.*?)</li>"#
    );
    static_regex!(
        bing_link,
        r#"(?s)<h2[^>]*>\s*<a[^>]*href="([^"]+)"[^>]*>(.*?)</a>\s*</h2>"#
    );
    static_regex!(
        bing_caption,
        r#"(?s)<div[^>]*class="[^"]*\bb_caption\b[^"]*"[^>]*>.*?<p[^>]*>(.*?)</p>"#
    );
}

#[derive(Deserialize)]
struct SearxngResponse {
    #[serde(default)]
    results: Vec<SearxngResult>,
}

/// Shape of a `POST https://api.exa.ai/search` response.
#[derive(Deserialize)]
struct ExaResponse {
    #[serde(default, rename = "requestId")]
    request_id: Option<String>,
    #[serde(default)]
    results: Vec<ExaResult>,
}

#[derive(Deserialize)]
struct ExaResult {
    #[serde(default)]
    title: String,
    #[serde(default)]
    url: String,
    /// Query-relevant snippets, requested via `contents.highlights`.
    #[serde(default)]
    highlights: Vec<String>,
}

#[derive(Deserialize)]
struct SearxngResult {
    #[serde(default)]
    title: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    content: Option<String>,
}

#[derive(Deserialize)]
struct BingApiResponse {
    #[serde(rename = "webPages")]
    web_pages: Option<BingWebPages>,
}

#[derive(Deserialize)]
struct BingWebPages {
    value: Vec<BingWebPage>,
}

#[derive(Deserialize)]
struct BingWebPage {
    name: String,
    url: String,
    #[serde(default)]
    snippet: String,
}

fn parse_bing_api_results(response: BingApiResponse, max_results: usize) -> Vec<SearchResult> {
    response
        .web_pages
        .map(|pages| {
            pages
                .value
                .into_iter()
                .take(max_results)
                .map(|page| SearchResult {
                    title: page.name,
                    url: page.url,
                    snippet: page.snippet,
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_bing_html_results(html: &str, max_results: usize) -> Vec<SearchResult> {
    let mut results = Vec::new();
    let (Some(block_re), Some(link_re), Some(caption_re), Some(tag_re)) = (
        search_regex::bing_result_block(),
        search_regex::bing_link(),
        search_regex::bing_caption(),
        search_regex::tag(),
    ) else {
        return results;
    };

    for block in block_re.captures_iter(html) {
        if results.len() >= max_results {
            break;
        }
        let Some(link) = link_re.captures(&block[1]) else {
            continue;
        };
        let url = html_decode(&link[1]);
        if !url.starts_with("http") || url.contains("bing.com") {
            continue;
        }
        let title = html_decode(&tag_re.replace_all(&link[2], ""));
        let snippet = caption_re
            .captures(&block[1])
            .map(|cap| html_decode(&tag_re.replace_all(&cap[1], "")))
            .unwrap_or_default();
        results.push(SearchResult {
            title,
            url,
            snippet,
        });
    }

    results
}

fn parse_ddg_results(html: &str, max_results: usize) -> Vec<SearchResult> {
    let mut results = Vec::new();

    let (Some(result_link), Some(result_snippet), Some(tag)) = (
        search_regex::result_link(),
        search_regex::result_snippet(),
        search_regex::tag(),
    ) else {
        return results;
    };

    let links: Vec<_> = result_link.captures_iter(html).collect();
    let snippets: Vec<_> = result_snippet.captures_iter(html).collect();

    for (i, link_cap) in links.iter().enumerate() {
        if results.len() >= max_results {
            break;
        }

        let url = decode_ddg_url(&link_cap[1]);
        let title = html_decode(&tag.replace_all(&link_cap[2], ""));

        if !url.starts_with("http") || url.contains("duckduckgo.com") {
            continue;
        }

        let snippet = if i < snippets.len() {
            let raw = &snippets[i][1];
            html_decode(&tag.replace_all(raw, ""))
        } else {
            String::new()
        };

        results.push(SearchResult {
            title,
            url,
            snippet,
        });
    }

    results
}

/// Detect whether an HTML body is an anti-bot/captcha challenge rather than a
/// real results page. DuckDuckGo (and similar) serve these with HTTP 200, so a
/// successful status plus zero parsed results is ambiguous without this check.
///
/// Returns a short human-readable reason when a challenge page is detected.
fn detect_anti_bot_page(html: &str) -> Option<&'static str> {
    let lowered = html.to_ascii_lowercase();
    const MARKERS: &[(&str, &str)] = &[
        ("anomaly-modal", "anomaly challenge"),
        ("anomaly.js", "anomaly challenge"),
        ("dpn=1", "anomaly challenge"),
        ("captcha", "captcha"),
        ("g-recaptcha", "recaptcha"),
        ("are you a robot", "bot check"),
        ("unusual traffic", "bot check"),
        ("verify you are human", "human verification"),
        ("challenge-platform", "cloudflare challenge"),
        ("cf-challenge", "cloudflare challenge"),
    ];
    for (needle, reason) in MARKERS {
        if lowered.contains(needle) {
            return Some(reason);
        }
    }
    None
}

fn decode_ddg_url(url: &str) -> String {
    // DDG wraps URLs like //duckduckgo.com/l/?uddg=ACTUAL_URL&...
    if let Some(uddg_start) = url.find("uddg=") {
        let start = uddg_start + 5;
        let end = url[start..]
            .find('&')
            .map(|i| start + i)
            .unwrap_or(url.len());
        let encoded = &url[start..end];
        urlencoding::decode(encoded)
            .map(|s| s.to_string())
            .unwrap_or_else(|_| encoded.to_string())
    } else {
        url.to_string()
    }
}

fn html_decode(s: &str) -> String {
    s.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&apos;", "'")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bing_html_results() {
        let html = r#"
            <li class="b_algo">
              <h2><a href="https://example.com/rust">Rust &amp; Cargo</a></h2>
              <div class="b_caption"><p>A <strong>systems</strong> language.</p></div>
            </li>
            <li class="b_algo"><h2><a href="https://www.bing.com/aclk">ad</a></h2></li>
            <li class="b_algo">
              <h2><a href="https://example.org/jcode">Jcode</a></h2>
              <div class="b_caption"><p>Agentic coding.</p></div>
            </li>
        "#;

        let results = parse_bing_html_results(html, 10);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Rust & Cargo");
        assert_eq!(results[0].url, "https://example.com/rust");
        assert_eq!(results[0].snippet, "A systems language.");
        assert_eq!(results[1].title, "Jcode");
    }

    #[test]
    fn parses_bing_api_results() {
        let response: BingApiResponse = serde_json::from_value(json!({
            "webPages": {
                "value": [
                    {"name": "One", "url": "https://one.test", "snippet": "first"},
                    {"name": "Two", "url": "https://two.test", "snippet": "second"}
                ]
            }
        }))
        .unwrap();

        let results = parse_bing_api_results(response, 1);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "One");
        assert_eq!(results[0].url, "https://one.test");
    }

    #[test]
    fn parses_ddg_html_results() {
        // Mirrors the markup html.duckduckgo.com returns for the POST form,
        // where titles and snippets contain inline <b> highlight tags.
        let html = r#"
            <div class="result results_links results_links_deep web-result">
              <a class="result__a" href="https://rust-lang.org/"><b>Rust</b> Language</a>
              <a class="result__snippet" href="https://rust-lang.org/">A <b>systems</b> programming language.</a>
            </div>
            <div class="result results_links results_links_deep web-result">
              <a class="result__a" href="https://en.wikipedia.org/wiki/Rust">Rust on Wikipedia</a>
              <a class="result__snippet" href="https://en.wikipedia.org/wiki/Rust">Encyclopedia <b>entry</b>.</a>
            </div>
        "#;

        let results = parse_ddg_results(html, 10);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Rust Language");
        assert_eq!(results[0].url, "https://rust-lang.org/");
        assert_eq!(results[0].snippet, "A systems programming language.");
        assert_eq!(results[1].url, "https://en.wikipedia.org/wiki/Rust");
        assert_eq!(results[1].snippet, "Encyclopedia entry.");
    }

    #[test]
    fn websearch_engine_accepts_aliases() {
        assert_eq!(
            WebSearchEngine::parse("ddg"),
            Some(WebSearchEngine::Duckduckgo)
        );
        assert_eq!(WebSearchEngine::parse("bing"), Some(WebSearchEngine::Bing));
        assert_eq!(WebSearchEngine::parse("google"), None);
    }

    #[test]
    fn detects_ddg_anomaly_challenge_page() {
        // Shape of the anti-bot challenge DDG serves (HTTP 200) instead of
        // results when a request is flagged (e.g. TLS fingerprint on Linux).
        let html = r#"<!DOCTYPE html><html><head>
            <script src="/dist/anomaly.js"></script></head>
            <body><div class="anomaly-modal__title">Unfortunately, bots use DuckDuckGo too.</div>
            </body></html>"#;
        assert_eq!(detect_anti_bot_page(html), Some("anomaly challenge"));
        // And it should parse to zero real results.
        assert!(parse_ddg_results(html, 10).is_empty());
    }

    #[test]
    fn detects_generic_captcha_page() {
        let html = r#"<html><body><div class="g-recaptcha"></div>
            Please verify you are human.</body></html>"#;
        assert!(detect_anti_bot_page(html).is_some());
    }

    #[test]
    fn real_results_are_not_flagged_as_anti_bot() {
        let html = r#"
            <div class="result results_links web-result">
              <a class="result__a" href="https://rust-lang.org/">Rust</a>
              <a class="result__snippet" href="https://rust-lang.org/">A language.</a>
            </div>
        "#;
        assert_eq!(detect_anti_bot_page(html), None);
        assert_eq!(parse_ddg_results(html, 10).len(), 1);
    }

    // Captured from a live DuckDuckGo request that was flagged on Linux (GH #270):
    // the HTML endpoint returns HTTP 202 with an "anomaly" challenge page and no
    // results. These fixtures pin the real-world shapes so the fix stays honest.
    #[test]
    fn real_captured_ddg_anomaly_fixture_is_detected() {
        let html = include_str!("testdata/ddg_anomaly.html");
        // The bug: this page parses to zero real results...
        assert!(
            parse_ddg_results(html, 10).is_empty(),
            "anomaly page should yield no results"
        );
        // ...but the fix now recognizes it as a challenge instead of a silent
        // "no results found".
        assert_eq!(detect_anti_bot_page(html), Some("anomaly challenge"));
    }

    #[test]
    fn real_captured_ddg_results_fixture_parses() {
        let html = include_str!("testdata/ddg_results.html");
        assert_eq!(detect_anti_bot_page(html), None);
        assert!(
            !parse_ddg_results(html, 10).is_empty(),
            "real results page should yield results"
        );
    }

    #[test]
    fn parses_searxng_json_results() {
        // Shape of a real SearXNG /search?format=json response (#270).
        let body = serde_json::json!({
            "query": "rust",
            "results": [
                {
                    "url": "https://www.rust-lang.org/",
                    "title": "Rust Programming Language",
                    "content": "A language empowering everyone."
                },
                {
                    "url": "https://doc.rust-lang.org/book/",
                    "title": "The Rust Book",
                    "content": "Learn Rust."
                },
                // Entry with empty url is dropped; missing content tolerated.
                { "url": "", "title": "junk" },
                { "url": "https://crates.io", "title": "" }
            ]
        });
        let parsed: SearxngResponse = serde_json::from_value(body).unwrap();
        let results = parse_searxng_results(parsed, 10);
        assert_eq!(results.len(), 3, "empty-url entry should be dropped");
        assert_eq!(results[0].url, "https://www.rust-lang.org/");
        assert_eq!(results[0].title, "Rust Programming Language");
        assert_eq!(results[0].snippet, "A language empowering everyone.");
        // Missing title falls back to the URL.
        assert_eq!(results[2].title, "https://crates.io");
        assert_eq!(results[2].snippet, "");
    }

    #[test]
    fn searxng_results_respect_limit() {
        let body = serde_json::json!({
            "results": (0..10)
                .map(|i| serde_json::json!({"url": format!("https://x/{i}"), "title": "t"}))
                .collect::<Vec<_>>()
        });
        let parsed: SearxngResponse = serde_json::from_value(body).unwrap();
        assert_eq!(parse_searxng_results(parsed, 3).len(), 3);
    }

    #[test]
    fn websearch_engine_parses_searxng_aliases() {
        assert_eq!(
            WebSearchEngine::parse("searxng"),
            Some(WebSearchEngine::Searxng)
        );
        assert_eq!(
            WebSearchEngine::parse("searx"),
            Some(WebSearchEngine::Searxng)
        );
        assert_eq!(WebSearchEngine::Searxng.as_str(), "searxng");
    }

    #[test]
    fn exa_request_body_asks_for_highlights() {
        let body = exa_request_body("rust async", 5);
        assert_eq!(body["query"], "rust async");
        assert_eq!(body["type"], "auto");
        assert_eq!(body["numResults"], 5);
        assert_eq!(body["contents"]["highlights"], true);
    }

    #[test]
    fn parses_exa_search_response() {
        // Shape of a real Exa /search response (highlights requested).
        let body = r#"{
            "requestId": "01a1b2c3-deadbeef",
            "results": [
                {
                    "title": "Exa — The Search Engine for AI",
                    "url": "https://exa.ai/",
                    "highlights": ["Exa is a search engine built for AI.", "Semantic search."]
                },
                {
                    "url": "https://docs.exa.ai/reference/search",
                    "highlights": []
                },
                { "title": "no url", "url": "  ", "highlights": ["dropped"] }
            ]
        }"#;

        let search = parse_exa_http_response(200, body, 10).unwrap();
        assert_eq!(search.engine, WebSearchEngine::Exa);
        assert_eq!(search.request_id.as_deref(), Some("01a1b2c3-deadbeef"));
        assert_eq!(search.results.len(), 2, "entry without url is dropped");
        assert_eq!(search.results[0].title, "Exa — The Search Engine for AI");
        assert_eq!(search.results[0].url, "https://exa.ai/");
        assert_eq!(
            search.results[0].snippet,
            "Exa is a search engine built for AI. Semantic search."
        );
        // Missing title falls back to the URL, missing highlights to "".
        assert_eq!(
            search.results[1].title,
            "https://docs.exa.ai/reference/search"
        );
        assert_eq!(search.results[1].snippet, "");
    }

    #[test]
    fn exa_results_respect_limit() {
        let results = (0..10)
            .map(|i| {
                json!({
                    "title": format!("t{i}"),
                    "url": format!("https://x/{i}"),
                    "highlights": ["h"]
                })
            })
            .collect::<Vec<_>>();
        let body = json!({ "results": results }).to_string();
        let search = parse_exa_http_response(200, &body, 3).unwrap();
        assert_eq!(search.results.len(), 3);
        assert_eq!(search.request_id, None);
    }

    #[test]
    fn exa_highlights_are_capped_to_a_snippet() {
        // Real Exa highlights are whole excerpts: a docs page returned ~4 KB.
        let long = "x".repeat(5000);
        let body = json!({
            "results": [{ "title": "t", "url": "https://x/1", "highlights": [long] }]
        })
        .to_string();
        let search = parse_exa_http_response(200, &body, 5).unwrap();
        let snippet = &search.results[0].snippet;
        assert_eq!(
            snippet.chars().count(),
            1201,
            "1200 chars plus the ellipsis"
        );
        assert!(snippet.ends_with('…'));
        // Multi-byte characters must not be split.
        let body = json!({
            "results": [{ "url": "https://x/2", "highlights": ["é".repeat(2000)] }]
        })
        .to_string();
        let search = parse_exa_http_response(200, &body, 5).unwrap();
        assert!(search.results[0].snippet.ends_with('…'));
    }

    #[test]
    fn exa_error_status_is_reported() {
        let err = parse_exa_http_response(401, r#"{"error":"Invalid API key"}"#, 5)
            .expect_err("a 401 must not parse as results");
        let message = err.to_string();
        assert!(message.contains("401"), "status missing from: {message}");
        assert!(
            message.contains("Invalid API key"),
            "provider detail missing from: {message}"
        );
    }

    #[test]
    fn exa_non_json_body_is_reported() {
        let err = parse_exa_http_response(200, "<html>gateway</html>", 5)
            .expect_err("a non-JSON body must not parse as results");
        assert!(
            err.to_string().contains("non-JSON"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn missing_exa_key_is_a_clear_error() {
        // A surely-unset variable name keeps the lookup off the local `exa.env`.
        let config = WebSearchConfig {
            exa_api_key: None,
            exa_api_key_env: "JCODE_TEST_UNSET_EXA_API_KEY".to_string(),
            ..WebSearchConfig::default()
        };
        let err = resolve_exa_api_key(&config).expect_err("no key available");
        let message = err.to_string();
        assert!(message.contains("Exa engine selected"), "{message}");
        assert!(
            message.contains("JCODE_TEST_UNSET_EXA_API_KEY"),
            "the missing variable should be named: {message}"
        );
    }

    #[test]
    fn configured_exa_key_wins_over_environment() {
        let config = WebSearchConfig {
            exa_api_key: Some("  from-config  ".to_string()),
            exa_api_key_env: "JCODE_TEST_UNSET_EXA_API_KEY".to_string(),
            ..WebSearchConfig::default()
        };
        assert_eq!(resolve_exa_api_key(&config).unwrap(), "from-config");
    }

    #[test]
    fn websearch_engine_parses_exa() {
        assert_eq!(WebSearchEngine::parse("exa"), Some(WebSearchEngine::Exa));
        assert_eq!(
            WebSearchEngine::parse(" EXA.AI "),
            Some(WebSearchEngine::Exa)
        );
        assert_eq!(WebSearchEngine::Exa.as_str(), "exa");
        assert_eq!(WebSearchEngine::Exa.to_string(), "exa");
    }
}
