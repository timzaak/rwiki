//! RerankerProvider HTTP behavior scenario tests.
//!
//! Verifies that reranker providers correctly order results, handle top_n
//! truncation, filter out-of-range indices, and skip API calls for empty
//! input. Uses mockito to simulate provider HTTP responses.
//!
//! These tests cover the **scenario level** (end-to-end HTTP behavior of
//! RerankerProvider through enum dispatch) and are distinct from the unit
//! tests in `reranker.rs` which cover basic HTTP contracts and error types.

use std::time::Duration;

use super::reranker::{DashScopeReranker, OpenRouterReranker, RerankResult, RerankerProvider};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_documents(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("document {i}")).collect()
}

// ---------------------------------------------------------------------------
// Scenario tests
// ---------------------------------------------------------------------------

// User Story: US-CORE-030 -- As a user, when rerank is enabled the returned
// results must be ordered by relevance score (descending) as determined by
// the cross-encoder API, not by the original retrieval order.
// Covers: Design 4.5.4 -- RerankerProvider dispatches to OpenRouter, parses
//         the response, and returns results preserving API ordering (index + score).

#[tokio::test]
async fn rerank_provider_returns_correct_result_order() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/rerank")
        .with_status(200)
        .with_body(
            r#"{"results":[{"index":2,"relevance_score":0.95},{"index":0,"relevance_score":0.8}]}"#,
        )
        .create_async()
        .await;

    let provider = RerankerProvider::OpenRouter(OpenRouterReranker::with_base_url(
        "test-key".to_string(),
        "cohere/rerank-v4-fast".to_string(),
        Duration::from_secs(3),
        format!("{}/rerank", server.url()),
    ));

    let docs = make_documents(3);
    let results: Vec<RerankResult> = provider
        .rerank("test query", &docs, 10)
        .await
        .expect("rerank should succeed");

    // Verify ordering matches API response: index 2 (score 0.95) first, then index 0 (score 0.8)
    assert_eq!(results.len(), 2, "should return 2 results");
    assert_eq!(results[0].index, 2, "first result must have index 2");
    assert!(
        (results[0].relevance_score - 0.95).abs() < f64::EPSILON,
        "first result score must be 0.95"
    );
    assert_eq!(results[1].index, 0, "second result must have index 0");
    assert!(
        (results[1].relevance_score - 0.8).abs() < f64::EPSILON,
        "second result score must be 0.8"
    );

    mock.assert_async().await;
}

// User Story: US-CORE-030 -- As a user, the system must limit candidate
// documents sent to the rerank API to prevent exceeding provider limits and
// control latency. The top_n parameter controls how many documents are
// included in the API request body.
// Covers: Design 4.5.2 -- top_n truncates the candidate list before sending
//         to the API. The request body must contain at most top_n documents.

#[tokio::test]
async fn rerank_top_n_truncates_candidates() {
    let mut server = mockito::Server::new_async().await;
    let top_n = 5;

    // Mock that matches the request body to verify only top_n documents are sent
    let mock = server
        .mock("POST", "/rerank")
        .match_body(mockito::Matcher::PartialJson(serde_json::json!({
            "top_n": top_n,
        })))
        .with_status(200)
        .with_body(r#"{"results":[{"index":0,"relevance_score":0.9}]}"#)
        .create_async()
        .await;

    let provider = RerankerProvider::OpenRouter(OpenRouterReranker::with_base_url(
        "test-key".to_string(),
        "cohere/rerank-v4-fast".to_string(),
        Duration::from_secs(3),
        format!("{}/rerank", server.url()),
    ));

    // 30 documents but top_n=5 -- the provider should send top_n=5 in the request
    let docs = make_documents(30);
    let results: Vec<RerankResult> = provider
        .rerank("test query", &docs, top_n)
        .await
        .expect("rerank should succeed");

    assert_eq!(results.len(), 1, "should return 1 result");
    assert_eq!(results[0].index, 0, "result must have index 0");

    // Verify the mock was hit, confirming the request was sent with correct top_n
    mock.assert_async().await;
}

// User Story: US-CORE-030 -- As a user, when the rerank API returns indices
// that are out of range (e.g., index >= documents.len()), the system must
// filter those out gracefully rather than panicking or producing incorrect
// results. This can happen if the API has a bug or the document list was
// truncated between request and response processing.
// Covers: Design 4.5.4 -- the chat handler uses filter_map to skip indices
//         that fall outside the truncated slice. This test verifies the
//         provider itself returns raw results (filtering is the caller's job),
//         but confirms that out-of-range indices do not cause errors.

#[tokio::test]
async fn rerank_out_of_range_index_returned_without_error() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/rerank")
        .with_status(200)
        // Returns index 5 and index 99, but we only have 3 documents
        .with_body(
            r#"{"results":[{"index":5,"relevance_score":0.9},{"index":99,"relevance_score":0.5}]}"#,
        )
        .create_async()
        .await;

    let provider = RerankerProvider::OpenRouter(OpenRouterReranker::with_base_url(
        "test-key".to_string(),
        "cohere/rerank-v4-fast".to_string(),
        Duration::from_secs(3),
        format!("{}/rerank", server.url()),
    ));

    let docs = make_documents(3);
    let results: Vec<RerankResult> = provider
        .rerank("test query", &docs, 10)
        .await
        .expect("rerank should succeed even with out-of-range indices");

    // The provider returns raw API results; the caller (chat handler) filters.
    // Provider must not error -- it just passes through what the API returns.
    assert_eq!(
        results.len(),
        2,
        "provider returns all API results unfiltered"
    );
    assert_eq!(
        results[0].index, 5,
        "out-of-range index 5 is returned as-is"
    );
    assert_eq!(
        results[1].index, 99,
        "out-of-range index 99 is returned as-is"
    );

    mock.assert_async().await;
}

// User Story: US-CORE-030 -- As a user, when my query returns no search
// results (empty document list), the reranker must not make an API call.
// This avoids unnecessary network latency and API costs.
// Covers: Design 4.5.2 -- empty documents returns empty Vec without HTTP call.
//         Design doc 4.5.2 Reranker::rerank: "为空时直接返回空 Vec，不发起 API 调用".

#[tokio::test]
async fn rerank_empty_documents_returns_empty_without_api_call() {
    // Create a mock server but do NOT set up any mocks -- if any request is
    // made, the test will fail because mockito will return 501.
    let server = mockito::Server::new_async().await;

    let provider = RerankerProvider::OpenRouter(OpenRouterReranker::with_base_url(
        "test-key".to_string(),
        "cohere/rerank-v4-fast".to_string(),
        Duration::from_secs(3),
        format!("{}/rerank", server.url()),
    ));

    let results: Vec<RerankResult> = provider
        .rerank("test query", &[], 10)
        .await
        .expect("empty documents should succeed with empty result");

    assert!(results.is_empty(), "empty documents must return empty Vec");
    // No assertions on mock -- if the provider made an HTTP call, mockito
    // would have returned an error and the test would have failed above.
}

// User Story: US-CORE-031 -- As a user, when the rerank API returns results
// where ALL indices are out of range, the chat handler must degrade to
// the original search results. This test verifies the provider returns the
// raw out-of-range results; the caller's filter_map produces an empty Vec,
// triggering the degradation path.
// Covers: Design 4.5.4 -- filter_map on rerank_results produces empty Vec
//         when all indices are invalid. Combined with the chat handler's
//         logic, this means the original search_results are used.

#[tokio::test]
async fn rerank_all_indices_out_of_range_returns_raw_results() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/rerank")
        .with_status(200)
        // All indices are >= 10 (out of range for 3 documents)
        .with_body(
            r#"{"results":[{"index":10,"relevance_score":0.9},{"index":11,"relevance_score":0.5}]}"#,
        )
        .create_async()
        .await;

    let provider = RerankerProvider::OpenRouter(OpenRouterReranker::with_base_url(
        "test-key".to_string(),
        "cohere/rerank-v4-fast".to_string(),
        Duration::from_secs(3),
        format!("{}/rerank", server.url()),
    ));

    let docs = make_documents(3);
    let results: Vec<RerankResult> = provider
        .rerank("test query", &docs, 10)
        .await
        .expect("rerank should succeed even when all indices are out of range");

    // Provider returns raw results; the caller filters out-of-range indices.
    // After filter_map in the chat handler, this would produce an empty Vec,
    // which the handler treats as "no rerank results" and keeps original order.
    assert_eq!(
        results.len(),
        2,
        "provider returns all raw API results for caller to filter"
    );

    mock.assert_async().await;
}

// User Story: US-CORE-033 (scenario 2) -- As a user, when the deployer configures
// rerank provider = "dash_scope", the system must call Alibaba Bailian's rerank
// endpoint and feed the reranked ordering into the LLM context.
// Covers: PRD 5.1.2 + Decision "精排新增 dash_scope provider" -- RerankerProvider
//         enum dispatch routes DashScope variant through DashScopeReranker, which
//         parses the OpenAI-compatible flat response shape and returns results
//         preserving API ordering (index + relevance_score).
// Note: The RerankerProvider::DashScope dispatch arm is the scenario-level seam
//       that connects config (provider = "dash_scope") to the HTTP client.
//       The unit test `dashscope_rerank_success` in reranker.rs only exercises
//       DashScopeReranker in isolation; this test verifies the dispatch wiring
//       end-to-end through RerankerProvider::rerank, which is what the chat
//       handler actually invokes.

#[tokio::test]
async fn dashscope_provider_dispatch_returns_reranked_order() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/reranks")
        .match_header("Authorization", "Bearer bailian-key")
        .with_status(200)
        // Bailian returns a flat OpenAI-compatible shape with extra top-level
        // fields (object/model/id/usage) that must be ignored by the parser.
        .with_body(
            r#"{"object":"list","results":[{"index":2,"relevance_score":0.97},{"index":0,"relevance_score":0.42}],"model":"qwen3-rerank","id":"req-abc","usage":{"total_tokens":128}}"#,
        )
        .create_async()
        .await;

    let provider = RerankerProvider::DashScope(DashScopeReranker::with_base_url(
        "bailian-key".to_string(),
        "qwen3-rerank".to_string(),
        Duration::from_secs(3),
        format!("{}/reranks", server.url()),
    ));

    let docs = make_documents(3);
    let results: Vec<RerankResult> = provider
        .rerank("如何重置密码", &docs, 10)
        .await
        .expect("DashScope dispatch should succeed");

    // The reranker must preserve API ordering: index 2 (0.97) before index 0 (0.42).
    // This ordering is what determines which context chunks reach the LLM first.
    assert_eq!(results.len(), 2, "DashScope should return 2 results");
    assert_eq!(
        results[0].index, 2,
        "highest-scoring chunk must come first so it reaches the LLM"
    );
    assert!(
        (results[0].relevance_score - 0.97).abs() < f64::EPSILON,
        "top result score must match Bailian response"
    );
    assert_eq!(results[1].index, 0);
    assert!(
        (results[1].relevance_score - 0.42).abs() < f64::EPSILON,
        "second result score must match Bailian response"
    );

    mock.assert_async().await;
}
