use super::{ChatChunk, ChatRequest, Provider};
use anyhow::{anyhow, Context};
use futures_core::stream::BoxStream;
use futures_core::Stream;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;

#[derive(Debug, Clone)]
pub struct GoogleProvider {
    http: reqwest::Client,
    auth: GoogleAuth,
    api_base: Url,
}

#[derive(Debug, Clone)]
pub enum GoogleAuth {
    ApiKey(String),
    BearerToken(String),
}

impl GoogleProvider {
    pub fn new(http: reqwest::Client, auth: GoogleAuth) -> anyhow::Result<Self> {
        Ok(Self {
            http,
            auth,
            api_base: Url::parse("https://generativelanguage.googleapis.com/")?,
        })
    }

    fn build_url(&self, model: &str) -> anyhow::Result<Url> {
        // v1beta:streamGenerateContent supports Server-Sent Events with alt=sse.
        // Docs: https://ai.google.dev/api/rest/v1beta/models/streamGenerateContent
        let mut url = self
            .api_base
            .join(&format!("v1beta/models/{model}:streamGenerateContent"))?;

        match &self.auth {
            GoogleAuth::ApiKey(key) => {
                url.query_pairs_mut().append_pair("key", key);
            }
            GoogleAuth::BearerToken(_) => {
                // OAuth uses Authorization header.
            }
        }

        url.query_pairs_mut().append_pair("alt", "sse");
        Ok(url)
    }

    fn headers(&self) -> anyhow::Result<HeaderMap> {
        let mut h = HeaderMap::new();
        h.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        if let GoogleAuth::BearerToken(tok) = &self.auth {
            let v = HeaderValue::from_str(&format!("Bearer {tok}"))
                .map_err(|e| anyhow!(e))?;
            h.insert(AUTHORIZATION, v);
        }
        Ok(h)
    }
}

impl Provider for GoogleProvider {
    fn name(&self) -> &'static str {
        "google"
    }

    fn stream_chat(
        &self,
        req: ChatRequest,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = anyhow::Result<BoxStream<'static, anyhow::Result<ChatChunk>>>>
                + Send,
        >,
    > {
        let http = self.http.clone();
        let this = self.clone();

        Box::pin(async move {
            let url = this.build_url(&req.model)?;
            let headers = this.headers()?;

            let body = StreamGenerateContentRequest {
                contents: vec![Content {
                    role: Some("user".to_string()),
                    parts: vec![Part { text: Some(req.prompt) }],
                }],
            };

            let resp = http
                .post(url)
                .headers(headers)
                .json(&body)
                .send()
                .await
                .context("failed to start Gemini request")?;

            let status = resp.status();
            if !status.is_success() {
                let text = resp.text().await.unwrap_or_default();
                return Err(anyhow!("Gemini API error: HTTP {status}: {text}"));
            }

            let (tx, rx) = mpsc::channel::<anyhow::Result<ChatChunk>>(64);

            tokio::spawn(async move {
                let mut stream = resp.bytes_stream();
                let mut parser = SseParser::new();

                while let Some(item) = stream.next().await {
                    let bytes = match item {
                        Ok(b) => b,
                        Err(e) => {
                            let _ = tx.send(Err(anyhow!(e).context("network stream error"))).await;
                            return;
                        }
                    };

                    for ev in parser.push(&bytes) {
                        match ev {
                            Ok(SseEvent::Data(data)) => {
                                // Some events are "[DONE]" in other APIs; Gemini uses JSON always.
                                if data.trim().is_empty() {
                                    continue;
                                }

                                let parsed: Result<StreamGenerateContentResponse, _> =
                                    serde_json::from_str(&data);
                                match parsed {
                                    Ok(r) => {
                                        if let Some(text) = extract_text(&r) {
                                            if tx.send(Ok(ChatChunk { text })).await.is_err() {
                                                return;
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        let _ = tx
                                            .send(Err(anyhow!(e).context("failed to parse SSE JSON")))
                                            .await;
                                        return;
                                    }
                                }
                            }
                            Ok(SseEvent::Other) => {}
                            Err(e) => {
                                let _ = tx.send(Err(e)).await;
                                return;
                            }
                        }
                    }
                }
            });

            let out = ReceiverStream::new(rx).map(|x| x);
            Ok(Box::pin(out) as BoxStream<'static, anyhow::Result<ChatChunk>>)
        })
    }
}

#[derive(Debug, Clone, Serialize)]
struct StreamGenerateContentRequest {
    contents: Vec<Content>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StreamGenerateContentResponse {
    #[serde(default)]
    candidates: Vec<Candidate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Candidate {
    #[serde(default)]
    content: Option<Content>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Content {
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    parts: Vec<Part>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Part {
    #[serde(default)]
    text: Option<String>,
}

fn extract_text(r: &StreamGenerateContentResponse) -> Option<String> {
    // Concatenate all text parts of the first candidate.
    let cand = r.candidates.first()?;
    let content = cand.content.as_ref()?;
    let mut out = String::new();
    for p in &content.parts {
        if let Some(t) = &p.text {
            out.push_str(t);
        }
    }
    if out.is_empty() { None } else { Some(out) }
}

#[derive(Debug, Clone)]
enum SseEvent {
    Data(String),
    Other,
}

/// Minimal SSE parser.
///
/// - Collects UTF-8 lines
/// - Emits Data events when a blank line ends an event
struct SseParser {
    buf: Vec<u8>,
    cur_data: String,
}

impl SseParser {
    fn new() -> Self {
        Self {
            buf: Vec::new(),
            cur_data: String::new(),
        }
    }

    fn push(&mut self, chunk: &[u8]) -> Vec<anyhow::Result<SseEvent>> {
        self.buf.extend_from_slice(chunk);
        let mut out = Vec::new();

        loop {
            let Some(pos) = memchr::memchr(b'\n', &self.buf) else {
                break;
            };
            let mut line = self.buf.drain(..=pos).collect::<Vec<u8>>();
            if line.ends_with(&[b'\n']) {
                line.pop();
            }
            if line.ends_with(&[b'\r']) {
                line.pop();
            }

            if line.is_empty() {
                if !self.cur_data.is_empty() {
                    // Remove trailing newline from data field accumulation.
                    if self.cur_data.ends_with('\n') {
                        self.cur_data.pop();
                    }
                    let data = std::mem::take(&mut self.cur_data);
                    out.push(Ok(SseEvent::Data(data)));
                }
                continue;
            }

            let s = match std::str::from_utf8(&line) {
                Ok(s) => s,
                Err(e) => {
                    out.push(Err(anyhow!(e).context("SSE line is not valid UTF-8")));
                    continue;
                }
            };

            if let Some(rest) = s.strip_prefix("data:") {
                // Spec allows optional leading space.
                let rest = rest.strip_prefix(' ').unwrap_or(rest);
                self.cur_data.push_str(rest);
                self.cur_data.push('\n');
            } else {
                // Ignore other fields: event:, id:, retry:, comments
                out.push(Ok(SseEvent::Other));
            }
        }

        out
    }
}

// memchr is tiny and speeds up newline search; keep it internal to this module.
mod memchr {
    pub fn memchr(needle: u8, haystack: &[u8]) -> Option<usize> {
        haystack.iter().position(|&b| b == needle)
    }
}
