use crate::application::http::state::AppState;
use axum::{extract::State, Json};
use rwiki_core::config::ChannelMetadata;
use serde::Serialize;
use std::sync::Arc;
use utoipa::ToSchema;

/// A single channel entry.
#[derive(Debug, Serialize, ToSchema)]
pub struct ChannelItem {
    pub id: String,
    pub name: String,
}

/// Channel list response.
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

/// List configured channels.
///
/// GET /api/channels
/// Returns channel metadata only (id + name), used by the admin upload widget
/// and the main-site navigation.
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
