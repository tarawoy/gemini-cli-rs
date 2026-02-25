use super::{ChatChunk, ChatRequest, Provider};
use futures_core::stream::BoxStream;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;

#[derive(Debug, Default, Clone)]
pub struct StubProvider;

impl StubProvider {
    pub fn new() -> Self {
        Self
    }
}

impl Provider for StubProvider {
    fn name(&self) -> &'static str {
        "stub"
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
        Box::pin(async move {
            // In a real provider, this would perform an HTTP request and parse streaming chunks.
            // Here we just drip a few chunks with delays.
            let (tx, rx) = mpsc::channel::<anyhow::Result<ChatChunk>>(32);

            tokio::spawn(async move {
                let _ = tx
                    .send(Ok(ChatChunk {
                        text: format!(
                            "[stub provider]\nmodel: {}\ninclude_directories: {:?}\n\n",
                            req.model, req.include_directories
                        ),
                    }))
                    .await;

                let parts = [
                    "You said: ",
                    req.prompt.as_str(),
                    "\n\n",
                    "(This is streaming scaffolding; Phase A does not call Gemini APIs yet.)",
                ];

                for p in parts {
                    tokio::time::sleep(std::time::Duration::from_millis(120)).await;
                    if tx.send(Ok(ChatChunk { text: p.to_string() })).await.is_err() {
                        break;
                    }
                }
            });

            let stream = ReceiverStream::new(rx).map(|x| x);
            Ok(Box::pin(stream) as BoxStream<'static, anyhow::Result<ChatChunk>>)
        })
    }
}
