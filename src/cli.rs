use clap::Parser;
use std::path::PathBuf;

/// Gemini CLI (Rust rewrite scaffold)
#[derive(Debug, Parser)]
#[command(name = "gemini")]
#[command(version)]
#[command(about = "Gemini CLI (Phase A scaffold)", long_about = None)]
pub struct Args {
    /// Model name (placeholder)
    #[arg(short = 'm', long = "model")]
    pub model: Option<String>,

    /// Directories to include as context (placeholder)
    #[arg(long = "include-directories", value_name = "DIR")]
    pub include_directories: Vec<PathBuf>,

    /// Prompt text (positional)
    #[arg(value_name = "PROMPT")]
    pub prompt: Vec<String>,
}
