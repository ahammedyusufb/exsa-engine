//! API request/response schemas

use crate::inference::{InferenceEngine, QueueHandle, SamplingParams};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub queue: QueueHandle,
    pub engine: Arc<InferenceEngine>,

    /// Serialize model switching/loading operations
    pub model_switch_lock: Arc<tokio::sync::Mutex<()>>,

    /// Shutdown flag for graceful shutdown coordination
    pub shutdown_flag: Arc<std::sync::atomic::AtomicBool>,

    /// Server start time for uptime calculation
    pub start_time: std::time::Instant,
}

/// Request to generate text
#[derive(Debug, Deserialize, Serialize)]
pub struct GenerateRequest {
    /// The input prompt
    pub prompt: String,

    /// Sampling parameters (optional, uses defaults if not provided)
    #[serde(default)]
    pub sampling_params: SamplingParams,

    /// Whether to apply chat template formatting (default: true)
    #[serde(default)]
    pub use_chat_template: Option<bool>,
}

/// Server-sent event for token streaming
#[derive(Debug, Serialize, Deserialize)]
pub struct TokenEvent {
    /// The generated token text
    pub token: String,

    /// Whether this is the final token
    pub done: bool,
}

/// Health check response with detailed diagnostics
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,

    // Optional detailed info
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_loaded: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_path: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_size: Option<usize>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu_layers: Option<i32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_requests: Option<usize>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue_size: Option<usize>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue_capacity: Option<usize>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub uptime_seconds: Option<u64>,
}

/// Server status response
#[derive(Debug, Serialize)]
pub struct StatusResponse {
    pub status: String,
    pub queue_capacity: usize,
    pub active_requests: usize,
}

/// Model information response
#[derive(Debug, Serialize)]
pub struct ModelInfoResponse {
    pub model_path: String,
    pub context_size: usize,
    pub gpu_layers: i32,
}

/// Model metadata
#[derive(Debug, Serialize, Clone)]
pub struct ModelInfo {
    pub model_path: String,
    pub context_size: usize,
    pub gpu_layers: i32,
}

impl From<ModelInfo> for ModelInfoResponse {
    fn from(info: ModelInfo) -> Self {
        Self {
            model_path: info.model_path,
            context_size: info.context_size,
            gpu_layers: info.gpu_layers,
        }
    }
}

/// Error response
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}
