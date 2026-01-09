//! Production configuration with TOML support
//!
//! Provides unified configuration for all EXSA engine components
//! with environment variable override and validation.

use crate::inference::context_config::OverflowPolicy;
use crate::model::config::{KvCacheQuantization, RopeScalingType};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;
use tracing::{info, warn};

/// Complete production configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProductionConfig {
    /// Server configuration
    pub server: ServerConfig,
    /// Model configuration
    pub model: ModelSettings,
    /// KV cache configuration
    pub kv_cache: KvCacheConfig,
    /// Context configuration
    pub context: ContextSettings,
    /// Session configuration
    pub session: SessionSettings,
    /// Performance tuning
    pub performance: PerformanceConfig,
    /// Logging configuration
    pub logging: LoggingConfig,
}

impl ProductionConfig {
    /// Load configuration from TOML file
    pub fn from_file(path: &str) -> Result<Self, String> {
        let contents = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read config file: {}", e))?;

        toml::from_str(&contents).map_err(|e| format!("Failed to parse config: {}", e))
    }

    /// Load from file or use defaults with env overrides
    pub fn load() -> Self {
        // Try loading from file first
        let mut config = if let Ok(path) = std::env::var("EXSA_CONFIG") {
            match Self::from_file(&path) {
                Ok(cfg) => {
                    info!("Loaded config from {}", path);
                    cfg
                }
                Err(e) => {
                    warn!("Failed to load config: {}, using defaults", e);
                    Self::default()
                }
            }
        } else {
            Self::default()
        };

        // Apply environment variable overrides
        config.apply_env_overrides();
        config
    }

    /// Apply environment variable overrides
    pub fn apply_env_overrides(&mut self) {
        // Server overrides
        if let Ok(host) = std::env::var("EXSA_HOST") {
            self.server.host = host;
        }
        if let Ok(port) = std::env::var("EXSA_PORT") {
            if let Ok(p) = port.parse() {
                self.server.port = p;
            }
        }

        // Model overrides
        if let Ok(path) = std::env::var("MODEL_PATH") {
            self.model.path = PathBuf::from(path);
        }
        if let Ok(layers) = std::env::var("GPU_LAYERS") {
            if let Ok(n) = layers.parse() {
                self.model.gpu_layers = n;
            }
        }
        if let Ok(ctx) = std::env::var("CONTEXT_SIZE") {
            if let Ok(n) = ctx.parse() {
                self.context.max_tokens = n;
            }
        }

        // KV cache overrides
        if let Ok(quant) = std::env::var("KV_CACHE_TYPE") {
            self.kv_cache.quantization = KvCacheQuantization::from_str_lossy(&quant);
        }

        // Performance overrides
        if let Ok(batch) = std::env::var("BATCH_SIZE") {
            if let Ok(n) = batch.parse() {
                self.performance.batch_size = n;
            }
        }
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        if self.server.port == 0 {
            errors.push("Server port cannot be 0".to_string());
        }

        if self.context.max_tokens < 512 {
            errors.push("Context size must be at least 512".to_string());
        }

        if self.context.n_keep >= self.context.max_tokens {
            errors.push("n_keep must be less than max_tokens".to_string());
        }

        if self.session.max_sessions == 0 {
            errors.push("max_sessions must be at least 1".to_string());
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Generate TOML string
    pub fn to_toml(&self) -> Result<String, String> {
        toml::to_string_pretty(self).map_err(|e| format!("Failed to serialize config: {}", e))
    }
}

/// Server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Host to bind to
    pub host: String,
    /// Port to listen on
    pub port: u16,
    /// Enable CORS
    pub cors_enabled: bool,
    /// Request timeout in seconds
    pub request_timeout_secs: u64,
    /// Maximum concurrent requests
    pub max_concurrent_requests: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 3000,
            cors_enabled: true,
            request_timeout_secs: 300,
            max_concurrent_requests: 100,
        }
    }
}

/// Model settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSettings {
    /// Path to GGUF model file
    pub path: PathBuf,
    /// Number of GPU layers to offload
    pub gpu_layers: u32,
    /// Number of CPU threads
    pub threads: u32,
    /// Use memory mapping
    pub use_mmap: bool,
    /// Use memory locking
    pub use_mlock: bool,
}

impl Default for ModelSettings {
    fn default() -> Self {
        Self {
            path: PathBuf::from("models/model.gguf"),
            gpu_layers: 0,
            threads: num_cpus::get() as u32,
            use_mmap: true,
            use_mlock: false,
        }
    }
}

/// KV cache configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KvCacheConfig {
    /// Quantization type for KV cache
    pub quantization: KvCacheQuantization,
    /// Maximum entries in cache pool
    pub max_entries: usize,
    /// Maximum memory in MB
    pub max_memory_mb: usize,
    /// Defragmentation threshold (0.0-1.0)
    pub defrag_threshold: f32,
}

impl Default for KvCacheConfig {
    fn default() -> Self {
        Self {
            quantization: KvCacheQuantization::F16,
            max_entries: 32,
            max_memory_mb: 4096,
            defrag_threshold: 0.2,
        }
    }
}

/// Context settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSettings {
    /// Maximum context tokens
    pub max_tokens: usize,
    /// Tokens to preserve (system prompt)
    pub n_keep: usize,
    /// Sliding window threshold (0.0-1.0)
    pub sliding_threshold: f32,
    /// Keep ratio after sliding (0.0-1.0)
    pub keep_ratio: f32,
    /// Overflow policy
    pub overflow_policy: OverflowPolicy,
    /// RoPE scaling type
    pub rope_scaling: RopeScalingType,
    /// RoPE scale factor
    pub rope_scale_factor: f32,
}

impl Default for ContextSettings {
    fn default() -> Self {
        Self {
            max_tokens: 4096,
            n_keep: 0,
            sliding_threshold: 0.92,
            keep_ratio: 0.70,
            overflow_policy: OverflowPolicy::SlidingWindow,
            rope_scaling: RopeScalingType::None,
            rope_scale_factor: 1.0,
        }
    }
}

/// Session settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSettings {
    /// Maximum concurrent sessions
    pub max_sessions: usize,
    /// Idle timeout in seconds
    pub idle_timeout_secs: u64,
    /// Maximum session lifetime in seconds
    pub max_lifetime_secs: u64,
    /// Enable prompt caching
    pub enable_prompt_cache: bool,
}

impl Default for SessionSettings {
    fn default() -> Self {
        Self {
            max_sessions: 100,
            idle_timeout_secs: 300,
            max_lifetime_secs: 3600,
            enable_prompt_cache: true,
        }
    }
}

impl SessionSettings {
    /// Convert to SessionConfig
    pub fn to_session_config(&self) -> crate::session::SessionConfig {
        crate::session::SessionConfig {
            idle_timeout: Duration::from_secs(self.idle_timeout_secs),
            max_lifetime: Duration::from_secs(self.max_lifetime_secs),
            n_keep: 0,
            max_context_tokens: 4096,
            enable_prompt_cache: self.enable_prompt_cache,
        }
    }
}

/// Performance tuning
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceConfig {
    /// Batch size for processing
    pub batch_size: u32,
    /// Maximum concurrent batches
    pub max_batch_size: usize,
    /// Batch timeout in milliseconds
    pub batch_timeout_ms: u64,
    /// Token channel buffer size
    pub token_buffer_size: usize,
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            batch_size: 2048,
            max_batch_size: 8,
            batch_timeout_ms: 100,
            token_buffer_size: 256,
        }
    }
}

/// Logging configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Log level (trace, debug, info, warn, error)
    pub level: String,
    /// Log format (json, pretty)
    pub format: String,
    /// Include timestamps
    pub timestamps: bool,
    /// Log to file path (None = stdout only)
    pub file: Option<PathBuf>,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            format: "pretty".to_string(),
            timestamps: true,
            file: None,
        }
    }
}

/// Generate example configuration
pub fn generate_example_config() -> String {
    let config = ProductionConfig::default();
    config
        .to_toml()
        .unwrap_or_else(|_| "# Failed to generate".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ProductionConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_validation() {
        let mut config = ProductionConfig::default();
        config.context.n_keep = 10000; // Invalid: > max_tokens
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_toml_roundtrip() {
        let config = ProductionConfig::default();
        let toml = config.to_toml().unwrap();
        assert!(!toml.is_empty());
    }
}
