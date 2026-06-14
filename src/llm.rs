use crate::config::{http_client, LlmEndpoint};
use crate::constants::{ANSWERER_SYSTEM_PROMPT, LLM_TIMEOUT_SECS, PLANNER_SYSTEM_PROMPT};
use crate::tools::{normalize_file_path, normalize_url};
use crate::types::ToolRequest;
use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::{json, Value};

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

async fn post_chat(endpoint: &LlmEndpoint, body: Value) -> Result<ChatMessage> {
    let client = http_client(LLM_TIMEOUT_SECS)?;
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or(&endpoint.model);

    let resp = client
        .post(&endpoint.chat_url)
        .header("Authorization", format!("Bearer {}", endpoint.api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    let status = resp.status();
    if !status.is_success() {
        let error_body = resp.text().await.unwrap_or_default();
        let detail = provider_error_message(&error_body)
            .map(|message| format!(": {message}"))
            .unwrap_or_default();
        let hint = if status.as_u16() == 404 {
            " (check the configured provider/model)"
        } else {
            ""
        };

        return Err(anyhow!(
            "{} request failed for model '{}': {}{}{}",
            endpoint.provider,
            model,
            status,
            detail,
            hint
        ));
    }

    let data: ChatResponse = resp.json().await?;
    data.choices
        .into_iter()
        .next()
        .map(|choice| choice.message)
        .ok_or_else(|| anyhow!("LLM response did not include any choices"))
}

fn provider_error_message(body: &str) -> Option<String> {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|json| {
            json.pointer("/error/message")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .map(|message| message.trim().to_string())
        .filter(|message| !message.is_empty())
}

pub(crate) async fn plan_tools(endpoint: &LlmEndpoint, question: &str) -> Result<Vec<ToolRequest>> {
    let body = json!({
        "model": endpoint.model,
        "messages": [
            {"role": "system", "content": PLANNER_SYSTEM_PROMPT},
            {"role": "user", "content": question}
        ],
        "tools": tool_definitions(),
        "tool_choice": "auto"
    });

    let message = post_chat(endpoint, body).await?;
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

pub(crate) async fn call_llm(endpoint: &LlmEndpoint, prompt: &str) -> Result<String> {
    let body = json!({
        "model": endpoint.model,
        "messages": [
            {"role": "system", "content": ANSWERER_SYSTEM_PROMPT},
            {"role": "user", "content": prompt}
        ]
    });

    let message = post_chat(endpoint, body).await?;
    let text = message
        .content
        .ok_or_else(|| anyhow!("LLM response did not include text content"))?;

    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn formats_openrouter_error_body() {
        let body = r#"{"error":{"message":"No endpoints found for baidu/cobuddy:free.","code":404},"user_id":"user_123"}"#;

        assert_eq!(
            provider_error_message(body).as_deref(),
            Some("No endpoints found for baidu/cobuddy:free.")
        );
    }

    #[test]
    fn ignores_missing_openrouter_error_message() {
        assert!(provider_error_message("{}").is_none());
        assert!(provider_error_message("not json").is_none());
    }
}
