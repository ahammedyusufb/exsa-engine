//! API route configuration

use super::handlers::{chat_completions, generate, health, status};
use super::lifecycle::{get_active_model, list_models, load_model, reload_model, unload_model};
use super::schema::AppState;
use axum::{
    routing::{get, post},
    Router,
};

/// Build the application router
pub fn build_router(state: AppState) -> Router {
    // Single router with AppState
    Router::new()
        // Generation endpoints (using AppState)
        .route("/v1/generate", post(generate))
        // OpenAI-compatible endpoint
        .route("/v1/chat/completions", post(chat_completions))
        // Status endpoints (using AppState)
        .route("/v1/health", get(health))
        .route("/v1/status", get(status))
        .route("/v1/model/info", get(super::handlers::model_info))
        .route("/v1/models/load", post(load_model))
        .route("/v1/models/unload", post(unload_model))
        .route("/v1/models/reload", post(reload_model))
        .route("/v1/models/list", get(list_models))
        .route("/v1/models/active", get(get_active_model))
        .with_state(state)
}
