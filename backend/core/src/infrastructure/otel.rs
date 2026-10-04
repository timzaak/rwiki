//! OpenTelemetry GenAI semantic-convention attribute keys, shared across the
//! rerank funnel.
//!
//! Defined inline (no `opentelemetry-semantic-conventions` crate) to match
//! the project's hand-written string style. rig (>= 0.43) emits gen_ai spans
//! natively for every operation it drives (chat, embeddings, ...), so those
//! funnels need no hand-rolled span. Rerank has no rig funnel here (the
//! reranker calls its provider directly), so the gen_ai span is added in the
//! reranker itself to keep ARMS grouping consistent.
//!
//! NOTE: the OpenTelemetry GenAI semantic conventions define no `rerank`
//! operation; `gen_ai.operation.name = "rerank"` is a deliberate rwiki
//! extension so ARMS groups rerank calls with chat/embedding under a
//! recognizable, provider-tagged operation.

/// OTel GenAI semantic-convention attribute keys.
pub mod gen_ai {
    pub const OPERATION_NAME: &str = "gen_ai.operation.name";
    pub const SYSTEM: &str = "gen_ai.system";
    pub const PROVIDER_NAME: &str = "gen_ai.provider.name";
    pub const REQUEST_MODEL: &str = "gen_ai.request.model";
}
