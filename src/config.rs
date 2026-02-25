use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    /// Default model (optional)
    pub model: Option<String>,

    /// Provider identifier (e.g., "google"); reserved for later.
    pub provider: Option<String>,
}

impl Config {
    /// Load config if the file exists, otherwise return Ok(None).
    pub fn load_optional(path: impl AsRef<Path>) -> anyhow::Result<Option<Self>> {
        let path = path.as_ref();
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => {
                return Err(anyhow::Error::new(e))
                    .with_context(|| format!("failed to read config: {}", path.display()))
            }
        };

        let s = String::from_utf8(bytes).context("config is not valid UTF-8")?;
        let cfg: Config = toml::from_str(&s)
            .with_context(|| format!("failed to parse TOML: {}", path.display()))?;
        Ok(Some(cfg))
    }
}
