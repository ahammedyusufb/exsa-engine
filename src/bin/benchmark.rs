//! Benchmark utility for Exsa-Engine
//!
//! Usage: cargo run --release --bin benchmark

use exsa_engine::utils::{BenchmarkTracker, MemorySnapshot};
use reqwest::Client;
use serde_json::json;
use std::time::{Duration, Instant};

#[tokio::main]
async fn main() {
    println!("=== Exsa-Engine Benchmark Utility ===\n");

    let server_url =
        std::env::var("SERVER_URL").unwrap_or_else(|_| "http://localhost:3000".to_string());

    println!("Server: {}", server_url);
    println!("Checking server health...\n");

    // Health check
    let client = Client::new();
    match client.get(format!("{}/v1/health", server_url)).send().await {
        Ok(resp) if resp.status().is_success() => {
            println!("✓ Server is healthy\n");
        }
        Ok(resp) => {
            eprintln!("✗ Server returned status: {}", resp.status());
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("✗ Failed to connect to server: {}", e);
            eprintln!("  Make sure the server is running at {}", server_url);
            std::process::exit(1);
        }
    }

    // Benchmark configuration
    let num_requests = std::env::var("BENCHMARK_REQUESTS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);

    let concurrent = std::env::var("BENCHMARK_CONCURRENT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);

    println!("Benchmark configuration:");
    println!("  Requests: {}", num_requests);
    println!("  Concurrency: {}", concurrent);
    println!();

    // Capture initial memory
    if let Some(mem) = MemorySnapshot::capture() {
        println!("Initial memory usage:");
        mem.display();
        println!();
    }

    println!("Starting benchmark...\n");

    let mut tracker = BenchmarkTracker::new();
    let start_time = Instant::now();

    // Run benchmark
    for batch in 0..(num_requests / concurrent) {
        let mut handles = vec![];

        for i in 0..concurrent {
            let client = client.clone();
            let url = format!("{}/v1/generate", server_url);
            let request_num = batch * concurrent + i + 1;

            let handle = tokio::spawn(async move {
                let request_start = Instant::now();

                let payload = json!({
                    "prompt": format!("Benchmark request {}: Explain quantum computing in one sentence.", request_num),
                    "sampling_params": {
                        "temperature": 0.7,
                        "max_tokens": 50
                    }
                });

                let mut token_count = 0;
                let mut first_token_time: Option<Instant> = None;

                match client.post(&url).json(&payload).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        // Read SSE stream
                        let body = resp.text().await.unwrap_or_default();
                        for line in body.lines() {
                            if line.starts_with("data:") {
                                if first_token_time.is_none() {
                                    first_token_time = Some(Instant::now());
                                }
                                token_count += 1;
                            }
                        }

                        let latency = request_start.elapsed();
                        let ttft = first_token_time
                            .map(|t| t.duration_since(request_start))
                            .unwrap_or(Duration::ZERO);

                        (Ok(()), token_count, latency, ttft)
                    }
                    Ok(resp) => {
                        eprintln!(
                            "Request {} failed with status: {}",
                            request_num,
                            resp.status()
                        );
                        (Err(()), 0, request_start.elapsed(), Duration::ZERO)
                    }
                    Err(e) => {
                        eprintln!("Request {} error: {}", request_num, e);
                        (Err(()), 0, request_start.elapsed(), Duration::ZERO)
                    }
                }
            });

            handles.push(handle);
        }

        // Wait for batch to complete
        for handle in handles {
            if let Ok((result, tokens, latency, _ttft)) = handle.await {
                if result.is_ok() {
                    for _ in 0..tokens {
                        tracker.record_token();
                    }
                    tracker.record_request(latency);
                }
            }
        }

        // Progress indicator
        let completed = (batch + 1) * concurrent;
        print!("\rProgress: {}/{} requests", completed, num_requests);
        std::io::Write::flush(&mut std::io::stdout()).ok();
    }

    println!("\n\nBenchmark completed!");
    println!("Total time: {:.2}s\n", start_time.elapsed().as_secs_f64());

    // Display results
    let results = tracker.finalize();
    results.display();

    // Capture final memory
    if let Some(mem) = MemorySnapshot::capture() {
        println!("\nFinal memory usage:");
        mem.display();
    }

    // Export results
    if let Err(e) = std::fs::write("benchmark_results.json", results.to_json()) {
        eprintln!("Failed to write results: {e}");
        std::process::exit(1);
    }
    println!("\n✓ Results exported to benchmark_results.json");
}
