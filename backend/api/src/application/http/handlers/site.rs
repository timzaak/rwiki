use crate::application::http::state::AppState;
use axum::{extract::State, Json};
use rwiki_core::config::SiteMetadata;
use serde::Serialize;
use std::sync::Arc;
use utoipa::ToSchema;

/// 站点列表单项。
#[derive(Debug, Serialize, ToSchema)]
pub struct SiteItem {
    pub id: String,
    pub name: String,
}

/// 站点列表响应。
#[derive(Debug, Serialize, ToSchema)]
pub struct SitesResponse {
    pub sites: Vec<SiteItem>,
}

impl From<SiteMetadata> for SiteItem {
    fn from(metadata: SiteMetadata) -> Self {
        Self {
            id: metadata.id,
            name: metadata.name,
        }
    }
}

/// 获取已配置站点列表。
///
/// GET /api/sites
/// 仅返回站点元数据（id + name），供管理后台上传组件、主站导航等使用。
#[utoipa::path(
    get,
    path = "/api/sites",
    tag = "sites",
    responses(
        (status = 200, description = "Configured sites", body = SitesResponse)
    )
)]
pub async fn list_sites(State(state): State<Arc<AppState>>) -> Json<SitesResponse> {
    let sites = state
        .sites_config
        .list_metadata()
        .into_iter()
        .map(SiteItem::from)
        .collect();
    Json(SitesResponse { sites })
}
