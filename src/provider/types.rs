use futures_core::stream::BoxStream;

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub model: String,
    pub prompt: String,

    /// Phase A placeholder for passing directory context.
    pub include_directories: Vec<std::path::PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ChatChunk {
    pub text: String,
}

/// Provider interface.
///
/// Phase A: only a streaming chat method.
pub trait Provider {
    fn name(&self) -> &'static str;

    /// Start streaming a response.
    fn stream_chat(
        &self,
        req: ChatRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<BoxStream<'static, anyhow::Result<ChatChunk>>>> + Send>>;
}
