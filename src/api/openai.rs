//! OpenAI-compatible API schemas
//!
//! This module implements OpenAI-compatible request/response structures
//! to enable ecosystem integration with LangChain, AutoGen, SillyTavern, etc.

use crate::inference::templates::ChatMessage;
use crate::rag::models::RagChatOptions;
use serde::{Deserialize, Serialize};

/// OpenAI chat completion request
#[derive(Debug, Clone, Deserialize)]
pub struct ChatCompletionRequest {
    /// Model identifier
    pub model: String,

    /// Array of messages in the conversation
    pub messages: Vec<ChatMessage>,

    /// Sampling temperature (0.0-2.0)
    #[serde(default = "default_temperature")]
    pub temperature: f32,

    /// Maximum tokens to generate
    #[serde(default = "default_max_tokens")]
    pub max_tokens: usize,

    /// Top-p sampling
    #[serde(default = "default_top_p")]
    pub top_p: f32,

    /// Top-k sampling
    #[serde(default = "default_top_k")]
    pub top_k: i32,

    /// Repeat penalty
    #[serde(default = "default_repeat_penalty")]
    pub repeat_penalty: f32,

    /// Number of completions to generate
    #[serde(default = "default_n")]
    pub n: usize,

    /// Whether to stream responses
    #[serde(default)]
    pub stream: bool,

    /// Stop sequences
    #[serde(default)]
    pub stop: Option<Vec<String>>,

    /// Presence penalty
    #[serde(default)]
    pub presence_penalty: f32,

    /// Frequency penalty
    #[serde(default)]
    pub frequency_penalty: f32,

    /// User identifier (optional)
    pub user: Option<String>,

    /// Optional EXSA extension: Retrieval-Augmented Generation controls.
    #[serde(default)]
    pub rag: Option<RagChatOptions>,
}

/// OpenAI-compatible embeddings request.
///
/// This is used by EXSA RAG to compute embeddings locally via llama.cpp.
#[derive(Debug, Clone, Deserialize)]
pub struct EmbeddingsRequest {
    /// Model identifier (accepted for compatibility; EXSA uses the active model)
    pub model: Option<String>,

    /// The input text(s) to embed.
    ///
    /// OpenAI accepts a string or an array of strings.
    pub input: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct EmbeddingsResponse {
    pub object: String,
    pub model: String,
    pub data: Vec<EmbeddingItem>,
    pub usage: Option<EmbeddingsUsage>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EmbeddingItem {
    pub object: String,
    pub index: usize,
    pub embedding: Vec<f32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EmbeddingsUsage {
    pub prompt_tokens: usize,
    pub total_tokens: usize,
}

/// OpenAI chat completion response
#[derive(Debug, Clone, Serialize)]
pub struct ChatCompletionResponse {
    /// Unique identifier
    pub id: String,

    /// Object type ("chat.completion")
    pub object: String,

    /// Unix timestamp
    pub created: u64,

    /// Model used
    pub model: String,

    /// Array of completion choices
    pub choices: Vec<ChatCompletionChoice>,

    /// Token usage statistics
    pub usage: Option<Usage>,
}

/// A single completion choice
#[derive(Debug, Clone, Serialize)]
pub struct ChatCompletionChoice {
    /// Choice index
    pub index: usize,

    /// Generated message
    pub message: ChatMessage,

    /// Finish reason ("stop", "length", "content_filter")
    pub finish_reason: String,
}

/// OpenAI streaming chunk
#[derive(Debug, Clone, Serialize)]
pub struct ChatCompletionChunk {
    /// Unique identifier
    pub id: String,

    /// Object type ("chat.completion.chunk")
    pub object: String,

    /// Unix timestamp
    pub created: u64,

    /// Model used
    pub model: String,

    /// Array of delta choices
    pub choices: Vec<ChatCompletionChunkChoice>,
}

/// A single delta choice in streaming
#[derive(Debug, Clone, Serialize)]
pub struct ChatCompletionChunkChoice {
    /// Choice index
    pub index: usize,

    /// Delta message (partial content)
    pub delta: ChatMessageDelta,

    /// Finish reason (null unless last chunk)
    pub finish_reason: Option<String>,
}

/// Delta message for streaming
#[derive(Debug, Clone, Serialize)]
pub struct ChatMessageDelta {
    /// Role (only in first chunk)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,

    /// Content delta
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

/// Token usage statistics
#[derive(Debug, Clone, Serialize)]
pub struct Usage {
    /// Prompt tokens
    pub prompt_tokens: usize,

    /// Completion tokens
    pub completion_tokens: usize,

    /// Total tokens
    pub total_tokens: usize,
}

// Default value functions
fn default_temperature() -> f32 {
    0.7
}
/// Default max tokens - 2048 allows for substantial responses without hitting limits
/// Can be overridden per-request via the max_tokens field
fn default_max_tokens() -> usize {
    2048
}
fn default_top_p() -> f32 {
    0.9
}
fn default_top_k() -> i32 {
    40
}
fn default_repeat_penalty() -> f32 {
    1.1
}
fn default_n() -> usize {
    1
}

impl ChatCompletionRequest {
    /// Convert to internal sampling parameters
    pub fn to_sampling_params(&self) -> crate::inference::SamplingParams {
        crate::inference::SamplingParams {
            temperature: self.temperature,
            max_tokens: self.max_tokens,
            top_k: self.top_k,
            top_p: self.top_p,
            repeat_penalty: self.repeat_penalty,
            presence_penalty: self.presence_penalty,
            frequency_penalty: self.frequency_penalty,
            stop_sequences: self.stop.clone().unwrap_or_default(),
            seed: None,
            min_p: 0.0, // DISABLED - causes llama.cpp crashes (as noted in params.rs)
            mirostat: 0,
            mirostat_tau: 5.0,
            mirostat_eta: 0.1,
            repeat_last_n: 64,
            tfs_z: 1.0,
            typical_p: 1.0,
            // Context management fields
            n_keep: None,     // Use default (no preserved tokens)
            session_id: None, // No session by default
        }
    }
}

impl ChatCompletionResponse {
    /// Create a new response
    pub fn new(id: String, model: String, message: ChatMessage, finish_reason: String) -> Self {
        let created = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or(std::time::Duration::from_secs(0))
            .as_secs();

        Self {
            id,
            object: "chat.completion".to_string(),
            created,
            model,
            choices: vec![ChatCompletionChoice {
                index: 0,
                message,
                finish_reason,
            }],
            usage: None,
        }
    }
}

impl ChatCompletionChunk {
    /// Create a new chunk
    pub fn new(
        id: String,
        model: String,
        content: Option<String>,
        finish_reason: Option<String>,
        is_first: bool,
    ) -> Self {
        let created = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or(std::time::Duration::from_secs(0))
            .as_secs();

        Self {
            id,
            object: "chat.completion.chunk".to_string(),
            created,
            model,
            choices: vec![ChatCompletionChunkChoice {
                index: 0,
                delta: ChatMessageDelta {
                    role: if is_first {
                        Some("assistant".to_string())
                    } else {
                        None
                    },
                    content,
                },
                finish_reason,
            }],
        }
    }
}
