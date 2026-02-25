use anyhow::Context;
use std::env;
use std::path::{Path, PathBuf};

fn home_dir() -> anyhow::Result<PathBuf> {
    // Minimal cross-platform-ish fallback without extra deps.
    // On Unix, HOME is standard. (Windows support can be expanded later.)
    let home = env::var_os("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home))
}

fn ensure_dir(path: &Path) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(path)
        .with_context(|| format!("failed to create directory: {}", path.display()))?;
    Ok(path.to_path_buf())
}

fn gemini_home() -> Option<PathBuf> {
    env::var_os("GEMINI_HOME").map(PathBuf::from)
}

pub fn config_dir() -> anyhow::Result<PathBuf> {
    if let Some(base) = gemini_home() {
        return ensure_dir(&base.join("config"));
    }

    if let Some(xdg) = env::var_os("XDG_CONFIG_HOME").map(PathBuf::from) {
        return ensure_dir(&xdg.join("gemini"));
    }

    ensure_dir(&home_dir()?.join(".config").join("gemini"))
}

pub fn state_dir() -> anyhow::Result<PathBuf> {
    if let Some(base) = gemini_home() {
        return ensure_dir(&base.join("state"));
    }

    if let Some(xdg) = env::var_os("XDG_STATE_HOME").map(PathBuf::from) {
        return ensure_dir(&xdg.join("gemini"));
    }

    ensure_dir(&home_dir()?.join(".local").join("state").join("gemini"))
}

pub fn google_token_path() -> anyhow::Result<PathBuf> {
    Ok(state_dir()?.join("google_oauth_token.json"))
}

#[cfg(feature = "mcp")]
pub fn mcp_servers_path() -> anyhow::Result<PathBuf> {
    Ok(state_dir()?.join("mcp_servers.json"))
}
