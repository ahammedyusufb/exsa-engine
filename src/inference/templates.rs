//! Chat template system for different model types
//!
//! This module implements prompt templating to ensure models receive
//! properly formatted input. This fixes the 0-token bug where models
//! would immediately return EOS due to malformed prompts.

use serde::{Deserialize, Serialize};

/// Supported chat template types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateType {
    /// ChatML format (used by LFM2, Qwen, etc.)
    /// Format: <|im_start|>role\ncontent<|im_end|>
    ChatML,

    /// Llama 3 format
    /// Format: <|start_header_id|>role<|end_header_id|>\ncontent<|eot_id|>
    Llama3,

    /// Alpaca format
    /// Format: ### Instruction:\ncontent\n\n### Response:\n
    Alpaca,

    /// Gemma format (Google Gemma models)
    /// Format: <start_of_turn>role\ncontent<end_of_turn>\n
    Gemma,

    /// Raw/no template (for completion models)
    Raw,
}

/// A single chat message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl TemplateType {
    /// Auto-detect template type from model name/path
    pub fn from_model_name(model_name: &str) -> Self {
        let name_lower = model_name.to_lowercase();

        // Check for Llama 3.x variants (llama3, llama-3, llama3.1, llama3.2, etc.)
        if name_lower.contains("llama-3") || name_lower.contains("llama3") {
            tracing::info!("Detected Llama 3 model from name: {}", model_name);
            Self::Llama3
        } else if name_lower.contains("gemma") {
            tracing::info!("Detected Gemma model from name: {}", model_name);
            Self::Gemma
        } else if name_lower.contains("lfm2") || name_lower.contains("qwen") {
            tracing::info!("Detected ChatML model from name: {}", model_name);
            Self::ChatML
        } else if name_lower.contains("alpaca") {
            tracing::info!("Detected Alpaca model from name: {}", model_name);
            Self::Alpaca
        } else {
            // Default to ChatML as it's widely supported
            tracing::info!("Using default ChatML template for model: {}", model_name);
            Self::ChatML
        }
    }

    /// Get template-specific stop sequences
    /// These prevent models from continuing past the response boundary
    pub fn stop_sequences(&self) -> Vec<String> {
        match self {
            Self::ChatML => vec!["<|im_end|>".to_string()],
            Self::Llama3 => vec!["<|eot_id|>".to_string()],
            Self::Alpaca => vec!["###".to_string(), "\n###".to_string()],
            Self::Gemma => vec!["<end_of_turn>".to_string()],
            Self::Raw => vec![],
        }
    }
}

/// Apply chat template to messages
pub fn apply_chat_template(messages: &[ChatMessage], template_type: TemplateType) -> String {
    match template_type {
        TemplateType::ChatML => apply_chatml_template(messages),
        TemplateType::Llama3 => apply_llama3_template(messages),
        TemplateType::Alpaca => apply_alpaca_template(messages),
        TemplateType::Gemma => apply_gemma_template(messages),
        TemplateType::Raw => apply_raw_template(messages),
    }
}

/// Apply ChatML template
/// Format: <|im_start|>role\ncontent<|im_end|>\n
fn apply_chatml_template(messages: &[ChatMessage]) -> String {
    let mut formatted = String::new();

    for message in messages {
        formatted.push_str(&format!(
            "<|im_start|>{}\n{}<|im_end|>\n",
            message.role, message.content
        ));
    }

    // Add assistant prompt to start generation
    formatted.push_str("<|im_start|>assistant\n");

    formatted
}

/// Apply Llama 3 template
/// Format: <|start_header_id|>role<|end_header_id|>\ncontent<|eot_id|>
fn apply_llama3_template(messages: &[ChatMessage]) -> String {
    let mut formatted = String::from("<|begin_of_text|>");

    for message in messages {
        formatted.push_str(&format!(
            "<|start_header_id|>{}<|end_header_id|>\n\n{}<|eot_id|>",
            message.role, message.content
        ));
    }

    // Add assistant header
    formatted.push_str("<|start_header_id|>assistant<|end_header_id|>\n\n");

    formatted
}

/// Apply Alpaca template
/// Format: ### Instruction:\ncontent\n\n### Response:\n
fn apply_alpaca_template(messages: &[ChatMessage]) -> String {
    let mut formatted = String::new();

    // Alpaca typically uses last message as instruction
    if let Some(last_message) = messages.last() {
        formatted.push_str("### Instruction:\n");
        formatted.push_str(&last_message.content);
        formatted.push_str("\n\n### Response:\n");
    }

    formatted
}

/// Apply Gemma template
/// Format: <start_of_turn>role\ncontent<end_of_turn>\n
fn apply_gemma_template(messages: &[ChatMessage]) -> String {
    let mut formatted = String::from("<bos>");

    for message in messages {
        formatted.push_str(&format!(
            "<start_of_turn>{}\n{}<end_of_turn>\n",
            message.role, message.content
        ));
    }

    // Add model turn
    formatted.push_str("<start_of_turn>model\n");

    formatted
}

/// Apply raw template (no formatting)
fn apply_raw_template(messages: &[ChatMessage]) -> String {
    // For completion models, just concatenate messages
    messages
        .iter()
        .map(|m| m.content.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Helper to create a single-message prompt
pub fn create_single_message(role: &str, content: &str) -> Vec<ChatMessage> {
    vec![ChatMessage {
        role: role.to_string(),
        content: content.to_string(),
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chatml_template() {
        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content: "Hello!".to_string(),
        }];

        let result = apply_chatml_template(&messages);
        assert!(result.contains("<|im_start|>user"));
        assert!(result.contains("Hello!"));
        assert!(result.contains("<|im_end|>"));
        assert!(result.contains("<|im_start|>assistant"));
    }

    #[test]
    fn test_template_type_detection() {
        assert_eq!(
            TemplateType::from_model_name("LFM2-2.6B"),
            TemplateType::ChatML
        );
        assert_eq!(
            TemplateType::from_model_name("llama-3-8b"),
            TemplateType::Llama3
        );
        assert_eq!(
            TemplateType::from_model_name("alpaca-7b"),
            TemplateType::Alpaca
        );
    }
}
