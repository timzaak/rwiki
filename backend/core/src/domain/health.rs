use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Health check status.
///
/// Returned by GET /health, contains connectivity status of each component.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HealthStatus {
    pub status: String,
    pub database: String,
}

impl HealthStatus {
    pub fn new() -> Self {
        Self {
            status: "healthy".to_string(),
            database: "unknown".to_string(),
        }
    }

    pub fn with_database(mut self, status: &str) -> Self {
        self.database = status.to_string();
        self
    }

    pub fn is_healthy(&self) -> bool {
        self.status == "healthy" && self.database == "connected"
    }
}

impl Default for HealthStatus {
    fn default() -> Self {
        Self::new()
    }
}
