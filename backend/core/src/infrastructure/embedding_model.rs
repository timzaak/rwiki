use rig::embeddings::{Embedding, EmbeddingError, EmbeddingModel};
use tracing_futures::Instrument;

use crate::infrastructure::otel::gen_ai;

/// Application-level embedding model wrapper.
///
/// Wraps an OpenAI-compatible embedding provider, delegating rig's
/// `EmbeddingModel` trait to the inner model. This is the single funnel every
/// embedding call passes through, so the gen_ai span is emitted here.
#[derive(Clone)]
pub struct AppEmbeddingModel(rig::providers::openai::EmbeddingModel);

impl AppEmbeddingModel {
    pub fn new(model: rig::providers::openai::EmbeddingModel) -> Self {
        Self(model)
    }
}

impl EmbeddingModel for AppEmbeddingModel {
    const MAX_DOCUMENTS: usize = 1024;

    type Client = ();

    fn make(_client: &Self::Client, _model: impl Into<String>, _dims: Option<usize>) -> Self {
        unimplemented!(
            "AppEmbeddingModel::make is not supported; construct models via provider clients"
        )
    }

    fn ndims(&self) -> usize {
        self.0.ndims()
    }

    async fn embed_texts(
        &self,
        texts: impl IntoIterator<Item = String> + Send,
    ) -> Result<Vec<Embedding>, EmbeddingError> {
        // Model name comes from rig's public `EmbeddingModel.model` field.
        let model = self.0.model.clone();
        // rig 0.38.1 does NOT emit a gen_ai span for embeddings, so we add one
        // here so ARMS groups embeddings alongside rig's chat spans. 用
        // `.instrument(span)` 让 span 包裹整个 future，而非 `span.enter()` 跨
        // .await（后者破坏父子 span 关联）。
        let span = tracing::info_span!(
            "embeddings",
            { gen_ai::OPERATION_NAME } = "embeddings",
            { gen_ai::SYSTEM } = "openai",
            { gen_ai::PROVIDER_NAME } = "openai",
            { gen_ai::REQUEST_MODEL } = model.as_str(),
        );
        self.0.embed_texts(texts).instrument(span).await
    }
}
