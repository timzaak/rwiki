use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Document processing status
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum DocumentStatus {
    Processing,
    Draft,
    Published,
    Failed,
}

impl std::fmt::Display for DocumentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DocumentStatus::Processing => write!(f, "processing"),
            DocumentStatus::Draft => write!(f, "draft"),
            DocumentStatus::Published => write!(f, "published"),
            DocumentStatus::Failed => write!(f, "failed"),
        }
    }
}

impl AsRef<str> for DocumentStatus {
    fn as_ref(&self) -> &str {
        match self {
            DocumentStatus::Processing => "processing",
            DocumentStatus::Draft => "draft",
            DocumentStatus::Published => "published",
            DocumentStatus::Failed => "failed",
        }
    }
}

/// 数据库行映射（SQLite TEXT 列，手动映射）
#[derive(Debug, Clone)]
pub struct DocumentRow {
    pub id: String,
    pub file_name: String,
    pub status: String,
    pub row_count: i32,
    pub channel_id: String,
    pub error_message: Option<String>,
    pub created_at: String,
}
