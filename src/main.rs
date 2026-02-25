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
            return cmd_login(&http, cfg.as_ref()).await;
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

    let provider = build_provider(&http, cfg.as_ref(), &provider_name).await?;

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

async fn cmd_login(http: &reqwest::Client, cfg: Option<&config::Config>) -> anyhow::Result<()> {
    use std::io::Write;
    let client_id = std::env::var("GEMINI_OAUTH_CLIENT_ID")
        .ok()
        .or_else(|| cfg.and_then(|c| c.google.oauth.client_id.clone()))
        .context("missing OAuth client id (set GEMINI_OAUTH_CLIENT_ID or config.toml google.oauth.client_id)")?;

    let client_secret = std::env::var("GEMINI_OAUTH_CLIENT_SECRET")
        .ok()
        .or_else(|| cfg.and_then(|c| c.google.oauth.client_secret.clone()));

    let scopes = cfg
        .and_then(|c| c.google.oauth.scopes.clone())
        .unwrap_or_else(|| vec!["https://www.googleapis.com/auth/generative-language".to_string()]);

    let oauth = auth::OAuthClient::google_device_flow(client_id, client_secret, scopes)?;

    let mut out = std::io::stdout();
    let tok = auth::device_login(http, &oauth, &mut out).await?;

    let path = paths::google_token_path()?;
    auth::save_token_atomic(&path, &tok)?;

    writeln!(out, "Saved token to: {}", path.display()).ok();
    Ok(())
}

async fn build_provider(
    http: &reqwest::Client,
    cfg: Option<&config::Config>,
    provider: &str,
) -> anyhow::Result<Box<dyn Provider + Send + Sync>> {
    match provider {
        "google" => {
            #[cfg(feature = "google")]
            {
                let api_key = std::env::var("GEMINI_API_KEY")
                    .ok()
                    .or_else(|| cfg.and_then(|c| c.google.api_key.clone()));

                let auth = if let Some(key) = api_key {
                    provider::google::GoogleAuth::ApiKey(key)
                } else {
                    // Fall back to OAuth token from state.
                    let tok_path = paths::google_token_path()?;
                    let Some(tok) = auth::load_token(&tok_path)? else {
                        anyhow::bail!(
                            "No API key or OAuth token found. Set GEMINI_API_KEY or run `gemini login`. (token path: {})",
                            tok_path.display()
                        );
                    };

                    let client_id = std::env::var("GEMINI_OAUTH_CLIENT_ID")
                        .ok()
                        .or_else(|| cfg.and_then(|c| c.google.oauth.client_id.clone()))
                        .context("missing OAuth client id for refresh (set GEMINI_OAUTH_CLIENT_ID or config.toml)")?;

                    let client_secret = std::env::var("GEMINI_OAUTH_CLIENT_SECRET")
                        .ok()
                        .or_else(|| cfg.and_then(|c| c.google.oauth.client_secret.clone()));

                    let scopes = cfg
                        .and_then(|c| c.google.oauth.scopes.clone())
                        .unwrap_or_else(|| vec!["https://www.googleapis.com/auth/generative-language".to_string()]);

                    let oauth = auth::OAuthClient::google_device_flow(client_id, client_secret, scopes)?;
                    let tok = auth::refresh_if_needed(http, &oauth, tok).await?;
                    auth::save_token_atomic(&tok_path, &tok)?;
                    provider::google::GoogleAuth::BearerToken(tok.access_token)
                };

                let p = provider::google::GoogleProvider::new(http.clone(), auth)?;
                Ok(Box::new(p))
            }
            #[cfg(not(feature = "google"))]
            {
                let _ = http;
                let _ = cfg;
                anyhow::bail!("google provider is not enabled in this build")
            }
        }
        "stub" => Ok(Box::new(provider::stub::StubProvider::new())),
        other => anyhow::bail!("unknown provider: {other}"),
    }
}
