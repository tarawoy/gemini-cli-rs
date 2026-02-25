use anyhow::{anyhow, Context};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// OAuth token persisted on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthToken {
    pub access_token: String,
    pub token_type: String,
    pub scope: Option<String>,
    pub refresh_token: Option<String>,

    /// Seconds since UNIX epoch.
    pub obtained_at: u64,

    /// Lifetime in seconds.
    pub expires_in: Option<u64>,
}

impl OAuthToken {
    pub fn expires_at(&self) -> Option<u64> {
        self.expires_in.map(|s| self.obtained_at.saturating_add(s))
    }

    pub fn is_valid_for(&self, skew: Duration) -> bool {
        let Some(exp) = self.expires_at() else {
            // No expiry? Treat as valid.
            return true;
        };
        let now = now_secs();
        let skew = skew.as_secs();
        now.saturating_add(skew) < exp
    }
}

#[derive(Debug, Clone)]
pub struct OAuthClient {
    pub client_id: String,
    pub client_secret: Option<String>,
    pub scopes: Vec<String>,

    pub device_code_url: Url,
    pub token_url: Url,
}

impl OAuthClient {
    pub fn google_device_flow(client_id: String, client_secret: Option<String>, scopes: Vec<String>) -> anyhow::Result<Self> {
        Ok(Self {
            client_id,
            client_secret,
            scopes,
            device_code_url: Url::parse("https://oauth2.googleapis.com/device/code")?,
            token_url: Url::parse("https://oauth2.googleapis.com/token")?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_url: String,
    #[serde(default)]
    verification_uri: Option<String>,
    #[serde(default)]
    verification_uri_complete: Option<String>,
    expires_in: u64,
    #[serde(default)]
    interval: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TokenSuccessResponse {
    access_token: String,
    token_type: String,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    refresh_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TokenErrorResponse {
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

pub async fn device_login(
    http: &reqwest::Client,
    oauth: &OAuthClient,
    out: &mut dyn std::io::Write,
) -> anyhow::Result<OAuthToken> {
    let scope = if oauth.scopes.is_empty() {
        "".to_string()
    } else {
        oauth.scopes.join(" ")
    };

    let mut form: Vec<(&str, String)> = vec![("client_id", oauth.client_id.clone()), ("scope", scope)];

    let resp = http
        .post(oauth.device_code_url.clone())
        .form(&form)
        .send()
        .await
        .context("failed to request device code")?;

    let status = resp.status();
    let body = resp.bytes().await.context("failed to read device code response")?;
    if !status.is_success() {
        return Err(anyhow!("device code request failed: HTTP {status}: {}", String::from_utf8_lossy(&body)));
    }

    let dc: DeviceCodeResponse = serde_json::from_slice(&body).context("failed to parse device code JSON")?;

    let verify = dc
        .verification_uri_complete
        .clone()
        .or_else(|| dc.verification_uri.clone())
        .unwrap_or_else(|| dc.verification_url.clone());

    writeln!(out, "Open this URL in your browser and complete sign-in:")?;
    writeln!(out, "  {verify}")?;
    writeln!(out, "Then enter code:")?;
    writeln!(out, "  {}", dc.user_code)?;
    writeln!(out)?;

    let interval = Duration::from_secs(dc.interval.unwrap_or(5).max(1));
    let expires_at = SystemTime::now()
        .checked_add(Duration::from_secs(dc.expires_in))
        .ok_or_else(|| anyhow!("time overflow"))?;

    let mut poll_interval = interval;

    loop {
        if SystemTime::now() > expires_at {
            return Err(anyhow!("device code expired; please run login again"));
        }

        tokio::time::sleep(poll_interval).await;

        let mut form: Vec<(&str, String)> = vec![
            ("client_id", oauth.client_id.clone()),
            ("device_code", dc.device_code.clone()),
            (
                "grant_type",
                "urn:ietf:params:oauth:grant-type:device_code".to_string(),
            ),
        ];
        if let Some(secret) = oauth.client_secret.clone() {
            form.push(("client_secret", secret));
        }

        let resp = http
            .post(oauth.token_url.clone())
            .form(&form)
            .send()
            .await
            .context("failed to poll token endpoint")?;

        let status = resp.status();
        let body = resp.bytes().await.context("failed to read token response")?;

        if status.is_success() {
            let ok: TokenSuccessResponse = serde_json::from_slice(&body).context("failed to parse token JSON")?;
            return Ok(OAuthToken {
                access_token: ok.access_token,
                token_type: ok.token_type,
                scope: ok.scope,
                refresh_token: ok.refresh_token,
                obtained_at: now_secs(),
                expires_in: ok.expires_in,
            });
        }

        // Google uses 400 with JSON body for device flow errors.
        let err: TokenErrorResponse = match serde_json::from_slice(&body) {
            Ok(e) => e,
            Err(_) => {
                return Err(anyhow!(
                    "token endpoint failed: HTTP {status}: {}",
                    String::from_utf8_lossy(&body)
                ))
            }
        };

        match err.error.as_str() {
            "authorization_pending" => {
                // keep polling
            }
            "slow_down" => {
                poll_interval += Duration::from_secs(5);
            }
            "expired_token" => {
                return Err(anyhow!("device code expired; please run login again"));
            }
            "access_denied" => {
                return Err(anyhow!("access denied by user"));
            }
            other => {
                let desc = err.error_description.unwrap_or_default();
                return Err(anyhow!("oauth error: {other}: {desc}"));
            }
        }
    }
}

pub async fn refresh_if_needed(
    http: &reqwest::Client,
    oauth: &OAuthClient,
    token: OAuthToken,
) -> anyhow::Result<OAuthToken> {
    if token.is_valid_for(Duration::from_secs(30)) {
        return Ok(token);
    }

    let Some(refresh_token) = token.refresh_token.clone() else {
        return Err(anyhow!("access token expired and no refresh_token is available; run `gemini login`"));
    };

    let mut form: Vec<(&str, String)> = vec![
        ("client_id", oauth.client_id.clone()),
        ("refresh_token", refresh_token),
        ("grant_type", "refresh_token".to_string()),
    ];
    if let Some(secret) = oauth.client_secret.clone() {
        form.push(("client_secret", secret));
    }

    let resp = http
        .post(oauth.token_url.clone())
        .form(&form)
        .send()
        .await
        .context("failed to refresh token")?;

    let status = resp.status();
    let body = resp.bytes().await.context("failed to read refresh response")?;
    if !status.is_success() {
        let msg = String::from_utf8_lossy(&body);
        return Err(anyhow!("refresh failed: HTTP {status}: {msg}"));
    }

    let ok: TokenSuccessResponse = serde_json::from_slice(&body).context("failed to parse refresh token JSON")?;

    Ok(OAuthToken {
        access_token: ok.access_token,
        token_type: ok.token_type,
        scope: ok.scope,
        // Google often omits refresh_token on refresh; keep existing.
        refresh_token: token.refresh_token,
        obtained_at: now_secs(),
        expires_in: ok.expires_in,
    })
}

pub fn load_token(path: impl AsRef<Path>) -> anyhow::Result<Option<OAuthToken>> {
    let path = path.as_ref();
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(anyhow!(e)).with_context(|| format!("failed to read token: {}", path.display())),
    };
    let tok: OAuthToken = serde_json::from_slice(&bytes).context("failed to parse token JSON")?;
    Ok(Some(tok))
}

pub fn save_token_atomic(path: impl AsRef<Path>, tok: &OAuthToken) -> anyhow::Result<()> {
    let path = path.as_ref();
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)
        .with_context(|| format!("failed to create token directory: {}", dir.display()))?;

    let tmp = tmp_path(path);
    let bytes = serde_json::to_vec_pretty(tok).context("failed to serialize token")?;
    std::fs::write(&tmp, bytes).with_context(|| format!("failed to write temp token: {}", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .with_context(|| format!("failed to move token into place: {}", path.display()))?;
    Ok(())
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut p = path.to_path_buf();
    let file = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "token.json".to_string());
    p.set_file_name(format!("{file}.tmp"));
    p
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs()
}
