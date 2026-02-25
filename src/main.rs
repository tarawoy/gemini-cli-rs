mod app;
mod auth;
mod cli;
mod config;
mod paths;
mod provider;

#[cfg(feature = "mcp")]
mod mcp;
#[cfg(feature = "tui")]
mod tui;

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
    let _state_dir = paths::state_dir()?;

    let cfg = config::Config::load_optional(config_dir.join("config.toml"))?;
    tracing::debug!(?config_dir, ?cfg, "resolved config");

    let http = reqwest::Client::builder()
        .user_agent(concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION")))
        .build()
        .context("failed to build HTTP client")?;

    match args.cmd {
        Some(cli::Command::Login) => {
            return app::cmd_login(&http, cfg.as_ref()).await;
        }
        #[cfg(feature = "mcp")]
        Some(cli::Command::Mcp { cmd }) => {
            return mcp::cmd_mcp(cmd).await;
        }
        #[cfg(feature = "tui")]
        Some(cli::Command::Tui) => {
            return tui::run_tui(cfg.as_ref(), args.model.clone()).await;
        }
        None => {}
    }

    let prompt = args.prompt.join(" ");
    if prompt.trim().is_empty() {
        anyhow::bail!("No prompt provided. Try: gemini \"Hello\" or `gemini tui` (feature flag)");
    }

    let model = args
        .model
        .clone()
        .or_else(|| cfg.as_ref().and_then(|c| c.model.clone()))
        .unwrap_or_else(|| "gemini-1.5-flash".to_string());

    let provider_name = args
        .provider
        .clone()
        .or_else(|| cfg.as_ref().and_then(|c| c.provider.clone()))
        .unwrap_or_else(|| "google".to_string());

    let provider = app::build_provider(&http, cfg.as_ref(), &provider_name).await?;

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
        use std::io::Write;
        std::io::stdout().flush().ok();
    }
    println!();

    Ok(())
}
