//! Request queue for managing concurrent inference requests

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use tracing::debug;
use uuid::Uuid;

use crate::inference::engine::InferenceEngine;
use crate::inference::params::SamplingParams;

/// A single inference request
#[derive(Debug)]
pub struct InferenceRequest {
    /// Unique request ID
    pub id: Uuid,

    /// The input prompt
    pub prompt: String,

    /// Sampling parameters
    pub params: SamplingParams,

    /// Channel to send generated tokens through
    pub token_tx: mpsc::Sender<TokenResponse>,

    /// Channel to signal completion or errors
    pub completion_tx: oneshot::Sender<Result<(), String>>,

    /// Cancellation token for request cancellation
    pub cancellation_token: CancellationToken,

    /// Request timeout duration (None = no timeout)
    pub timeout_duration: Option<Duration>,
}

/// Response for a single generated token
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenResponse {
    /// The generated token text
    pub token: String,

    /// Whether this is the final token
    pub done: bool,

    /// Request ID this token belongs to
    pub request_id: Uuid,
}

/// Request queue for managing concurrent inference requests
pub struct RequestQueue {
    /// Channel for submitting inference requests
    request_tx: mpsc::Sender<InferenceRequest>,

    /// Queue capacity
    capacity: usize,
}

impl RequestQueue {
    /// Create a new request queue
    pub fn new(capacity: usize, engine: Arc<InferenceEngine>) -> Self {
        let (request_tx, mut request_rx) = mpsc::channel::<InferenceRequest>(capacity);

        // Spawn worker task to process requests
        tokio::spawn(async move {
            while let Some(request) = request_rx.recv().await {
                debug!("Processing inference request: {}", request.id);

                if let Err(e) = engine.process_request(request).await {
                    eprintln!("Request processing failed: {}", e);
                }
            }
        });

        Self {
            request_tx,
            capacity,
        }
    }

    /// Get queue capacity
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Get a handle to submit requests to this queue
    pub fn handle(&self) -> QueueHandle {
        QueueHandle {
            request_tx: self.request_tx.clone(),
        }
    }
}

/// Handle for submitting requests to the queue
#[derive(Clone)]
pub struct QueueHandle {
    request_tx: mpsc::Sender<InferenceRequest>,
}

impl QueueHandle {
    /// Submit a new inference request with optional timeout
    pub async fn submit(
        &self,
        prompt: String,
        params: SamplingParams,
    ) -> Result<QueuedRequest, String> {
        self.submit_with_timeout(prompt, params, None).await
    }

    /// Submit a new inference request with explicit timeout
    pub async fn submit_with_timeout(
        &self,
        prompt: String,
        params: SamplingParams,
        timeout: Option<Duration>,
    ) -> Result<QueuedRequest, String> {
        let request_id = Uuid::new_v4();
        // Buffer size of 100 tokens balances memory usage with streaming throughput.
        // Larger buffers reduce backpressure but increase memory; 100 is optimal for most cases.
        let (token_tx, token_rx) = mpsc::channel(100);
        let (completion_tx, completion_rx) = oneshot::channel();
        let cancellation_token = CancellationToken::new();

        let request = InferenceRequest {
            id: request_id,
            prompt,
            params,
            token_tx,
            completion_tx,
            cancellation_token: cancellation_token.clone(),
            timeout_duration: timeout,
        };

        self.request_tx
            .send(request)
            .await
            .map_err(|_| "Queue is full or closed".to_string())?;

        debug!(
            "Request {} submitted to queue with timeout: {:?}",
            request_id, timeout
        );

        Ok(QueuedRequest {
            id: request_id,
            token_rx,
            completion_rx,
            cancellation_token,
        })
    }

    /// Get the current queue capacity
    pub fn capacity(&self) -> usize {
        self.request_tx.capacity()
    }

    /// Get the number of pending requests in the queue
    pub fn pending_count(&self) -> usize {
        // The max_capacity is the initial capacity, capacity() is remaining slots
        self.request_tx.max_capacity() - self.request_tx.capacity()
    }
}

/// A queued request with channels to receive results
pub struct QueuedRequest {
    /// Request ID
    pub id: Uuid,

    /// Channel to receive generated tokens
    pub token_rx: mpsc::Receiver<TokenResponse>,

    /// Channel to receive completion signal
    pub completion_rx: oneshot::Receiver<Result<(), String>>,

    /// Cancellation token to cancel this request
    pub cancellation_token: CancellationToken,
}
