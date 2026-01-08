//! Model loading and management

use crate::model::config::ModelConfig;
use crate::utils::error::{ExsaError, Result};
use std::path::Path;
use tracing::{info, warn};

/// Model metadata
#[derive(Debug, Clone)]
pub struct ModelMetadata {
    pub name: String,
    pub path: String,
    pub size_bytes: u64,
    pub n_params: Option<u64>,
}

/// Model loader and manager
pub struct ModelLoader {
    config: ModelConfig,
}

impl ModelLoader {
    /// Create a new model loader with the given configuration
    pub fn new(config: ModelConfig) -> Self {
        Self { config }
    }

    /// Validate that the model file exists and is accessible
    pub fn validate(&self) -> Result<()> {
        let path = Path::new(&self.config.model_path);

        if !path.exists() {
            return Err(ExsaError::ModelError(format!(
                "Model file not found: {}",
                self.config.model_path
            )));
        }

        if !path.is_file() {
            return Err(ExsaError::ModelError(format!(
                "Path is not a file: {}",
                self.config.model_path
            )));
        }

        // Check file extension
        if let Some(ext) = path.extension() {
            if ext != "gguf" {
                warn!("Model file does not have .gguf extension, but will attempt to load");
            }
        }

        info!("Model validation passed: {}", self.config.model_path);

        Ok(())
    }

    /// Get metadata about the model
    pub fn get_metadata(&self) -> Result<ModelMetadata> {
        let path = Path::new(&self.config.model_path);
        let metadata = std::fs::metadata(path)?;

        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        Ok(ModelMetadata {
            name,
            path: self.config.model_path.clone(),
            size_bytes: metadata.len(),
            n_params: None, // Will be populated after loading
        })
    }

    /// Get the model configuration
    pub fn config(&self) -> &ModelConfig {
        &self.config
    }
}
