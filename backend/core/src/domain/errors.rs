use thiserror::Error;

#[derive(Clone, Debug, Error, PartialEq)]
pub enum CoreError {
    #[error("Resource not found")]
    NotFound,
    #[error("Document not found")]
    DocumentNotFound,
    #[error("Invalid input: {0}")]
    BadRequest(String),
    #[error("Conflict: {0}")]
    Conflict(String),
    #[error("Internal server error: {0}")]
    InternalServerError(String),
    #[error("Database error: {0}")]
    DatabaseError(String),
    #[error("Processing error: {0}")]
    ProcessingError(String),
}
