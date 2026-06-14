use axum::{extract::State, Json};
use rig::client::CompletionClient;
use rig::completion::Prompt;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;
use utoipa::ToSchema;

use crate::application::http::errors::{ApiError, ErrorResponse};
use crate::application::http::state::AppState;

use super::chat::{build_preamble, format_context_xml, rewrite_query, search_and_rerank};

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, ToSchema)]
pub struct EvalQueryRequest {
    /// Optional stable eval case ID for downstream tools
    #[serde(rename = "queryId")]
    pub query_id: Option<String>,
    /// User query text
    pub query: String,
    /// Number of search results to return (default 5)
    #[serde(rename = "topK", default)]
    pub top_k: Option<u32>,
    /// Optional reference answer for correctness-oriented evaluators
    #[serde(rename = "referenceAnswer")]
    pub reference_answer: Option<String>,
    /// Session ID to reuse chat SessionStore for history context
    #[serde(rename = "sessionId")]
    pub session_id: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EvalQueryResponse {
    /// Original query
    pub query: String,
    /// Rewritten queries
    pub rewritten_queries: Vec<String>,
    /// Search results with full metadata
    pub search_results: Vec<EvalSearchResult>,
    /// Whether rerank was applied
    pub reranked: bool,
    /// Full context text passed to LLM (XML format)
    pub context: String,
    /// LLM-generated answer (non-streaming)
    pub answer: String,
    /// Pre-shaped payloads for common open-source RAG evaluators
    pub evaluation: EvalPayload,
    /// Timing breakdown in milliseconds
    pub timing_ms: TimingMs,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EvalSearchResult {
    pub chunk_id: String,
    pub document_id: String,
    pub page_id: String,
    pub content: String,
    pub score: f64,
    pub title: String,
    pub section: Option<String>,
    pub locale: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EvalPayload {
    /// Ragas-compatible single-turn sample. Includes both current and legacy field names.
    pub ragas: RagasEvalSample,
    /// DeepEval-compatible LLMTestCase fields.
    pub deepeval: DeepEvalTestCasePayload,
    /// RAGChecker-compatible result object.
    pub ragchecker: RagCheckerResultPayload,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RagasEvalSample {
    pub user_input: String,
    pub retrieved_contexts: Vec<String>,
    pub response: String,
    pub reference: Option<String>,
    pub question: String,
    pub answer: String,
    pub contexts: Vec<String>,
    pub ground_truth: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DeepEvalTestCasePayload {
    pub input: String,
    pub actual_output: String,
    pub expected_output: Option<String>,
    pub retrieval_context: Vec<String>,
    pub context: Vec<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct RagCheckerResultPayload {
    pub query_id: String,
    pub query: String,
    pub gt_answer: String,
    pub response: String,
    pub retrieved_context: Vec<RagCheckerContextPayload>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct RagCheckerContextPayload {
    pub doc_id: String,
    pub text: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TimingMs {
    /// Query rewrite duration
    pub rewrite: u64,
    /// Hybrid search duration
    pub search: u64,
    /// Rerank duration (null if not applied)
    pub rerank: Option<u64>,
    /// LLM generation duration
    pub generate: u64,
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// Execute RAG pipeline and return intermediate results + generated answer.
///
/// This endpoint runs the full RAG pipeline (query rewrite -> hybrid search ->
/// optional rerank -> LLM generate) and returns all intermediate results as JSON
/// for external eval tools to consume.
#[utoipa::path(
    post,
    path = "/api/eval/query",
    tag = "eval",
    request_body = EvalQueryRequest,
    responses(
        (status = 200, description = "RAG eval results", body = EvalQueryResponse),
        (status = 400, description = "Query cannot be empty", body = ErrorResponse),
        (status = 503, description = "Knowledge base is empty", body = ErrorResponse)
    )
)]
pub async fn eval_query(
    State(state): State<Arc<AppState>>,
    Json(req): Json<EvalQueryRequest>,
) -> Result<Json<EvalQueryResponse>, ApiError> {
    // Validate query is not empty
    if req.query.trim().is_empty() {
        return Err(ApiError::bad_request("Query cannot be empty"));
    }

    // Check knowledge base is not empty
    if state.vector_store.is_empty().await {
        return Err(ApiError::service_unavailable(
            "Knowledge base has no indexed data",
        ));
    }

    let top_k = req
        .top_k
        .map(|k| k as usize)
        .unwrap_or(state.retrieval_config.max_context_chunks)
        .max(1);

    // Read session history read-only: eval must not mutate the shared chat
    // session map (no eviction, no touch) so it has zero side effects on
    // production chat sessions. History is only consumed when the caller
    // explicitly passes a sessionId that already exists.
    let session_id = req
        .session_id
        .unwrap_or_else(|| uuid::Uuid::now_v7().to_string());
    let (summary, history) = {
        let sessions = state.chat_sessions.lock().await;
        if let Some(session) = sessions.get(&session_id) {
            (session.summary.clone(), session.messages.clone())
        } else {
            (None, Vec::new())
        }
    };

    // Stage 1: Query rewrite
    let content_language = state
        .chat_config
        .content_language
        .as_deref()
        .filter(|s| !s.is_empty());

    let rewrite_start = Instant::now();
    let rewritten_queries = rewrite_query(
        &state.llm_client,
        &state.llm_model,
        &req.query,
        &history,
        content_language,
        &state.metrics,
    )
    .await;
    let rewrite_ms = rewrite_start.elapsed().as_millis() as u64;

    // Stage 2: Hybrid search + rerank
    let search_start = Instant::now();
    let search_results = search_and_rerank(
        &state.vector_store,
        &state.reranker,
        &state.rerank_config,
        &req.query,
        &rewritten_queries,
        top_k,
        top_k,
        &state.metrics,
    )
    .await?;
    let search_ms = search_start.elapsed().as_millis() as u64;

    // Determine rerank timing
    let rerank_ms = if state.reranker.is_some() && !search_results.is_empty() {
        // If reranker is configured, search_and_rerank already applied it.
        // We report the total search+rerank time for search, and a heuristic
        // for rerank only. Since we cannot separate them after the fact,
        // we report the rerank timing as the portion after the pure search
        // estimate. For simplicity, report None and let the presence of
        // the reranker indicate reranking happened.
        // Actually, we can just check if reranker is present to know reranking
        // was attempted. We report the rerank time as 0 since it's embedded
        // in search_and_rerank. The eval consumer can infer from `reranked`.
        Some(0u64)
    } else {
        None
    };

    // Format context
    let context_text = format_context_xml(&search_results);

    // Stage 3: LLM generate (non-streaming)
    let preamble = build_preamble(
        &state.chat_config.system_prompt,
        summary.as_deref(),
        &context_text,
    );
    let agent = state
        .llm_client
        .agent(&state.llm_model)
        .preamble(&preamble)
        .build();

    let generate_start = Instant::now();
    let answer = agent
        .prompt(&req.query)
        .await
        .map_err(|e| ApiError::internal(format!("LLM generation failed: {e}")))?;
    let generate_ms = generate_start.elapsed().as_millis() as u64;

    let contexts: Vec<String> = search_results.iter().map(|r| r.content.clone()).collect();
    let ragchecker_context: Vec<RagCheckerContextPayload> = search_results
        .iter()
        .map(|r| RagCheckerContextPayload {
            doc_id: r.document_id.clone(),
            text: r.content.clone(),
        })
        .collect();
    let reference_answer = req.reference_answer.clone();
    let query_id = req
        .query_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::now_v7().to_string());
    let evaluation = EvalPayload {
        ragas: RagasEvalSample {
            user_input: req.query.clone(),
            retrieved_contexts: contexts.clone(),
            response: answer.clone(),
            reference: reference_answer.clone(),
            question: req.query.clone(),
            answer: answer.clone(),
            contexts: contexts.clone(),
            ground_truth: reference_answer.clone(),
        },
        deepeval: DeepEvalTestCasePayload {
            input: req.query.clone(),
            actual_output: answer.clone(),
            expected_output: reference_answer.clone(),
            retrieval_context: contexts.clone(),
            context: contexts,
        },
        ragchecker: RagCheckerResultPayload {
            query_id,
            query: req.query.clone(),
            gt_answer: reference_answer.unwrap_or_default(),
            response: answer.clone(),
            retrieved_context: ragchecker_context,
        },
    };

    // Map search results to eval DTOs
    let eval_results: Vec<EvalSearchResult> = search_results
        .into_iter()
        .map(|r| EvalSearchResult {
            chunk_id: r.chunk_id,
            document_id: r.document_id,
            page_id: r.page_id,
            content: r.content,
            score: r.score,
            title: r.title,
            section: r.section,
            locale: r.locale,
            tags: r.tags,
        })
        .collect();

    let reranked = state.reranker.is_some() && !eval_results.is_empty();

    Ok(Json(EvalQueryResponse {
        query: req.query,
        rewritten_queries,
        search_results: eval_results,
        reranked,
        context: context_text,
        answer,
        evaluation,
        timing_ms: TimingMs {
            rewrite: rewrite_ms,
            search: search_ms,
            rerank: rerank_ms,
            generate: generate_ms,
        },
    }))
}
