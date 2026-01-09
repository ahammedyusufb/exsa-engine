//! Speculative Decoding Engine
//!
//! Implements speculative decoding for 2-3x faster token generation.
//!
//! ## How it works:
//! 1. Draft model (small, fast) predicts next N tokens
//! 2. Target model (main) verifies all N in ONE batch
//! 3. Accept verified tokens, reject rest
//! 4. Repeat from last accepted token
//!
//! This achieves massive speedups because the draft model is much faster,
//! and the target model can verify multiple tokens in parallel.

use crate::inference::params::SamplingParams;
use crate::inference::queue::TokenResponse;
use crate::model::ModelConfig;
use crate::utils::error::{ExsaError, Result};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, info};

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel, Special};
use llama_cpp_2::sampling::LlamaSampler;
use uuid::Uuid; // Add Uuid import

/// Speculative decoding configuration
#[derive(Debug, Clone)]
pub struct SpeculativeConfig {
    /// How many tokens the draft model predicts ahead
    pub speculation_depth: usize,

    /// Draft model path (small, fast model like TinyLlama-1B)
    pub draft_model_path: String,

    /// Whether speculative decoding is enabled
    pub enabled: bool,
}

impl Default for SpeculativeConfig {
    fn default() -> Self {
        Self {
            speculation_depth: 5, // Predict 5 tokens ahead
            draft_model_path: String::new(),
            enabled: false,
        }
    }
}

/// Speculative Decoding Engine
///
/// Uses two models:
/// - Draft (small, fast): Predicts tokens quickly but less accurately
/// - Target (main): Verifies draft predictions in batch
#[allow(dead_code)] // Some fields reserved for advanced features
pub struct SpeculativeEngine {
    /// Small, fast draft model (e.g., Llama-1B)
    draft_model: Arc<LlamaModel>,

    /// Main target model (e.g., Llama-7B)
    target_model: Arc<LlamaModel>,

    /// Backend reference
    backend: Arc<LlamaBackend>,

    /// Configuration
    config: SpeculativeConfig,

    /// Target model config for context params
    target_config: ModelConfig,
}

impl SpeculativeEngine {
    /// Create a new speculative decoding engine
    pub async fn new(
        draft_model_path: String,
        target_model: Arc<LlamaModel>,
        backend: Arc<LlamaBackend>,
        target_config: ModelConfig,
        config: SpeculativeConfig,
    ) -> Result<Self> {
        info!("🚀 Initializing SPECULATIVE DECODING (BEAST MODE)");
        info!("  Draft model: {}", draft_model_path);
        info!("  Speculation depth: {}", config.speculation_depth);

        // Load draft model (small, fast)
        let draft_params = LlamaModelParams::default().with_n_gpu_layers(999); // Put draft on GPU too

        let draft_model =
            LlamaModel::load_from_file(&backend, draft_model_path.clone(), &draft_params)
                .map_err(|e| ExsaError::ModelError(format!("Failed to load draft model: {}", e)))?;

        info!("✅ Draft model loaded successfully");
        info!("🎯 SPECULATIVE DECODING ACTIVE - Expecting 2-3x speedup!");

        Ok(Self {
            draft_model: Arc::new(draft_model),
            target_model,
            backend,
            config,
            target_config,
        })
    }

    /// Generate tokens using speculative decoding - THE BEAST MODE!
    ///
    /// This is where the magic happens:
    /// 1. Draft model predicts N tokens (FAST)
    /// 2. Target verifies all N in ONE batch (EFFICIENT)  
    /// 3. Accept verified, reject rest
    /// 4. Repeat → 2-3x speedup!
    pub async fn generate_speculative(
        &self,
        prompt: &str,
        params: &SamplingParams,
        token_tx: mpsc::Sender<TokenResponse>,
        request_id: Uuid,
    ) -> Result<()> {
        info!("🚀 SPECULATIVE DECODING ACTIVE for request {}", request_id);

        let draft_model: Arc<LlamaModel> = Arc::clone(&self.draft_model);
        let target_model: Arc<LlamaModel> = Arc::clone(&self.target_model);
        let backend = Arc::clone(&self.backend);
        let max_tokens = params.max_tokens;
        let prompt = prompt.to_string();
        let speculation_depth = self.config.speculation_depth;

        tokio::task::spawn_blocking(move || {
            // Create contexts for both models
            let ctx_params = LlamaContextParams::default()
                .with_n_ctx(Some(std::num::NonZero::new(4096).unwrap()))
                .with_n_batch(1024);

            let mut draft_ctx = draft_model
                .new_context(&backend, ctx_params.clone())
                .map_err(|e| ExsaError::InferenceError(format!("Draft context failed: {}", e)))?;

            let mut target_ctx = target_model
                .new_context(&backend, ctx_params)
                .map_err(|e| ExsaError::InferenceError(format!("Target context failed: {}", e)))?;

            // Tokenize prompt (both models share same tokenizer usually)
            let prompt_tokens = target_model
                .str_to_token(&prompt, AddBos::Always)
                .map_err(|e| ExsaError::InferenceError(format!("Tokenization failed: {}", e)))?;

            debug!("Prompt tokenized: {} tokens", prompt_tokens.len());

            // Process prompt in both models
            let mut batch = LlamaBatch::new(1024, 1);

            // Decode prompt in both models
            for (i, token) in prompt_tokens.iter().enumerate() {
                batch
                    .add(*token, i as i32, &[0], false)
                    .map_err(|e| ExsaError::InferenceError(e.to_string()))?;
            }

            draft_ctx
                .decode(&mut batch)
                .map_err(|e| ExsaError::InferenceError(format!("Draft decode failed: {}", e)))?;
            target_ctx
                .decode(&mut batch)
                .map_err(|e| ExsaError::InferenceError(format!("Target decode failed: {}", e)))?;

            debug!("✅ Prompt processed in both models");

            // Create samplers
            let mut draft_sampler = LlamaSampler::chain_simple(vec![LlamaSampler::dist(12345)]);
            let mut target_sampler = LlamaSampler::chain_simple(vec![LlamaSampler::dist(12345)]);

            let mut generated_count = 0;
            let mut current_pos = prompt_tokens.len();

            // SPECULATIVE DECODING MAIN LOOP 🔥
            while generated_count < max_tokens {
                // STEP 1: DRAFT PREDICTS N TOKENS (FAST!)
                let mut draft_predictions = Vec::new();
                let mut draft_batch = LlamaBatch::new(speculation_depth, 1);

                for i in 0..speculation_depth {
                    let draft_token = draft_sampler.sample(&draft_ctx, -1);

                    // Check for EOS in draft
                    if draft_model.is_eog_token(draft_token) {
                        break;
                    }

                    draft_predictions.push(draft_token);

                    // Continue draft prediction
                    draft_batch.clear();
                    draft_batch
                        .add(draft_token, (current_pos + i) as i32, &[0], true)
                        .map_err(|e| ExsaError::InferenceError(e.to_string()))?;

                    draft_ctx.decode(&mut draft_batch).map_err(|e| {
                        ExsaError::InferenceError(format!("Draft decode failed: {}", e))
                    })?;
                }

                if draft_predictions.is_empty() {
                    break;
                }

                debug!("Draft predicted {} tokens", draft_predictions.len());

                // STEP 2: TARGET VERIFIES ALL IN ONE BATCH (EFFICIENT!)
                let mut verify_batch = LlamaBatch::new(draft_predictions.len(), 1);
                for (i, &token) in draft_predictions.iter().enumerate() {
                    verify_batch
                        .add(
                            token,
                            (current_pos + i) as i32,
                            &[0],
                            i == draft_predictions.len() - 1,
                        )
                        .map_err(|e| ExsaError::InferenceError(e.to_string()))?;
                }

                target_ctx.decode(&mut verify_batch).map_err(|e| {
                    ExsaError::InferenceError(format!("Target verify failed: {}", e))
                })?;

                // STEP 3: ACCEPT/REJECT (Greedy matching for now)
                let mut accepted = 0;

                for (i, &draft_token) in draft_predictions.iter().enumerate() {
                    // Sample from target at this position
                    let target_token = target_sampler.sample(&target_ctx, (current_pos + i) as i32);

                    // Check if target agrees with draft
                    if target_token == draft_token {
                        accepted += 1;

                        // Send accepted token
                        let token_str = target_model
                            .token_to_str(target_token, Special::Tokenize)
                            .unwrap_or_default();

                        let _ = token_tx.blocking_send(TokenResponse {
                            request_id,
                            token: token_str,
                            done: false,
                        });

                        generated_count += 1;

                        // Check for EOS
                        if target_model.is_eog_token(target_token) {
                            let _ = token_tx.blocking_send(TokenResponse {
                                request_id,
                                token: String::new(),
                                done: true,
                            });
                            return Ok(());
                        }
                    } else {
                        // REJECTION! Send the target's choice instead
                        let token_str = target_model
                            .token_to_str(target_token, Special::Tokenize)
                            .unwrap_or_default();

                        let _ = token_tx.blocking_send(TokenResponse {
                            request_id,
                            token: token_str,
                            done: false,
                        });

                        generated_count += 1;
                        accepted += 1; // We accepted the correction

                        // Resync draft model from this position
                        let mut resync_batch = LlamaBatch::new(1, 1);
                        resync_batch
                            .add(target_token, (current_pos + i) as i32, &[0], true)
                            .map_err(|e| ExsaError::InferenceError(e.to_string()))?;
                        draft_ctx
                            .decode(&mut resync_batch)
                            .map_err(|e| ExsaError::InferenceError(e.to_string()))?;

                        break; // Stop at first mismatch
                    }

                    if generated_count >= max_tokens {
                        break;
                    }
                }

                current_pos += accepted;

                let acceptance_rate = (accepted as f32 / draft_predictions.len() as f32) * 100.0;
                debug!(
                    "✅ Accepted {}/{} tokens ({:.1}% acceptance)",
                    accepted,
                    draft_predictions.len(),
                    acceptance_rate
                );

                if generated_count >= max_tokens {
                    break;
                }
            }

            // Send final done signal
            let _ = token_tx.blocking_send(TokenResponse {
                request_id,
                token: String::new(),
                done: true,
            });

            info!(
                "✅ Speculative generation complete: {} tokens generated",
                generated_count
            );

            Ok::<(), ExsaError>(())
        })
        .await
        .map_err(|e| ExsaError::InferenceError(format!("Task join error: {}", e)))??;

        Ok(())
    }

    /// Standard generation (fallback)
    /// This is kept as a fallback in case speculative decoding fails
    #[allow(dead_code)] // Fallback method, may be used in error recovery
    async fn generate_standard(
        &self,
        prompt: &str,
        params: &SamplingParams,
        token_tx: mpsc::Sender<TokenResponse>,
    ) -> Result<()> {
        // Use target model for standard generation
        let model: Arc<LlamaModel> = Arc::clone(&self.target_model);
        let backend = Arc::clone(&self.backend);
        let max_tokens = params.max_tokens;
        let prompt = prompt.to_string();

        tokio::task::spawn_blocking(move || {
            // Create context
            let ctx_params = LlamaContextParams::default()
                .with_n_ctx(Some(std::num::NonZero::new(2048).unwrap())) // Fix: wrap in Some()
                .with_n_batch(512);

            let mut ctx = model.new_context(&backend, ctx_params).map_err(|e| {
                ExsaError::InferenceError(format!("Context creation failed: {}", e))
            })?;

            // Tokenize prompt
            let tokens = model
                .str_to_token(&prompt, AddBos::Always)
                .map_err(|e| ExsaError::InferenceError(format!("Tokenization failed: {}", e)))?;

            debug!("Tokenized prompt: {} tokens", tokens.len());

            // Create batch and decode prompt
            let mut batch = LlamaBatch::new(512, 1);
            for (i, token) in tokens.iter().enumerate() {
                batch
                    .add(*token, i as i32, &[0], false)
                    .map_err(|e| ExsaError::InferenceError(e.to_string()))?;
            }

            ctx.decode(&mut batch)
                .map_err(|e| ExsaError::InferenceError(format!("Decode failed: {}", e)))?;

            // Create sampler - use chain with greedy sampler
            let mut sampler = LlamaSampler::chain_simple(vec![
                LlamaSampler::dist(1234), // Seed doesn't matter for greedy
            ]);

            let mut generated_count = 0;
            let mut current_pos = tokens.len(); // Track position for token generation

            // Generate tokens
            for _ in 0..max_tokens {
                let new_token = sampler.sample(&ctx, -1); // Use -1 for last logits

                // Check for EOS
                if model.is_eog_token(new_token) {
                    let _ = token_tx.blocking_send(TokenResponse {
                        request_id: Uuid::new_v4(), // Generate UUID
                        token: String::new(),
                        done: true,
                    });
                    break;
                }

                // Convert token to string
                let token_str = model
                    .token_to_str(new_token, Special::Tokenize)
                    .unwrap_or_default();

                // Send token
                let _ = token_tx.blocking_send(TokenResponse {
                    request_id: Uuid::new_v4(), // Generate UUID
                    token: token_str,
                    done: false,
                });

                generated_count += 1;

                // Add to batch for next iteration with correct position
                batch.clear();
                batch
                    .add(new_token, current_pos as i32, &[0], true)
                    .map_err(|e| ExsaError::InferenceError(e.to_string()))?;

                current_pos += 1; // Increment position for next token

                ctx.decode(&mut batch)
                    .map_err(|e| ExsaError::InferenceError(format!("Decode failed: {}", e)))?;
            }

            debug!("Generated {} tokens", generated_count);

            Ok::<(), ExsaError>(())
        })
        .await
        .map_err(|e| ExsaError::InferenceError(format!("Task join error: {}", e)))??;

        Ok(())
    }
}

/// Helper struct for draft predictions
#[derive(Debug, Clone)]
pub struct DraftPrediction {
    pub tokens: Vec<i32>,
    pub logits: Vec<f32>,
}

/// Helper struct for verification results
#[derive(Debug)]
pub struct VerificationResult {
    pub accepted_tokens: Vec<i32>,
    pub num_accepted: usize,
}
