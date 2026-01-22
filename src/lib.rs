//! Exsa-Engine: Production-grade local LLM inference engine
//!
//! This library provides the core functionality for hosting and serving
//! GGUF-based language models using llama.cpp as the inference backend.
//!
//! ## Security
//!
//! Exsa-Engine is designed for privacy-first, offline deployments:
//! - 100% local processing with zero external dependencies
//! - Localhost-only binding by default
//! - Optional rate limiting for resource protection
//! - No telemetry or tracking
//!
//! ## Example
//!
//! ```no_run
//! use exsa_engine::{ModelConfig, InferenceEngine};
//!
//! #[tokio::main]
//! async fn main() {
//!     let config = ModelConfig::new("models/model.gguf")
//!         .with_gpu_layers(32)
//!         .with_context_size(2048);
//!     
//!     let engine = InferenceEngine::new(
//!         "my-model".to_string(),
//!         "models/model.gguf".to_string(),
//!         config,
//!     ).unwrap();
//!     // Use engine for inference...
//! }
//! ```

pub mod api;
pub mod config;
pub mod inference;
pub mod metrics;
pub mod model;
pub mod rag;
pub mod session;
pub mod tests;
pub mod utils;

pub use api::{build_router, AppState};
pub use config::ProductionConfig;
pub use inference::{InferenceEngine, SamplingParams};
pub use metrics::{create_metrics, EngineMetrics, MetricsSnapshot, SharedMetrics};
pub use model::{ModelConfig, ModelLoader};
pub use session::{Session, SessionConfig, SessionManager, SharedSessionManager};
pub use utils::error::{ExsaError, Result};
