//! 统一检索编排模块：查询改写 → 混合检索 → 内容去重 → 可选 rerank →
//! 最终排序（标题命中加权 + page 级得分排序 + 页内 `sub_index` 稳定排列）。
//!
//! chat（公开 + scoped）、MCP（`rwiki_qa`/`rwiki_search`）、eval
//! （`/api/eval/query`）与公开搜索 `/api/search` 全部经本模块完成检索，
//! 「同一编排、同一排序口径」由该结构保证。查询改写及其 prompt 构建/解析
//! 辅助仅本编排消费，随编排同居此模块（`strip_markdown_fences` 例外：
//! chat 侧追问建议解析也复用它，保持 pub）；`build_preamble`/
//! `format_context_xml` 等 chat 语义的 prompt 组装留在 api 层
//! `handlers/chat.rs`。

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use opentelemetry::KeyValue;

use crate::config::{RerankConfig, RerankProviderType};
use crate::domain::chat::ChatMessage;
use crate::domain::errors::CoreError;
use crate::infrastructure::metrics::RwikiMetrics;
use crate::infrastructure::reranker::RerankerProvider;
use crate::infrastructure::vector_store::{
    tokenize_for_fts, RetrievalScope, SearchResult, VectorStoreManager,
};

const REWRITE_TIMEOUT_MS: u64 = 8000;
const REWRITE_MAX_QUERIES: usize = 2;
const RRF_K: u64 = 60;

/// 标题命中加权 bonus。
///
/// 作用于 RRF 融合及 rerank 之后的最终排序阶段。量级校准：
/// - rerank 启用时 score 为 0..1 相关性分，0.005 仅翻转分差 <0.5% 的
///   近似并列对，不颠覆内容高度相关的头部排序；
/// - rerank 未启用时 score 为 RRF 融合分（top_k=5 头部单名次差约 0.001，
///   双路命中层级差约 0.016），0.005 约等于 5 个名次，可见且保守。
///
/// 初值 0.005，经 eval 基线对比（对照组 bonus=0）验证。
const TITLE_MATCH_BONUS: f64 = 0.005;

/// 构建查询改写提示词。将对话历史与用户当前追问拼接，
/// 引导 LLM 将追问改写为独立的、自包含的查询。
pub(crate) fn build_rewrite_prompt(
    history: &[ChatMessage],
    user_message: &str,
    content_language: Option<&str>,
) -> String {
    let history_text = history
        .iter()
        .map(|msg| format!("{}: {}", msg.role, msg.content))
        .collect::<Vec<_>>()
        .join("\n");
    let mut prompt = format!(
        "对话历史:\n{history_text}\n\n当前用户追问: {user_message}\n\n\
         请将用户的追问改写为一个独立的、自包含的查询。\n"
    );
    if let Some(lang) = content_language {
        if !lang.is_empty() {
            prompt.push_str(&format!(
                "知识库文档主要使用 {lang} 语言。请将查询改写为 {lang}，以确保检索能命中相关文档。\n"
            ));
        }
    }
    prompt.push_str(&format!(
        "输出严格的 JSON 格式：{{\"queries\": [\"改写1\", \"改写2\"]}}\n\
         最多生成 {REWRITE_MAX_QUERIES} 条查询变体。如果只有一个查询，也用数组包裹。\n\
         只输出 JSON，不要输出其他内容。"
    ));
    prompt
}

/// Build the first-turn rewrite prompt. Extends short/ambiguous queries
/// into more specific, searchable forms with JSON output constraint.
pub(crate) fn build_first_turn_rewrite_prompt(
    user_message: &str,
    content_language: Option<&str>,
) -> String {
    let mut prompt = format!(
        "用户查询: {user_message}\n\n\
         请将这个查询改写为更具体、更可检索的形式。\n\
         如果查询使用了非正式术语或缩写，替换为对应的正式术语。\n\
         如果查询包含多个独立的子问题，将每个子问题分别列出。\n"
    );
    if let Some(lang) = content_language {
        if !lang.is_empty() {
            prompt.push_str(&format!(
                "知识库文档主要使用 {lang} 语言。请将查询改写为 {lang}，以确保检索能命中相关文档。\n"
            ));
        }
    }
    prompt.push_str(&format!(
        "输出严格的 JSON 格式：{{\"queries\": [\"改写1\", \"改写2\"]}}\n\
         最多生成 {REWRITE_MAX_QUERIES} 条查询。如果只有一个查询，也用数组包裹。\n\
         只输出 JSON，不要输出其他内容。"
    ));
    prompt
}

/// Strip optional ```json ... ``` or ``` ... ``` fences from a raw LLM response.
pub fn strip_markdown_fences(raw: &str) -> &str {
    let trimmed = raw.trim();
    if trimmed.starts_with("```json") {
        trimmed
            .strip_prefix("```json")
            .unwrap_or(trimmed)
            .strip_suffix("```")
            .unwrap_or(trimmed)
            .trim()
    } else if trimmed.starts_with("```") {
        trimmed
            .strip_prefix("```")
            .unwrap_or(trimmed)
            .strip_suffix("```")
            .unwrap_or(trimmed)
            .trim()
    } else {
        trimmed
    }
}

/// Extract usable queries from an already-parsed LLM JSON response.
/// Returns None when the queries array is missing or yields no non-empty
/// entries (after trim, capped at REWRITE_MAX_QUERIES).
fn extract_queries_from_json(val: &serde_json::Value) -> Option<Vec<String>> {
    let queries: Vec<String> = val
        .get("queries")?
        .as_array()?
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
        .filter(|s| !s.is_empty())
        .take(REWRITE_MAX_QUERIES)
        .collect();
    (!queries.is_empty()).then_some(queries)
}

/// Parse the LLM rewrite response into a list of queries.
/// Three-level degradation:
///   1. Parse JSON {"queries": [...]} (with optional ```json fence stripping)
///   2. If JSON parse fails, use raw trimmed output as single query
///   3. Caller handles LLM failure by providing original query
///
/// Test-facing wrapper locking the degradation contract; the production path
/// in `rewrite_query` shares `extract_queries_from_json` + the same fallback.
#[cfg(test)]
pub(crate) fn parse_rewrite_response(raw: &str) -> Vec<String> {
    let parsed: Option<serde_json::Value> = serde_json::from_str(strip_markdown_fences(raw)).ok();
    parsed
        .as_ref()
        .and_then(extract_queries_from_json)
        .unwrap_or_else(|| {
            tracing::debug!("rewrite response is not valid JSON, using raw output as single query");
            vec![raw.trim().to_string()]
        })
}

/// Execute query rewrite, returning a list of rewritten queries.
/// Falls back to `original_query` on timeout or LLM error.
pub async fn rewrite_query(
    llm_client: &rig::providers::openai::OpenAI,
    llm_model: &str,
    message: &str,
    history: &[ChatMessage],
    content_language: Option<&str>,
    metrics: &RwikiMetrics,
) -> Vec<String> {
    let rewrite_start = Instant::now();
    let is_first_turn = history.is_empty();
    let (rewrite_preamble, rewrite_prompt) = if history.is_empty() {
        (
            "You are a query rewriting assistant. Expand short or vague queries into more specific, retrievable forms.",
            build_first_turn_rewrite_prompt(message, content_language),
        )
    } else {
        ("You are a query rewriting assistant. Based on conversation history, rewrite the user's follow-up into a standalone, self-contained query.",
         build_rewrite_prompt(history, message, content_language))
    };

    let rewrite_agent = rig::AgentBuilder::new(llm_client.chat(llm_model))
        .preamble(rewrite_preamble)
        .max_tokens(200)
        .build();

    let queries = match tokio::time::timeout(
        Duration::from_millis(REWRITE_TIMEOUT_MS),
        rewrite_agent.prompt(&rewrite_prompt),
    )
    .await
    {
        Ok(Ok(response)) => {
            let raw_response = response.output;
            // 单次解析：queries 与 fallback_reason 共享同一份解析结果，
            // 不对同一响应重复剥壳 + serde 解析。
            let parsed: Option<serde_json::Value> =
                serde_json::from_str(strip_markdown_fences(&raw_response)).ok();
            let queries_from_json = parsed.as_ref().and_then(extract_queries_from_json);
            let fallback_reason = if queries_from_json.is_some() {
                "none"
            } else if parsed.is_some() {
                "empty_queries"
            } else {
                "invalid_json"
            };
            let queries =
                queries_from_json.unwrap_or_else(|| vec![raw_response.trim().to_string()]);
            tracing::debug!(fallback_reason, "rewrite completed");
            let elapsed_ms = rewrite_start.elapsed().as_secs_f64() * 1000.0;
            metrics
                .rewrite_duration
                .record(elapsed_ms, &[KeyValue::new("is_first_turn", is_first_turn)]);
            if fallback_reason != "none" {
                metrics
                    .rewrite_fallback_count
                    .add(1, &[KeyValue::new("fallback_reason", fallback_reason)]);
            }
            queries
        }
        Ok(Err(e)) => {
            tracing::warn!("query rewriting failed: {e}, falling back to original query");
            let elapsed_ms = rewrite_start.elapsed().as_secs_f64() * 1000.0;
            metrics
                .rewrite_duration
                .record(elapsed_ms, &[KeyValue::new("is_first_turn", is_first_turn)]);
            metrics
                .rewrite_fallback_count
                .add(1, &[KeyValue::new("fallback_reason", "llm_error")]);
            vec![message.to_string()]
        }
        Err(_) => {
            tracing::warn!(
                "query rewriting timed out after {REWRITE_TIMEOUT_MS}ms, falling back to original query"
            );
            let elapsed_ms = rewrite_start.elapsed().as_secs_f64() * 1000.0;
            metrics
                .rewrite_duration
                .record(elapsed_ms, &[KeyValue::new("is_first_turn", is_first_turn)]);
            metrics.rewrite_timeout_count.add(1, &[]);
            metrics
                .rewrite_fallback_count
                .add(1, &[KeyValue::new("fallback_reason", "timeout")]);
            vec![message.to_string()]
        }
    };
    queries
}

/// Execute hybrid search + optional rerank + final ranking, returning final
/// search results ordered at page granularity.
#[allow(clippy::too_many_arguments)]
pub async fn search_and_rerank(
    vector_store: &VectorStoreManager,
    reranker: &Option<RerankerProvider>,
    rerank_config: &RerankConfig,
    original_query: &str,
    search_queries: &[String],
    top_k_per_query: usize,
    max_total_context_chunks: usize,
    metrics: &RwikiMetrics,
    scope: &RetrievalScope,
) -> Result<Vec<SearchResult>, CoreError> {
    let retrieval_start = Instant::now();
    let search_type = if search_queries.len() == 1 {
        "hybrid"
    } else {
        "multi_query_hybrid"
    };

    let results = if search_queries.len() == 1 {
        vector_store
            .search_hybrid(
                &search_queries[0],
                top_k_per_query,
                1,
                3,
                max_total_context_chunks,
                RRF_K,
                scope,
            )
            .await
    } else {
        let results = vector_store
            .search_multi_query_hybrid(
                search_queries,
                top_k_per_query,
                1,
                3,
                max_total_context_chunks,
                RRF_K,
                scope,
            )
            .await?;
        if results.is_empty() {
            tracing::warn!("all rewrite queries returned empty, falling back to original query");
            vector_store
                .search_hybrid(
                    original_query,
                    top_k_per_query,
                    1,
                    3,
                    max_total_context_chunks,
                    RRF_K,
                    scope,
                )
                .await
        } else {
            Ok(results)
        }
    };

    let search_results = match results {
        Ok(results) => {
            let elapsed_ms = retrieval_start.elapsed().as_secs_f64() * 1000.0;
            metrics
                .retrieval_duration
                .record(elapsed_ms, &[KeyValue::new("search_type", search_type)]);
            metrics
                .retrieval_results_count
                .record(results.len() as f64, &[]);
            if results.is_empty() {
                metrics.retrieval_empty_count.add(1, &[]);
            }
            results
        }
        Err(e) => {
            return Err(e);
        }
    };

    // US-CORE-034: 同一内容的 chunk 在不同文档下只保留得分最高的一个，避免重复召回。
    // 在 rerank 之前执行，保证 reranker 不会看到重复内容。
    let search_results = dedupe_by_content(search_results);

    let search_results = if let Some(reranker) = reranker {
        let truncated: Vec<&SearchResult> =
            search_results.iter().take(rerank_config.top_n).collect();
        let documents: Vec<String> = truncated.iter().map(|r| r.content.clone()).collect();
        let provider_str = match rerank_config.provider {
            RerankProviderType::OpenRouter => "openrouter",
            RerankProviderType::BigModel => "bigmodel",
            RerankProviderType::DashScope => "dashscope",
        };
        let rerank_start = Instant::now();

        match reranker
            .rerank(original_query, &documents, rerank_config.top_n)
            .await
        {
            Ok(rerank_results) => {
                tracing::debug!("rerank returned {} results", rerank_results.len());
                let elapsed_ms = rerank_start.elapsed().as_secs_f64() * 1000.0;
                metrics
                    .rerank_duration
                    .record(elapsed_ms, &[KeyValue::new("provider", provider_str)]);
                rerank_results
                    .into_iter()
                    .filter_map(|rr| truncated.get(rr.index).map(|r| (r, rr.relevance_score)))
                    .map(|(r, score)| {
                        let mut result = (*r).clone();
                        result.score = score;
                        result
                    })
                    .collect()
            }
            Err(e) => {
                tracing::warn!("Rerank failed, degrading to RRF fusion results: {e}");
                metrics.rerank_error_count.add(1, &[]);
                search_results
            }
        }
    } else {
        search_results
    };

    // 最终排序：原始查询 ∪ 改写变体作为标题命中判定的查询来源。
    let mut queries: Vec<&str> = vec![original_query];
    queries.extend(search_queries.iter().map(String::as_str));
    Ok(rank_pages_with_title_boost(search_results, &queries))
}

/// 折叠内容完全相同的 chunk，仅保留得分最高的一个。
/// 保持稳定：以首次出现的位置为准（US-CORE-034: 避免同一内容重复召回）。
pub(crate) fn dedupe_by_content(results: Vec<SearchResult>) -> Vec<SearchResult> {
    let mut out: Vec<SearchResult> = Vec::with_capacity(results.len());
    let mut index: HashMap<String, usize> = HashMap::new();
    for r in results {
        match index.get(&r.content).copied() {
            Some(i) => {
                if r.score > out[i].score {
                    out[i] = r;
                }
            }
            None => {
                index.insert(r.content.clone(), out.len());
                out.push(r);
            }
        }
    }
    out
}

/// 最终排序：先按 page 分组，对标题命中查询关键词的 page 整组叠加
/// `TITLE_MATCH_BONUS`，page 之间按页内最高 score 稳定降序，同一 page 内按
/// `sub_index` 升序稳定排列（已发布检索 PRD 的页内顺序条款）。标题是
/// page 级属性，bonus 由此只改变 page 的先后，不打乱页内顺序；未命中
/// 片段间的相对顺序不变。`queries` = 原始查询 ∪ 改写变体。
pub(crate) fn rank_pages_with_title_boost(
    results: Vec<SearchResult>,
    queries: &[&str],
) -> Vec<SearchResult> {
    let keywords = query_keywords(queries);

    // 按 page 分组（保持组的首次出现顺序）；page_id 为空串的旧数据无页
    // 语义，每个 chunk 自成一组。
    let mut groups: Vec<Vec<SearchResult>> = Vec::new();
    let mut group_index: HashMap<String, usize> = HashMap::new();
    for r in results {
        let key = if r.page_id.is_empty() {
            r.chunk_id.clone()
        } else {
            r.page_id.clone()
        };
        match group_index.get(&key).copied() {
            Some(i) => groups[i].push(r),
            None => {
                group_index.insert(key, groups.len());
                groups.push(vec![r]);
            }
        }
    }

    // 标题按组判定一次、整组加分：同页多个 chunk（窗口扩展下最多 3-4 个）
    // 不再对同一标题重复 jieba 分词。
    for group in &mut groups {
        if title_matches(&group[0].title, &keywords) {
            for r in group.iter_mut() {
                r.score += TITLE_MATCH_BONUS;
            }
        }
    }

    for group in &mut groups {
        // 稳定排序；Option 序 None 在前，与窗口扩展的排序键一致
        group.sort_by_key(|r| r.sub_index);
    }

    // page 键 = 组内最高 score（含 bonus）；键相同时保持首次出现顺序，
    // NaN 回退 Equal 与 rrf_fuse 的排序写法一致
    groups.sort_by(|a, b| {
        let key_a = a.iter().map(|r| r.score).fold(f64::NEG_INFINITY, f64::max);
        let key_b = b.iter().map(|r| r.score).fold(f64::NEG_INFINITY, f64::max);
        key_b
            .partial_cmp(&key_a)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    groups.into_iter().flatten().collect()
}

/// 标题命中判定的查询关键词集：与 FTS 同一分词口径，词级匹配需要滤掉单字
/// 虚词与纯标点 token，否则「的/是/a」会让几乎所有标题命中，bonus 退化为
/// 无差别加分。rank_pages_with_title_boost 与 max_relevance_score 共用。
fn query_keywords(queries: &[&str]) -> HashSet<String> {
    queries
        .iter()
        .flat_map(|q| {
            tokenize_for_fts(q)
                .split_whitespace()
                .map(|t| t.to_lowercase())
                .collect::<Vec<_>>()
        })
        .filter(|t| t.chars().count() >= 2 && t.chars().any(char::is_alphanumeric))
        .collect()
}

/// 标题命中判定：任一查询关键词属于标题的分词 token 集即命中（词级匹配，
/// 不做子串包含——`to`⊄`tokio`、`rust`⊄`trust`；大小写不敏感）。
pub(crate) fn title_matches(title: &str, query_keywords: &HashSet<String>) -> bool {
    !query_keywords.is_empty()
        && !title.is_empty()
        && tokenize_for_fts(title)
            .split_whitespace()
            .map(|t| t.to_lowercase())
            .any(|t| query_keywords.contains(&t))
}

/// 结果中的最高「检索相关度」：`TITLE_MATCH_BONUS` 是排序加权，虽已写入
/// 对外 score（US-CORE-042），但 low_recall threshold 的语义是检索相关度
/// （config 文档），故比较前对标题命中组剔除 bonus，避免排序加权抬高
/// 阈值判定口径。`queries` = 原始查询 ∪ 改写变体（与最终排序一致）。
pub fn max_relevance_score(results: &[SearchResult], queries: &[&str]) -> Option<f64> {
    let keywords = query_keywords(queries);
    results
        .iter()
        .map(|r| {
            if title_matches(&r.title, &keywords) {
                r.score - TITLE_MATCH_BONUS
            } else {
                r.score
            }
        })
        .reduce(f64::max)
}
