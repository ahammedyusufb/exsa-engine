//! Server configuration

use serde::{Deserialize, Serialize};
use std::net::IpAddr;

/// Server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Host to bind to
    /// - 127.0.0.1 = localhost only (default, most secure)
    /// - 0.0.0.0 = all interfaces (LAN access)
    pub host: String,

    /// Port to listen on
    pub port: u16,

    /// Maximum queue size for pending requests
    pub max_queue_size: usize,

    /// Enable CORS (allow cross-origin requests)
    pub enable_cors: bool,

    /// Rate limiting configuration
    pub rate_limit: RateLimitConfig,

    /// Request timeout in seconds (0 = no timeout)
    pub request_timeout_secs: u64,

    /// Token channel buffer size
    pub token_channel_size: usize,

    /// Default random seed for generation
    pub default_seed: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(), // Secure by default
            port: 3000,
            max_queue_size: 100,
            enable_cors: false, // Disabled by default for security
            rate_limit: RateLimitConfig::default(),
            request_timeout_secs: 300, // 5 minute default timeout
            token_channel_size: 100,
            default_seed: 1234,
        }
    }
}

impl ServerConfig {
    /// Create a new server configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the host address
    pub fn with_host(mut self, host: impl Into<String>) -> Self {
        self.host = host.into();
        self
    }

    /// Set the port
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Enable LAN access (bind to 0.0.0.0)
    pub fn enable_lan_access(mut self) -> Self {
        self.host = "0.0.0.0".to_string();
        self
    }

    /// Enable CORS
    pub fn enable_cors(mut self) -> Self {
        self.enable_cors = true;
        self
    }

    /// Set rate limiting configuration
    pub fn with_rate_limit(mut self, rate_limit: RateLimitConfig) -> Self {
        self.rate_limit = rate_limit;
        self
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<(), String> {
        if self.port == 0 {
            return Err("Port cannot be 0".to_string());
        }

        if self.max_queue_size == 0 {
            return Err("Max queue size must be greater than 0".to_string());
        }

        // Validate host is a valid IP address
        if self.host.parse::<IpAddr>().is_err() {
            return Err(format!("Invalid host address: {}", self.host));
        }

        Ok(())
    }
}

/// Rate limiting configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Enable rate limiting
    pub enabled: bool,

    /// Maximum requests per window
    pub max_requests: usize,

    /// Time window in seconds
    pub window_secs: u64,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            enabled: false,   // Disabled by default
            max_requests: 60, // 60 requests
            window_secs: 60,  // per 60 seconds (1 req/sec average)
        }
    }
}

impl RateLimitConfig {
    /// Create a new rate limit configuration
    pub fn new(max_requests: usize, window_secs: u64) -> Self {
        Self {
            enabled: true,
            max_requests,
            window_secs,
        }
    }

    /// Disable rate limiting
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Default::default()
        }
    }
}
