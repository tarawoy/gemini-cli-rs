use crate::{auth, config, paths, provider};
use anyhow::Context;
use provider::Provider;

pub async fn cmd_login(http: &reqwest::Client, cfg: Option<&config::Config>) -> anyhow::Result<()> {
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

pub async fn build_provider(
    http: &reqwest::Client,
    cfg: Option<&config::Config>,
    provider_name: &str,
) -> anyhow::Result<Box<dyn Provider + Send + Sync>> {
    match provider_name {
        "google" => {
            #[cfg(feature = "google")]
            {
                let api_key = std::env::var("GEMINI_API_KEY")
                    .ok()
                    .or_else(|| cfg.and_then(|c| c.google.api_key.clone()));

                let auth = if let Some(key) = api_key {
                    provider::google::GoogleAuth::ApiKey(key)
                } else {
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
                        .unwrap_or_else(|| {
                            vec!["https://www.googleapis.com/auth/generative-language".to_string()]
                        });

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
