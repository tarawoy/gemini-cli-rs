#![cfg(feature = "mcp")]

mod stdio;
mod tools;

use crate::cli::McpCommand;
use crate::paths;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct McpServersFile {
    #[serde(default)]
    servers: Vec<McpServerConfig>,
}

pub async fn cmd_mcp(cmd: McpCommand) -> anyhow::Result<()> {
    match cmd {
        McpCommand::Add { name, command, args } => {
            let mut file = load()?;
            if file.servers.iter().any(|s| s.name == name) {
                anyhow::bail!("server already exists: {name}");
            }
            file.servers.push(McpServerConfig {
                name,
                command,
                args,
                enabled: true,
            });
            save(&file)?;
            Ok(())
        }
        McpCommand::List => {
            let file = load()?;
            if file.servers.is_empty() {
                println!("(no MCP servers configured)");
                return Ok(());
            }
            for s in &file.servers {
                println!(
                    "{}\t{}\t{} {:?}",
                    if s.enabled { "enabled" } else { "disabled" },
                    s.name,
                    s.command,
                    s.args
                );
            }
            Ok(())
        }
        McpCommand::Remove { name } => {
            let mut file = load()?;
            let before = file.servers.len();
            file.servers.retain(|s| s.name != name);
            if file.servers.len() == before {
                anyhow::bail!("no such server: {name}");
            }
            save(&file)?;
            Ok(())
        }
        McpCommand::Enable { name } => {
            let mut file = load()?;
            let mut found = false;
            for s in &mut file.servers {
                if s.name == name {
                    s.enabled = true;
                    found = true;
                }
            }
            if !found {
                anyhow::bail!("no such server: {name}");
            }
            save(&file)?;
            Ok(())
        }
        McpCommand::Disable { name } => {
            let mut file = load()?;
            let mut found = false;
            for s in &mut file.servers {
                if s.name == name {
                    s.enabled = false;
                    found = true;
                }
            }
            if !found {
                anyhow::bail!("no such server: {name}");
            }
            save(&file)?;
            Ok(())
        }
        McpCommand::Tools => {
            let file = load()?;
            let enabled: Vec<_> = file.servers.iter().filter(|s| s.enabled).cloned().collect();
            if enabled.is_empty() {
                println!("(no enabled MCP servers)");
                return Ok(());
            }

            let mut reg = tools::ToolRegistry::default();
            for s in enabled {
                let tools = stdio::list_tools(&s)
                    .await
                    .with_context(|| format!("failed to list tools from server {}", s.name))?;
                reg.register_server_tools(&s.name, tools);
            }

            for t in reg.list() {
                println!("{}\t{}\t{}", t.server, t.name, t.description.as_deref().unwrap_or(""));
            }
            Ok(())
        }
    }
}

fn load() -> anyhow::Result<McpServersFile> {
    let path = paths::mcp_servers_path()?;
    load_from(&path)
}

fn load_from(path: &Path) -> anyhow::Result<McpServersFile> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(McpServersFile::default()),
        Err(e) => {
            return Err(anyhow::Error::new(e))
                .with_context(|| format!("failed to read MCP servers file: {}", path.display()))
        }
    };

    let parsed: McpServersFile = serde_json::from_slice(&bytes)
        .with_context(|| format!("failed to parse JSON: {}", path.display()))?;
    Ok(parsed)
}

fn save(file: &McpServersFile) -> anyhow::Result<()> {
    let path = paths::mcp_servers_path()?;
    save_to(&path, file)
}

fn save_to(path: &PathBuf, file: &McpServersFile) -> anyhow::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)
        .with_context(|| format!("failed to create state dir: {}", dir.display()))?;

    let tmp = {
        let mut p = path.clone();
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "mcp_servers.json".to_string());
        p.set_file_name(format!("{name}.tmp"));
        p
    };

    let bytes = serde_json::to_vec_pretty(file).context("failed to encode JSON")?;
    std::fs::write(&tmp, bytes).with_context(|| format!("failed to write: {}", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .with_context(|| format!("failed to replace: {}", path.display()))?;
    Ok(())
}
