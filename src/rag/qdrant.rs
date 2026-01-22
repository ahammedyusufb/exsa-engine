use crate::utils::error::{ExsaError, Result};
use reqwest::Client;
use serde_json::json;
use std::time::Duration;
use tokio::sync::OnceCell;
use uuid::Uuid;

#[derive(Clone)]
pub struct QdrantStore {
    http: Client,
    base_url: String,
    collection: String,
    vector_size: std::sync::Arc<OnceCell<u64>>,
    ensured: std::sync::Arc<OnceCell<()>>,
}

impl QdrantStore {
    pub fn new(base_url: &str, collection: String, timeout: Duration) -> Result<Self> {
        let http = Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| ExsaError::InternalError(format!("Failed to build HTTP client: {e}")))?;
        Ok(Self {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
            collection,
            vector_size: std::sync::Arc::new(OnceCell::new()),
            ensured: std::sync::Arc::new(OnceCell::new()),
        })
    }

    pub async fn ensure_collection(&self, vector_size: u64) -> Result<()> {
        // Lock in the vector size on first use (and validate on subsequent calls).
        if let Some(existing) = self.vector_size.get() {
            if *existing != vector_size {
                return Err(ExsaError::InvalidParameters(format!(
                    "Qdrant vector size mismatch: existing={}, requested={}",
                    existing, vector_size
                )));
            }
        } else {
            let _ = self.vector_size.set(vector_size);
        }

        let size = *self.vector_size.get().unwrap_or(&vector_size);

        self.ensured
            .get_or_try_init(|| async move {
                let url = format!("{}/collections/{}", self.base_url, self.collection);
                let resp = self.http.get(&url).send().await.map_err(|e| {
                    ExsaError::InternalError(format!("Qdrant GET collection failed: {e}"))
                })?;

                if resp.status().is_success() {
                    return Ok(());
                }

                // Create collection
                let body = json!({
                    "vectors": {
                        "size": size,
                        "distance": "Cosine"
                    }
                });

                let resp = self.http.put(&url).json(&body).send().await.map_err(|e| {
                    ExsaError::InternalError(format!("Qdrant create collection failed: {e}"))
                })?;

                if !resp.status().is_success() {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    return Err(ExsaError::InternalError(format!(
                        "Qdrant create collection error ({status}): {text}"
                    )));
                }

                Ok(())
            })
            .await
            .map(|_| ())
    }

    pub async fn upsert_chunk_vectors(
        &self,
        points: Vec<(Uuid, Vec<f32>, serde_json::Value)>,
    ) -> Result<()> {
        if points.is_empty() {
            return Ok(());
        }

        let url = format!(
            "{}/collections/{}/points?wait=true",
            self.base_url, self.collection
        );

        let body = json!({
            "points": points.into_iter().map(|(id, vector, payload)| json!({
                "id": id.to_string(),
                "vector": vector,
                "payload": payload,
            })).collect::<Vec<_>>()
        });

        let resp = self
            .http
            .put(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| ExsaError::InternalError(format!("Qdrant upsert failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(ExsaError::InternalError(format!(
                "Qdrant upsert error ({status}): {text}"
            )));
        }

        Ok(())
    }

    pub async fn delete_document_points(&self, document_id: Uuid) -> Result<()> {
        let url = format!(
            "{}/collections/{}/points/delete?wait=true",
            self.base_url, self.collection
        );

        let body = json!({
            "filter": {
                "must": [
                    {"key": "document_id", "match": {"value": document_id.to_string()}}
                ]
            }
        });

        let resp = self
            .http
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| ExsaError::InternalError(format!("Qdrant delete failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();

            // If the collection doesn't exist yet, treat deletion as a no-op.
            // This can happen on fresh setups before the first ingest creates the collection.
            if status.as_u16() == 404 && text.contains("doesn't exist") {
                return Ok(());
            }

            return Err(ExsaError::InternalError(format!(
                "Qdrant delete error ({status}): {text}"
            )));
        }

        Ok(())
    }

    pub async fn search(&self, vector: Vec<f32>, kb: &str, top_k: u64) -> Result<Vec<(Uuid, f32)>> {
        let url = format!(
            "{}/collections/{}/points/search",
            self.base_url, self.collection
        );

        let body = json!({
            "vector": vector,
            "limit": top_k,
            "filter": {
                "must": [
                    {"key": "kb", "match": {"value": kb}}
                ]
            },
            "with_payload": false,
            "with_vector": false
        });

        let resp = self
            .http
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| ExsaError::InternalError(format!("Qdrant search failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(ExsaError::InternalError(format!(
                "Qdrant search error ({status}): {text}"
            )));
        }

        let parsed: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ExsaError::InternalError(format!("Qdrant search decode failed: {e}")))?;

        let mut out = Vec::new();
        if let Some(arr) = parsed.get("result").and_then(|v| v.as_array()) {
            for item in arr {
                let id_str = item.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let score = item.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                if let Ok(id) = Uuid::parse_str(id_str) {
                    out.push((id, score));
                }
            }
        }

        Ok(out)
    }
}
