//! OpenTelemetry GenAI semantic-convention attribute keys, shared across the
//! rerank and embedding funnels.
//!
//! Defined inline (no `opentelemetry-semantic-conventions` crate) to match the
//! project's hand-written string style. rig 0.38.1 emits a gen_ai span for
//! chat but NOT for embeddings, and has no rerank support at all, so we add
//! the gen_ai span ourselves in those two funnels to keep ARMS grouping
//! consistent.
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
