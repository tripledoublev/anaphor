use crate::types::ContextBlock;

pub(crate) fn build_prompt(question: &str, contexts: &[ContextBlock]) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
