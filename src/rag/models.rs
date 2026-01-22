use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct RagDocument {
    pub id: Uuid,
    pub kb: String,
    pub title: String,
    pub source_name: String,
    pub sha256: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct RagChunk {
    pub id: Uuid,
    pub document_id: Uuid,
    pub kb: String,
    pub chunk_index: i32,
    pub content: String,
    pub sha256: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RagSearchRequest {
    pub query: String,
    #[serde(default)]
    pub kb: Option<String>,
    #[serde(default)]
    pub top_k: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RagSearchResult {
    pub chunk_id: Uuid,
    pub document_id: Uuid,
    pub title: String,
    pub source_name: String,
    pub score: f32,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RagIngestResponse {
    pub document_id: Uuid,
    pub chunks_indexed: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RagStatusResponse {
    pub enabled: bool,
    pub default_kb: String,
    pub qdrant_collection: String,
}

/// Optional per-chat RAG options sent in /v1/chat/completions.
#[derive(Debug, Clone, Deserialize)]
pub struct RagChatOptions {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub kb: Option<String>,
    #[serde(default)]
    pub top_k: Option<usize>,
}
