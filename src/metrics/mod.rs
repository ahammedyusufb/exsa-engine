//! Metrics and observability for EXSA engine
//!
//! Provides production-grade metrics collection including:
//! - KV cache statistics (usage, hit rate, evictions)
//! - Latency tracking (TTFT, tokens/sec)
//! - Request throughput
//! - Memory usage

use serde::Serialize;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// Latency sample for histograms
#[derive(Debug, Clone, Copy)]
pub struct LatencySample {
    pub duration_ms: f64,
    pub timestamp: Instant,
}

/// Rolling latency histogram with percentile calculation
#[derive(Debug)]
pub struct LatencyHistogram {
    samples: VecDeque<LatencySample>,
    max_samples: usize,
}

impl LatencyHistogram {
    pub fn new(max_samples: usize) -> Self {
        Self {
            samples: VecDeque::with_capacity(max_samples),
            max_samples,
        }
    }

    /// Record a latency sample
    pub fn record(&mut self, duration: Duration) {
        let sample = LatencySample {
            duration_ms: duration.as_secs_f64() * 1000.0,
            timestamp: Instant::now(),
        };

        if self.samples.len() >= self.max_samples {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
    }

    /// Get percentile value (0-100)
    pub fn percentile(&self, p: f64) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }

        let mut sorted: Vec<f64> = self.samples.iter().map(|s| s.duration_ms).collect();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let idx = ((p / 100.0) * (sorted.len() - 1) as f64).round() as usize;
        sorted.get(idx).copied().unwrap_or(0.0)
    }

    /// Get average latency
    pub fn average(&self) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }
        let sum: f64 = self.samples.iter().map(|s| s.duration_ms).sum();
        sum / self.samples.len() as f64
    }

    /// Get min latency
    pub fn min(&self) -> f64 {
        self.samples
            .iter()
            .map(|s| s.duration_ms)
            .fold(f64::INFINITY, f64::min)
    }

    /// Get max latency
    pub fn max(&self) -> f64 {
        self.samples
            .iter()
            .map(|s| s.duration_ms)
            .fold(0.0, f64::max)
    }

    /// Sample count
    pub fn count(&self) -> usize {
        self.samples.len()
    }

    /// Clear old samples (older than duration)
    pub fn clear_old(&mut self, max_age: Duration) {
        let now = Instant::now();
        self.samples
            .retain(|s| now.duration_since(s.timestamp) < max_age);
    }
}

/// Core engine metrics
pub struct EngineMetrics {
    // Request counters
    pub total_requests: AtomicU64,
    pub successful_requests: AtomicU64,
    pub failed_requests: AtomicU64,
    pub active_requests: AtomicUsize,

    // Token counters
    pub total_tokens_generated: AtomicU64,
    pub total_prompt_tokens: AtomicU64,

    // Latency histograms (require lock for mutation)
    ttft_histogram: RwLock<LatencyHistogram>, // Time to First Token
    tpot_histogram: RwLock<LatencyHistogram>, // Time per Output Token
    total_latency_histogram: RwLock<LatencyHistogram>,

    // Cache metrics
    pub cache_hits: AtomicU64,
    pub cache_misses: AtomicU64,
    pub cache_evictions: AtomicU64,

    // Start time for uptime calculation
    start_time: Instant,
}

impl EngineMetrics {
    pub fn new() -> Self {
        Self {
            total_requests: AtomicU64::new(0),
            successful_requests: AtomicU64::new(0),
            failed_requests: AtomicU64::new(0),
            active_requests: AtomicUsize::new(0),
            total_tokens_generated: AtomicU64::new(0),
            total_prompt_tokens: AtomicU64::new(0),
            ttft_histogram: RwLock::new(LatencyHistogram::new(1000)),
            tpot_histogram: RwLock::new(LatencyHistogram::new(1000)),
            total_latency_histogram: RwLock::new(LatencyHistogram::new(1000)),
            cache_hits: AtomicU64::new(0),
            cache_misses: AtomicU64::new(0),
            cache_evictions: AtomicU64::new(0),
            start_time: Instant::now(),
        }
    }

    // ==================== REQUEST TRACKING ====================

    /// Start tracking a request
    pub fn request_start(&self) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        self.active_requests.fetch_add(1, Ordering::Relaxed);
    }

    /// Complete a successful request
    pub fn request_success(&self, total_duration: Duration, tokens_generated: usize) {
        self.successful_requests.fetch_add(1, Ordering::Relaxed);
        self.active_requests.fetch_sub(1, Ordering::Relaxed);
        self.total_tokens_generated
            .fetch_add(tokens_generated as u64, Ordering::Relaxed);

        if let Ok(mut hist) = self.total_latency_histogram.try_write() {
            hist.record(total_duration);
        }
    }

    /// Complete a failed request
    pub fn request_failure(&self) {
        self.failed_requests.fetch_add(1, Ordering::Relaxed);
        self.active_requests.fetch_sub(1, Ordering::Relaxed);
    }

    // ==================== LATENCY TRACKING ====================

    /// Record time to first token
    pub async fn record_ttft(&self, duration: Duration) {
        self.ttft_histogram.write().await.record(duration);
    }

    /// Record time per output token
    pub async fn record_tpot(&self, duration: Duration) {
        self.tpot_histogram.write().await.record(duration);
    }

    /// Record prompt tokens
    pub fn record_prompt_tokens(&self, count: usize) {
        self.total_prompt_tokens
            .fetch_add(count as u64, Ordering::Relaxed);
    }

    // ==================== CACHE TRACKING ====================

    /// Record cache hit
    pub fn cache_hit(&self) {
        self.cache_hits.fetch_add(1, Ordering::Relaxed);
    }

    /// Record cache miss
    pub fn cache_miss(&self) {
        self.cache_misses.fetch_add(1, Ordering::Relaxed);
    }

    /// Record cache eviction
    pub fn cache_eviction(&self) {
        self.cache_evictions.fetch_add(1, Ordering::Relaxed);
    }

    // ==================== STATISTICS ====================

    /// Get cache hit rate
    pub fn cache_hit_rate(&self) -> f64 {
        let hits = self.cache_hits.load(Ordering::Relaxed);
        let misses = self.cache_misses.load(Ordering::Relaxed);
        let total = hits + misses;
        if total > 0 {
            hits as f64 / total as f64
        } else {
            0.0
        }
    }

    /// Get tokens per second (overall)
    pub fn tokens_per_second(&self) -> f64 {
        let tokens = self.total_tokens_generated.load(Ordering::Relaxed);
        let uptime = self.start_time.elapsed().as_secs_f64();
        if uptime > 0.0 {
            tokens as f64 / uptime
        } else {
            0.0
        }
    }

    /// Get success rate
    pub fn success_rate(&self) -> f64 {
        let total = self.total_requests.load(Ordering::Relaxed);
        let success = self.successful_requests.load(Ordering::Relaxed);
        if total > 0 {
            success as f64 / total as f64
        } else {
            1.0
        }
    }

    /// Get uptime in seconds
    pub fn uptime_secs(&self) -> f64 {
        self.start_time.elapsed().as_secs_f64()
    }

    /// Get comprehensive metrics snapshot
    pub async fn snapshot(&self) -> MetricsSnapshot {
        let ttft = self.ttft_histogram.read().await;
        let tpot = self.tpot_histogram.read().await;
        let total_lat = self.total_latency_histogram.read().await;

        MetricsSnapshot {
            // Counters
            total_requests: self.total_requests.load(Ordering::Relaxed),
            successful_requests: self.successful_requests.load(Ordering::Relaxed),
            failed_requests: self.failed_requests.load(Ordering::Relaxed),
            active_requests: self.active_requests.load(Ordering::Relaxed),

            // Tokens
            total_tokens_generated: self.total_tokens_generated.load(Ordering::Relaxed),
            total_prompt_tokens: self.total_prompt_tokens.load(Ordering::Relaxed),
            tokens_per_second: self.tokens_per_second(),

            // Latency (ms)
            ttft_p50: ttft.percentile(50.0),
            ttft_p95: ttft.percentile(95.0),
            ttft_p99: ttft.percentile(99.0),
            tpot_avg: tpot.average(),
            total_latency_p50: total_lat.percentile(50.0),
            total_latency_p95: total_lat.percentile(95.0),

            // Cache
            cache_hits: self.cache_hits.load(Ordering::Relaxed),
            cache_misses: self.cache_misses.load(Ordering::Relaxed),
            cache_evictions: self.cache_evictions.load(Ordering::Relaxed),
            cache_hit_rate: self.cache_hit_rate(),

            // Health
            success_rate: self.success_rate(),
            uptime_secs: self.uptime_secs(),
        }
    }
}

impl Default for EngineMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Metrics snapshot for API responses
#[derive(Debug, Clone, Serialize)]
pub struct MetricsSnapshot {
    // Request counts
    pub total_requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub active_requests: usize,

    // Token throughput
    pub total_tokens_generated: u64,
    pub total_prompt_tokens: u64,
    pub tokens_per_second: f64,

    // Latency percentiles (ms)
    pub ttft_p50: f64,
    pub ttft_p95: f64,
    pub ttft_p99: f64,
    pub tpot_avg: f64,
    pub total_latency_p50: f64,
    pub total_latency_p95: f64,

    // Cache statistics
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub cache_evictions: u64,
    pub cache_hit_rate: f64,

    // Health
    pub success_rate: f64,
    pub uptime_secs: f64,
}

/// Shared metrics instance
pub type SharedMetrics = Arc<EngineMetrics>;

/// Create shared metrics
pub fn create_metrics() -> SharedMetrics {
    Arc::new(EngineMetrics::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_latency_histogram() {
        let mut hist = LatencyHistogram::new(100);

        hist.record(Duration::from_millis(10));
        hist.record(Duration::from_millis(20));
        hist.record(Duration::from_millis(30));

        assert!(hist.average() > 0.0);
        assert!(hist.min() <= hist.max());
        assert_eq!(hist.count(), 3);
    }

    #[test]
    fn test_engine_metrics() {
        let metrics = EngineMetrics::new();

        metrics.request_start();
        assert_eq!(metrics.active_requests.load(Ordering::Relaxed), 1);

        metrics.request_success(Duration::from_millis(100), 50);
        assert_eq!(metrics.active_requests.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.successful_requests.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.total_tokens_generated.load(Ordering::Relaxed), 50);
    }

    #[test]
    fn test_cache_hit_rate() {
        let metrics = EngineMetrics::new();

        metrics.cache_hit();
        metrics.cache_hit();
        metrics.cache_miss();

        let rate = metrics.cache_hit_rate();
        assert!((rate - 0.666).abs() < 0.01);
    }
}
