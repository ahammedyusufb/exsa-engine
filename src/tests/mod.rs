//! Integration tests for EXSA engine
//!
//! Tests for long-context, multi-session, and performance scenarios.

use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// Stress test configuration
pub struct StressTestConfig {
    /// Number of concurrent sessions
    pub concurrent_sessions: usize,
    /// Tokens per request
    pub tokens_per_request: usize,
    /// Total requests to send
    pub total_requests: usize,
    /// Request timeout
    pub timeout: Duration,
}

impl Default for StressTestConfig {
    fn default() -> Self {
        Self {
            concurrent_sessions: 10,
            tokens_per_request: 100,
            total_requests: 100,
            timeout: Duration::from_secs(30),
        }
    }
}

/// Test results summary
#[derive(Debug, Clone)]
pub struct TestResults {
    pub total_requests: usize,
    pub successful_requests: usize,
    pub failed_requests: usize,
    pub total_tokens: usize,
    pub duration: Duration,
    pub tokens_per_second: f64,
    pub avg_latency_ms: f64,
    pub p95_latency_ms: f64,
}

/// Long context stress test
///
/// Tests the engine's ability to handle long contexts near the limit.
pub async fn test_long_context(
    session_manager: Arc<RwLock<crate::session::SessionManager>>,
    context_limit: usize,
) -> Result<TestResults, String> {
    let start = Instant::now();
    let mut successful = 0;
    let mut tokens = 0;
    let mut latencies = Vec::new();

    // Test at various context percentages
    for percentage in [50, 70, 85, 92, 95] {
        let target_tokens = (context_limit * percentage) / 100;
        let request_start = Instant::now();

        // Simulate request at this context level
        let mut mgr = session_manager.write().await;
        match mgr.create_session(Some(format!("stress-{}", percentage)), None) {
            Ok(session_id) => {
                if let Some(session) = mgr.get_session_mut(session_id) {
                    // Simulate token generation
                    session.record_request(target_tokens, request_start.elapsed());
                    successful += 1;
                    tokens += target_tokens;
                    latencies.push(request_start.elapsed().as_secs_f64() * 1000.0);
                }
                mgr.close_session(session_id);
            }
            Err(e) => {
                return Err(format!("Session creation failed at {}%: {}", percentage, e));
            }
        }
    }

    let duration = start.elapsed();
    latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());

    Ok(TestResults {
        total_requests: 5,
        successful_requests: successful,
        failed_requests: 5 - successful,
        total_tokens: tokens,
        duration,
        tokens_per_second: tokens as f64 / duration.as_secs_f64(),
        avg_latency_ms: latencies.iter().sum::<f64>() / latencies.len() as f64,
        p95_latency_ms: latencies
            .get((latencies.len() * 95) / 100)
            .copied()
            .unwrap_or(0.0),
    })
}

/// Multi-session concurrency test
///
/// Tests the engine's ability to handle many concurrent sessions.
pub async fn test_multi_session_concurrency(
    config: StressTestConfig,
) -> Result<TestResults, String> {
    let session_manager = Arc::new(RwLock::new(crate::session::SessionManager::new(
        config.concurrent_sessions * 2,
    )));

    let start = Instant::now();
    let successful = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let total_tokens = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let latencies = Arc::new(RwLock::new(Vec::new()));

    let mut handles = Vec::new();

    for i in 0..config.concurrent_sessions {
        let mgr = session_manager.clone();
        let success_counter = successful.clone();
        let token_counter = total_tokens.clone();
        let lats = latencies.clone();
        let tokens_per_req = config.tokens_per_request;
        let reqs_per_session = config.total_requests / config.concurrent_sessions;

        let handle = tokio::spawn(async move {
            for _j in 0..reqs_per_session {
                let req_start = Instant::now();

                let mut lock = mgr.write().await;
                let session_id = lock
                    .get_or_create_for_user(&format!("user-{}", i))
                    .unwrap_or_else(|_| uuid::Uuid::new_v4());

                if let Some(session) = lock.get_session_mut(session_id) {
                    session.record_request(tokens_per_req, req_start.elapsed());
                    success_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    token_counter.fetch_add(tokens_per_req, std::sync::atomic::Ordering::Relaxed);
                }
                drop(lock);

                let lat = req_start.elapsed().as_secs_f64() * 1000.0;
                lats.write().await.push(lat);

                // Small delay between requests
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        });

        handles.push(handle);
    }

    // Wait for all sessions to complete
    for handle in handles {
        let _ = handle.await;
    }

    let duration = start.elapsed();
    let mut lats = latencies.write().await;
    lats.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let success_count = successful.load(std::sync::atomic::Ordering::Relaxed);
    let token_count = total_tokens.load(std::sync::atomic::Ordering::Relaxed);

    Ok(TestResults {
        total_requests: config.total_requests,
        successful_requests: success_count,
        failed_requests: config.total_requests - success_count,
        total_tokens: token_count,
        duration,
        tokens_per_second: token_count as f64 / duration.as_secs_f64(),
        avg_latency_ms: if !lats.is_empty() {
            lats.iter().sum::<f64>() / lats.len() as f64
        } else {
            0.0
        },
        p95_latency_ms: lats.get((lats.len() * 95) / 100).copied().unwrap_or(0.0),
    })
}

/// Memory leak detection test
///
/// Creates and destroys many sessions to check for memory leaks.
pub async fn test_memory_stability(iterations: usize) -> Result<(), String> {
    let session_manager = Arc::new(RwLock::new(crate::session::SessionManager::new(100)));

    for i in 0..iterations {
        // Create sessions
        let mut session_ids = Vec::new();
        {
            let mut mgr = session_manager.write().await;
            for j in 0..10 {
                if let Ok(id) = mgr.create_session(Some(format!("leak-test-{}-{}", i, j)), None) {
                    session_ids.push(id);
                }
            }
        }

        // Use sessions
        {
            let mut mgr = session_manager.write().await;
            for &id in &session_ids {
                if let Some(session) = mgr.get_session_mut(id) {
                    session.cache_prompt(i as u64, 100, 100);
                    session.record_request(50, Duration::from_millis(10));
                }
            }
        }

        // Close sessions
        {
            let mut mgr = session_manager.write().await;
            for id in session_ids {
                mgr.close_session(id);
            }
            mgr.cleanup_expired();
        }
    }

    // Final cleanup check
    let mgr = session_manager.read().await;
    let stats = mgr.manager_stats();

    if stats.total_sessions > 0 {
        return Err(format!("Leaked {} sessions", stats.total_sessions));
    }

    Ok(())
}

/// Benchmark results
#[derive(Debug, Clone)]
pub struct BenchmarkResults {
    pub name: String,
    pub iterations: usize,
    pub total_time_ms: f64,
    pub avg_time_ms: f64,
    pub min_time_ms: f64,
    pub max_time_ms: f64,
    pub ops_per_second: f64,
}

/// Run a benchmark
pub async fn run_benchmark<F, Fut>(name: &str, iterations: usize, mut func: F) -> BenchmarkResults
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let mut times = Vec::with_capacity(iterations);

    for _ in 0..iterations {
        let start = Instant::now();
        func().await;
        times.push(start.elapsed().as_secs_f64() * 1000.0);
    }

    let total: f64 = times.iter().sum();
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());

    BenchmarkResults {
        name: name.to_string(),
        iterations,
        total_time_ms: total,
        avg_time_ms: total / iterations as f64,
        min_time_ms: times.first().copied().unwrap_or(0.0),
        max_time_ms: times.last().copied().unwrap_or(0.0),
        ops_per_second: (iterations as f64 / total) * 1000.0,
    }
}

#[cfg(test)]
mod test_cases {
    use super::*;

    #[tokio::test]
    async fn test_stress_test_config() {
        let config = StressTestConfig::default();
        assert_eq!(config.concurrent_sessions, 10);
        assert_eq!(config.total_requests, 100);
    }

    #[tokio::test]
    async fn test_memory_stability_quick() {
        let result = test_memory_stability(5).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_concurrency_quick() {
        let config = StressTestConfig {
            concurrent_sessions: 5,
            tokens_per_request: 10,
            total_requests: 50,
            timeout: Duration::from_secs(10),
        };

        let result = test_multi_session_concurrency(config).await;
        assert!(result.is_ok());

        let results = result.unwrap();
        assert!(results.successful_requests > 0);
    }
}
