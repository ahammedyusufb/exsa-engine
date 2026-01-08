//! Benchmarking utilities for Exsa-Engine

use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tracing::info;

/// Benchmark results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkResults {
    /// Total tokens generated
    pub total_tokens: usize,

    /// Total time elapsed
    pub total_duration: Duration,

    /// Tokens per second
    pub tokens_per_second: f64,

    /// Average time per token
    pub avg_time_per_token: Duration,

    /// Time to first token
    pub time_to_first_token: Duration,

    /// Number of requests processed
    pub num_requests: usize,

    /// Average request latency
    pub avg_request_latency: Duration,
}

impl BenchmarkResults {
    /// Calculate and display results
    pub fn display(&self) {
        info!("=== Benchmark Results ===");
        info!("Total Tokens: {}", self.total_tokens);
        info!("Total Duration: {:.2}s", self.total_duration.as_secs_f64());
        info!("Tokens/Second: {:.2}", self.tokens_per_second);
        info!(
            "Avg Time/Token: {:.2}ms",
            self.avg_time_per_token.as_millis()
        );
        info!(
            "Time to First Token: {:.2}ms",
            self.time_to_first_token.as_millis()
        );
        info!("Requests Processed: {}", self.num_requests);
        info!(
            "Avg Request Latency: {:.2}ms",
            self.avg_request_latency.as_millis()
        );
        info!("========================");
    }

    /// Export to JSON
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }
}

/// Benchmark tracker
pub struct BenchmarkTracker {
    start_time: Instant,
    first_token_time: Option<Instant>,
    token_count: usize,
    request_count: usize,
    request_latencies: Vec<Duration>,
}

impl BenchmarkTracker {
    /// Create a new benchmark tracker
    pub fn new() -> Self {
        Self {
            start_time: Instant::now(),
            first_token_time: None,
            token_count: 0,
            request_count: 0,
            request_latencies: Vec::new(),
        }
    }

    /// Record a token generation
    pub fn record_token(&mut self) {
        if self.first_token_time.is_none() {
            self.first_token_time = Some(Instant::now());
        }
        self.token_count += 1;
    }

    /// Record a completed request
    pub fn record_request(&mut self, latency: Duration) {
        self.request_count += 1;
        self.request_latencies.push(latency);
    }

    /// Finalize and get results
    pub fn finalize(self) -> BenchmarkResults {
        let total_duration = self.start_time.elapsed();
        let tokens_per_second = if total_duration.as_secs_f64() > 0.0 {
            self.token_count as f64 / total_duration.as_secs_f64()
        } else {
            0.0
        };

        let avg_time_per_token = if self.token_count > 0 {
            total_duration / self.token_count as u32
        } else {
            Duration::ZERO
        };

        let time_to_first_token = self
            .first_token_time
            .map(|t| t.duration_since(self.start_time))
            .unwrap_or(Duration::ZERO);

        let avg_request_latency = if !self.request_latencies.is_empty() {
            let sum: Duration = self.request_latencies.iter().sum();
            sum / self.request_latencies.len() as u32
        } else {
            Duration::ZERO
        };

        BenchmarkResults {
            total_tokens: self.token_count,
            total_duration,
            tokens_per_second,
            avg_time_per_token,
            time_to_first_token,
            num_requests: self.request_count,
            avg_request_latency,
        }
    }
}

impl Default for BenchmarkTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Memory usage snapshot
#[derive(Debug, Clone, Serialize)]
pub struct MemorySnapshot {
    /// Timestamp
    pub timestamp: u64,

    /// Resident set size (RAM) in bytes
    pub rss_bytes: u64,

    /// Virtual memory size in bytes
    pub vms_bytes: u64,
}

impl MemorySnapshot {
    /// Capture current memory usage (Linux implementation)
    #[cfg(target_os = "linux")]
    pub fn capture() -> Option<Self> {
        use std::fs;

        let status = fs::read_to_string("/proc/self/status").ok()?;
        let mut rss = 0u64;
        let mut vms = 0u64;

        for line in status.lines() {
            if line.starts_with("VmRSS:") {
                rss = line.split_whitespace().nth(1)?.parse::<u64>().ok()? * 1024;
            // Convert kB to bytes
            } else if line.starts_with("VmSize:") {
                vms = line.split_whitespace().nth(1)?.parse::<u64>().ok()? * 1024;
            }
        }

        Some(Self {
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()?
                .as_secs(),
            rss_bytes: rss,
            vms_bytes: vms,
        })
    }

    /// Capture current memory usage (macOS implementation)
    ///
    /// Note: RSS (Resident Set Size) reporting is not yet implemented on macOS.
    /// This would require using mach_task_basic_info() which is platform-specific.
    /// For now, returns 0 for rss_bytes on macOS.
    #[cfg(target_os = "macos")]
    pub fn capture() -> Option<Self> {
        Some(Self {
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()?
                .as_secs(),
            rss_bytes: 0, // Platform-specific implementation needed
            vms_bytes: 0,
        })
    }

    /// Capture current memory usage (other platforms)
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    pub fn capture() -> Option<Self> {
        Some(Self {
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()?
                .as_secs(),
            rss_bytes: 0,
            vms_bytes: 0,
        })
    }

    /// Display memory usage
    pub fn display(&self) {
        info!(
            "Memory: RSS={:.2}MB, VMS={:.2}MB",
            self.rss_bytes as f64 / 1024.0 / 1024.0,
            self.vms_bytes as f64 / 1024.0 / 1024.0
        );
    }
}
