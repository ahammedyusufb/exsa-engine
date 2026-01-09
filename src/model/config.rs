//! Model configuration structures

use serde::{Deserialize, Serialize};

/// KV cache quantization type for memory optimization
///
/// Lower bit quantization saves memory but may slightly reduce quality.
/// Recommended: Q8_0 for quality, Q4_0 for memory-constrained environments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
#[allow(non_camel_case_types)]
pub enum KvCacheQuantization {
    /// Full precision 32-bit float (maximum quality, highest memory)
    F32,
    /// Half precision 16-bit float (good quality, moderate memory)
    #[default]
    F16,
    /// 8-bit quantization (recommended balance of quality and memory)
    Q8_0,
    /// 4-bit quantization (aggressive memory savings, some quality loss)
    Q4_0,
    /// 4-bit with improved accuracy
    Q4_1,
    /// K-quant 4-bit (better quality than Q4_0)
    Q4_K,
    /// K-quant 5-bit
    Q5_K,
    /// K-quant 6-bit
    Q6_K,
    /// K-quant 8-bit (best K-quant quality)
    Q8_K,
}

impl KvCacheQuantization {
    /// Convert to llama-cpp-2 KvCacheType
    pub fn to_llama_type(self) -> llama_cpp_2::context::params::KvCacheType {
        use llama_cpp_2::context::params::KvCacheType;
        match self {
            Self::F32 => KvCacheType::F32,
            Self::F16 => KvCacheType::F16,
            Self::Q8_0 => KvCacheType::Q8_0,
            Self::Q4_0 => KvCacheType::Q4_0,
            Self::Q4_1 => KvCacheType::Q4_1,
            Self::Q4_K => KvCacheType::Q4_K,
            Self::Q5_K => KvCacheType::Q5_K,
            Self::Q6_K => KvCacheType::Q6_K,
            Self::Q8_K => KvCacheType::Q8_K,
        }
    }

    /// Estimated memory savings ratio compared to F16
    pub fn memory_ratio(self) -> f32 {
        match self {
            Self::F32 => 2.0,               // 2x more than F16
            Self::F16 => 1.0,               // Baseline
            Self::Q8_0 | Self::Q8_K => 0.5, // 50% of F16
            Self::Q6_K => 0.375,
            Self::Q5_K => 0.3125,
            Self::Q4_0 | Self::Q4_1 | Self::Q4_K => 0.25, // 25% of F16
        }
    }

    /// Parse from string (case-insensitive)
    pub fn from_str_lossy(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "f32" => Self::F32,
            "f16" => Self::F16,
            "q8_0" | "q8" | "8" => Self::Q8_0,
            "q4_0" | "q4" | "4" => Self::Q4_0,
            "q4_1" => Self::Q4_1,
            "q4_k" => Self::Q4_K,
            "q5_k" => Self::Q5_K,
            "q6_k" => Self::Q6_K,
            "q8_k" => Self::Q8_K,
            _ => Self::F16, // Default to F16
        }
    }
}

/// RoPE (Rotary Position Embedding) scaling type for extended context support
///
/// Different scaling methods allow models to handle contexts longer than their training length.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RopeScalingType {
    /// No scaling - use native context length
    #[default]
    None,
    /// Linear interpolation scaling (simple, works well for 2-4x extension)
    Linear,
    /// YaRN scaling (better quality for large extensions, supports 8-16x)
    Yarn,
    /// Dynamic NTK scaling
    NtkDynamic,
}

impl RopeScalingType {
    /// Parse from string (case-insensitive)
    pub fn from_str_lossy(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "linear" | "lin" => Self::Linear,
            "yarn" => Self::Yarn,
            "ntk" | "ntk_dynamic" | "dynamic" => Self::NtkDynamic,
            _ => Self::None,
        }
    }

    /// Whether this scaling type is active
    pub fn is_active(self) -> bool {
        !matches!(self, Self::None)
    }
}

/// Model configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    /// Path to the GGUF model file
    pub model_path: String,

    /// Number of threads to use for CPU inference
    pub n_threads: u32,

    /// Number of GPU layers to offload (0 = CPU only)
    pub n_gpu_layers: u32,

    /// Context size (maximum tokens in context window)
    pub n_ctx: u32,

    /// Batch size for prompt processing
    pub n_batch: u32,

    /// Use memory mapping for model loading
    pub use_mmap: bool,

    /// Use memory locking (prevents swapping to disk)
    pub use_mlock: bool,

    /// Token channel buffer size (for streaming responses)
    pub token_channel_size: usize,

    /// KV cache quantization for keys (affects memory usage)
    pub kv_cache_type_k: KvCacheQuantization,

    /// KV cache quantization for values (affects memory usage)
    pub kv_cache_type_v: KvCacheQuantization,

    /// RoPE scaling type for extended context (None = no scaling)
    pub rope_scaling_type: RopeScalingType,

    /// RoPE scale factor (used with Linear/Yarn scaling, e.g., 2.0 for 2x context)
    pub rope_scale_factor: f32,

    /// RoPE frequency base (default: 10000.0, higher for extended context)
    pub rope_freq_base: f32,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            model_path: String::new(),
            n_threads: num_cpus::get() as u32,
            n_gpu_layers: 0,
            n_ctx: 4096,   // BEAST MODE: Large context
            n_batch: 2048, // FIXED: Increased to handle long prompts (was 256)
            use_mmap: true,
            use_mlock: false,
            token_channel_size: 256, // Token streaming buffer size
            kv_cache_type_k: KvCacheQuantization::F16, // Default: half precision
            kv_cache_type_v: KvCacheQuantization::F16,
            rope_scaling_type: RopeScalingType::None,
            rope_scale_factor: 1.0,
            rope_freq_base: 10000.0,
        }
    }
}

impl ModelConfig {
    /// Create a new model configuration with the given path
    pub fn new(model_path: impl Into<String>) -> Self {
        Self {
            model_path: model_path.into(),
            ..Default::default()
        }
    }

    /// Set the number of CPU threads
    pub fn with_threads(mut self, n_threads: u32) -> Self {
        self.n_threads = n_threads;
        self
    }

    /// Set the number of GPU layers
    pub fn with_gpu_layers(mut self, n_gpu_layers: u32) -> Self {
        self.n_gpu_layers = n_gpu_layers;
        self
    }

    /// Set the context size
    pub fn with_context_size(mut self, n_ctx: u32) -> Self {
        self.n_ctx = n_ctx;
        self
    }

    /// Set the batch size
    pub fn with_batch_size(mut self, n_batch: u32) -> Self {
        self.n_batch = n_batch;
        self
    }

    /// Enable or disable memory mapping
    pub fn with_mmap(mut self, use_mmap: bool) -> Self {
        self.use_mmap = use_mmap;
        self
    }

    /// Enable or disable memory locking
    pub fn with_mlock(mut self, use_mlock: bool) -> Self {
        self.use_mlock = use_mlock;
        self
    }

    /// Set KV cache quantization for both keys and values
    pub fn with_kv_cache_quant(mut self, quant: KvCacheQuantization) -> Self {
        self.kv_cache_type_k = quant;
        self.kv_cache_type_v = quant;
        self
    }

    /// Set KV cache quantization separately for keys and values
    pub fn with_kv_cache_type(
        mut self,
        type_k: KvCacheQuantization,
        type_v: KvCacheQuantization,
    ) -> Self {
        self.kv_cache_type_k = type_k;
        self.kv_cache_type_v = type_v;
        self
    }

    /// BEAST MODE: Auto-optimize GPU layers (offload everything to GPU)
    /// Sets GPU layers to 999 (maximum) to fully utilize GPU
    pub fn with_auto_gpu(mut self) -> Self {
        self.n_gpu_layers = 999;
        self
    }

    /// BEAST MODE: Enable all performance optimizations
    pub fn with_beast_mode(mut self) -> Self {
        self.n_gpu_layers = 999; // Max GPU offload
        self.n_ctx = 8192; // Large context window
        self.n_batch = 2048; // Maximum batch size
        self.use_mmap = true; // Memory mapping
        self
    }

    /// MEMORY SAVER MODE: Aggressive memory optimization via KV quantization
    pub fn with_memory_saver(mut self) -> Self {
        self.kv_cache_type_k = KvCacheQuantization::Q4_0;
        self.kv_cache_type_v = KvCacheQuantization::Q4_0;
        self
    }

    /// Convert to llama.cpp model parameters
    pub fn into_params(&self) -> llama_cpp_2::model::params::LlamaModelParams {
        llama_cpp_2::model::params::LlamaModelParams::default().with_n_gpu_layers(self.n_gpu_layers)
    }

    /// Convert to llama.cpp context parameters
    pub fn into_context_params(&self) -> llama_cpp_2::context::params::LlamaContextParams {
        let ctx = std::num::NonZero::new(self.n_ctx);

        llama_cpp_2::context::params::LlamaContextParams::default()
            .with_n_ctx(ctx)
            .with_n_batch(self.n_batch)
            .with_type_k(self.kv_cache_type_k.to_llama_type())
            .with_type_v(self.kv_cache_type_v.to_llama_type())
    }

    /// Estimate KV cache memory usage in bytes for given context size
    pub fn estimate_kv_cache_memory(
        &self,
        model_hidden_size: usize,
        model_num_layers: usize,
    ) -> usize {
        // KV cache size = 2 * num_layers * context_size * hidden_size * bytes_per_element
        let base_bytes = 2 * model_num_layers * (self.n_ctx as usize) * model_hidden_size * 2; // 2 bytes for F16
        let k_ratio = self.kv_cache_type_k.memory_ratio();
        let v_ratio = self.kv_cache_type_v.memory_ratio();
        let avg_ratio = (k_ratio + v_ratio) / 2.0;
        (base_bytes as f32 * avg_ratio) as usize
    }
}
