use crate::config::http_client;
use crate::constants::HTTP_TIMEOUT_SECS;
use crate::types::{ContextBlock, SearchResult, ToolRequest};
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub(crate) fn parse_brave_results(data: &Value) -> Vec<SearchResult> {
    let mut results = Vec::new();

    if let Some(results_data) = data
        .get("web")
        .and_then(|v| v.get("results"))
        .and_then(|v| v.as_array())
    {
        for item in results_data {
            if let (Some(title), Some(url), Some(snippet)) = (
                item.get("title").and_then(|v| v.as_str()),
                item.get("url").and_then(|v| v.as_str()),
                item.get("description").and_then(|v| v.as_str()),
            ) {
                results.push(SearchResult {
                    title: title.to_string(),
                    url: url.to_string(),
                    snippet: snippet.to_string(),
                });
            }
        }
    }

    results
}

pub(crate) fn has_content(s: &str) -> bool {
    !s.trim().is_empty()
}

pub(crate) async fn brave_search(query: &str) -> Result<Vec<SearchResult>> {
    let api_key = std::env::var("BRAVE_API_KEY")
        .map_err(|_| anyhow!("BRAVE_API_KEY environment variable not set"))?;

    let client = http_client(HTTP_TIMEOUT_SECS)?;
    let resp = client
        .get("https://api.search.brave.com/res/v1/web/search")
        .header("Accept", "application/json")
        .header("X-Subscription-Token", &api_key)
        .query(&[("q", query), ("count", "5")])
        .send()
        .await?
        .error_for_status()?;

    let data: Value = resp.json().await?;
    Ok(parse_brave_results(&data))
}

pub(crate) async fn fetch_url(url: &str) -> Result<String> {
    let client = http_client(HTTP_TIMEOUT_SECS)?;
    let resp = client.get(url).send().await?.error_for_status()?;
    let html = resp.text().await?;
    let text = html2text::from_read(html.as_bytes(), 80);
    Ok(text)
}

pub(crate) fn normalize_url(url: &str) -> String {
    let trimmed = url.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else {
        format!("https://{}", trimmed)
    }
}

pub(crate) fn normalize_file_path(path: &str) -> String {
    match path.trim().to_lowercase().as_str() {
        "readme" | "readme.md" => "README.md".to_string(),
        "cargo.toml" => "Cargo.toml".to_string(),
        "main.rs" => "src/main.rs".to_string(),
        "gitignore" | ".gitignore" => ".gitignore".to_string(),
        "makefile" => "Makefile".to_string(),
        other => other.to_string(),
    }
}

pub(crate) fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else if max_chars <= 3 {
        ".".repeat(max_chars)
    } else {
        s.chars().take(max_chars - 3).collect::<String>() + "..."
    }
}

#[derive(Debug)]
struct ContextBudget {
    remaining: usize,
    seen: HashSet<String>,
}

impl ContextBudget {
    fn new(max_chars: usize) -> Self {
        Self {
            remaining: max_chars,
            seen: HashSet::new(),
        }
    }

    fn push(
        &mut self,
        contexts: &mut Vec<ContextBlock>,
        dedupe_key: String,
        label: String,
        text: String,
        url: Option<String>,
    ) {
        if self.remaining == 0 || !self.seen.insert(dedupe_key) {
            return;
        }

        let text = truncate(&text, self.remaining);
        self.remaining = self.remaining.saturating_sub(text.chars().count());
        contexts.push(ContextBlock { label, text, url });
    }
}

fn safe_file_path(path: &str) -> Result<PathBuf> {
    let cwd = std::env::current_dir()?;
    safe_file_path_in(path, &cwd)
}

fn safe_file_path_in(path: &str, cwd: &Path) -> Result<PathBuf> {
    let candidate = cwd.join(path);
    let abs_path = candidate.canonicalize()?;
    let abs_cwd = cwd.canonicalize()?;

    if !abs_path.starts_with(&abs_cwd) {
        return Err(anyhow!(
            "requested file is outside the current working directory"
        ));
    }

    if !abs_path.is_file() {
        return Err(anyhow!("requested path is not a file"));
    }

    Ok(abs_path)
}

fn read_local_file(path: &str) -> Result<String> {
    let abs_path = safe_file_path(path)?;
    std::fs::read_to_string(abs_path).map_err(Into::into)
}

fn sanitized_tool_error(tool_name: &str) -> String {
    format!("{tool_name} failed; details omitted")
}

pub(crate) async fn gather_context(
    requests: Vec<ToolRequest>,
    mut contexts: Vec<ContextBlock>,
    max_chars: usize,
) -> Vec<ContextBlock> {
    let mut budget = ContextBudget::new(max_chars);
    let existing_contexts = std::mem::take(&mut contexts);

    for ctx in existing_contexts {
        let dedupe_key = ctx
            .url
            .clone()
            .unwrap_or_else(|| format!("label:{}", ctx.label));
        budget.push(&mut contexts, dedupe_key, ctx.label, ctx.text, ctx.url);
    }

    for request in requests {
        match request {
            ToolRequest::SearchWeb { query } => match brave_search(&query).await {
                Ok(results) => {
                    for (idx, result) in results.iter().enumerate() {
                        let context_text = format!(
                            "title: {}\nurl: {}\n{}",
                            result.title, result.url, result.snippet
                        );
                        budget.push(
                            &mut contexts,
                            format!("url:{}", result.url),
                            format!("search result {}", idx + 1),
                            context_text,
                            Some(result.url.clone()),
                        );
                    }
                }
                Err(_) => budget.push(
                    &mut contexts,
                    format!("error:search_web:{query}"),
                    "tool errors".to_string(),
                    sanitized_tool_error("search_web"),
                    None,
                ),
            },
            ToolRequest::ReadUrl { url } => match fetch_url(&url).await {
                Ok(fetched) => {
                    budget.push(
                        &mut contexts,
                        format!("url:{url}"),
                        format!("url {}", url),
                        fetched,
                        Some(url),
                    );
                }
                Err(_) => budget.push(
                    &mut contexts,
                    format!("error:read_url:{url}"),
                    "tool errors".to_string(),
                    sanitized_tool_error("read_url"),
                    None,
                ),
            },
            ToolRequest::ReadFile { path } => match read_local_file(&path) {
                Ok(file_text) => {
                    budget.push(
                        &mut contexts,
                        format!("file:{path}"),
                        format!("file {}", path),
                        file_text,
                        None,
                    );
                }
                Err(_) => budget.push(
                    &mut contexts,
                    format!("error:read_file:{path}"),
                    "tool errors".to_string(),
                    sanitized_tool_error("read_file"),
                    None,
                ),
            },
        }
    }

    contexts
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_brave_web_results() {
        let data = json!({
            "web": {
                "results": [
                    {
                        "title": "Rust",
                        "url": "https://www.rust-lang.org/",
                        "description": "A language empowering everyone."
                    },
                    {
                        "title": "The Rust Book",
                        "url": "https://doc.rust-lang.org/book/",
                        "description": "The Rust Programming Language book."
                    }
                ]
            }
        });

        assert_eq!(
            parse_brave_results(&data),
            vec![
                SearchResult {
                    title: "Rust".to_string(),
                    url: "https://www.rust-lang.org/".to_string(),
                    snippet: "A language empowering everyone.".to_string(),
                },
                SearchResult {
                    title: "The Rust Book".to_string(),
                    url: "https://doc.rust-lang.org/book/".to_string(),
                    snippet: "The Rust Programming Language book.".to_string(),
                },
            ]
        );
    }

    #[test]
    fn skips_incomplete_brave_results() {
        let data = json!({
            "web": {
                "results": [
                    {
                        "title": "Missing URL",
                        "description": "This should be ignored."
                    },
                    {
                        "title": "Complete",
                        "url": "https://example.com/",
                        "description": "This should be kept."
                    }
                ]
            }
        });

        assert_eq!(
            parse_brave_results(&data),
            vec![SearchResult {
                title: "Complete".to_string(),
                url: "https://example.com/".to_string(),
                snippet: "This should be kept.".to_string(),
            }]
        );
    }

    #[test]
    fn returns_no_results_for_missing_brave_web_results() {
        let data = json!({ "web": {} });

        assert!(parse_brave_results(&data).is_empty());
    }

    #[test]
    fn normalizes_url_and_readme_path() {
        assert_eq!(normalize_url("https://example.com"), "https://example.com");
        assert_eq!(normalize_url("example.com"), "https://example.com");
        assert_eq!(normalize_file_path("README"), "README.md");
        assert_eq!(normalize_file_path("readme.md"), "README.md");
        assert_eq!(normalize_file_path("cargo.toml"), "Cargo.toml");
        assert_eq!(normalize_file_path("main.rs"), "src/main.rs");
        assert_eq!(normalize_file_path("gitignore"), ".gitignore");
        assert_eq!(normalize_file_path("src/main.rs"), "src/main.rs");
    }

    #[test]
    fn detects_non_empty_context() {
        assert!(has_content("notes"));
        assert!(has_content("\nnotes\n"));
        assert!(!has_content(""));
        assert!(!has_content(" \n\t"));
    }

    #[test]
    fn truncates_on_char_boundary() {
        assert_eq!(truncate("aébc", 4), "aébc");
        assert_eq!(truncate("aébcdef", 5), "aé...");
        assert_eq!(truncate("aébc", 3), "...");
        assert_eq!(truncate("aébc", 2), "..");
    }

    #[test]
    fn rejects_file_paths_outside_cwd() {
        let cwd = std::env::current_dir().unwrap();
        let parent = cwd.parent().unwrap();

        let err = safe_file_path_in("../Cargo.toml", &cwd).unwrap_err();
        assert!(err.to_string().contains("No such file") || err.to_string().contains("outside"));

        if parent.join("Cargo.toml").exists() {
            let err = safe_file_path_in("../Cargo.toml", &cwd).unwrap_err();
            assert!(err.to_string().contains("outside"));
        }
    }

    #[test]
    fn context_budget_applies_to_total_context_and_dedupes() {
        let mut budget = ContextBudget::new(8);
        let mut contexts = Vec::new();

        budget.push(
            &mut contexts,
            "same".to_string(),
            "first".to_string(),
            "abcdef".to_string(),
            None,
        );
        budget.push(
            &mut contexts,
            "same".to_string(),
            "duplicate".to_string(),
            "ignored".to_string(),
            None,
        );
        budget.push(
            &mut contexts,
            "other".to_string(),
            "second".to_string(),
            "uvwxyz".to_string(),
            None,
        );

        assert_eq!(contexts.len(), 2);
        assert_eq!(contexts[0].text, "abcdef");
        assert_eq!(contexts[1].text, "..");
        assert_eq!(
            contexts
                .iter()
                .map(|ctx| ctx.text.chars().count())
                .sum::<usize>(),
            8
        );
    }

    #[tokio::test]
    async fn gather_context_sanitizes_file_errors() {
        let contexts = gather_context(
            vec![ToolRequest::ReadFile {
                path: "../../etc/passwd".to_string(),
            }],
            Vec::new(),
            30000,
        )
        .await;

        assert_eq!(contexts.len(), 1);
        assert_eq!(contexts[0].label, "tool errors");
        assert_eq!(contexts[0].text, "read_file failed; details omitted");
        assert!(!contexts[0].text.contains("../../etc/passwd"));
    }
}
