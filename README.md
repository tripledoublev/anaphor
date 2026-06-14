# Anaphor

A minimal terminal context resolver. Ask in natural language; Anaphor gathers the relevant context and answers with an LLM.
  
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
- **LLM API access** — OpenRouter by default, or a GLM/OpenAI-compatible endpoint
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
   # Fill in OPENROUTER_API_KEY for the default provider, or configure another provider
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
| `OPENROUTER_API_KEY` | Default provider | — | Get at https://openrouter.ai |
| `ANAPHOR_PROVIDER` | No | `openrouter` | Use `openrouter`, `glm`, `openai`, or `custom` |
| `ANAPHOR_API_KEY` | Custom/GLM | — | API key for OpenAI-compatible endpoints; `custom` requires this, `openai` may also use `OPENAI_API_KEY`, and GLM also checks `ZHIPU_API_KEY` plus `~/.config/v100/config.toml` |
| `ANAPHOR_BASE_URL` | Custom | — | OpenAI-compatible base URL, for example `https://api.example.com/v1` |
| `BRAVE_API_KEY` | No | — | Get at https://api.search.brave.com (only needed for web search) |
| `ANAPHOR_MODEL` | No | `openai/gpt-4o-mini` | Model ID for OpenRouter/OpenAI-compatible providers |
| `ANAPHOR_GLM_MODEL` | No | `glm-5.1` | Used only with `ANAPHOR_PROVIDER=glm` |
| `ANAPHOR_MAX_CHARS` | No | `30000` | Total context budget across all sources |

### Direct GLM

If you have the v100 GLM config locally, Anaphor can use it directly:

```bash
ANAPHOR_PROVIDER=glm anaphor "what is Rust?"
```

This uses `https://api.z.ai/api/coding/paas/v4`, model `glm-5.1`, and reads `providers.glm.auth.key` from `~/.config/v100/config.toml` when no GLM key is set in the environment.

### Recommended models

- `openai/gpt-4o-mini` — best quality, very cheap
- `z-ai/glm-5.1` — strong OpenRouter GLM option with tool support
- `glm-5.1` — direct GLM/Z.ai subscription option
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
