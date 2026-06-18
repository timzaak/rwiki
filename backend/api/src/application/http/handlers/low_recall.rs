use axum::{
    extract::{Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::{IntoParams, ToSchema};

use crate::application::http::errors::ApiError;
use crate::application::http::state::AppState;

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

/// 查询参数（camelCase，均可选）
#[derive(Debug, Deserialize, IntoParams, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LowRecallQueryParams {
    /// 仅返回 topScore >= minScore 的记录
    pub min_score: Option<f64>,
    /// 仅返回 topScore <= maxScore 的记录
    pub max_score: Option<f64>,
    /// createdAt >= from（ISO8601）
    pub from: Option<String>,
    /// createdAt <= to（ISO8601）
    pub to: Option<String>,
    /// 分页大小，默认 20，上限 100
    pub limit: Option<u32>,
    /// 分页偏移，默认 0
    pub offset: Option<u32>,
}

/// 召回来源摘要（top-K）。含 Deserialize 以便从 sources JSON 列反序列化。
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LowRecallSource {
    pub document_id: String,
    pub chunk_id: String,
    pub title: String,
    pub score: f64,
}

/// 单条低相关召回记录。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LowRecallRecord {
    pub id: i64,
    pub session_id: Option<String>,
    pub query: String,
    /// top-1 相关度分数；None 表示完全未命中
    pub top_score: Option<f64>,
    pub result_count: i64,
    pub sources: Vec<LowRecallSource>,
    pub created_at: String,
}

/// 列表响应。
#[derive(Debug, Serialize, ToSchema)]
pub struct LowRecallListResponse {
    pub items: Vec<LowRecallRecord>,
    pub total: i64,
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// 查询低相关召回记录（分数区间 + 时间段 + 分页）。
///
/// 挂入 doc_router，受 Bearer Token 鉴权保护。
#[utoipa::path(
    get,
    path = "/api/low-recall/records",
    tag = "low-recall",
    security(("bearer_auth" = [])),
    params(LowRecallQueryParams),
    responses(
        (status = 200, body = LowRecallListResponse, description = "Low-recall records list"),
        (status = 400, body = crate::application::http::errors::ErrorResponse, description = "Invalid filter parameters"),
        (status = 401, body = crate::application::http::errors::ErrorResponse, description = "Unauthorized"),
        (status = 500, body = crate::application::http::errors::ErrorResponse, description = "Database error")
    )
)]
pub async fn list_low_recall_records(
    State(state): State<Arc<AppState>>,
    Query(params): Query<LowRecallQueryParams>,
) -> Result<Json<LowRecallListResponse>, ApiError> {
    let limit = params.limit.unwrap_or(20).clamp(1, 100);
    let offset = params.offset.unwrap_or(0);

    // 校验：分数区间
    if let (Some(min), Some(max)) = (params.min_score, params.max_score) {
        if min > max {
            return Err(ApiError::bad_request("minScore must be <= maxScore"));
        }
    }

    // 校验：from/to 须可解析为 RFC3339；同传时 from <= to（ISO8601 字典序可比）
    let from_ts: Option<String> = match params.from.as_deref() {
        Some(s) => match chrono::DateTime::parse_from_rfc3339(s) {
            Ok(_) => Some(s.to_string()),
            Err(_) => {
                return Err(ApiError::bad_request(
                    "from must be a valid RFC3339 / ISO8601 timestamp",
                ));
            }
        },
        None => None,
    };
    let to_ts: Option<String> = match params.to.as_deref() {
        Some(s) => match chrono::DateTime::parse_from_rfc3339(s) {
            Ok(_) => Some(s.to_string()),
            Err(_) => {
                return Err(ApiError::bad_request(
                    "to must be a valid RFC3339 / ISO8601 timestamp",
                ));
            }
        },
        None => None,
    };
    if let (Some(ref f), Some(ref t)) = (&from_ts, &to_ts) {
        if f > t {
            return Err(ApiError::bad_request("from must be <= to"));
        }
    }

    // 移入闭包的拥有值（闭包要求 'static + Send）
    let min_score = params.min_score;
    let max_score = params.max_score;

    state
        .sqlite
        .call(move |conn| {
            // 动态拼 WHERE 与参数
            let mut where_clauses: Vec<String> = Vec::new();
            let mut sql_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
            let mut idx = 1usize;

            if let Some(min) = min_score {
                where_clauses.push(format!("top_score >= ?{}", idx));
                sql_params.push(Box::new(min));
                idx += 1;
            }
            if let Some(max) = max_score {
                where_clauses.push(format!("top_score <= ?{}", idx));
                sql_params.push(Box::new(max));
                idx += 1;
            }
            if let Some(f) = from_ts {
                where_clauses.push(format!("created_at >= ?{}", idx));
                sql_params.push(Box::new(f));
                idx += 1;
            }
            if let Some(t) = to_ts {
                where_clauses.push(format!("created_at <= ?{}", idx));
                sql_params.push(Box::new(t));
                idx += 1;
            }

            let where_sql = if where_clauses.is_empty() {
                String::new()
            } else {
                format!("WHERE {}", where_clauses.join(" AND "))
            };

            // COUNT(*) 取 total
            let count_sql = format!("SELECT COUNT(*) FROM low_recall_records {}", where_sql);
            let total: i64 = {
                let param_refs: Vec<&dyn rusqlite::ToSql> =
                    sql_params.iter().map(|p| p.as_ref()).collect();
                conn.query_row(&count_sql, param_refs.as_slice(), |row| row.get(0))?
            };

            // 数据查询：追加 LIMIT/OFFSET 占位
            let limit_idx = idx;
            let offset_idx = idx + 1;
            let data_sql = format!(
                "SELECT id, session_id, query, top_score, result_count, sources, created_at \
                 FROM low_recall_records {} \
                 ORDER BY created_at DESC LIMIT ?{} OFFSET ?{}",
                where_sql, limit_idx, offset_idx
            );

            // 完整参数顺序：where 参数 + limit + offset
            let mut all_params: Vec<Box<dyn rusqlite::ToSql>> = sql_params;
            all_params.push(Box::new(limit));
            all_params.push(Box::new(offset));
            let param_refs: Vec<&dyn rusqlite::ToSql> =
                all_params.iter().map(|p| p.as_ref()).collect();

            let mut stmt = conn.prepare(&data_sql)?;
            let items: Vec<LowRecallRecord> = stmt
                .query_map(param_refs.as_slice(), |row| {
                    let sources_json: String = row.get(5)?;
                    let sources = serde_json::from_str::<Vec<LowRecallSource>>(&sources_json)
                        .unwrap_or_else(|e| {
                            tracing::warn!(
                                error = %e,
                                "failed to deserialize low_recall sources JSON; falling back to empty vec"
                            );
                            Vec::new()
                        });
                    Ok(LowRecallRecord {
                        id: row.get(0)?,
                        session_id: row.get(1)?,
                        query: row.get(2)?,
                        top_score: row.get(3)?,
                        result_count: row.get(4)?,
                        sources,
                        created_at: row.get(6)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;

            Ok::<LowRecallListResponse, rusqlite::Error>(LowRecallListResponse { items, total })
        })
        .await
        .map(Json)
        .map_err(|e| ApiError::internal(e.to_string()))
}
