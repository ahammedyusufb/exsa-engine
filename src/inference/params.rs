//! Sampling parameters for inference

use crate::utils::error::{ExsaError, Result};
use serde::{Deserialize, Serialize};

/// Sampling parameters for text generation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SamplingParams {
    /// Temperature for sampling (0.0 = deterministic, higher = more random)
    pub temperature: f32,

    /// Top-k sampling (0 = disabled)
    pub top_k: i32,

    /// Top-p (nucleus) sampling (1.0 = disabled)
    pub top_p: f32,

    /// Repetition penalty (1.0 = no penalty)
    pub repeat_penalty: f32,

    /// Maximum number of tokens to generate
    pub max_tokens: usize,

    /// Sequences that stop generation
    pub stop_sequences: Vec<String>,

    /// Random seed for deterministic generation (None = random)
    pub seed: Option<u64>,

    /// Minimum probability threshold (min_p sampling)
    pub min_p: f32,

    /// Mirostat sampling mode (0 = disabled, 1 = Mirostat, 2 = Mirostat 2.0)
    pub mirostat: i32,

    /// Mirostat target entropy (tau parameter)
    pub mirostat_tau: f32,

    /// Mirostat learning rate (eta parameter)
    pub mirostat_eta: f32,

    /// Presence penalty (-2.0 to 2.0, penalizes tokens that appeared)
    pub presence_penalty: f32,

    /// Frequency penalty (-2.0 to 2.0, penalizes based on frequency)
    pub frequency_penalty: f32,

    /// Number of tokens to consider for repeat penalty
    pub repeat_last_n: i32,

    /// Tail free sampling parameter (1.0 = disabled)
    pub tfs_z: f32,

    /// Typical sampling parameter (1.0 = disabled)
    pub typical_p: f32,

    // ==================== CONTEXT MANAGEMENT ====================
    /// Number of tokens to preserve during context sliding window (system prompt)
    /// If None, defaults to 0 (no preserved tokens)
    #[serde(default)]
    pub n_keep: Option<usize>,

    /// Session ID for KV cache isolation across requests
    /// If provided, enables session-based context reuse
    #[serde(default)]
    pub session_id: Option<String>,
}

impl Default for SamplingParams {
    fn default() -> Self {
        Self {
            temperature: 0.7,
            top_k: 40,
            top_p: 0.9,
            repeat_penalty: 1.1,
            max_tokens: 512,
            stop_sequences: vec![],
            seed: None,
            min_p: 0.0, // DISABLED - causes llama.cpp crashes
            mirostat: 0,
            mirostat_tau: 5.0,
            mirostat_eta: 0.1,
            presence_penalty: 0.0,
            frequency_penalty: 0.0,
            repeat_last_n: 64,
            tfs_z: 1.0,
            typical_p: 1.0,
            // Context management defaults
            n_keep: None,
            session_id: None,
        }
    }
}

impl SamplingParams {
    /// Validate sampling parameters
    pub fn validate(&self) -> Result<()> {
        if self.temperature < 0.0 {
            return Err(ExsaError::InvalidParameters(format!(
                "Temperature must be non-negative, got {}",
                self.temperature
            )));
        }

        if self.top_k < 0 {
            return Err(ExsaError::InvalidParameters(format!(
                "Top-k must be non-negative, got {}",
                self.top_k
            )));
        }

        if self.top_p < 0.0 || self.top_p > 1.0 {
            return Err(ExsaError::InvalidParameters(format!(
                "Top-p must be between 0.0 and 1.0, got {}",
                self.top_p
            )));
        }

        if self.repeat_penalty < 0.0 {
            return Err(ExsaError::InvalidParameters(format!(
                "Repeat penalty must be non-negative, got {}",
                self.repeat_penalty
            )));
        }

        if self.max_tokens == 0 {
            return Err(ExsaError::InvalidParameters(
                "Max tokens must be greater than 0".to_string(),
            ));
        }

        if self.min_p < 0.0 || self.min_p > 1.0 {
            return Err(ExsaError::InvalidParameters(format!(
                "Min-p must be between 0.0 and 1.0, got {}",
                self.min_p
            )));
        }

        if self.mirostat < 0 || self.mirostat > 2 {
            return Err(ExsaError::InvalidParameters(format!(
                "Mirostat must be 0, 1, or 2, got {}",
                self.mirostat
            )));
        }

        if self.mirostat_tau < 0.0 {
            return Err(ExsaError::InvalidParameters(format!(
                "Mirostat tau must be non-negative, got {}",
                self.mirostat_tau
            )));
        }

        if self.mirostat_eta < 0.0 || self.mirostat_eta > 1.0 {
            return Err(ExsaError::InvalidParameters(format!(
                "Mirostat eta must be between 0.0 and 1.0, got {}",
                self.mirostat_eta
            )));
        }

        if self.presence_penalty < -2.0 || self.presence_penalty > 2.0 {
            return Err(ExsaError::InvalidParameters(format!(
                "Presence penalty must be between -2.0 and 2.0, got {}",
                self.presence_penalty
            )));
        }

        if self.frequency_penalty < -2.0 || self.frequency_penalty > 2.0 {
            return Err(ExsaError::InvalidParameters(format!(
                "Frequency penalty must be between -2.0 and 2.0, got {}",
                self.frequency_penalty
            )));
        }

        if self.tfs_z < 0.0 {
            return Err(ExsaError::InvalidParameters(format!(
                "TFS-Z must be non-negative, got {}",
                self.tfs_z
            )));
        }

        if self.typical_p < 0.0 || self.typical_p > 1.0 {
            return Err(ExsaError::InvalidParameters(format!(
                "Typical-P must be between 0.0 and 1.0, got {}",
                self.typical_p
            )));
        }

        Ok(())
    }
}
