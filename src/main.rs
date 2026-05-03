use anyhow::{anyhow, Result};
use clap::Parser;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

const HTTP_TIMEOUT_SECS: u64 = 30;
const LLM_TIMEOUT_SECS: u64 = 90;
const ANSI_RESET: &str = "\x1b[0m";
const ANSI_BOLD: &str = "\x1b[1m";
const ANSI_DIM: &str = "\x1b[2m";
const ANSI_CYAN: &str = "\x1b[36m";
const ANSI_GREEN: &str = "\x1b[32m";
const ANSI_YELLOW: &str = "\x1b[33m";
const PLANNER_SYSTEM_PROMPT: &str = "You are Anaphor's tool router. Select zero or more tools needed to answer the user's request.\n\nUse read_file for local file requests like README, Cargo.toml, or source files.\nUse read_url when the request names a URL or bare domain; normalize bare domains to https://.\nUse search_web when the user asks to find links, search the web, or asks about current/external information without naming a specific URL. Do not use search_web for requests that name a specific URL or domain.\nIf no tool is needed, answer without tool calls.";
const ANSWERER_SYSTEM_PROMPT: &str = "You are Anaphor, a compact terminal context resolver.\n\nAnswer the user's question using the provided context.\nIf sources are provided, cite them numerically using their URLs or source labels.\nDo not invent sources.\nIf the context is insufficient, say so.\nBe concise, technical, and direct.\nFor structured answers, use compact Markdown: headings with ###, bullets with -, inline code with backticks, and bold labels with **label**.";

#[derive(Parser)]
#[command(name = "anaphor", about = "Terminal context resolver")]
struct Cli {
    #[arg(help = "Question to answer")]
    question: Option<String>,
}

#[derive(Debug, Clone)]
struct ContextBlock {
    label: String,
    text: String,
    url: Option<String>,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct SearchResult {
    title: String,
    url: String,
    snippet: String,
}

#[derive(Debug, PartialEq, Eq)]
enum ToolRequest {
    SearchWeb { query: String },
    ReadUrl { url: String },
    ReadFile { path: String },
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Debug, Deserialize)]
struct ChatMessage {
    content: Option<String>,
    tool_calls: Option<Vec<OpenRouterToolCall>>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterToolCall {
    function: OpenRouterFunctionCall,
}

#[derive(Debug, Deserialize)]
struct OpenRouterFunctionCall {
    name: String,
    arguments: String,
}

fn parse_brave_results(data: &Value) -> Vec<SearchResult> {
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

fn has_content(s: &str) -> bool {
    !s.trim().is_empty()
}

fn default_max_chars() -> usize {
    std::env::var("ANAPHOR_MAX_CHARS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30000)
}

fn default_model() -> String {
    std::env::var("ANAPHOR_MODEL").unwrap_or_else(|_| "openai/gpt-4o-mini".to_string())
}

fn openrouter_api_key() -> Result<String> {
    std::env::var("OPENROUTER_API_KEY")
        .map_err(|_| anyhow!("OPENROUTER_API_KEY environment variable not set"))
}

fn http_client(timeout_secs: u64) -> Result<Client> {
    Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .build()
        .map_err(Into::into)
}

fn tool_definitions() -> Value {
    json!([
        {
            "type": "function",
            "function": {
                "name": "search_web",
                "description": "Search the web when the user asks to find links, look up current information, or discover pages about a topic. Do not use this when the user names a specific URL or domain.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "The web search query."
                        }
                    },
                    "required": ["query"],
                    "additionalProperties": false
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "read_url",
                "description": "Fetch and read a web page when the user mentions a URL or domain, such as rust-lang.org. Use https:// for bare domains.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "url": {
                            "type": "string",
                            "description": "The URL to fetch. Bare domains should be normalized to https://."
                        }
                    },
                    "required": ["url"],
                    "additionalProperties": false
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "read_file",
                "description": "Read a local file from the current working directory when the user asks about a file, for example README, Cargo.toml, or src/main.rs.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "The local file path to read."
                        }
                    },
                    "required": ["path"],
                    "additionalProperties": false
                }
            }
        }
    ])
}

async fn brave_search(query: &str) -> Result<Vec<SearchResult>> {
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

async fn fetch_url(url: &str) -> Result<String> {
    let client = http_client(HTTP_TIMEOUT_SECS)?;
    let resp = client.get(url).send().await?.error_for_status()?;
    let html = resp.text().await?;
    let text = html2text::from_read(html.as_bytes(), 80);
    Ok(text)
}

async fn post_openrouter(body: Value) -> Result<ChatMessage> {
    let api_key = openrouter_api_key()?;
    let client = http_client(LLM_TIMEOUT_SECS)?;

    let resp = client
        .post("https://openrouter.ai/api/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?
        .error_for_status()?;

    let data: ChatResponse = resp.json().await?;
    data.choices
        .into_iter()
        .next()
        .map(|choice| choice.message)
        .ok_or_else(|| anyhow!("LLM response did not include any choices"))
}

async fn plan_tools(model: &str, question: &str) -> Result<Vec<ToolRequest>> {
    let body = json!({
        "model": model,
        "messages": [
            {"role": "system", "content": PLANNER_SYSTEM_PROMPT},
            {"role": "user", "content": question}
        ],
        "tools": tool_definitions(),
        "tool_choice": "auto"
    });

    let message = post_openrouter(body).await?;
    Ok(parse_tool_requests(message.tool_calls.as_deref()))
}

fn parse_tool_requests(tool_calls: Option<&[OpenRouterToolCall]>) -> Vec<ToolRequest> {
    let Some(tool_calls) = tool_calls else {
        return Vec::new();
    };

    let mut requests = Vec::new();

    for tool_call in tool_calls {
        let Ok(args) = serde_json::from_str::<Value>(&tool_call.function.arguments) else {
            continue;
        };

        match tool_call.function.name.as_str() {
            "search_web" => {
                if let Some(query) = args.get("query").and_then(|v| v.as_str()) {
                    requests.push(ToolRequest::SearchWeb {
                        query: query.to_string(),
                    });
                }
            }
            "read_url" => {
                if let Some(url) = args.get("url").and_then(|v| v.as_str()) {
                    requests.push(ToolRequest::ReadUrl {
                        url: normalize_url(url),
                    });
                }
            }
            "read_file" => {
                if let Some(path) = args.get("path").and_then(|v| v.as_str()) {
                    requests.push(ToolRequest::ReadFile {
                        path: normalize_file_path(path),
                    });
                }
            }
            _ => {}
        }
    }

    requests
}

fn normalize_url(url: &str) -> String {
    let trimmed = url.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else {
        format!("https://{}", trimmed)
    }
}

fn normalize_file_path(path: &str) -> String {
    match path.trim().to_lowercase().as_str() {
        "readme" | "readme.md" => "README.md".to_string(),
        "cargo.toml" => "Cargo.toml".to_string(),
        "main.rs" => "src/main.rs".to_string(),
        "gitignore" | ".gitignore" => ".gitignore".to_string(),
        "makefile" => "Makefile".to_string(),
        other => other.to_string(),
    }
}

async fn call_llm(model: &str, prompt: &str) -> Result<String> {
    let body = json!({
        "model": model,
        "messages": [
            {"role": "system", "content": ANSWERER_SYSTEM_PROMPT},
            {"role": "user", "content": prompt}
        ]
    });

    let message = post_openrouter(body).await?;
    let text = message
        .content
        .ok_or_else(|| anyhow!("LLM response did not include text content"))?;

    Ok(text)
}

fn truncate(s: &str, max_chars: usize) -> String {
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

async fn gather_context(
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

fn build_prompt(question: &str, contexts: &[ContextBlock]) -> String {
    let mut prompt = String::new();

    if !contexts.is_empty() {
        prompt.push_str("Context:\n---\n");
        for ctx in contexts.iter() {
            prompt.push_str(&format!("[source: {}]\n", ctx.label));
            if let Some(url) = &ctx.url {
                prompt.push_str(&format!("url: {}\n", url));
            }
            prompt.push_str(&format!("{}\n\n", ctx.text));
        }
        prompt.push_str("---\n\n");
    }

    prompt.push_str(&format!("Question:\n{}", question));
    prompt
}

fn should_color() -> bool {
    std::env::var("NO_COLOR").is_err() && atty::is(atty::Stream::Stdout)
}

fn style(text: &str, codes: &[&str], color: bool) -> String {
    if !color {
        return text.to_string();
    }

    format!("{}{}{}", codes.join(""), text, ANSI_RESET)
}

fn render_inline(text: &str, color: bool) -> String {
    let mut out = String::new();
    let mut in_code = false;
    let mut in_bold = false;
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '`' {
            in_code = !in_code;
            continue;
        }

        if ch == '*' && chars.peek() == Some(&'*') {
            chars.next();
            in_bold = !in_bold;
            continue;
        }

        let mut segment = String::new();
        segment.push(ch);

        while let Some(&next) = chars.peek() {
            if next == '`' || next == '*' {
                break;
            }
            segment.push(chars.next().unwrap());
        }

        if in_code {
            out.push_str(&style(&segment, &[ANSI_GREEN], color));
        } else if in_bold {
            out.push_str(&style(&segment, &[ANSI_BOLD], color));
        } else {
            out.push_str(&segment);
        }
    }

    out
}

fn strip_heading(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let hashes = trimmed.chars().take_while(|ch| *ch == '#').count();

    if (1..=6).contains(&hashes) && trimmed.chars().nth(hashes) == Some(' ') {
        Some(trimmed[hashes + 1..].trim())
    } else {
        None
    }
}

fn render_markdownish(input: &str, color: bool) -> String {
    let mut out = String::new();
    let mut in_code_block = false;

    for line in input.lines() {
        let trimmed = line.trim_start();

        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }

        if in_code_block {
            out.push_str(&style(line, &[ANSI_GREEN], color));
            out.push('\n');
            continue;
        }

        if let Some(heading) = strip_heading(line) {
            if !out.ends_with("\n\n") && !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&style(heading, &[ANSI_BOLD, ANSI_CYAN], color));
            out.push('\n');
            continue;
        }

        if trimmed.ends_with(':') && !trimmed.starts_with("- ") && trimmed.len() <= 120 {
            if !out.ends_with("\n\n") && !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&style(
                trimmed.trim_end_matches(':'),
                &[ANSI_BOLD, ANSI_CYAN],
                color,
            ));
            out.push('\n');
            continue;
        }

        if let Some(item) = trimmed.strip_prefix("- ") {
            out.push_str(&style("  • ", &[ANSI_YELLOW], color));
            out.push_str(&render_inline(item, color));
            out.push('\n');
            continue;
        }

        if trimmed.starts_with('[') && trimmed.contains(']') {
            out.push_str(&style(&render_inline(line, color), &[ANSI_DIM], color));
            out.push('\n');
            continue;
        }

        out.push_str(&render_inline(line, color));
        out.push('\n');
    }

    out.trim_end().to_string()
}

fn is_stdin_piped() -> bool {
    !atty::is(atty::Stream::Stdin)
}

async fn read_stdin(max_chars: usize) -> Result<String> {
    let mut buf = String::new();
    std::io::stdin()
        .take((max_chars.saturating_mul(4).saturating_add(1)) as u64)
        .read_to_string(&mut buf)?;
    Ok(buf)
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();
    let cli = Cli::parse();

    let model = default_model();
    let max_chars = default_max_chars();
    let mut contexts: Vec<ContextBlock> = Vec::new();

    // Read stdin if piped
    if is_stdin_piped() {
        let stdin_text = read_stdin(max_chars).await?;
        if has_content(&stdin_text) {
            let truncated = truncate(&stdin_text, max_chars);
            contexts.push(ContextBlock {
                label: "stdin".to_string(),
                text: truncated,
                url: None,
            });
        }
    }

    // Determine question
    let question = if let Some(q) = cli.question {
        if q.trim().is_empty() {
            eprintln!("No question provided. Use 'anaphor --help' for usage.");
            std::process::exit(1);
        }
        q
    } else if !contexts.is_empty() {
        eprintln!("Warning: no question provided, answering based on context only");
        "Summarize the provided context.".to_string()
    } else {
        eprintln!("No question or context provided. Use 'anaphor --help' for usage.");
        std::process::exit(1);
    };

    let requests = plan_tools(&model, &question).await?;
    let contexts = gather_context(requests, contexts, max_chars).await;

    // Build and send prompt
    let prompt = build_prompt(&question, &contexts);
    let answer = call_llm(&model, &prompt).await?;

    // Format output
    println!("{}", render_markdownish(&answer, should_color()));

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn parses_tool_requests() {
        let tool_calls = vec![
            OpenRouterToolCall {
                function: OpenRouterFunctionCall {
                    name: "read_file".to_string(),
                    arguments: r#"{"path":"readme"}"#.to_string(),
                },
            },
            OpenRouterToolCall {
                function: OpenRouterFunctionCall {
                    name: "read_url".to_string(),
                    arguments: r#"{"url":"rust-lang.org"}"#.to_string(),
                },
            },
            OpenRouterToolCall {
                function: OpenRouterFunctionCall {
                    name: "search_web".to_string(),
                    arguments: r#"{"query":"axum web framework"}"#.to_string(),
                },
            },
        ];

        assert_eq!(
            parse_tool_requests(Some(&tool_calls)),
            vec![
                ToolRequest::ReadFile {
                    path: "README.md".to_string()
                },
                ToolRequest::ReadUrl {
                    url: "https://rust-lang.org".to_string()
                },
                ToolRequest::SearchWeb {
                    query: "axum web framework".to_string()
                },
            ]
        );
    }

    #[test]
    fn skips_unknown_or_invalid_tool_requests() {
        let tool_calls = vec![
            OpenRouterToolCall {
                function: OpenRouterFunctionCall {
                    name: "read_file".to_string(),
                    arguments: "not json".to_string(),
                },
            },
            OpenRouterToolCall {
                function: OpenRouterFunctionCall {
                    name: "unknown".to_string(),
                    arguments: r#"{"query":"ignored"}"#.to_string(),
                },
            },
        ];

        assert!(parse_tool_requests(Some(&tool_calls)).is_empty());
        assert!(parse_tool_requests(None).is_empty());
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
    fn builds_prompt_with_context_url() {
        let prompt = build_prompt(
            "What changed?",
            &[ContextBlock {
                label: "url https://example.com".to_string(),
                text: "Page body".to_string(),
                url: Some("https://example.com".to_string()),
            }],
        );

        assert!(prompt.contains("[source: url https://example.com]"));
        assert!(prompt.contains("url: https://example.com"));
        assert!(prompt.contains("Page body"));
        assert!(prompt.ends_with("Question:\nWhat changed?"));
    }

    #[test]
    fn renders_markdownish_without_color() {
        let rendered = render_markdownish(
            "### Setup\n- **Run**: Use `cargo run`\n\n[1] file README.md",
            false,
        );

        assert_eq!(
            rendered,
            "Setup\n  • Run: Use cargo run\n\n[1] file README.md"
        );
    }

    #[test]
    fn renders_markdownish_with_color() {
        let rendered = render_markdownish("Summary:\n- **Run**: Use `cargo run`", true);

        assert!(rendered.contains(ANSI_BOLD));
        assert!(rendered.contains(ANSI_CYAN));
        assert!(rendered.contains(ANSI_YELLOW));
        assert!(rendered.contains(ANSI_GREEN));
        assert!(rendered.contains("Summary"));
        assert!(rendered.contains("Run"));
        assert!(rendered.contains("cargo run"));
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
