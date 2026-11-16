use axum::{
    extract::{Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::{IntoParams, ToSchema};

use crate::application::http::errors::ApiError;
use crate::application::http::state::AppState;
use rwiki_core::config::ChannelValidationError;
use rwiki_core::infrastructure::storage::{LowRecallFilter, MetadataStore};

/// Query parameters (channelId required, others optional)
#[derive(Debug, Deserialize, IntoParams, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LowRecallQueryParams {
    /// Channel identifier; required
    pub channel_id: String,
    /// Only return records with topScore >= minScore
    pub min_score: Option<f64>,
    /// Only return records with topScore <= maxScore
    pub max_score: Option<f64>,
    /// createdAt >= from (ISO8601)
    pub from: Option<String>,
    /// createdAt <= to (ISO8601)
    pub to: Option<String>,
    /// Page size, default 20, max 100
    pub limit: Option<u32>,
    /// Page offset, default 0
    pub offset: Option<u32>,
}

/// Retrieval source summary (top-K). Derives Deserialize so it can be parsed
/// from the sources JSON column.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LowRecallSource {
    pub document_id: String,
    pub chunk_id: String,
    pub title: String,
    pub score: f64,
}

/// A single low-recall record.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LowRecallRecord {
    pub id: i64,
    /// Channel the record belongs to
    pub channel_id: String,
    pub session_id: Option<String>,
    pub query: String,
    /// Top-1 relevance score; None means no hits at all
    pub top_score: Option<f64>,
    pub result_count: i64,
    pub sources: Vec<LowRecallSource>,
    pub created_at: String,
}

/// List response.
#[derive(Debug, Serialize, ToSchema)]
pub struct LowRecallListResponse {
    pub items: Vec<LowRecallRecord>,
    pub total: i64,
}

/// Query low-relevance recall records (score range + time range + pagination).
///
/// Mounted on doc_router and protected by Bearer token auth.
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
    let channel_id = state
        .channels_config
        .require_configured(&params.channel_id)
        .map_err(|e| match e {
            ChannelValidationError::Empty => ApiError::bad_request("channelId 不能为空"),
            ChannelValidationError::NotConfigured(id) => {
                ApiError::bad_request(format!("频道 {id} 未配置"))
            }
        })?
        .to_string();

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

    // 动态过滤由存储边界渲染为各自方言谓词
    let page = state
        .store
        .list_low_recall_records(
            LowRecallFilter {
                channel_id,
                min_score: params.min_score,
                max_score: params.max_score,
                from_ts,
                to_ts,
            },
            limit as i64,
            offset as i64,
        )
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let items: Vec<LowRecallRecord> = page
        .items
        .into_iter()
        .map(|row| LowRecallRecord {
            id: row.id,
            channel_id: row.channel_id,
            session_id: row.session_id,
            query: row.query,
            top_score: row.top_score,
            result_count: row.result_count,
            sources: serde_json::from_str(&row.sources_json).unwrap_or_else(|e| {
                tracing::warn!(
                    error = %e,
                    "failed to deserialize low_recall sources JSON; falling back to empty vec"
                );
                Vec::new()
            }),
            created_at: row.created_at,
        })
        .collect();

    Ok(Json(LowRecallListResponse {
        items,
        total: page.total,
    }))
}
