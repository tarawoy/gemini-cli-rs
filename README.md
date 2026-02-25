# gemini-cli-rs (Phase A scaffold)

Rust rewrite scaffold for a `gemini` CLI.

This repo intentionally **does not** ship real API keys or a working Gemini network implementation yet. Phase A focuses on:

- A Clap-based `gemini` binary
- CLI placeholders: `-m/--model` and `--include-directories`
- Config + state directory resolution with `GEMINI_HOME` override
- A stub provider module with a streaming output scaffold

## Build

```bash
cargo build
cargo run -- --help
```

Musl-friendly dependency choices:
- `reqwest` with `rustls-tls` (no OpenSSL)
- `tokio` async runtime

## Usage

```bash
# Basic (stub) streaming output
cargo run -- "Hello from Phase A"

# Choose model (placeholder)
cargo run -- -m gemini-2.0-flash "Say hello"

# Include directories (placeholder; currently only resolved/printed)
cargo run -- --include-directories ./src -- "Summarize this project"
```

## Directories

The CLI resolves config + state directories as follows:

### GEMINI_HOME override
If `GEMINI_HOME` is set:
- config dir: `$GEMINI_HOME/config`
- state dir:  `$GEMINI_HOME/state`

### XDG fallback (Linux/macOS)
If `GEMINI_HOME` is not set:
- config dir: `${XDG_CONFIG_HOME:-$HOME/.config}/gemini`
- state dir:  `${XDG_STATE_HOME:-$HOME/.local/state}/gemini`

Both directories are created on startup.

## Configuration (placeholder)

A config file location is reserved at:

- `{config_dir}/config.toml`

Phase A loads it if present but does not require it.

Example `config.toml` (future-facing):

```toml
# provider = "google"
# model = "gemini-2.0-flash"
# api_key_env = "GEMINI_API_KEY"
```

## Provider scaffold

`provider::stub::StubProvider` demonstrates a streaming interface that yields text chunks over time.

This is where a real Gemini provider will be implemented later using `reqwest` streaming (SSE/HTTP chunked) and proper auth.

## Next steps (Phase B+ ideas)

- Implement real Gemini provider:
  - Read API key from env (e.g., `GEMINI_API_KEY`) or OS keychain
  - Perform streaming request with `reqwest` and parse chunks
- Add subcommands:
  - `gemini chat`, `gemini prompt`, `gemini models`, `gemini auth`
- Implement `--include-directories` behavior:
  - Walk directories, apply ignore rules (.gitignore), size caps
  - Provide file snippets or structured context to the model
- Persist state:
  - history, cache, last-used model
- Add tests:
  - path resolution matrix (GEMINI_HOME vs XDG)
  - CLI parsing snapshots

