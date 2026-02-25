use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// Gemini CLI (Rust)
#[derive(Debug, Parser)]
#[command(name = "gemini")]
#[command(version)]
#[command(about = "Gemini CLI (Rust)", long_about = None)]
pub struct Args {
    /// Model name
    #[arg(short = 'm', long = "model")]
    pub model: Option<String>,

    /// Directories to include as context (placeholder)
    #[arg(long = "include-directories", value_name = "DIR")]
    pub include_directories: Vec<PathBuf>,

    /// Provider (default: config/provider or "google")
    #[arg(long = "provider")]
    pub provider: Option<String>,

    #[command(subcommand)]
    pub cmd: Option<Command>,

    /// Prompt text (positional) (used when no subcommand is given)
    #[arg(value_name = "PROMPT")]
    pub prompt: Vec<String>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Authenticate using Google OAuth device-code flow and save token under state
    Login,

    /// Run an interactive terminal chat UI
    #[cfg(feature = "tui")]
    Tui,

    /// Manage MCP stdio servers (config) and inspect tools
    #[cfg(feature = "mcp")]
    Mcp {
        #[command(subcommand)]
        cmd: McpCommand,
    },
}

#[cfg(feature = "mcp")]
#[derive(Debug, Subcommand)]
pub enum McpCommand {
    /// Add a server
    Add {
        /// Server name
        name: String,
        /// Command to execute (e.g. "node")
        command: String,
        /// Remaining args passed to the command
        args: Vec<String>,
    },
    /// List configured servers
    List,
    /// Remove a server by name
    Remove { name: String },
    /// Enable a server
    Enable { name: String },
    /// Disable a server
    Disable { name: String },
    /// Print discovered tools from enabled servers
    Tools,
}
