# Anaphor

A minimal Rust CLI tool for resolving terminal context using LLMs.

## Run

Works anywhere Rust/Cargo is available:

```bash
cargo run -- "summarize the readme"
```

On Linux/macOS, build once and install shell commands:

```bash
cargo build --release
mkdir -p ~/.local/bin
ln -sf "$PWD/target/release/anaphor" ~/.local/bin/anaphor
ln -sf "$PWD/target/release/anaphor" ~/.local/bin/a
```

Make sure `~/.local/bin` is on your `PATH`. After that, use `anaphor` or the shorter `a` from any shell:

```bash
anaphor "summarize the readme"
a "summarize the readme"
```

## Setup

1. Copy `.env.example` to `.env`:
   ```bash
   cp .env.example .env
   ```

2. Get API keys:
   - OpenRouter: https://openrouter.ai (free tier available)
   - Brave Search: https://api.search.brave.com (free tier available)

3. Fill in your `.env` file with your API keys

## Usage

Ask questions with context from various sources:

```bash
# Direct question
cargo run -- "what is WAL mode in sqlite?"

# Web search chosen automatically
cargo run -- "find me links about the axum web framework"

# Fetch a URL chosen automatically
cargo run -- "what is rust-lang.org about?"

# Read a local file chosen automatically
cargo run -- "summarize the readme"

# Stdin
cat notes.md | cargo run -- "extract action items"
```

## Environment Variables

- `OPENROUTER_API_KEY` — required for LLM calls
- `BRAVE_API_KEY` — required when Anaphor chooses web search
- `ANAPHOR_MODEL` — optional, defaults to `openai/gpt-4o-mini`
- `ANAPHOR_MAX_CHARS` — optional total context character budget, defaults to `30000`

### OpenRouter Models

You can use any OpenRouter model. Some cheap options:
- `openai/gpt-4o-mini` — excellent quality, very cheap
- `google/gemini-flash-1.5` — fast and cheap
- `meta-llama/llama-3-8b-instruct` — fast and free

## Interface

Anaphor intentionally has no source-selection flags. Ask in natural language and it will choose from three internal tools:

- `search_web` — searches Brave for links and snippets
- `read_url` — fetches a URL or bare domain
- `read_file` — reads a local file from the current working directory
