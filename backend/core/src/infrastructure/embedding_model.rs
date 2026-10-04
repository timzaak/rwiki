use crate::domain::errors::CoreError;
use rig::embeddings::{Embedding, EmbeddingsBuilder};
use rig::operation::Embedding as EmbeddingOp;
use rig::{DynModel, ProviderError};

use super::document_chunk::DocumentChunk;

/// Application-level embedding model wrapper.
///
/// Wraps the provider-agnostic `DynModel<Embedding>` built from an
/// OpenAI-compatible provider client. This is the single funnel every
/// embedding call passes through (query texts via [`Self::embed_texts`],
/// document chunks via [`Self::embed_documents`]). rig (>= 0.43) emits the
/// gen_ai span for embeddings natively, so no span is added here.
#[derive(Clone)]
pub struct AppEmbeddingModel(DynModel<EmbeddingOp>);

impl AppEmbeddingModel {
    pub fn new(model: impl Into<DynModel<EmbeddingOp>>) -> Self {
        Self(model.into())
    }

    /// The model's vector width (0 when unknown), used to size the
    /// sqlite-vec column at migration time.
    pub fn ndims(&self) -> usize {
        self.0.capabilities().ndims
    }

    /// Embed a batch of texts, one `Embedding` per input, in input order.
    // rig 的 ProviderError 体积较大（224B），类型不属于本项目，无法收窄。
    #[allow(clippy::result_large_err)]
    pub async fn embed_texts(
        &self,
        texts: impl IntoIterator<Item = String> + Send,
    ) -> Result<Vec<Embedding>, ProviderError> {
        let texts: Vec<String> = texts.into_iter().collect();
        Ok(self.0.call(texts).await?.embeddings)
    }

    /// Embed document chunks in batches of 64 (guard against the provider's
    /// per-request document cap), one `(chunk, vectors)` entry per input
    /// chunk, in input order.
    pub async fn embed_documents(
        &self,
        chunks: Vec<DocumentChunk>,
    ) -> Result<Vec<(DocumentChunk, Vec<Embedding>)>, CoreError> {
        let mut results = Vec::with_capacity(chunks.len());
        for batch in chunks.chunks(64) {
            let batch_results = EmbeddingsBuilder::new(self.0.clone())
                .documents(batch.to_vec())
                .map_err(|e| CoreError::ProcessingError(format!("文档分块嵌入失败: {e}")))?
                .build()
                .await
                .map_err(|e| CoreError::ProcessingError(format!("向量化失败: {e}")))?;
            results.extend(batch_results);
        }
        Ok(results)
    }
}
