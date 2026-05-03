use anyhow::{anyhow, Result};
use reqwest::Client;
use std::time::Duration;

pub(crate) const HTTP_TIMEOUT_SECS: u64 = 30;
pub(crate) const LLM_TIMEOUT_SECS: u64 = 90;

pub(crate) fn default_max_chars() -> usize {
    std::env::var("ANAPHOR_MAX_CHARS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30000)
}

pub(crate) fn default_model() -> String {
    std::env::var("ANAPHOR_MODEL").unwrap_or_else(|_| "openai/gpt-4o-mini".to_string())
}

pub(crate) fn openrouter_api_key() -> Result<String> {
    std::env::var("OPENROUTER_API_KEY")
        .map_err(|_| anyhow!("OPENROUTER_API_KEY environment variable not set"))
}

pub(crate) fn http_client(timeout_secs: u64) -> Result<Client> {
    Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .build()
        .map_err(Into::into)
}
