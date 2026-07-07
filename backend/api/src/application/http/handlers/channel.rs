use crate::application::http::state::AppState;
use axum::{extract::State, Json};
use rwiki_core::config::ChannelMetadata;
use serde::Serialize;
use std::sync::Arc;
use utoipa::ToSchema;

/// 频道列表单项。
#[derive(Debug, Serialize, ToSchema)]
pub struct ChannelItem {
    pub id: String,
    pub name: String,
}

/// 频道列表响应。
#[derive(Debug, Serialize, ToSchema)]
pub struct ChannelsResponse {
    pub channels: Vec<ChannelItem>,
}

impl From<ChannelMetadata> for ChannelItem {
    fn from(metadata: ChannelMetadata) -> Self {
        Self {
            id: metadata.id,
            name: metadata.name,
        }
    }
}

/// 获取已配置频道列表。
///
/// GET /api/channels
/// 仅返回频道元数据（id + name），供管理后台上传组件、主站导航等使用。
#[utoipa::path(
    get,
    path = "/api/channels",
    tag = "channels",
    responses(
        (status = 200, description = "Configured channels", body = ChannelsResponse)
    )
)]
pub async fn list_channels(State(state): State<Arc<AppState>>) -> Json<ChannelsResponse> {
    let channels = state
        .channels_config
        .list_metadata()
        .into_iter()
        .map(ChannelItem::from)
        .collect();
    Json(ChannelsResponse { channels })
}
