use crate::model::config::ModelConfig;
use crate::utils::error::{ExsaError, Result};
use llama_cpp_2::{llama_backend::LlamaBackend, model::LlamaModel};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

/// Information about a loaded model
#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub name: String,
    pub path: PathBuf,
    pub size_bytes: u64,
    pub load_time_ms: u64,
    pub n_vocab: i32,
    pub n_ctx_max: usize,
    pub loaded_at: std::time::SystemTime,
    pub last_used: std::time::SystemTime, // For LRU eviction
}

/// Manages multiple models with hot-swapping capability
pub struct ModelManager {
    /// Currently active model
    active_model: Arc<RwLock<(String, Arc<LlamaModel>)>>,

    /// Cached models (name -> model)
    model_cache: Arc<RwLock<HashMap<String, Arc<LlamaModel>>>>,

    /// Model configurations (name -> config)
    model_configs: Arc<RwLock<HashMap<String, ModelConfig>>>,

    /// Model metadata (name -> info)
    model_info: Arc<RwLock<HashMap<String, ModelInfo>>>,

    /// Backend (shared across all models)
    backend: Arc<LlamaBackend>,

    /// Maximum number of models to cache
    max_cache_size: usize,
}

impl ModelManager {
    /// Create a new model manager with an initial model
    pub fn new(
        initial_name: String,
        initial_path: PathBuf,
        config: ModelConfig,
        backend: Arc<LlamaBackend>,
        max_cache_size: usize,
    ) -> Result<Self> {
        tracing::info!("Initializing ModelManager with model: {}", initial_name);

        let start = std::time::Instant::now();

        // Load initial model
        let model = LlamaModel::load_from_file(&backend, &initial_path, &config.into_params())
            .map_err(|e| {
                ExsaError::ModelLoadError(format!("Failed to load initial model: {}", e))
            })?;

        let model_arc = Arc::new(model);
        let load_time = start.elapsed().as_millis() as u64;

        // Get model info
        let size_bytes = std::fs::metadata(&initial_path)
            .map(|m| m.len())
            .unwrap_or(0);

        let info = ModelInfo {
            name: initial_name.clone(),
            path: initial_path.clone(),
            size_bytes,
            load_time_ms: load_time,
            n_vocab: model_arc.n_vocab(),
            n_ctx_max: config.n_ctx as usize,
            loaded_at: std::time::SystemTime::now(),
            last_used: std::time::SystemTime::now(),
        };

        // Initialize collections
        let mut cache = HashMap::new();
        cache.insert(initial_name.clone(), model_arc.clone());

        let mut configs = HashMap::new();
        configs.insert(initial_name.clone(), config);

        let mut infos = HashMap::new();
        infos.insert(initial_name.clone(), info);

        tracing::info!("Model loaded in {}ms", load_time);

        Ok(Self {
            active_model: Arc::new(RwLock::new((initial_name, model_arc))),
            model_cache: Arc::new(RwLock::new(cache)),
            model_configs: Arc::new(RwLock::new(configs)),
            model_info: Arc::new(RwLock::new(infos)),
            backend,
            max_cache_size,
        })
    }

    /// Create a new model manager asynchronously (non-blocking)
    pub async fn async_new(
        initial_name: String,
        initial_path: PathBuf,
        config: ModelConfig,
        backend: Arc<LlamaBackend>,
        max_cache_size: usize,
    ) -> Result<Self> {
        tracing::info!(
            "Initializing ModelManager asynchronously with model: {}",
            initial_name
        );

        let start = std::time::Instant::now();

        // Load initial model in background task
        let backend_clone = backend.clone();
        let path_clone = initial_path.clone();
        let config_clone = config.clone();

        let (model_arc, load_time, size_bytes) = tokio::task::spawn_blocking(move || {
            let model = LlamaModel::load_from_file(
                &backend_clone,
                &path_clone,
                &config_clone.into_params(),
            )
            .map_err(|e| {
                ExsaError::ModelLoadError(format!("Failed to load initial model: {}", e))
            })?;

            let model_arc = Arc::new(model);
            let load_time = start.elapsed().as_millis() as u64;

            let size_bytes = std::fs::metadata(&path_clone).map(|m| m.len()).unwrap_or(0);

            Ok::<_, ExsaError>((model_arc, load_time, size_bytes))
        })
        .await
        .map_err(|e| ExsaError::InternalError(format!("Task join error: {}", e)))??;

        // Get model info
        let info = ModelInfo {
            name: initial_name.clone(),
            path: initial_path.clone(),
            size_bytes,
            load_time_ms: load_time,
            n_vocab: model_arc.n_vocab(),
            n_ctx_max: config.n_ctx as usize,
            loaded_at: std::time::SystemTime::now(),
            last_used: std::time::SystemTime::now(),
        };

        // Initialize collections
        let mut cache = HashMap::new();
        cache.insert(initial_name.clone(), model_arc.clone());

        let mut configs = HashMap::new();
        configs.insert(initial_name.clone(), config);

        let mut infos = HashMap::new();
        infos.insert(initial_name.clone(), info);

        tracing::info!("Model loaded asynchronously in {}ms", load_time);

        Ok(Self {
            active_model: Arc::new(RwLock::new((initial_name, model_arc))),
            model_cache: Arc::new(RwLock::new(cache)),
            model_configs: Arc::new(RwLock::new(configs)),
            model_info: Arc::new(RwLock::new(infos)),
            backend,
            max_cache_size,
        })
    }

    /// Get the currently active model
    pub fn get_active_model(&self) -> Result<Arc<LlamaModel>> {
        let active = self
            .active_model
            .read()
            .map_err(|e| ExsaError::InternalError(format!("Lock error: {}", e)))?;

        // Update last_used time
        let model_name = active.0.clone();
        drop(active);
        self.update_last_used(&model_name)?;

        let active = self
            .active_model
            .read()
            .map_err(|e| ExsaError::InternalError(format!("Lock error: {}", e)))?;
        Ok(active.1.clone())
    }

    /// Get the name of the active model
    pub fn get_active_model_name(&self) -> Result<String> {
        let active = self
            .active_model
            .read()
            .map_err(|e| ExsaError::InternalError(format!("Lock error: {}", e)))?;
        Ok(active.0.clone())
    }

    /// Switch to a different model (hot-swap)
    pub fn switch_model(&self, model_name: &str) -> Result<()> {
        tracing::info!("Switching to model: {}", model_name);

        // Check if model is already cached
        let cache = self
            .model_cache
            .read()
            .map_err(|e| ExsaError::InternalError(format!("Lock error: {}", e)))?;

        if let Some(model) = cache.get(model_name) {
            // Model is cached, just switch to it
            let mut active = self
                .active_model
                .write()
                .map_err(|e| ExsaError::InternalError(format!("Lock error: {}", e)))?;
            *active = (model_name.to_string(), model.clone());
            drop(active);
            self.update_last_used(model_name)?;
            tracing::info!("✅ Switched to cached model: {}", model_name);
            Ok(())
        } else {
            // Model not cached
            drop(cache); // Release read lock before error
            Err(ExsaError::ModelLoadError(format!(
                "Model '{}' not found in cache. Use load_model() first.",
                model_name
            )))
        }
    }

    /// Load a new model into the cache
    pub fn load_model(&self, name: String, path: PathBuf, config: ModelConfig) -> Result<()> {
        tracing::info!("Loading new model: {} from {:?}", name, path);

        // If the model is already cached, we may still need to reload it.
        // In llama.cpp, GPU offload (n_gpu_layers) is applied at model load time.
        // If a model was cached with n_gpu_layers=0, switching back to it later will
        // silently fall back to CPU even if the active runtime config requests GPU.
        let reload_needed = {
            let cache = self
                .model_cache
                .read()
                .map_err(|e| ExsaError::InternalError(format!("Lock error: {}", e)))?;

            if !cache.contains_key(&name) {
                false
            } else {
                drop(cache);
                let configs = self
                    .model_configs
                    .read()
                    .map_err(|e| ExsaError::InternalError(format!("Lock error: {}", e)))?;
                let existing = configs.get(&name);
                existing
                    .map(|c| c.n_gpu_layers != config.n_gpu_layers)
                    .unwrap_or(false)
            }
        };

        if !reload_needed {
            let cache = self
                .model_cache
                .read()
                .map_err(|e| ExsaError::InternalError(format!("Lock error: {}", e)))?;
            if cache.contains_key(&name) {
                drop(cache);
                tracing::info!("Model {} already loaded", name);
                return Ok(());
            }
        } else {
            tracing::info!(
                "Reloading cached model {} due to GPU layer config change",
                name
            );
        }

        // Load the model
        let start = std::time::Instant::now();
        let model = LlamaModel::load_from_file(&self.backend, &path, &config.into_params())
            .map_err(|e| ExsaError::ModelLoadError(format!("Failed to load model: {}", e)))?;

        let model_arc = Arc::new(model);
        let load_time = start.elapsed().as_millis() as u64;

        // Get metadata
        let size_bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);

        let info = ModelInfo {
            name: name.clone(),
            path: path.clone(),
            size_bytes,
            load_time_ms: load_time,
            n_vocab: model_arc.n_vocab(),
            n_ctx_max: config.n_ctx as usize,
            loaded_at: std::time::SystemTime::now(),
            last_used: std::time::SystemTime::now(),
        };

        // Check cache size and evict if needed (only when inserting a new entry)
        {
            let cache_read = self
                .model_cache
                .read()
                .map_err(|e| ExsaError::InternalError(format!("Lock error: {}", e)))?;
            let exists = cache_read.contains_key(&name);
            let cache_len = cache_read.len();
            drop(cache_read);

            if !exists && cache_len >= self.max_cache_size {
                self.evict_lru_model()?;
            }
        }

        // Add/replace in cache
        let mut cache = self
            .model_cache
            .write()
            .map_err(|e| ExsaError::InternalError(format!("Lock error: {}", e)))?;
        cache.insert(name.clone(), model_arc.clone());
        drop(cache);

        // If this model is currently active, update active pointer as well.
        if let Ok(mut active) = self.active_model.write() {
            if active.0 == name {
                *active = (name.clone(), model_arc.clone());
            }
        }

        // Store config and info
        let mut configs = self
            .model_configs
            .write()
            .map_err(|e| ExsaError::InternalError(format!("Lock error: {}", e)))?;
        configs.insert(name.clone(), config);
        drop(configs);

        let mut infos = self
            .model_info
            .write()
            .map_err(|e| ExsaError::InternalError(format!("Lock error: {}", e)))?;
        infos.insert(name.clone(), info);

        tracing::info!("✅ Model {} loaded in {}ms", name, load_time);
        Ok(())
    }

    /// List all available models
    pub fn list_models(&self) -> Result<Vec<String>> {
        let cache = self
            .model_cache
            .read()
            .map_err(|e| ExsaError::InternalError(format!("Lock error: {}", e)))?;
        Ok(cache.keys().cloned().collect())
    }

    /// Get information about a specific model
    pub fn get_model_info(&self, name: &str) -> Result<ModelInfo> {
        let infos = self
            .model_info
            .read()
            .map_err(|e| ExsaError::InternalError(format!("Lock error: {}", e)))?;

        infos
            .get(name)
            .cloned()
            .ok_or_else(|| ExsaError::ModelLoadError(format!("Model {} not found", name)))
    }

    /// Get information about all loaded models
    pub fn get_all_model_info(&self) -> Result<Vec<ModelInfo>> {
        let infos = self
            .model_info
            .read()
            .map_err(|e| ExsaError::InternalError(format!("Lock error: {}", e)))?;
        Ok(infos.values().cloned().collect())
    }

    /// Unload a model from cache (except active model)
    pub fn unload_model(&self, name: &str) -> Result<()> {
        let active_name = self.get_active_model_name()?;

        if name == active_name {
            return Err(ExsaError::InvalidParameters(
                "Cannot unload the active model".to_string(),
            ));
        }

        let mut cache = self
            .model_cache
            .write()
            .map_err(|e| ExsaError::InternalError(format!("Lock error: {}", e)))?;

        cache.remove(name);
        tracing::info!("Unloaded model: {}", name);
        Ok(())
    }

    /// Update last_used timestamp for a model (for LRU tracking)
    fn update_last_used(&self, name: &str) -> Result<()> {
        let mut infos = self
            .model_info
            .write()
            .map_err(|e| ExsaError::InternalError(format!("Lock error: {}", e)))?;

        if let Some(info) = infos.get_mut(name) {
            info.last_used = std::time::SystemTime::now();
        }

        Ok(())
    }

    /// Evict least recently used model from cache
    fn evict_lru_model(&self) -> Result<()> {
        // Find LRU model (excluding active model)
        let active_name = self.get_active_model_name()?;

        let infos = self
            .model_info
            .read()
            .map_err(|e| ExsaError::InternalError(format!("Lock error: {}", e)))?;

        let lru_model = infos
            .iter()
            .filter(|(name, _)| **name != active_name) // Don't evict active model
            .min_by_key(|(_, info)| info.last_used)
            .map(|(name, _)| name.clone());

        drop(infos);

        if let Some(name) = lru_model {
            tracing::info!("Evicting least recently used model: {}", name);
            self.unload_model(&name)?;
        } else {
            tracing::warn!("No model available for eviction (all models in use)");
        }

        Ok(())
    }
}
