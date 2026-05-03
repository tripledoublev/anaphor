const ANSI_RESET: &str = "\x1b[0m";
const ANSI_BOLD: &str = "\x1b[1m";
const ANSI_DIM: &str = "\x1b[2m";
const ANSI_CYAN: &str = "\x1b[36m";
const ANSI_GREEN: &str = "\x1b[32m";
const ANSI_YELLOW: &str = "\x1b[33m";

pub(crate) fn should_color() -> bool {
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

pub(crate) fn render_markdownish(input: &str, color: bool) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
