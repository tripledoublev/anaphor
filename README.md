# Anaphor

A minimal terminal context resolver. Ask in natural language; Anaphor gathers the relevant context and answers with an LLM.

## Why the name?

In linguistics, *anaphora* is an expression whose meaning depends on another expression in context. Anaphor does the same for terminal questions: it finds the antecedent context needed to answer.

## How it works

You ask a question. Anaphor:

1. Plans which tools to use (web search, fetch URL, read file, or none)
2. Gathers context from selected sources
3. Provides an LLM-powered answer grounded in that context

No source-selection flags. Just natural language.

```bash
anaphor "what is rust-lang.org about?"
# recognizes a URL, fetches it, and answers about the page

anaphor "find me links about axum"
# recognizes search intent and returns links with summaries

anaphor "summarize the readme"
# recognizes a file reference, reads README.md, and summarizes it
```

## Prerequisites

- **Rust 1.70+** — to build from source
- **OpenRouter API key** — required, free tier at https://openrouter.ai
- **Brave Search API key** — optional, free tier at https://api.search.brave.com, only needed for web search
- **Internet connection** — for API calls (30s timeout for web/URLs, 90s for LLM)

## Setup

1. Run from source:
   ```bash
   cargo run -- "summarize the readme"
   ```

2. Or build once:
   ```bash
   cargo build --release
   ```

3. Optional: install commands on Linux/macOS:
   ```bash
   mkdir -p ~/.local/bin
   ln -sf "$PWD/target/release/anaphor" ~/.local/bin/anaphor
   ln -sf "$PWD/target/release/anaphor" ~/.local/bin/a
   ```
   Ensure `~/.local/bin` is on `$PATH`. Then use `anaphor` or the shorter `a`.

4. Configure:
   ```bash
   cp .env.example .env
   # Fill in OPENROUTER_API_KEY (required) and BRAVE_API_KEY (optional)
   ```

5. Test:
   ```bash
   cargo run -- "what is Rust?"
   ```

## Usage

```bash
# Direct question (no tools needed)
cargo run -- "what is WAL mode in sqlite?"

# Web search (auto-detected)
cargo run -- "find me links about the axum web framework"

# Fetch URL (auto-detected)
cargo run -- "what is rust-lang.org about?"

# Read file (auto-detected)
cargo run -- "summarize the readme"

# Stdin (auto-included)
cat notes.md | cargo run -- "extract action items"
```

If you installed the command, replace `cargo run --` with `anaphor` or `a`.

## Configuration

Environment variables (set in `.env` or shell):

| Variable | Required | Default | Notes |
|----------|----------|---------|-------|
| `OPENROUTER_API_KEY` | Yes | — | Get at https://openrouter.ai |
| `BRAVE_API_KEY` | No | — | Get at https://api.search.brave.com (only needed for web search) |
| `ANAPHOR_MODEL` | No | `openai/gpt-4o-mini` | Any OpenRouter model ID |
| `ANAPHOR_MAX_CHARS` | No | `30000` | Total context budget across all sources |

### Recommended models

- `openai/gpt-4o-mini` — best quality, very cheap
- `google/gemini-flash-1.5` — fast and cheap
- `meta-llama/llama-3-8b-instruct` — fast and free

## Tools

Anaphor automatically routes to three internal tools based on your question:

- **search_web** — "find links about X", "search for Y"
- **read_url** — "what is example.com?", "summarize rust-lang.org"
- **read_file** — "summarize README", "what's in Cargo.toml?"

No flags. No syntax to learn. Just ask.

## Security

- File reads are sandboxed to the current working directory
- HTTP requests timeout (30s for web/URLs, 90s for LLM) to prevent hanging
- Tool errors are sanitized before being sent to the LLM
- Context is truncated to your budget (default 30KB) to manage API costs
