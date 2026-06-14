use anyhow::{anyhow, Result};
use reqwest::Client;
use std::{fs, path::PathBuf, time::Duration};

const OPENROUTER_CHAT_COMPLETIONS_URL: &str = "https://openrouter.ai/api/v1/chat/completions";
const GLM_BASE_URL: &str = "https://api.z.ai/api/coding/paas/v4";
const GLM_DEFAULT_MODEL: &str = "glm-5.1";

#[derive(Clone)]
pub(crate) struct LlmEndpoint {
    pub(crate) provider: String,
    pub(crate) model: String,
    pub(crate) chat_url: String,
    pub(crate) api_key: String,
}

pub(crate) fn default_max_chars() -> usize {
    std::env::var("ANAPHOR_MAX_CHARS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30000)
}

pub(crate) fn llm_endpoint() -> Result<LlmEndpoint> {
    let provider = std::env::var("ANAPHOR_PROVIDER").ok();
    let provider_name = provider
        .as_deref()
        .unwrap_or("openrouter")
        .to_ascii_lowercase();

    match provider_name.as_str() {
        "glm" | "zai" | "z-ai" | "z.ai" => glm_endpoint(),
        "openai" | "custom" => openai_compatible_endpoint(&provider_name),
        "openrouter" => {
            if provider.is_none() && std::env::var("ANAPHOR_BASE_URL").is_ok() {
                openai_compatible_endpoint("custom")
            } else {
                openrouter_endpoint()
            }
        }
        other => Err(anyhow!(
            "unsupported ANAPHOR_PROVIDER '{}'; use openrouter, glm, openai, or custom",
            other
        )),
    }
}

fn openrouter_endpoint() -> Result<LlmEndpoint> {
    let api_key = std::env::var("OPENROUTER_API_KEY")
        .map_err(|_| anyhow!("OPENROUTER_API_KEY environment variable not set"))?;
    let model = std::env::var("ANAPHOR_MODEL").unwrap_or_else(|_| "openai/gpt-4o-mini".to_string());

    Ok(LlmEndpoint {
        provider: "openrouter".to_string(),
        model,
        chat_url: OPENROUTER_CHAT_COMPLETIONS_URL.to_string(),
        api_key,
    })
}

fn openai_compatible_endpoint(provider: &str) -> Result<LlmEndpoint> {
    let api_key = openai_compatible_api_key(provider)?;
    let base_url = std::env::var("ANAPHOR_BASE_URL").unwrap_or_else(|_| {
        if provider == "openai" {
            "https://api.openai.com/v1".to_string()
        } else {
            String::new()
        }
    });
    if base_url.trim().is_empty() {
        return Err(anyhow!("ANAPHOR_BASE_URL environment variable not set"));
    }
    let model = std::env::var("ANAPHOR_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());

    Ok(LlmEndpoint {
        provider: provider.to_string(),
        model,
        chat_url: chat_completions_url(&base_url),
        api_key,
    })
}

fn openai_compatible_api_key(provider: &str) -> Result<String> {
    if provider == "openai" {
        return std::env::var("ANAPHOR_API_KEY")
            .or_else(|_| std::env::var("OPENAI_API_KEY"))
            .map_err(|_| {
                anyhow!("ANAPHOR_API_KEY or OPENAI_API_KEY environment variable not set")
            });
    }

    std::env::var("ANAPHOR_API_KEY")
        .map_err(|_| anyhow!("ANAPHOR_API_KEY environment variable not set"))
}

fn glm_endpoint() -> Result<LlmEndpoint> {
    let api_key = std::env::var("ANAPHOR_API_KEY")
        .or_else(|_| std::env::var("ZHIPU_API_KEY"))
        .ok()
        .or_else(v100_glm_api_key)
        .ok_or_else(|| {
            anyhow!(
                "GLM key not found; set ANAPHOR_API_KEY, ZHIPU_API_KEY, or configure providers.glm.auth.key in ~/.config/v100/config.toml"
            )
        })?;
    let base_url = std::env::var("ANAPHOR_BASE_URL").unwrap_or_else(|_| GLM_BASE_URL.to_string());
    let model =
        std::env::var("ANAPHOR_GLM_MODEL").unwrap_or_else(|_| GLM_DEFAULT_MODEL.to_string());

    Ok(LlmEndpoint {
        provider: "glm".to_string(),
        model,
        chat_url: chat_completions_url(&base_url),
        api_key,
    })
}

pub(crate) fn http_client(timeout_secs: u64) -> Result<Client> {
    Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .build()
        .map_err(Into::into)
}

fn chat_completions_url(base_url: &str) -> String {
    let base_url = base_url.trim().trim_end_matches('/');
    if base_url.ends_with("/chat/completions") {
        base_url.to_string()
    } else {
        format!("{base_url}/chat/completions")
    }
}

fn v100_glm_api_key() -> Option<String> {
    let path = v100_config_path()?;
    let text = fs::read_to_string(path).ok()?;
    v100_glm_api_key_from_str(&text)
}

fn v100_config_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("V100_CONFIG") {
        return Some(PathBuf::from(path));
    }

    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join(".config/v100/config.toml"))
}

fn v100_glm_api_key_from_str(text: &str) -> Option<String> {
    let mut in_glm_auth = false;

    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_glm_auth = line == "[providers.glm.auth]";
            continue;
        }

        if in_glm_auth {
            if let Some(value) = toml_string_assignment(line, "key") {
                return Some(value);
            }
        }
    }

    None
}

fn toml_string_assignment(line: &str, key: &str) -> Option<String> {
    let line = line.split('#').next()?.trim();
    let (left, right) = line.split_once('=')?;
    if left.trim() != key {
        return None;
    }

    let value = right
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .to_string();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{ffi::OsString, sync::Mutex};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvGuard {
        saved: Vec<(&'static str, Option<OsString>)>,
    }

    impl EnvGuard {
        fn new(keys: &[&'static str]) -> Self {
            Self {
                saved: keys
                    .iter()
                    .map(|key| (*key, std::env::var_os(key)))
                    .collect(),
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (key, value) in &self.saved {
                if let Some(value) = value {
                    std::env::set_var(key, value);
                } else {
                    std::env::remove_var(key);
                }
            }
        }
    }

    #[test]
    fn builds_chat_completions_url() {
        assert_eq!(
            chat_completions_url("https://api.example.com/v1"),
            "https://api.example.com/v1/chat/completions"
        );
        assert_eq!(
            chat_completions_url("https://api.example.com/v1/chat/completions"),
            "https://api.example.com/v1/chat/completions"
        );
    }

    #[test]
    fn reads_v100_glm_key() {
        let text = r#"
[providers.glm]
default_model = "GLM-5.1"

[providers.glm.auth]
key = "secret"

[providers.other.auth]
key = "ignored"
"#;

        assert_eq!(v100_glm_api_key_from_str(text).as_deref(), Some("secret"));
    }

    #[test]
    fn custom_provider_requires_anaphor_api_key() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _env = EnvGuard::new(&["ANAPHOR_API_KEY", "OPENAI_API_KEY", "ANAPHOR_BASE_URL"]);
        std::env::remove_var("ANAPHOR_API_KEY");
        std::env::set_var("OPENAI_API_KEY", "openai-secret");
        std::env::set_var("ANAPHOR_BASE_URL", "https://api.example.com/v1");

        let error = match openai_compatible_endpoint("custom") {
            Ok(_) => panic!("custom provider accepted OPENAI_API_KEY"),
            Err(error) => error.to_string(),
        };
        assert!(error.contains("ANAPHOR_API_KEY"));

        std::env::set_var("ANAPHOR_API_KEY", "custom-secret");
        let endpoint = openai_compatible_endpoint("custom").unwrap();

        assert_eq!(endpoint.api_key, "custom-secret");
        assert_eq!(
            endpoint.chat_url,
            "https://api.example.com/v1/chat/completions"
        );
    }

    #[test]
    fn openai_provider_can_use_openai_api_key() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _env = EnvGuard::new(&[
            "ANAPHOR_API_KEY",
            "OPENAI_API_KEY",
            "ANAPHOR_BASE_URL",
            "ANAPHOR_MODEL",
        ]);
        std::env::remove_var("ANAPHOR_API_KEY");
        std::env::remove_var("ANAPHOR_BASE_URL");
        std::env::remove_var("ANAPHOR_MODEL");
        std::env::set_var("OPENAI_API_KEY", "openai-secret");

        let endpoint = openai_compatible_endpoint("openai").unwrap();

        assert_eq!(endpoint.api_key, "openai-secret");
        assert_eq!(endpoint.model, "gpt-4o-mini");
        assert_eq!(
            endpoint.chat_url,
            "https://api.openai.com/v1/chat/completions"
        );
    }
}
