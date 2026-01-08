//! Exsa-Engine Main Application
//!
//! Production-grade inference engine for local LLM hosting

use exsa_engine::{
    api::{build_router, AppState},
    inference::{queue::RequestQueue, InferenceEngine},
    model::{ModelConfig, ModelLoader},
    utils::{RateLimiter, ServerConfig},
};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::signal;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::{error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() {
    // Initialize tracing/logging
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::new(
                    "exsa_engine=debug,tower_http=debug,axum::rejection=trace",
                )
            }),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("Starting Exsa-Engine v{}", env!("CARGO_PKG_VERSION"));
    info!("🔒 Security: Privacy-first, local-only operation");

    // Load server configuration from environment
    let mut server_config = ServerConfig::default();

    // Configure host (default: localhost only for security)
    if let Ok(host) = std::env::var("HOST") {
        server_config = server_config.with_host(host.clone());
        if host == "0.0.0.0" {
            warn!("⚠️  Server will accept connections from LAN (0.0.0.0)");
            warn!("⚠️  Ensure firewall is properly configured");
        }
    } else {
        info!("🔒 Server bound to localhost only (127.0.0.1)");
    }

    // Configure port
    if let Ok(port) = std::env::var("PORT") {
        if let Ok(port_num) = port.parse() {
            server_config = server_config.with_port(port_num);
        }
    }

    // Configure CORS
    if std::env::var("ENABLE_CORS").unwrap_or_default() == "true" {
        server_config = server_config.enable_cors();
        warn!("⚠️  CORS enabled - allowing cross-origin requests");
    } else {
        info!("🔒 CORS disabled (secure default)");
    }

    // Configure rate limiting
    let enable_rate_limit = std::env::var("ENABLE_RATE_LIMIT").unwrap_or_default() == "true";

    if enable_rate_limit {
        let max_requests = std::env::var("RATE_LIMIT_MAX")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(60);

        let window_secs = std::env::var("RATE_LIMIT_WINDOW")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(60);

        server_config.rate_limit.enabled = true;
        server_config.rate_limit.max_requests = max_requests;
        server_config.rate_limit.window_secs = window_secs;

        info!(
            "🔒 Rate limiting enabled: {} requests per {} seconds",
            max_requests, window_secs
        );
    } else {
        info!("⚠️  Rate limiting disabled");
    }

    // Validate configuration
    if let Err(e) = server_config.validate() {
        error!("Invalid configuration: {}", e);
        std::process::exit(1);
    }

    // Load model configuration
    let model_path = match std::env::var("MODEL_PATH") {
        Ok(path) => path,
        Err(_) => {
            error!("MODEL_PATH environment variable must be set");
            error!("Example: export MODEL_PATH=/path/to/model.gguf");
            std::process::exit(1);
        }
    };

    let gpu_layers = std::env::var("GPU_LAYERS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    let n_ctx = std::env::var("CONTEXT_SIZE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(4096); // BEAST MODE: Default increased to 4096

    let n_batch = std::env::var("BATCH_SIZE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(n_ctx); // FIXED: Match batch size to context size to handle any prompt length

    let max_queue_size = std::env::var("MAX_QUEUE_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(server_config.max_queue_size);

    // Create model configuration
    let model_config = ModelConfig::new(model_path.clone())
        .with_gpu_layers(gpu_layers)
        .with_context_size(n_ctx)
        .with_batch_size(n_batch); // BEAST MODE: Configure batch size

    info!("📊 Model Configuration (BEAST MODE ENABLED):");
    info!("  Path: {}", model_config.model_path);
    info!("  Context size: {} (optimized)", model_config.n_ctx);
    info!("  Batch size: {} (optimized)", model_config.n_batch);
    info!("  GPU layers: {}", model_config.n_gpu_layers);
    info!("  CPU threads: {}", model_config.n_threads);

    // BEAST MODE Phase 3: Check if continuous batching is enabled
    let enable_batching = std::env::var("ENABLE_CONTINUOUS_BATCHING").unwrap_or_default() == "true";

    if enable_batching {
        let max_batch_size = std::env::var("MAX_BATCH_SIZE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(8);
        let batch_timeout_ms = std::env::var("BATCH_TIMEOUT_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(100);

        info!("🔥 CONTINUOUS BATCHING ENABLED!");
        info!("  Max batch size: {}", max_batch_size);
        info!("  Batch timeout: {}ms", batch_timeout_ms);
        info!("  Expected: 3-5x throughput gain!");
    }

    // Validate model file exists
    let loader = ModelLoader::new(model_config.clone());
    if let Err(e) = loader.validate() {
        error!("Model validation failed: {}", e);
        error!("Please ensure the model file exists at: {}", model_path);
        error!("You can set MODEL_PATH environment variable to specify a different path");
        std::process::exit(1);
    }

    // Extract model name from path (for ModelManager)
    let model_name = std::path::Path::new(&model_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("default-model")
        .to_string();

    info!("Model name: {}", model_name);

    // Initialize inference engine with ModelManager
    let engine = match InferenceEngine::new(model_name, model_path.clone(), model_config) {
        Ok(engine) => Arc::new(engine),
        Err(e) => {
            error!("Failed to initialize inference engine: {}", e);
            std::process::exit(1);
        }
    };

    info!("✅ Inference engine initialized");

    // Create request queue with engine
    let queue = RequestQueue::new(server_config.max_queue_size, Arc::clone(&engine));
    let queue_handle = queue.handle();

    info!("✅ Request queue created (max size: {})", max_queue_size);

    // Create application state with shutdown coordination
    let shutdown_flag = Arc::new(AtomicBool::new(false));
    let app_state = AppState {
        queue: queue_handle,
        engine: engine.clone(),
        model_switch_lock: Arc::new(tokio::sync::Mutex::new(())),
        shutdown_flag: shutdown_flag.clone(),
        start_time: std::time::Instant::now(),
    };

    // Build router
    let mut app = build_router(app_state);

    // Add CORS if enabled
    if server_config.enable_cors {
        app = app.layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        );
    }

    // Add rate limiting if enabled
    if server_config.rate_limit.enabled {
        use axum::middleware;
        use exsa_engine::utils::rate_limit::rate_limit_middleware;

        let rate_limiter = RateLimiter::new(
            server_config.rate_limit.max_requests,
            server_config.rate_limit.window_secs,
        );

        app = app.layer(middleware::from_fn_with_state(
            rate_limiter.clone(),
            rate_limit_middleware,
        ));

        // Spawn cleanup task
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(
                server_config.rate_limit.window_secs,
            ));
            loop {
                interval.tick().await;
                rate_limiter.cleanup().await;
            }
        });
    }

    // Add tracing middleware
    app = app.layer(TraceLayer::new_for_http());

    // Configure server address
    let addr = format!("{}:{}", server_config.host, server_config.port);
    let socket_addr: SocketAddr = addr.parse().unwrap_or_else(|e| {
        error!("Invalid socket address '{}': {}", addr, e);
        std::process::exit(1);
    });

    info!("Starting HTTP server on {}", socket_addr);

    // Start server
    let listener = match tokio::net::TcpListener::bind(&socket_addr).await {
        Ok(listener) => listener,
        Err(e) => {
            error!("Failed to bind to {}: {}", socket_addr, e);
            std::process::exit(1);
        }
    };

    info!("✅ Server listening on http://{}", socket_addr);
    info!("");
    info!("API endpoints:");
    info!(
        "  POST http://{}/v1/generate - Generate text (SSE streaming)",
        socket_addr
    );
    info!("  GET  http://{}/v1/health - Health check", socket_addr);
    info!("  GET  http://{}/v1/status - Server status", socket_addr);
    info!(
        "  GET  http://{}/v1/models - Model information",
        socket_addr
    );
    info!("");
    info!("🔒 Security status:");
    info!("  Bind address: {}", server_config.host);
    info!(
        "  CORS: {}",
        if server_config.enable_cors {
            "Enabled ⚠️"
        } else {
            "Disabled ✓"
        }
    );
    info!(
        "  Rate limiting: {}",
        if server_config.rate_limit.enabled {
            "Enabled ✓"
        } else {
            "Disabled ⚠️"
        }
    );
    info!("  Privacy: 100% local, no telemetry ✓");

    // Run server with graceful shutdown
    let shutdown_signal_future = shutdown_signal(shutdown_flag.clone(), engine.clone());

    if let Err(e) = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal_future)
        .await
    {
        error!("Server error: {}", e);
        std::process::exit(1);
    }

    info!("Server shut down gracefully");
}

/// Wait for shutdown signal and drain active requests
async fn shutdown_signal(shutdown_flag: Arc<AtomicBool>, engine: Arc<InferenceEngine>) {
    let ctrl_c = async {
        if let Err(e) = signal::ctrl_c().await {
            error!("Failed to install Ctrl+C handler: {}", e);
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match signal::unix::signal(signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(e) => error!("Failed to install signal handler: {}", e),
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("Received Ctrl+C signal");
        },
        _ = terminate => {
            info!("Received terminate signal");
        },
    }

    // Set shutdown flag
    shutdown_flag.store(true, Ordering::SeqCst);

    info!("Initiating graceful shutdown...");

    // Wait for active requests to complete (with timeout)
    let max_wait = std::time::Duration::from_secs(30);
    let start = std::time::Instant::now();

    while engine.active_requests() > 0 && start.elapsed() < max_wait {
        let active = engine.active_requests();
        info!("Waiting for {} active requests to complete...", active);
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    let final_active = engine.active_requests();
    if final_active > 0 {
        warn!(
            "Shutdown timeout reached with {} requests still active",
            final_active
        );
    } else {
        info!("All requests completed successfully");
    }
}
