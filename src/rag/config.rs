use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RagConfig {
    /// Master switch.
    pub enabled: bool,

    /// Postgres connection string, e.g. postgres://user:pass@localhost:5432/exsa
    pub postgres_url: Option<String>,

    /// Qdrant base URL, e.g. http://localhost:6334 (gRPC) or http://localhost:6333 (REST).
    /// This implementation uses the Qdrant gRPC endpoint.
    pub qdrant_url: Option<String>,

    /// Qdrant collection name for chunk vectors.
    pub qdrant_collection: String,

    /// OpenAI-compatible embeddings endpoint URL, e.g. http://localhost:8081/v1/embeddings.
    pub embeddings_url: Option<String>,

    /// Embeddings model identifier sent to the embeddings endpoint (optional).
    pub embeddings_model: Option<String>,

    /// Default knowledgebase/collection name used for documents.
    pub default_kb: String,

    /// Chunking parameters.
    pub chunk_max_chars: usize,
    pub chunk_overlap_chars: usize,

    /// Retrieval parameters.
    pub retrieve_top_k: usize,
    pub max_context_chars: usize,

    /// Enables vector (embeddings + Qdrant) retrieval.
    ///
    /// If disabled, RAG falls back to a Postgres lexical search over chunk text.
    /// This avoids calling the embeddings endpoint during chat, which can be
    /// unstable on some platforms/backends.
    pub vector_search_enabled: bool,

    /// Timeout (seconds) for RAG initialization (Postgres connect + schema init).
    pub init_timeout_secs: u64,

    /// Postgres connection timeout (seconds).
    pub postgres_connect_timeout_secs: u64,

    /// Postgres acquire timeout (seconds) when pulling a connection from the pool.
    pub postgres_acquire_timeout_secs: u64,

    /// HTTP request timeout (seconds) for Qdrant + embeddings calls.
    pub http_timeout_secs: u64,
}

impl Default for RagConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            postgres_url: None,
            qdrant_url: None,
            qdrant_collection: "exsa_rag_chunks".to_string(),
            embeddings_url: None,
            embeddings_model: None,
            default_kb: "default".to_string(),
            chunk_max_chars: 1400,
            chunk_overlap_chars: 200,
            retrieve_top_k: 6,
            max_context_chars: 8000,

            vector_search_enabled: true,

            // Timeouts (safe defaults to prevent hangs)
            init_timeout_secs: 15,
            postgres_connect_timeout_secs: 5,
            postgres_acquire_timeout_secs: 5,
            http_timeout_secs: 10,
        }
    }
}

impl RagConfig {
    /// Loads config from environment variables.
    ///
    /// - EXSA_RAG_ENABLED=true|false
    /// - EXSA_RAG_POSTGRES_URL=...
    /// - EXSA_RAG_QDRANT_URL=...
    /// - EXSA_RAG_QDRANT_COLLECTION=...
    /// - EXSA_RAG_EMBEDDINGS_URL=...
    /// - EXSA_RAG_EMBEDDINGS_MODEL=...
    /// - EXSA_RAG_DEFAULT_KB=...
    /// - EXSA_RAG_CHUNK_MAX_CHARS=...
    /// - EXSA_RAG_CHUNK_OVERLAP_CHARS=...
    /// - EXSA_RAG_RETRIEVE_TOP_K=...
    /// - EXSA_RAG_MAX_CONTEXT_CHARS=...
    /// - EXSA_RAG_VECTOR_SEARCH_ENABLED=true|false
    /// - EXSA_RAG_INIT_TIMEOUT_SECS=...
    /// - EXSA_RAG_PG_CONNECT_TIMEOUT_SECS=...
    /// - EXSA_RAG_PG_ACQUIRE_TIMEOUT_SECS=...
    /// - EXSA_RAG_HTTP_TIMEOUT_SECS=...
    pub fn from_env() -> Self {
        let defaults = RagConfig::default();

        let vector_search_enabled_env = std::env::var("EXSA_RAG_VECTOR_SEARCH_ENABLED").ok();
        let mut vector_search_enabled = vector_search_enabled_env
            .as_deref()
            .map(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "yes" | "on"))
            .unwrap_or(defaults.vector_search_enabled);

        let qdrant_collection = std::env::var("EXSA_RAG_QDRANT_COLLECTION")
            .ok()
            .and_then(|v| (!v.trim().is_empty()).then_some(v))
            .unwrap_or_else(|| defaults.qdrant_collection.clone());

        let default_kb = std::env::var("EXSA_RAG_DEFAULT_KB")
            .ok()
            .and_then(|v| (!v.trim().is_empty()).then_some(v))
            .unwrap_or_else(|| defaults.default_kb.clone());

        let chunk_max_chars = std::env::var("EXSA_RAG_CHUNK_MAX_CHARS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(defaults.chunk_max_chars);

        let chunk_overlap_chars = std::env::var("EXSA_RAG_CHUNK_OVERLAP_CHARS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(defaults.chunk_overlap_chars);

        let retrieve_top_k = std::env::var("EXSA_RAG_RETRIEVE_TOP_K")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(defaults.retrieve_top_k);

        let max_context_chars = std::env::var("EXSA_RAG_MAX_CONTEXT_CHARS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(defaults.max_context_chars);

        let init_timeout_secs = std::env::var("EXSA_RAG_INIT_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(defaults.init_timeout_secs);

        let postgres_connect_timeout_secs = std::env::var("EXSA_RAG_PG_CONNECT_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(defaults.postgres_connect_timeout_secs);

        let postgres_acquire_timeout_secs = std::env::var("EXSA_RAG_PG_ACQUIRE_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(defaults.postgres_acquire_timeout_secs);

        let http_timeout_secs = std::env::var("EXSA_RAG_HTTP_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(defaults.http_timeout_secs);

        // Safety default: on macOS, using in-process /v1/embeddings (same engine)
        // can trigger hard crashes in the Metal backend when creating a second
        // llama.cpp context. If the user did not explicitly set the toggle and
        // embeddings points at the default local engine port, prefer lexical mode.
        if cfg!(target_os = "macos") && vector_search_enabled_env.is_none() {
            if let Ok(url) = std::env::var("EXSA_RAG_EMBEDDINGS_URL") {
                let u = url.to_lowercase();
                let looks_like_self = (u.contains("127.0.0.1:8080")
                    || u.contains("localhost:8080"))
                    && u.contains("/v1/embeddings");
                if looks_like_self {
                    vector_search_enabled = false;
                }
            }
        }

        RagConfig {
            enabled: std::env::var("EXSA_RAG_ENABLED")
                .ok()
                .map(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "yes" | "on"))
                .unwrap_or(false),
            postgres_url: std::env::var("EXSA_RAG_POSTGRES_URL").ok(),
            qdrant_url: std::env::var("EXSA_RAG_QDRANT_URL").ok(),
            qdrant_collection,
            embeddings_url: std::env::var("EXSA_RAG_EMBEDDINGS_URL").ok(),
            embeddings_model: std::env::var("EXSA_RAG_EMBEDDINGS_MODEL").ok(),
            default_kb,
            chunk_max_chars,
            chunk_overlap_chars,
            retrieve_top_k,
            max_context_chars,

            vector_search_enabled,

            init_timeout_secs,
            postgres_connect_timeout_secs,
            postgres_acquire_timeout_secs,
            http_timeout_secs,
        }
    }
}
