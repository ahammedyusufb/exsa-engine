//! HTTP request handlers

use crate::api::openai::{ChatCompletionChunk, ChatCompletionRequest};
use crate::api::schema::{AppState, GenerateRequest, HealthResponse, StatusResponse, TokenEvent};
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

/// Health check handler with detailed status
pub async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    let active_requests = state.engine.active_requests();
    let model_info = state.engine.model_info();
    let queue_size = state.queue.pending_count();

    // Check if shutdown is in progress
    let shutting_down = state
        .shutdown_flag
        .load(std::sync::atomic::Ordering::Relaxed);

    let status = if shutting_down {
        "shutting_down"
    } else if active_requests > 100 {
        "overloaded"
    } else {
        "healthy"
    };

    // Calculate uptime
    let uptime = state.start_time.elapsed().as_secs();

    Json(HealthResponse {
        status: status.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        model_loaded: Some(true),
        model_path: Some(model_info.model_path),
        context_size: Some(model_info.context_size),
        gpu_layers: Some(model_info.gpu_layers),
        active_requests: Some(active_requests),
        queue_size: Some(queue_size),
        queue_capacity: Some(state.queue.capacity()),
        uptime_seconds: Some(uptime),
    })
}

/// Server status handler
pub async fn status(State(state): State<AppState>) -> Json<StatusResponse> {
    let active = state.engine.active_requests();

    Json(StatusResponse {
        status: "running".to_string(),
        queue_capacity: state.queue.capacity(),
        active_requests: active,
    })
}

/// Model information handler
pub async fn model_info(State(state): State<AppState>) -> Json<serde_json::Value> {
    let info = state.engine.model_info();
    Json(serde_json::json!({
        "model_path": info.model_path,
        "context_size": info.context_size,
        "gpu_layers": info.gpu_layers
    }))
}

/// Generate text handler with SSE streaming
pub async fn generate(
    State(state): State<AppState>,
    Json(request): Json<GenerateRequest>,
) -> std::result::Result<Sse<impl Stream<Item = std::result::Result<Event, Infallible>>>, ExsaError>
{
    // Log prompt length instead of full content for security/privacy
    info!(
        "Received generation request with prompt length: {} chars",
        request.prompt.len()
    );

    // Validate request
    if request.prompt.is_empty() {
        return Err(ExsaError::InvalidParameters(
            "Prompt cannot be empty".to_string(),
        ));
    }

    // Validate prompt length (rough estimate: 4 chars per token)
    let estimated_prompt_tokens = request.prompt.len() / 4;
    let context_size = state.engine.model_info().context_size;

    if estimated_prompt_tokens > context_size {
        return Err(ExsaError::InvalidParameters(format!(
            "Prompt too long: estimated {} tokens exceeds context size of {} tokens",
            estimated_prompt_tokens, context_size
        )));
    }

    // Validate max_tokens + prompt doesn't exceed context
    if estimated_prompt_tokens + request.sampling_params.max_tokens > context_size {
        return Err(ExsaError::InvalidParameters(format!(
            "Prompt ({} tokens) + max_tokens ({}) exceeds context size ({})",
            estimated_prompt_tokens, request.sampling_params.max_tokens, context_size
        )));
    }

    if let Err(e) = request.sampling_params.validate() {
        return Err(ExsaError::InvalidParameters(e.to_string()));
    }

    // Apply chat template if enabled (fixes 24-token bug)
    use crate::inference::templates::{apply_chat_template, create_single_message, TemplateType};

    let (formatted_prompt, sampling_params) = if request.use_chat_template.unwrap_or(true) {
        // Auto-detect template type from model
        let model_path = state.engine.model_info().model_path;
        let template_type = TemplateType::from_model_name(&model_path);

        // Convert prompt to chat message and apply template
        let messages = create_single_message("user", &request.prompt);
        let formatted = apply_chat_template(&messages, template_type);

        // Add template-specific stop sequences
        let mut params = request.sampling_params.clone();
        let template_stops = template_type.stop_sequences();

        // Merge with user-provided stop sequences, avoiding duplicates
        for stop in template_stops {
            if !params.stop_sequences.contains(&stop) {
                params.stop_sequences.push(stop);
            }
        }

        info!(
            "Applied {:?} template to prompt with stop sequences: {:?}",
            template_type, params.stop_sequences
        );
        (formatted, params)
    } else {
        (request.prompt.clone(), request.sampling_params.clone())
    };

    // Submit request to queue with formatted prompt
    let queued_request = state
        .queue
        .submit(formatted_prompt, sampling_params)
        .await
        .map_err(|_| ExsaError::QueueFull)?;

    info!("Request {} queued successfully", queued_request.id);

    // Create SSE stream from token receiver
    let token_stream = ReceiverStream::new(queued_request.token_rx).map(|token_response| {
        let event = TokenEvent {
            token: token_response.token,
            done: token_response.done,
        };

        let json = serde_json::to_string(&event).unwrap_or_else(|e| {
            error!("Failed to serialize token event: {}", e);
            "{}".to_string()
        });

        Ok(Event::default().data(json))
    });

    Ok(Sse::new(token_stream).keep_alive(KeepAlive::default()))
}

/// OpenAI-compatible chat completions endpoint
pub async fn chat_completions(
    State(state): State<AppState>,
    Json(request): Json<ChatCompletionRequest>,
) -> std::result::Result<Sse<impl Stream<Item = std::result::Result<Event, Infallible>>>, ExsaError>
{
    info!(
        "Received OpenAI chat completion request with {} messages",
        request.messages.len()
    );

    // Validate request
    if request.messages.is_empty() {
        return Err(ExsaError::InvalidParameters(
            "Messages cannot be empty".to_string(),
        ));
    }

    // Apply chat template to messages
    use crate::inference::templates::{apply_chat_template, TemplateType};

    let model_path = state.engine.model_info().model_path;
    let template_type = TemplateType::from_model_name(&model_path);
    let formatted_prompt = apply_chat_template(&request.messages, template_type);

    // Convert to sampling parameters and add template stop sequences
    let mut sampling_params = request.to_sampling_params();
    let template_stops = template_type.stop_sequences();

    // Merge with user-provided stop sequences, avoiding duplicates
    for stop in template_stops {
        if !sampling_params.stop_sequences.contains(&stop) {
            sampling_params.stop_sequences.push(stop);
        }
    }

    info!(
        "Applied {:?} template to {} messages with stop sequences: {:?}",
        template_type,
        request.messages.len(),
        sampling_params.stop_sequences
    );

    // Validate sampling parameters
    sampling_params
        .validate()
        .map_err(|e| ExsaError::InvalidParameters(e.to_string()))?;

    // Submit request to queue
    let queued_request = state
        .queue
        .submit(formatted_prompt, sampling_params)
        .await
        .map_err(|_| ExsaError::QueueFull)?;

    let request_id = queued_request.id.to_string();
    info!("OpenAI request {} queued successfully", request_id);

    // Create SSE stream for OpenAI-compatible responses
    let model_name = request.model.clone();
    let mut is_first = true;

    let token_stream = ReceiverStream::new(queued_request.token_rx).map(move |token_response| {
        let chunk = if token_response.done {
            // Final chunk with finish reason
            ChatCompletionChunk::new(
                request_id.clone(),
                model_name.clone(),
                None,
                Some("stop".to_string()),
                false,
            )
        } else {
            // Regular content chunk
            let chunk = ChatCompletionChunk::new(
                request_id.clone(),
                model_name.clone(),
                Some(token_response.token.clone()),
                None,
                is_first,
            );
            is_first = false;
            chunk
        };

        let json = serde_json::to_string(&chunk).unwrap_or_else(|e| {
            error!("Failed to serialize OpenAI chunk: {}", e);
            "{}".to_string()
        });

        Ok(Event::default().data(json))
    });

    Ok(Sse::new(token_stream).keep_alive(KeepAlive::default()))
}
