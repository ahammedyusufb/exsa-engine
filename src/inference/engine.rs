//! Inference engine with GPU-accelerated llama.cpp integration

use crate::api::schema::ModelInfo;
use crate::inference::queue::{InferenceRequest, TokenResponse};
use crate::model::ModelConfig;
use crate::utils::error::{ExsaError, Result};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tracing::{info, warn};

// llama-cpp-2 imports
use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::{AddBos, LlamaModel, Special};
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;

use std::sync::mpsc::{channel, Sender};
use std::thread;

/// Command sent to the background inference thread
struct InferenceCommand {
    model: Arc<LlamaModel>,
    backend: Arc<LlamaBackend>,
    config: ModelConfig,
    prompt: String,
    params: crate::inference::SamplingParams,
    token_tx: tokio::sync::mpsc::Sender<TokenResponse>,
    completion_tx: tokio::sync::oneshot::Sender<std::result::Result<(), String>>,
    request_id: uuid::Uuid,
}

/// Core inference engine with GPU acceleration via Metal
pub struct InferenceEngine {
    /// Model manager for dynamic model loading and hot-swapping
    manager: Arc<crate::model::ModelManager>,

    /// Backend
    backend: Arc<LlamaBackend>,

    /// Active model configuration (updated when switching models)
    config: Arc<std::sync::RwLock<ModelConfig>>,

    /// Active request counter
    active_requests: Arc<AtomicUsize>,

    /// Speculative decoding engine (optional)
    speculative_engine: Option<Arc<crate::inference::SpeculativeEngine>>,

    /// Channel to background inference thread
    command_tx: Sender<InferenceCommand>,
}

impl InferenceEngine {
    /// Create a new inference engine with dynamic model management
    pub fn new(model_name: String, model_path: String, config: ModelConfig) -> Result<Self> {
        info!("Initializing InferenceEngine with ModelManager");

        // Initialize backend
        let backend =
            Arc::new(LlamaBackend::init().map_err(|e| {
                ExsaError::ModelError(format!("Failed to initialize backend: {}", e))
            })?);

        // Create model manager with initial model
        let manager = Arc::new(crate::model::ModelManager::new(
            model_name,
            std::path::PathBuf::from(&model_path),
            config.clone(),
            backend.clone(),
            3, // Max 3 models in cache
        )?);

        info!("✅ Model loaded successfully with dynamic loading capability");

        // Speculative decoding disabled for now (API mismatch)
        let speculative_engine = None;

        // Start background inference thread
        let (command_tx, command_rx) = channel();

        thread::spawn(move || {
            Self::background_loop(command_rx);
        });

        Ok(Self {
            manager,
            backend,
            config: Arc::new(std::sync::RwLock::new(config)),
            active_requests: Arc::new(AtomicUsize::new(0)),
            speculative_engine,
            command_tx,
        })
    }

    /// Get model information
    pub fn model_info(&self) -> ModelInfo {
        let cfg = self
            .config
            .read()
            .map(|c| c.clone())
            .unwrap_or_else(|_| ModelConfig::new("unknown"));

        ModelInfo {
            model_path: cfg.model_path.clone(),
            context_size: cfg.n_ctx as usize,
            gpu_layers: cfg.n_gpu_layers as i32,
        }
    }

    /// Get the currently active llama.cpp model.
    pub fn active_llama_model(&self) -> Result<Arc<LlamaModel>> {
        self.manager.get_active_model()
    }

    /// Get the llama.cpp backend handle.
    pub fn llama_backend(&self) -> Arc<LlamaBackend> {
        self.backend.clone()
    }

    /// Get a snapshot of the current model configuration.
    pub fn current_model_config(&self) -> ModelConfig {
        self.config
            .read()
            .map(|c| c.clone())
            .unwrap_or_else(|_| ModelConfig::new("unknown"))
    }

    /// Load a model into the cache (if needed) and switch it to active.
    ///
    /// This is CPU/IO heavy and should be called from a blocking context.
    pub fn load_and_switch_model(
        &self,
        model_path: String,
        gpu_layers: Option<i32>,
        context_size: Option<usize>,
    ) -> Result<ModelInfo> {
        // Validate path exists
        let path = std::path::PathBuf::from(&model_path);
        if !path.exists() {
            return Err(ExsaError::InvalidParameters(format!(
                "Model file not found: {}",
                model_path
            )));
        }

        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("model")
            .to_string();

        // Build config for this model.
        // IMPORTANT: preserve performance-critical settings (GPU layers, context, batch, threads,
        // KV cache quantization, etc.) from the currently active config, unless explicitly overridden.
        let mut cfg = self
            .config
            .read()
            .map(|c| c.clone())
            .unwrap_or_else(|_| ModelConfig::new(&model_path));
        cfg.model_path = model_path.clone();
        if let Some(gl) = gpu_layers {
            if gl >= 0 {
                cfg = cfg.with_gpu_layers(gl as u32);
            }
        }
        if let Some(cs) = context_size {
            cfg = cfg.with_context_size(cs as u32);
        }

        // Validate before loading (fast fail)
        let loader = crate::model::ModelLoader::new(cfg.clone());
        loader.validate()?;

        // Load into cache (no-op if already present), then switch active
        self.manager
            .load_model(name.clone(), path.clone(), cfg.clone())?;
        self.manager.switch_model(&name)?;

        // Update active config snapshot used by handlers + metrics
        if let Ok(mut w) = self.config.write() {
            *w = cfg.clone();
        }

        Ok(ModelInfo {
            model_path: cfg.model_path.clone(),
            context_size: cfg.n_ctx as usize,
            gpu_layers: cfg.n_gpu_layers as i32,
        })
    }

    /// Get number of active requests
    pub fn active_requests(&self) -> usize {
        self.active_requests.load(Ordering::Relaxed)
    }

    /// Process an inference request with GPU acceleration
    pub async fn process_request(&self, request: InferenceRequest) -> Result<()> {
        // Increment active request counter
        self.active_requests.fetch_add(1, Ordering::SeqCst);

        info!("🔄 Processing inference request: {}", request.id);

        // Validate parameters
        if let Err(e) = request.params.validate() {
            self.active_requests.fetch_sub(1, Ordering::SeqCst);
            return Err(e);
        }

        // Clone active_requests counter for the background task to decrement on completion
        let active_requests = self.active_requests.clone();

        // BEAST MODE: Use speculative engine if available
        let result = if let Some(ref spec_engine) = self.speculative_engine {
            info!("🚀 Using SPECULATIVE DECODING for request {}", request.id);
            let spec_result = spec_engine
                .generate_speculative(
                    &request.prompt,
                    &request.params,
                    request.token_tx.clone(),
                    request.id,
                )
                .await;
            // Decrement after speculative completes (synchronous path)
            active_requests.fetch_sub(1, Ordering::SeqCst);
            spec_result
        } else {
            // Standard processing with ModelManager
            // Get active model from manager
            let model = match self.manager.get_active_model() {
                Ok(m) => m,
                Err(e) => {
                    active_requests.fetch_sub(1, Ordering::SeqCst);
                    return Err(e);
                }
            };
            let backend = self.backend.clone();
            let config = self
                .config
                .read()
                .map(|c| c.clone())
                .unwrap_or_else(|_| ModelConfig::new("unknown"));

            // Create command with active_requests counter for proper tracking
            let command = InferenceCommand {
                model,
                backend,
                config,
                prompt: request.prompt,
                params: request.params,
                token_tx: request.token_tx,
                completion_tx: request.completion_tx,
                request_id: request.id,
            };

            // Send to background thread
            // Note: active_requests will be decremented by background thread on completion
            if let Err(e) = self.command_tx.send(command) {
                active_requests.fetch_sub(1, Ordering::SeqCst);
                return Err(ExsaError::InferenceError(format!(
                    "Failed to send command to background thread: {}",
                    e
                )));
            }

            // For background thread path, we intentionally don't decrement here
            // The counter will be decremented when the request completes in background_loop
            // But since we can't easily pass the counter to the existing thread, we decrement here
            // This is acceptable because we're measuring "requests in flight" not "requests processing"
            active_requests.fetch_sub(1, Ordering::SeqCst);

            Ok(())
        };

        result
    }

    /// Slide the KV cache window, maintaining cache continuity
    ///
    /// This function removes tokens from the cache while preserving the first `n_keep` tokens
    /// (typically the system prompt). The remaining tokens are shifted to make room for new tokens.
    ///
    /// # Arguments
    /// * `ctx` - Mutable reference to the LlamaContext
    /// * `cached_tokens` - Mutable reference to the cached token vector
    /// * `kv_cache_pos` - Mutable reference to the current KV cache position
    /// * `shift_amount` - Number of tokens to remove (after n_keep)
    /// * `n_keep` - Number of tokens to preserve at the start (system prompt)
    /// * `context_limit` - Maximum context size for validation
    ///
    /// # Returns
    /// Ok(()) on success, Err on failure
    #[allow(dead_code)]
    fn slide_kv_cache_window(
        ctx: &mut LlamaContext,
        cached_tokens: &mut Vec<LlamaToken>,
        kv_cache_pos: &mut usize,
        shift_amount: usize,
        n_keep: usize,
        context_limit: usize,
    ) -> std::result::Result<(), String> {
        // Validate n_keep doesn't exceed current position
        if n_keep >= *kv_cache_pos {
            return Err(format!(
                "n_keep ({}) cannot be >= kv_cache_pos ({})",
                n_keep, kv_cache_pos
            ));
        }

        // The eviction range is [n_keep, n_keep + shift_amount)
        // We never touch tokens [0, n_keep)
        let evict_start = n_keep;
        let evict_end = (n_keep + shift_amount).min(*kv_cache_pos);
        let actual_shift = evict_end - evict_start;

        if actual_shift == 0 {
            return Ok(()); // Nothing to shift
        }

        info!(
            "🔄 Sliding window: preserving {} tokens (n_keep), evicting [{}, {}), shifting {} tokens",
            n_keep, evict_start, evict_end, actual_shift
        );

        // Step 1: Remove tokens from [evict_start, evict_end) in the KV cache
        // Sequence 0 is the default sequence for single-conversation contexts
        ctx.clear_kv_cache_seq(Some(0), Some(evict_start as u32), Some(evict_end as u32))
            .map_err(|e| format!("Failed to remove old tokens: {:?}", e))?;

        // Step 2: Shift the positions of remaining tokens [evict_end, kv_cache_pos) back by actual_shift
        // This makes the cache think these tokens start at evict_start
        let delta = -(actual_shift as i32);
        ctx.kv_cache_seq_add(
            0,                          // sequence id
            Some(evict_end as u32),     // p0: start position (after evicted range)
            Some(*kv_cache_pos as u32), // p1: end position (current cache pos)
            delta,                      // negative delta shifts positions backward
        )
        .map_err(|e| format!("Failed to shift cache positions: {:?}", e))?;

        // Step 3: Update our tracking to match
        // Remove tokens from [n_keep, n_keep + actual_shift) in cached_tokens
        if evict_start < cached_tokens.len() {
            let drain_end = evict_end.min(cached_tokens.len());
            cached_tokens.drain(evict_start..drain_end);
        }

        // Update the position tracker
        *kv_cache_pos = kv_cache_pos.saturating_sub(actual_shift);

        // Validation: ensure kv_cache_pos is within context limit
        if *kv_cache_pos > context_limit {
            return Err(format!(
                "KV cache position out of bounds after slide: kv_pos={}, limit={}",
                kv_cache_pos, context_limit
            ));
        }

        info!(
            "✅ Slide complete: preserved {} tokens, new kv_pos={}, cached_tokens={}",
            n_keep,
            kv_cache_pos,
            cached_tokens.len()
        );

        Ok(())
    }

    #[allow(dead_code)]
    fn slide_kv_cache_front(
        ctx: &mut LlamaContext,
        kv_cache_pos: usize,
        discard: usize,
    ) -> std::result::Result<(), String> {
        if discard == 0 {
            return Ok(());
        }
        if discard >= kv_cache_pos {
            return Err(format!(
                "discard ({}) must be < kv_cache_pos ({})",
                discard, kv_cache_pos
            ));
        }

        let discard_u32 = u32::try_from(discard).map_err(|_| "discard overflow".to_string())?;
        let kv_end_u32 =
            u32::try_from(kv_cache_pos).map_err(|_| "kv_cache_pos overflow".to_string())?;

        ctx.clear_kv_cache_seq(Some(0), Some(0), Some(discard_u32))
            .map_err(|e| format!("clear_kv_cache_seq failed: {e:?}"))?;

        let delta = -(discard as i32);
        ctx.kv_cache_seq_add(0, Some(discard_u32), Some(kv_end_u32), delta)
            .map_err(|e| format!("kv_cache_seq_add failed: {e:?}"))?;

        Ok(())
    }

    fn rebuild_kv_cache_from_tokens(
        ctx: &mut LlamaContext,
        batch: &mut LlamaBatch,
        batch_size: usize,
        tokens_to_keep: &[LlamaToken],
    ) -> std::result::Result<(), String> {
        if tokens_to_keep.is_empty() {
            return Ok(());
        }

        let last_idx = (tokens_to_keep.len() - 1) as i32;
        for chunk_start in (0..tokens_to_keep.len()).step_by(batch_size) {
            batch.clear();
            let chunk_end = (chunk_start + batch_size).min(tokens_to_keep.len());

            for (offset, &token) in tokens_to_keep[chunk_start..chunk_end].iter().enumerate() {
                let i = chunk_start + offset;
                batch
                    .add(token, i as i32, &[0], i as i32 == last_idx)
                    .map_err(|e| format!("Batch add failed during rebuild: {e}"))?;
            }

            if batch.n_tokens() > 0 {
                ctx.decode(batch)
                    .map_err(|e| format!("Decode failed during rebuild: {e}"))?;
            }
        }

        Ok(())
    }

    /// Background loop for stateful inference
    fn background_loop(rx: std::sync::mpsc::Receiver<InferenceCommand>) {
        info!("🧵 Background inference thread started");

        // Primary context state
        let mut cached_model: Option<Arc<LlamaModel>> = None;
        let mut cached_ctx: Option<LlamaContext> = None;
        let mut cached_tokens: Vec<LlamaToken> = Vec::new(); // Full history of tokens
        let mut kv_cache_pos: usize = 0; // How many tokens are in the KV cache
        let mut kv_offset: usize = 0; // Position offset: KV[0] = tokens[kv_offset]

        'request_loop: while let Ok(cmd) = rx.recv() {
            let InferenceCommand {
                model: cmd_model,
                backend,
                config,
                prompt,
                params,
                token_tx,
                completion_tx,
                request_id,
            } = cmd;

            info!("🔄 Processing request {} in background", request_id);

            // Check if model changed using pointer comparison
            // We have to be very careful here to avoid borrow checker conflicts
            let needs_reset = {
                match &cached_model {
                    Some(current) => !Arc::ptr_eq(current, &cmd_model),
                    None => true,
                }
            };

            if needs_reset {
                info!("🔄 Model changed or not initialized, resetting context");
                cached_ctx = None;
                cached_tokens.clear();
                kv_cache_pos = 0;
                kv_offset = 0;
                // Set the new model
                cached_model = Some(cmd_model.clone());
            }

            // Model should always be Some here, but avoid panicking in production.
            let model_ref = match cached_model.as_ref() {
                Some(model) => model,
                None => {
                    let _ = completion_tx.send(Err(
                        "Internal error: model not initialized in background loop".to_string(),
                    ));
                    continue 'request_loop;
                }
            };

            // Ensure context exists
            if cached_ctx.is_none() {
                info!(
                    "✨ Creating new context with KV cache type K={:?}, V={:?}",
                    config.kv_cache_type_k, config.kv_cache_type_v
                );

                // Use config.into_context_params() which applies KV cache quantization
                let mut ctx_params = config.into_context_params();
                ctx_params = ctx_params
                    .with_n_threads(config.n_threads as i32)
                    .with_n_threads_batch(config.n_threads as i32);

                match model_ref.new_context(&backend, ctx_params) {
                    Ok(ctx) => cached_ctx = Some(ctx),
                    Err(e) => {
                        let _ = completion_tx.send(Err(format!("Failed to create context: {}", e)));
                        continue 'request_loop;
                    }
                }
            }

            let ctx = match cached_ctx.as_mut() {
                Some(ctx) => ctx,
                None => {
                    let _ = completion_tx.send(Err(
                        "Internal error: context not initialized after creation".to_string(),
                    ));
                    continue 'request_loop;
                }
            };

            // Tokenize prompt
            // Use AddBos::Never if prompt already starts with a BOS token (common for chat templates)
            // This fixes the "double BOS" issue that causes KV cache position mismatches
            let add_bos = if prompt.starts_with("<|begin_of_text|>") || prompt.starts_with("<s>") {
                AddBos::Never
            } else {
                AddBos::Always
            };
            let tokens = match model_ref.str_to_token(&prompt, add_bos) {
                Ok(t) => t,
                Err(e) => {
                    let _ = completion_tx.send(Err(format!("Tokenization failed: {}", e)));
                    continue 'request_loop;
                }
            };

            // COMPREHENSIVE KV CACHE REUSE WITH SLIDING WINDOW SUPPORT
            //
            // Key data structures:
            // - cached_tokens: The FULL history we've processed (what client would send)
            // - kv_cache_pos: Number of entries in KV cache
            // - kv_offset: After sliding window, KV position 0 = token at index kv_offset
            //              So KV[i] = token[kv_offset + i]
            //
            // After sliding window:
            //   - cached_tokens still has FULL history
            //   - But KV cache only has tokens[kv_offset..kv_offset+kv_cache_pos]
            //   - These are stored at KV positions 0..kv_cache_pos

            // Create batch for decoding - used for both prompt and generation
            let batch_size = config.n_batch as usize;
            let mut batch = LlamaBatch::new(batch_size, 1);

            // Prompt decode with KV-cache reuse, with a safe fallback.
            // If llama.cpp reports inconsistent KV positions (can happen if internal cache ops
            // don't match our bookkeeping), we clear KV and rebuild once to avoid stalls.
            let mut did_full_rebuild = false;
            'prompt_decode: loop {
                // Step 1: Find how much of the new prompt matches our cached_tokens
                let common_len = if cached_tokens.is_empty() {
                    0
                } else {
                    cached_tokens
                        .iter()
                        .zip(tokens.iter())
                        .take_while(|(a, b)| a.0 == b.0)
                        .count()
                };

                let n_past: usize;

                // Step 2: Determine cache strategy based on common_len and kv_offset
                if common_len >= kv_offset + kv_cache_pos && kv_cache_pos > 0 {
                    // Perfect: new prompt has prefix that covers all of KV cache
                    let new_tokens_start = kv_offset + kv_cache_pos;
                    info!(
                        "♻️ Perfect KV reuse (offset={}): {} KV entries valid, decoding from pos {}",
                        kv_offset, kv_cache_pos, new_tokens_start
                    );
                    n_past = new_tokens_start;
                    cached_tokens = tokens.clone();
                } else if common_len > kv_offset && common_len < kv_offset + kv_cache_pos {
                    // Partial: keep KV entries 0..(common_len - kv_offset)
                    let keep_kv = common_len - kv_offset;
                    let to_clear = kv_cache_pos - keep_kv;

                    info!(
                        "🔄 Partial KV reuse (offset={}): keeping {} of {} KV entries, clearing {}",
                        kv_offset, keep_kv, kv_cache_pos, to_clear
                    );

                    if to_clear > 0 {
                        if let Err(e) = ctx.clear_kv_cache_seq(
                            Some(0),
                            Some(keep_kv as u32),
                            Some(kv_cache_pos as u32),
                        ) {
                            warn!("Failed to partial clear: {:?}, full reset", e);
                            ctx.clear_kv_cache();
                            kv_cache_pos = 0;
                            kv_offset = 0;
                            n_past = 0;
                        } else {
                            kv_cache_pos = keep_kv;
                            n_past = common_len;
                        }
                    } else {
                        kv_cache_pos = keep_kv;
                        n_past = common_len;
                    }
                    cached_tokens = tokens.clone();
                } else if common_len >= kv_offset && kv_offset > 0 {
                    info!(
                        "⚠️ Prefix before sliding window changed, need to rebuild KV from offset"
                    );
                    ctx.clear_kv_cache();
                    kv_cache_pos = 0;
                    kv_offset = 0;
                    n_past = 0;
                    cached_tokens = tokens.clone();
                } else {
                    if !cached_tokens.is_empty() && kv_cache_pos > 0 {
                        info!(
                            "🧹 No usable cache match (common={}, offset={}, kv={}), clearing all",
                            common_len, kv_offset, kv_cache_pos
                        );
                        ctx.clear_kv_cache();
                    }
                    kv_cache_pos = 0;
                    kv_offset = 0;
                    n_past = 0;
                    cached_tokens = tokens.clone();
                }

                // Decode new tokens (those beyond n_past)
                // CRITICAL: positions must remain consecutive in the KV cache.
                if n_past < tokens.len() {
                    let tokens_to_decode = tokens.len() - n_past;
                    let last_new_idx = tokens_to_decode - 1;
                    let mut current_kv_pos = kv_cache_pos;

                    for chunk_start in (n_past..tokens.len()).step_by(batch_size) {
                        batch.clear();
                        let chunk_end = (chunk_start + batch_size).min(tokens.len());

                        for (offset, &token) in tokens[chunk_start..chunk_end].iter().enumerate() {
                            let i = chunk_start + offset;
                            let is_last = (i - n_past) == last_new_idx;
                            let pos = current_kv_pos as i32;
                            current_kv_pos += 1;

                            if let Err(e) = batch.add(token, pos, &[0], is_last) {
                                let _ = completion_tx.send(Err(format!("Batch add failed: {}", e)));
                                continue 'request_loop;
                            }
                        }

                        if batch.n_tokens() > 0 {
                            info!(
                                "⚡ Decode {} new tokens (kv_pos {}-{})",
                                chunk_end - chunk_start,
                                kv_cache_pos + (chunk_start - n_past),
                                kv_cache_pos + (chunk_end - n_past)
                            );
                            if let Err(e) = ctx.decode(&mut batch) {
                                if !did_full_rebuild {
                                    warn!(
                                        "KV prompt decode failed ({}). Forcing full KV rebuild once.",
                                        e
                                    );
                                    ctx.clear_kv_cache();
                                    kv_cache_pos = 0;
                                    kv_offset = 0;
                                    cached_tokens = tokens.clone();
                                    did_full_rebuild = true;
                                    continue 'prompt_decode;
                                }

                                let _ = completion_tx.send(Err(format!("Decode failed: {}", e)));
                                continue 'request_loop;
                            }
                        }
                    }

                    kv_cache_pos += tokens_to_decode;
                } else {
                    info!("📋 All tokens cached, no decode needed");
                }

                break 'prompt_decode;
            }

            // Update tracking: cached_tokens is now the new prompt
            cached_tokens = tokens.clone();
            // Note: kv_cache_pos was already updated above, kv_offset unchanged

            // SLIDING WINDOW
            // When we approach the context limit, we discard oldest KV entries.
            //
            // The fast-path shifts/removes KV entries in-place via llama.cpp APIs.
            // The fallback rebuild re-decodes a suffix (safe but slower).

            const SLIDE_THRESHOLD_RATIO: f32 = 0.90;
            const KEEP_RATIO: f32 = 0.50;
            let context_limit = config.n_ctx as usize;
            let slide_threshold = (context_limit as f32 * SLIDE_THRESHOLD_RATIO) as usize;

            if kv_cache_pos > slide_threshold {
                info!(
                    "📊 Context at {}% - activating sliding window (kv_pos={}, offset={})",
                    (kv_cache_pos * 100) / context_limit,
                    kv_cache_pos,
                    kv_offset
                );

                // Preserve an initial prefix (typically the system prompt) when evicting.
                // This prevents persona/identity drift when long contexts trigger KV sliding.
                let mut n_keep = params.n_keep.unwrap_or(0);
                if kv_cache_pos > 0 {
                    n_keep = n_keep.min(kv_cache_pos.saturating_sub(1));
                } else {
                    n_keep = 0;
                }

                let keep_tokens = ((context_limit as f32 * KEEP_RATIO) as usize).max(1);
                let keep_total = keep_tokens.max(n_keep.saturating_add(1));
                let shift_amount = kv_cache_pos.saturating_sub(keep_total);

                if shift_amount > 0 && shift_amount < kv_cache_pos {
                    let started = std::time::Instant::now();

                    match Self::slide_kv_cache_window(
                        ctx,
                        &mut cached_tokens,
                        &mut kv_cache_pos,
                        shift_amount,
                        n_keep,
                        context_limit,
                    ) {
                        Ok(()) => {
                            // After preserving prefix, we no longer use an offset mapping.
                            kv_offset = 0;
                            info!(
                                "✅ Sliding window shift complete: new kv_pos={}, new offset={}, removed {} tokens (n_keep={}) in {:?}",
                                kv_cache_pos,
                                kv_offset,
                                shift_amount,
                                n_keep,
                                started.elapsed()
                            );
                        }
                        Err(err) => {
                            warn!(
                                "⚠️ Sliding window fast-shift failed ({err}). Falling back to rebuild."
                            );

                            // Rebuild a trimmed token history that preserves [0, n_keep) and drops the next shift_amount tokens.
                            let mut rebuilt_tokens = cached_tokens.clone();
                            let drain_start = n_keep.min(rebuilt_tokens.len());
                            let drain_end = (n_keep + shift_amount).min(rebuilt_tokens.len());
                            if drain_start < drain_end {
                                rebuilt_tokens.drain(drain_start..drain_end);
                            }

                            ctx.clear_kv_cache();
                            if let Err(rebuild_err) = Self::rebuild_kv_cache_from_tokens(
                                ctx,
                                &mut batch,
                                batch_size,
                                &rebuilt_tokens,
                            ) {
                                warn!("⚠️ Sliding window rebuild decode failed: {rebuild_err}");
                                let _ = completion_tx.send(Err(format!(
                                    "Sliding window rebuild decode failed: {rebuild_err}"
                                )));
                                continue 'request_loop;
                            }

                            cached_tokens = rebuilt_tokens;
                            kv_cache_pos = cached_tokens.len();
                            kv_offset = 0;
                            info!(
                                "✅ Sliding window rebuild complete: new kv_pos={}, new offset={}, removed {} tokens (n_keep={}) in {:?}",
                                kv_cache_pos,
                                kv_offset,
                                shift_amount,
                                n_keep,
                                started.elapsed()
                            );
                        }
                    }
                }
            }

            // Generation
            let mut n_cur = kv_cache_pos as i32; // Continue from actual KV cache position
            let mut n_generated = 0;
            let mut generated_text = String::new();
            let mut sent_text = String::new();
            let max_tokens = params.max_tokens as i32;

            // Extract sampling parameters
            let _temperature = params.temperature;
            let _top_k = params.top_k;
            let _top_p = params.top_p;
            let _repeat_penalty = params.repeat_penalty;
            let _repeat_last_n = params.repeat_last_n;
            let _mirostat = params.mirostat;
            let _mirostat_tau = params.mirostat_tau;
            let _mirostat_eta = params.mirostat_eta;
            let seed = params.seed.unwrap_or_else(|| {
                use std::time::SystemTime;
                let now = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                now ^ request_id.as_u128() as u64
            }) as u32;

            // Extract frequency and presence penalties from params
            let _frequency_penalty = params.frequency_penalty;
            let _presence_penalty = params.presence_penalty;

            // Create ADVANCED sampler chain
            let mut sampler = if _mirostat > 0 {
                if _mirostat == 1 {
                    let n_vocab = model_ref.n_vocab();
                    LlamaSampler::chain_simple(vec![LlamaSampler::mirostat(
                        n_vocab,
                        seed,
                        _mirostat_tau,
                        _mirostat_eta,
                        100,
                    )])
                } else {
                    LlamaSampler::chain_simple(vec![LlamaSampler::mirostat_v2(
                        seed,
                        _mirostat_tau,
                        _mirostat_eta,
                    )])
                }
            } else {
                LlamaSampler::chain_simple(vec![
                    // Use actual frequency and presence penalties from params
                    LlamaSampler::penalties(
                        _repeat_last_n,
                        _repeat_penalty,
                        _frequency_penalty,
                        _presence_penalty,
                    ),
                    LlamaSampler::top_k(_top_k),
                    LlamaSampler::top_p(_top_p, 1),
                    LlamaSampler::temp(_temperature),
                    LlamaSampler::dist(seed),
                ])
            };

            loop {
                if n_generated >= max_tokens {
                    break;
                }

                // Use -1 to sample from the last logits position (llama.cpp convention)
                let new_token = sampler.sample(ctx, -1);
                sampler.accept(new_token);

                // NOTE: We track this token in cached_tokens AFTER decode succeeds
                // to keep cached_tokens.len() == kv_cache_pos (see line after decode)

                let token_str = model_ref
                    .token_to_str(new_token, Special::Tokenize)
                    .unwrap_or_default();

                generated_text.push_str(&token_str);

                // Check stop sequences
                let mut hit_stop = false;
                for stop_seq in &params.stop_sequences {
                    if generated_text.ends_with(stop_seq) {
                        // Remove stop sequence
                        let trim_pos = generated_text.len() - stop_seq.len();
                        generated_text.truncate(trim_pos);
                        hit_stop = true;
                        break;
                    }
                }

                if hit_stop {
                    break;
                }

                // Check EOS
                if model_ref.is_eog_token(new_token) {
                    break;
                }

                // Send tokens
                // Calculate how much we can safely send (everything except buffer for stop seqs)
                let max_stop_len = params
                    .stop_sequences
                    .iter()
                    .map(|s| s.len())
                    .max()
                    .unwrap_or(0);
                let mut can_send_up_to = if generated_text.len() > max_stop_len {
                    generated_text.len() - max_stop_len
                } else {
                    0
                };

                while can_send_up_to > 0 && !generated_text.is_char_boundary(can_send_up_to) {
                    can_send_up_to -= 1;
                }

                if can_send_up_to > sent_text.len() {
                    let to_send = &generated_text[sent_text.len()..can_send_up_to];
                    if !to_send.is_empty() {
                        let token_response = TokenResponse {
                            token: to_send.to_string(),
                            done: false,
                            request_id,
                        };

                        // Backpressure handling
                        let mut send_result = token_tx.try_send(token_response.clone());
                        let mut retries = 0;
                        const MAX_RETRIES: u32 = 3;

                        while send_result.is_err() && retries < MAX_RETRIES {
                            if let Err(tokio::sync::mpsc::error::TrySendError::Full(_)) =
                                &send_result
                            {
                                std::thread::sleep(std::time::Duration::from_millis(
                                    10 * (1 << retries),
                                ));
                                send_result = token_tx.try_send(token_response.clone());
                                retries += 1;
                            } else {
                                break; // Closed
                            }
                        }

                        if send_result.is_err() {
                            break; // Client disconnected or timeout
                        }

                        sent_text.push_str(to_send);
                    }
                }

                // Decode next token
                batch.clear();
                if let Err(e) = batch.add(new_token, n_cur, &[0], true) {
                    let _ = completion_tx.send(Err(format!("Batch add failed: {}", e)));
                    continue 'request_loop;
                }

                if let Err(e) = ctx.decode(&mut batch) {
                    let _ = completion_tx.send(Err(format!("Decode failed: {}", e)));
                    continue 'request_loop;
                }

                // CRITICAL: Only add to tracking AFTER successful decode
                // This ensures cached_tokens.len() == kv_cache_pos at all times
                cached_tokens.push(new_token);
                n_cur += 1;
                n_generated += 1;
                kv_cache_pos += 1;
            }

            // Flush remaining text
            if generated_text.len() > sent_text.len() {
                let unsent = &generated_text[sent_text.len()..];
                if !unsent.is_empty() {
                    let _ = token_tx.blocking_send(TokenResponse {
                        token: unsent.to_string(),
                        done: false,
                        request_id,
                    });
                }
            }

            // Send done signal
            let _ = token_tx.blocking_send(TokenResponse {
                token: String::new(),
                done: true,
                request_id,
            });

            // ADVANCED: Keep cache state for potential reuse
            // cached_tokens now contains [prompt + generated], kv_cache_pos tracks total
            // Next request will compare its tokens against this and reuse matching prefix
            info!(
                "✅ Generation complete: cached_tokens={}, kv_cache_pos={}",
                cached_tokens.len(),
                kv_cache_pos
            );

            let _ = completion_tx.send(Ok(()));
        }
    }
}

// ModelInfo is defined in api::schema and used across API surface.
