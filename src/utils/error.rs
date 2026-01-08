//! Error types for Exsa-Engine

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

/// Main error type for Exsa-Engine
#[derive(Error, Debug)]
pub enum ExsaError {
    #[error("Model error: {0}")]
    ModelError(String),

    #[error("Inference error: {0}")]
    InferenceError(String),

    /// Resource exhausted (for batching)
    #[error("Resource exhausted: {0}")]
    ResourceExhausted(String),

    #[error("Model loading error: {0}")]
    ModelLoadError(String),

    #[error("Internal error: {0}")]
    InternalError(String),

    #[error("Invalid parameters: {0}")]
    InvalidParameters(String),

    #[error("Request timeout")]
    Timeout,

    #[error("Queue is full")]
    QueueFull,

    #[error("No model loaded")]
    ModelNotLoaded,

    #[error("Not implemented: {0}")]
    NotImplemented(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

impl IntoResponse for ExsaError {
    fn into_response(self) -> Response {
        let (status, error_message) = match self {
            ExsaError::ModelError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
            ExsaError::InferenceError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
            ExsaError::ResourceExhausted(msg) => (StatusCode::INSUFFICIENT_STORAGE, msg.clone()),
            ExsaError::ModelLoadError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
            ExsaError::InternalError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
            ExsaError::InvalidParameters(msg) => (StatusCode::BAD_REQUEST, msg),
            ExsaError::Timeout => (StatusCode::REQUEST_TIMEOUT, "Request timeout".to_string()),
            ExsaError::QueueFull => (
                StatusCode::SERVICE_UNAVAILABLE,
                "Server is at capacity".to_string(),
            ),
            ExsaError::ModelNotLoaded => (
                StatusCode::SERVICE_UNAVAILABLE,
                "No model loaded".to_string(),
            ),
            ExsaError::NotImplemented(msg) => (StatusCode::NOT_IMPLEMENTED, msg),
            ExsaError::Io(err) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("IO error: {}", err),
            ),
        };

        let body = Json(json!({
            "error": error_message,
        }));

        (status, body).into_response()
    }
}

pub type Result<T> = std::result::Result<T, ExsaError>;
