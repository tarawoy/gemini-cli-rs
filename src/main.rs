mod cli;
mod config;
mod paths;
mod provider;

use anyhow::Context;
use clap::Parser;
use provider::{ChatRequest, Provider};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let args = cli::Args::parse();

    // Resolve and create dirs early.
    let config_dir = paths::config_dir()?;
    let state_dir = paths::state_dir()?;

    let cfg = config::Config::load_optional(config_dir.join("config.toml"))?;

    tracing::debug!(?config_dir, ?state_dir, ?cfg, "resolved directories and config");

    // Placeholder: pick provider based on config/flags.
    let provider = provider::stub::StubProvider::new();

    let prompt = args.prompt.join(" ");
    if prompt.trim().is_empty() {
        anyhow::bail!("No prompt provided. Try: gemini \"Hello\"");
    }

    // Phase A: parse flags but don't do heavy logic yet.
    let model = args
        .model
        .clone()
        .or_else(|| cfg.as_ref().and_then(|c| c.model.clone()))
        .unwrap_or_else(|| "(default)".to_string());

    tracing::info!(model = %model, include_directories = ?args.include_directories, "starting request");

    let req = ChatRequest {
        model,
        prompt,
        include_directories: args.include_directories,
    };

    let mut stream = provider
        .stream_chat(req)
        .await
        .context("provider failed to start streaming")?;

    use tokio_stream::StreamExt;
    while let Some(item) = stream.next().await {
        let chunk = item.context("stream chunk error")?;
        print!("{}", chunk.text);
        // Keep stdout responsive during streaming.
        use std::io::Write;
        std::io::stdout().flush().ok();
    }
    println!();

    Ok(())
}
