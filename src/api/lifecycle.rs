//! Model lifecycle management API

use crate::api::schema::{AppState, ModelInfo};
use crate::utils::error::{ExsaError, Result};
use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

fn resolve_models_dir() -> Result<PathBuf> {
    // Prefer explicit configuration
    if let Ok(dir) = std::env::var("MODELS_DIR") {
        let p = PathBuf::from(dir);
        return std::fs::canonicalize(&p)
            .map_err(|_| ExsaError::InvalidParameters("MODELS_DIR is invalid".to_string()));
    }

    // Common dev/prod working directories:
    // - repo root: ./models
    // - exsa-engine folder: ../models
    for candidate in ["models", "../models"] {
        let p = PathBuf::from(candidate);
        if p.exists() {
            if let Ok(canon) = std::fs::canonicalize(&p) {
                return Ok(canon);
            }
        }
    }

    Err(ExsaError::InvalidParameters(
        "Models directory not found (set MODELS_DIR)".to_string(),
    ))
}

fn resolve_model_path(models_dir: &PathBuf, raw: &str) -> Result<PathBuf> {
    let p = PathBuf::from(raw);
    let candidate = if p.is_absolute() {
        p
    } else {
        models_dir.join(p)
    };

    let canon = std::fs::canonicalize(&candidate)
        .map_err(|_| ExsaError::InvalidParameters(format!("Model file not found: {}", raw)))?;

    if !canon.starts_with(models_dir) {
        return Err(ExsaError::InvalidParameters(
            "Model path must be inside the configured models directory".to_string(),
        ));
    }

    Ok(canon)
}

/// Load model request
#[derive(Debug, Deserialize)]
pub struct LoadModelRequest {
    /// Path to GGUF model file
    pub model_path: String,

    /// Number of GPU layers (optional)
    pub gpu_layers: Option<i32>,

    /// Context size (optional)
    pub context_size: Option<usize>,
}

/// Load model response
#[derive(Debug, Serialize)]
pub struct LoadModelResponse {
    pub success: bool,
    pub message: String,
    pub model_info: Option<ModelInfo>,
}

/// List models response
#[derive(Debug, Serialize)]
pub struct ListModelsResponse {
    pub models: Vec<String>,
}

/// Load a model from disk
pub async fn load_model(
    State(state): State<AppState>,
    Json(request): Json<LoadModelRequest>,
) -> Result<impl IntoResponse> {
    // Serialize model switching across requests
    let _guard = state.model_switch_lock.lock().await;

    // Only allow loading GGUF models from the local ./models directory
    // (matches /v1/models/list and prevents arbitrary path access).
    if !request.model_path.to_lowercase().ends_with(".gguf") {
        return Err(ExsaError::InvalidParameters(
            "Only .gguf models are supported".to_string(),
        ));
    }

    let models_dir = resolve_models_dir()?;
    let target_path = resolve_model_path(&models_dir, &request.model_path)?;

    // Refuse switching while there are queued requests (avoid user-perceived "random" latency)
    if state.queue.pending_count() > 0 {
        return Err(ExsaError::InvalidParameters(
            "Cannot switch models while requests are queued".to_string(),
        ));
    }

    let engine = state.engine.clone();
    // Use canonical absolute path for engine stability
    let model_path = target_path.to_string_lossy().to_string();
    let gpu_layers = request.gpu_layers;
    let context_size = request.context_size;

    let info = tokio::task::spawn_blocking(move || {
        engine.load_and_switch_model(model_path, gpu_layers, context_size)
    })
    .await
    .map_err(|e| ExsaError::InternalError(format!("Model switch task failed: {}", e)))??;

    let response = LoadModelResponse {
        success: true,
        message: format!("Model loaded: {}", info.model_path),
        model_info: Some(info),
    };

    Ok((StatusCode::OK, Json(response)))
}

/// Unload the currently active model
pub async fn unload_model(State(_state): State<AppState>) -> Result<impl IntoResponse> {
    let response = LoadModelResponse {
        success: false,
        message: "Unload is not supported via API at the moment".to_string(),
        model_info: None,
    };
    Ok((StatusCode::BAD_REQUEST, Json(response)))
}

/// Reload the currently active model
pub async fn reload_model(State(state): State<AppState>) -> Result<impl IntoResponse> {
    let _guard = state.model_switch_lock.lock().await;

    if state.queue.pending_count() > 0 {
        return Err(ExsaError::InvalidParameters(
            "Cannot reload model while requests are queued".to_string(),
        ));
    }

    let current = state.engine.model_info();
    let engine = state.engine.clone();
    let model_path = current.model_path;

    let info =
        tokio::task::spawn_blocking(move || engine.load_and_switch_model(model_path, None, None))
            .await
            .map_err(|e| ExsaError::InternalError(format!("Model reload task failed: {}", e)))??;

    let response = LoadModelResponse {
        success: true,
        message: format!("Model reloaded: {}", info.model_path),
        model_info: Some(info),
    };

    Ok((StatusCode::OK, Json(response)))
}

/// Get currently active model information
pub async fn get_active_model(State(state): State<AppState>) -> Result<impl IntoResponse> {
    let info = state.engine.model_info();
    Ok((StatusCode::OK, Json(info)))
}

/// List available GGUF models in models directory
pub async fn list_models() -> Result<impl IntoResponse> {
    let models_dir = resolve_models_dir()?;

    // Avoid blocking the async runtime with std::fs in request handlers.
    let mut models = Vec::new();

    let mut entries = match tokio::fs::read_dir(&models_dir).await {
        Ok(entries) => entries,
        Err(_) => {
            return Ok((StatusCode::OK, Json(ListModelsResponse { models })));
        }
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let file_type = match entry.file_type().await {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        if !file_type.is_file() {
            continue;
        }
        let path = entry.path();
        if let Ok(canon) = std::fs::canonicalize(&path) {
            if let Some(path_str) = canon.to_str() {
                if path_str.to_lowercase().ends_with(".gguf") {
                    models.push(path_str.to_string());
                }
            }
        }
    }

    models.sort();

    Ok((StatusCode::OK, Json(ListModelsResponse { models })))
}
