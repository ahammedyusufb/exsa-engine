//! Rate limiting middleware

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// Rate limiter state
#[derive(Clone)]
pub struct RateLimiter {
    /// Client request tracking
    clients: Arc<Mutex<HashMap<String, ClientInfo>>>,

    /// Maximum requests per window
    max_requests: usize,

    /// Time window duration
    window: Duration,
}

/// Per-client tracking information
#[derive(Debug, Clone)]
struct ClientInfo {
    /// Request count in current window
    count: usize,

    /// Window start time
    window_start: Instant,
}

impl RateLimiter {
    /// Create a new rate limiter
    pub fn new(max_requests: usize, window_secs: u64) -> Self {
        Self {
            clients: Arc::new(Mutex::new(HashMap::new())),
            max_requests,
            window: Duration::from_secs(window_secs),
        }
    }

    /// Check if a client can make a request
    pub async fn check(&self, client_id: String) -> Result<(), ()> {
        let mut clients = self.clients.lock().await;
        let now = Instant::now();

        let client_info = clients.entry(client_id).or_insert_with(|| ClientInfo {
            count: 0,
            window_start: now,
        });

        // Reset window if expired
        if now.duration_since(client_info.window_start) > self.window {
            client_info.count = 0;
            client_info.window_start = now;
        }

        // Check if under limit
        if client_info.count >= self.max_requests {
            return Err(());
        }

        // Increment count
        client_info.count += 1;
        Ok(())
    }

    /// Cleanup expired entries (call periodically)
    pub async fn cleanup(&self) {
        let mut clients = self.clients.lock().await;
        let now = Instant::now();

        clients.retain(|_, info| now.duration_since(info.window_start) <= self.window);
    }
}

/// Rate limiting middleware
pub async fn rate_limit_middleware(
    State(limiter): State<RateLimiter>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // Extract client identifier with fallback to X-Forwarded-For header
    let client_id = request
        .extensions()
        .get::<SocketAddr>()
        .map(|addr| addr.ip().to_string())
        .or_else(|| {
            // Fallback: try X-Forwarded-For header for proxied requests
            request
                .headers()
                .get("x-forwarded-for")
                .and_then(|h| h.to_str().ok())
                .and_then(|s| s.split(',').next())
                .map(|s| s.trim().to_string())
        })
        .unwrap_or_else(|| {
            // Last resort: use "unknown" but log warning
            tracing::warn!("Could not identify client for rate limiting");
            "unknown".to_string()
        });

    // Check rate limit
    match limiter.check(client_id).await {
        Ok(_) => Ok(next.run(request).await),
        Err(_) => Err(StatusCode::TOO_MANY_REQUESTS),
    }
}
