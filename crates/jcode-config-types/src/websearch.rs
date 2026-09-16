//! Web search engine selection (`[websearch]` in config.toml).
//!
//! Split out of `lib.rs` so the engine/config types own their file; re-exported
//! from the crate root, so `jcode_config_types::WebSearchConfig` keeps working.

use serde::{Deserialize, Serialize};

/// Search engine used by the websearch tool.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "lowercase")]
pub enum WebSearchEngine {
    /// DuckDuckGo HTML search, no API key required.
    #[default]
    Duckduckgo,
    /// Bing search. Uses the Bing API when configured, otherwise Bing HTML search.
    Bing,
    /// SearXNG metasearch instance (JSON API). Requires `searxng_url` (or the
    /// `JCODE_SEARXNG_URL` env var) to point at a SearXNG instance. Useful on
    /// hosts where DuckDuckGo/Bing block the request via TLS fingerprinting.
    Searxng,
    /// Exa semantic search API (`https://api.exa.ai/search`). Requires an API
    /// key via `exa_api_key` (or the `EXA_API_KEY` env var).
    Exa,
}

impl WebSearchEngine {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Duckduckgo => "duckduckgo",
            Self::Bing => "bing",
            Self::Searxng => "searxng",
            Self::Exa => "exa",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "duckduckgo" | "ddg" => Some(Self::Duckduckgo),
            "bing" => Some(Self::Bing),
            "searxng" | "searx" => Some(Self::Searxng),
            "exa" | "exa.ai" => Some(Self::Exa),
            _ => None,
        }
    }
}

impl std::fmt::Display for WebSearchEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Configuration for the websearch tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WebSearchConfig {
    /// Preferred engine when the tool input does not specify one.
    pub engine: WebSearchEngine,
    /// Keyless HTML engines to try after the preferred engine fails.
    pub fallback_engines: Vec<WebSearchEngine>,
    /// Optional Bing API key for primary Bing searches. Fallback Bing uses keyless HTML search.
    pub bing_api_key: Option<String>,
    /// Environment variable containing the Bing API key.
    pub bing_api_key_env: String,
    /// Bing market, e.g. "en-US" or "zh-CN".
    pub bing_market: String,
    /// Base URL of a SearXNG instance (e.g. "https://searx.example.org"), used
    /// by the `searxng` engine. When empty, the `searxng_url_env` variable is
    /// consulted instead.
    pub searxng_url: Option<String>,
    /// Environment variable containing the SearXNG base URL.
    pub searxng_url_env: String,
    /// Optional Exa API key (https://api.exa.ai). Prefer the env var.
    pub exa_api_key: Option<String>,
    /// Environment variable containing the Exa API key.
    pub exa_api_key_env: String,
}

impl Default for WebSearchConfig {
    fn default() -> Self {
        Self {
            engine: WebSearchEngine::Duckduckgo,
            fallback_engines: vec![WebSearchEngine::Bing],
            bing_api_key: None,
            bing_api_key_env: "JCODE_BING_API_KEY".to_string(),
            bing_market: "en-US".to_string(),
            searxng_url: None,
            searxng_url_env: "JCODE_SEARXNG_URL".to_string(),
            exa_api_key: None,
            exa_api_key_env: "EXA_API_KEY".to_string(),
        }
    }
}
