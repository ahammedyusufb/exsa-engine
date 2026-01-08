//! OpenAI-compatible chat completions handler

use crate::api::openai::{ChatCompletionChunk, ChatCompletionRequest, ChatCompletionResponse};
use crate::api::schema::AppState;
use crate::inference::templates::{apply_chat_template, TemplateType};
use crate::utils::error::ExsaError;
use axum::{
    extract::State,
    response::sse::{Event, KeepAlive, Sse},
    Json,
};
use futures::stream::Stream;
use std::convert::Infallible;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;
use tracing::{error, info};

/// Chat completions handler (OpenAI-compatible)
pub async fn chat_completions(
    State(state): State<AppState>,
    Json(request): Json<ChatCompletionRequest>,
) -> std::result::Result<Sse<impl Stream<Item = std::result::Result<Event, Infallible>>>, ExsaError>
{
    info!(
        "Received chat completion request for model: {}",
        request.model
    );

    // Log the sampling parameters received from client
    info!(
        "📊 Request params: temperature={}, max_tokens={}, top_p={}, top_k={}, repeat_penalty={}",
        request.temperature,
        request.max_tokens,
        request.top_p,
        request.top_k,
        request.repeat_penalty
    );

    // Validate request
    if request.messages.is_empty() {
        return Err(ExsaError::InvalidParameters(
            "Messages cannot be empty".to_string(),
        ));
    }

    // Detect template type from model name
    let template_type = TemplateType::from_model_name(&request.model);
    info!("Using template type: {:?}", template_type);

    // EMERGENCY TRIMMING ONLY: Let most conversations through to engine
    // Engine will handle sliding window. This is only for extreme cases (> 99%)
    let context_limit = state.engine.model_info().context_size;
    let emergency_threshold = (context_limit as f32 * 0.99) as usize;

    let mut messages_to_use = request.messages.clone();

    // Rough token estimation: 1 token ≈ 4 characters (OpenAI standard estimate)
    let estimate_tokens = |text: &str| -> usize { text.len() / 4 };

    // Estimate total tokens in all messages
    let total_estimated_tokens: usize = messages_to_use
        .iter()
        .map(|m| estimate_tokens(&m.content))
        .sum();

    if total_estimated_tokens > emergency_threshold {
        info!(
            "⚠️  EMERGENCY: Conversation exceeds context limit ({} est. tokens > {} threshold)",
            total_estimated_tokens, emergency_threshold
        );

        // Separate system messages from conversation
        let (system_msgs, mut conversation_msgs): (Vec<_>, Vec<_>) = messages_to_use
            .into_iter()
            .partition(|m| m.role == "system");

        // Keep last N message pairs (user + assistant)
        // Keep at least 16 messages (8 exchanges) to maintain context
        let keep_count = 16.min(conversation_msgs.len());
        let trim_count = conversation_msgs.len().saturating_sub(keep_count);

        if trim_count > 0 {
            info!(
                "🗑️  Trimming {} oldest messages, keeping {} recent messages",
                trim_count, keep_count
            );
            conversation_msgs.drain(0..trim_count);
        }

        // Rebuild: system messages + recent conversation
        messages_to_use = system_msgs;
        messages_to_use.extend(conversation_msgs);

        let new_estimated_tokens: usize = messages_to_use
            .iter()
            .map(|m| estimate_tokens(&m.content))
            .sum();

        info!(
            "✂️  After trimming: {} est. tokens ({} messages)",
            new_estimated_tokens,
            messages_to_use.len()
        );
    }

    // Apply chat template to messages
    let formatted_prompt = apply_chat_template(&messages_to_use, template_type);

    info!(
        "Formatted prompt (first 100 chars): {}",
        &formatted_prompt.chars().take(100).collect::<String>()
    );

    // Convert to internal sampling params and merge stop sequences
    let mut sampling_params = request.to_sampling_params();

    // Get template-specific stop sequences
    let template_stops = template_type.stop_sequences();

    // Merge with user-provided stop sequences (avoid duplicates)
    for stop_seq in template_stops {
        if !sampling_params.stop_sequences.contains(&stop_seq) {
            sampling_params.stop_sequences.push(stop_seq);
        }
    }

    if !sampling_params.stop_sequences.is_empty() {
        info!("Using stop sequences: {:?}", sampling_params.stop_sequences);
    }

    // Validate sampling params
    if let Err(e) = sampling_params.validate() {
        return Err(ExsaError::InvalidParameters(e.to_string()));
    }

    // Submit to queue with formatted prompt
    let queued_request = state
        .queue
        .submit(formatted_prompt, sampling_params)
        .await
        .map_err(|_| ExsaError::QueueFull)?;

    let request_id = queued_request.id.to_string();
    info!("Request {} queued successfully", request_id);

    // Create streaming response
    let model_name = request.model.clone();
    let _is_streaming = request.stream;
    let mut is_first_chunk = true;

    let token_stream = ReceiverStream::new(queued_request.token_rx).map(move |token_response| {
        let chunk = if token_response.done {
            // Final chunk with finish_reason
            ChatCompletionChunk::new(
                request_id.clone(),
                model_name.clone(),
                if token_response.token.is_empty() {
                    None
                } else {
                    Some(token_response.token)
                },
                Some("stop".to_string()),
                false,
            )
        } else {
            // Regular content chunk
            let chunk = ChatCompletionChunk::new(
                request_id.clone(),
                model_name.clone(),
                Some(token_response.token),
                None,
                is_first_chunk,
            );
            is_first_chunk = false;
            chunk
        };

        let json = serde_json::to_string(&chunk).unwrap_or_else(|e| {
            error!("Failed to serialize chunk: {}", e);
            "{}".to_string()
        });

        Ok(Event::default().data(json))
    });

    Ok(Sse::new(token_stream).keep_alive(KeepAlive::default()))
}

/// Non-streaming chat completions (for compatibility)
pub async fn chat_completions_non_streaming(
    State(_state): State<AppState>,
    Json(request): Json<ChatCompletionRequest>,
) -> std::result::Result<Json<ChatCompletionResponse>, ExsaError> {
    info!(
        "Received non-streaming chat completion request for model: {}",
        request.model
    );

    // Similar to streaming but collect all tokens
    // For now, return error suggesting to use streaming
    Err(ExsaError::NotImplemented(
        "Non-streaming chat completions not yet implemented. Please use stream=true".to_string(),
    ))
}
