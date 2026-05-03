mod config;
mod constants;
mod llm;
mod prompt;
mod render;
mod tools;
mod types;

use anyhow::Result;
use clap::Parser;
use config::{default_max_chars, default_model};
use llm::{call_llm, plan_tools};
use prompt::build_prompt;
use render::{render_markdownish, should_color};
use std::io::Read;
use tools::{gather_context, has_content, truncate};
use types::ContextBlock;

#[derive(Parser)]
#[command(name = "anaphor", about = "Terminal context resolver")]
struct Cli {
    #[arg(help = "Question to answer")]
    question: Option<String>,
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
    let prompt = build_prompt(&question, &contexts);
    let answer = call_llm(&model, &prompt).await?;

    println!("{}", render_markdownish(&answer, should_color()));

    Ok(())
}
