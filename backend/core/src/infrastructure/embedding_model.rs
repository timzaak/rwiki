use rig::embeddings::{Embedding, EmbeddingError, EmbeddingModel};

/// Application-level embedding model wrapper.
///
/// Wraps an OpenAI-compatible embedding provider, delegating rig's
/// `EmbeddingModel` trait to the inner model.
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
        self.0.embed_texts(texts).await
    }
}
