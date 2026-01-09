//! Context configuration for production context management
//!
//! Provides configurable sliding window, n_keep, and overflow policies.

use serde::{Deserialize, Serialize};

/// Production context configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextConfig {
    /// Maximum context size (tokens)
    pub n_ctx: usize,

    /// Number of tokens to preserve during sliding window (system prompt)
    /// These tokens are NEVER evicted from the KV cache.
    pub n_keep: usize,

    /// Threshold to trigger sliding window (fraction of n_ctx, e.g., 0.92)
    /// When context usage exceeds this ratio, sliding window activates.
    pub sliding_threshold: f32,

    /// Fraction of context to keep after sliding (e.g., 0.70)
    /// This is the target usage after sliding window operation.
    pub keep_ratio: f32,

    /// Policy when context is exhausted
    pub overflow_policy: OverflowPolicy,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            n_ctx: 4096,
            n_keep: 0,               // No preserved tokens by default
            sliding_threshold: 0.92, // Trigger at 92% capacity
            keep_ratio: 0.70,        // Keep 70% after sliding
            overflow_policy: OverflowPolicy::SlidingWindow,
        }
    }
}

impl ContextConfig {
    /// Create config with specific context size
    pub fn with_n_ctx(mut self, n_ctx: usize) -> Self {
        self.n_ctx = n_ctx;
        self
    }

    /// Set number of tokens to keep (system prompt)
    pub fn with_n_keep(mut self, n_keep: usize) -> Self {
        self.n_keep = n_keep;
        self
    }

    /// Set sliding window threshold
    pub fn with_sliding_threshold(mut self, threshold: f32) -> Self {
        self.sliding_threshold = threshold.clamp(0.5, 0.99);
        self
    }

    /// Set keep ratio after sliding
    pub fn with_keep_ratio(mut self, ratio: f32) -> Self {
        self.keep_ratio = ratio.clamp(0.3, 0.9);
        self
    }

    /// Set overflow policy
    pub fn with_overflow_policy(mut self, policy: OverflowPolicy) -> Self {
        self.overflow_policy = policy;
        self
    }

    /// Calculate the threshold position (in tokens) when sliding should trigger
    pub fn sliding_threshold_tokens(&self) -> usize {
        (self.n_ctx as f32 * self.sliding_threshold) as usize
    }

    /// Calculate how many tokens to shift during sliding window
    /// This ensures we keep approximately keep_ratio of the context
    pub fn calculate_shift_amount(&self, current_pos: usize) -> usize {
        if current_pos <= self.sliding_threshold_tokens() {
            return 0;
        }

        let target_pos = (self.n_ctx as f32 * self.keep_ratio) as usize;

        // Never shift more than what would leave us at n_keep
        let max_shift = current_pos.saturating_sub(self.n_keep);
        let desired_shift = current_pos.saturating_sub(target_pos);

        desired_shift.min(max_shift)
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<(), String> {
        if self.n_keep >= self.n_ctx {
            return Err(format!(
                "n_keep ({}) must be less than n_ctx ({})",
                self.n_keep, self.n_ctx
            ));
        }

        if self.sliding_threshold <= self.keep_ratio {
            return Err(format!(
                "sliding_threshold ({}) must be greater than keep_ratio ({})",
                self.sliding_threshold, self.keep_ratio
            ));
        }

        Ok(())
    }
}

/// Policy when context is exhausted
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OverflowPolicy {
    /// Slide window automatically (default) - discards oldest tokens after n_keep
    /// Graceful: maintains context coherence, may lose early conversation
    #[default]
    SlidingWindow,

    /// Truncate oldest messages at prompt level (no KV cache manipulation)
    /// Graceful: simpler, works well with short contexts
    Truncate,

    /// Return error to client (forces client to manage context)
    /// Not graceful: requires client-side handling
    Error,

    /// Summarize old context before discarding (requires API call)
    /// Most graceful: preserves semantic content
    Summarize,
}

impl OverflowPolicy {
    /// Whether this policy is considered graceful (doesn't interrupt the user)
    pub fn is_graceful(self) -> bool {
        matches!(self, Self::SlidingWindow | Self::Truncate | Self::Summarize)
    }

    /// Human-readable description of what this policy does
    pub fn description(self) -> &'static str {
        match self {
            Self::SlidingWindow => "Automatically slides context window, preserving n_keep tokens",
            Self::Truncate => "Truncates oldest messages without KV cache manipulation",
            Self::Error => "Returns error when context limit approached",
            Self::Summarize => "Summarizes old context before discarding (requires LLM call)",
        }
    }

    /// Parse from string (case-insensitive)
    pub fn from_str_lossy(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "sliding" | "sliding_window" | "slide" => Self::SlidingWindow,
            "truncate" | "trunc" | "cut" => Self::Truncate,
            "error" | "fail" | "strict" => Self::Error,
            "summarize" | "summary" | "compress" => Self::Summarize,
            _ => Self::SlidingWindow, // Default
        }
    }
}

/// Session slot state for tracking KV cache availability
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SlotState {
    /// Currently processing a request
    Active,
    /// Idle but holding valid KV cache (warm)
    Warm,
    /// Can be evicted immediately
    #[default]
    Evictable,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ContextConfig::default();
        assert_eq!(config.n_ctx, 4096);
        assert_eq!(config.n_keep, 0);
        assert!((config.sliding_threshold - 0.92).abs() < 0.001);
    }

    #[test]
    fn test_threshold_calculation() {
        let config = ContextConfig::default().with_n_ctx(8192);
        let threshold = config.sliding_threshold_tokens();
        assert_eq!(threshold, (8192.0 * 0.92) as usize);
    }

    #[test]
    fn test_shift_amount_with_n_keep() {
        let config = ContextConfig::default()
            .with_n_ctx(4096)
            .with_n_keep(100)
            .with_keep_ratio(0.70);

        // At 3800 tokens (above threshold)
        let shift = config.calculate_shift_amount(3800);
        // Should shift to ~2867 (70% of 4096) but not below n_keep
        assert!(shift > 0);
        assert!(shift <= 3800 - 100); // Never shift below n_keep
    }

    #[test]
    fn test_validation() {
        // Invalid: n_keep >= n_ctx
        let config = ContextConfig::default().with_n_ctx(4096).with_n_keep(5000);
        assert!(config.validate().is_err());

        // Valid config
        let config = ContextConfig::default().with_n_ctx(4096).with_n_keep(100);
        assert!(config.validate().is_ok());
    }
}
