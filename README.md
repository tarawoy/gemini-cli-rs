# gemini-cli-rs

A musl-friendly Rust `gemini` CLI with:

- Google Gemini (Generative Language API) streaming (SSE) via `reqwest` + `rustls`
- Google OAuth **device-code** login (optional) with token persisted under `GEMINI_HOME`/XDG state
- Optional TUI chat (`ratatui`) with streaming
- Optional MCP stdio client + tool discovery and server config

## Build

```bash
cargo build
cargo run -- --help
```

Feature flags:

```bash
# TUI
cargo build --features tui

# MCP
cargo build --features mcp

# TUI + MCP
cargo build --features "tui mcp"
```

## Quick start (API key)

1) Create an API key in Google AI Studio.
2) Run with `GEMINI_API_KEY`:

```bash
export GEMINI_API_KEY="..."

# basic prompt
cargo run -- "Hello"

# pick a model
cargo run -- -m gemini-1.5-flash "Write a haiku about Rust"
```

## OAuth device-code login (optional)

This is useful when you want to use OAuth instead of an API key.

### 1) Provide OAuth client id (and optional secret)

Set env vars:

```bash
export GEMINI_OAUTH_CLIENT_ID="..."
# optional
export GEMINI_OAUTH_CLIENT_SECRET="..."
```

Or put them in `{config_dir}/config.toml`:

```toml
provider = "google"
model = "gemini-1.5-flash"

[google]
# api_key = "..." # optional alternative

[google.oauth]
client_id = "..."
# client_secret = "..." # optional
# scopes = ["https://www.googleapis.com/auth/generative-language"]
```

### 2) Login

```bash
cargo run -- login
```

The device-code token is saved under the state directory (see **Directories** below) as:

- `google_oauth_token.json`

After login, running `gemini ...` will use the saved token if no API key is present.

## TUI chat (streaming)

Requires the `tui` feature:

```bash
cargo run --features tui -- tui
```

TUI commands:

- `/quit` (or `Esc`) to exit
- `/clear` to clear chat
- `/model <name>` to change model

## MCP stdio servers (config + tool discovery)

Requires the `mcp` feature.

Servers are stored under state as `mcp_servers.json`.

```bash
# add a server (enabled by default)
cargo run --features mcp -- mcp add myserver node path/to/server.js

# list configured servers
cargo run --features mcp -- mcp list

# disable / enable
cargo run --features mcp -- mcp disable myserver
cargo run --features mcp -- mcp enable myserver

# list tools from all enabled servers
cargo run --features mcp -- mcp tools
```

## Directories

The CLI resolves config + state directories as follows.

### `GEMINI_HOME` override
If `GEMINI_HOME` is set:
- config dir: `$GEMINI_HOME/config`
- state dir:  `$GEMINI_HOME/state`

### XDG fallback (Linux/macOS)
If `GEMINI_HOME` is not set:
- config dir: `${XDG_CONFIG_HOME:-$HOME/.config}/gemini`
- state dir:  `${XDG_STATE_HOME:-$HOME/.local/state}/gemini`

Both directories are created on startup.

## Notes

- HTTP is `reqwest` with `rustls-tls` (no OpenSSL).
- Streaming uses SSE (`alt=sse`) for `models/{model}:streamGenerateContent`.
