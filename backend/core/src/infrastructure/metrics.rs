use opentelemetry::global;
use opentelemetry::metrics::{Counter, Histogram, Meter};

pub struct RwikiMetrics {
    // --- Chat end-to-end ---
    pub chat_request_count: Counter<u64>,
    pub chat_duration: Histogram<f64>,
    pub chat_error_count: Counter<u64>,

    // --- Query Rewrite ---
    pub rewrite_duration: Histogram<f64>,
    pub rewrite_timeout_count: Counter<u64>,
    pub rewrite_fallback_count: Counter<u64>,

    // --- Retrieval ---
    pub retrieval_duration: Histogram<f64>,
    pub retrieval_results_count: Histogram<f64>,
    pub retrieval_empty_count: Counter<u64>,

    // --- Rerank ---
    pub rerank_duration: Histogram<f64>,
    pub rerank_error_count: Counter<u64>,

    // --- LLM Generation ---
    pub llm_duration: Histogram<f64>,
    pub llm_first_token_duration: Histogram<f64>,
    pub llm_error_count: Counter<u64>,
    pub llm_output_chars: Histogram<f64>,
    pub llm_context_chunks: Histogram<f64>,
}

impl Default for RwikiMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl RwikiMetrics {
    pub fn new() -> Self {
        Self::with_meter(global::meter("rwiki"))
    }

    pub fn with_meter(meter: Meter) -> Self {
        Self {
            chat_request_count: meter
                .u64_counter("rag.chat.request.count")
                .with_description("Total chat requests")
                .build(),
            chat_duration: meter
                .f64_histogram("rag.chat.duration")
                .with_unit("ms")
                .with_description("End-to-end chat latency")
                .build(),
            chat_error_count: meter
                .u64_counter("rag.chat.error.count")
                .with_description("Chat request errors")
                .build(),

            rewrite_duration: meter
                .f64_histogram("rag.rewrite.duration")
                .with_unit("ms")
                .with_description("Query rewrite LLM call latency")
                .build(),
            rewrite_timeout_count: meter
                .u64_counter("rag.rewrite.timeout.count")
                .with_description("Query rewrite timeouts")
                .build(),
            rewrite_fallback_count: meter
                .u64_counter("rag.rewrite.fallback.count")
                .with_description("Query rewrite fallbacks")
                .build(),

            retrieval_duration: meter
                .f64_histogram("rag.retrieval.duration")
                .with_unit("ms")
                .with_description("Retrieval total latency")
                .build(),
            retrieval_results_count: meter
                .f64_histogram("rag.retrieval.results.count")
                .with_description("Retrieval results count distribution")
                .build(),
            retrieval_empty_count: meter
                .u64_counter("rag.retrieval.empty.count")
                .with_description("Empty retrieval results")
                .build(),

            rerank_duration: meter
                .f64_histogram("rag.rerank.duration")
                .with_unit("ms")
                .with_description("Rerank API call latency")
                .build(),
            rerank_error_count: meter
                .u64_counter("rag.rerank.error.count")
                .with_description("Rerank errors (degraded to RRF)")
                .build(),

            llm_duration: meter
                .f64_histogram("rag.llm.duration")
                .with_unit("ms")
                .with_description("LLM streaming total latency")
                .build(),
            llm_first_token_duration: meter
                .f64_histogram("rag.llm.first_token.duration")
                .with_unit("ms")
                .with_description("LLM time to first token")
                .build(),
            llm_error_count: meter
                .u64_counter("rag.llm.error.count")
                .with_description("LLM streaming errors")
                .build(),
            llm_output_chars: meter
                .f64_histogram("rag.llm.output.chars")
                .with_description("LLM output character count distribution")
                .build(),
            llm_context_chunks: meter
                .f64_histogram("rag.llm.context.chunks")
                .with_description("LLM context chunk count distribution")
                .build(),
        }
    }
}
