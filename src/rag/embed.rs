use crate::utils::error::{ExsaError, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Clone)]
pub struct EmbeddingsClient {
    http: Client,
    url: String,
    model: Option<String>,
}

#[derive(Debug, Serialize)]
struct OpenAIEmbeddingsRequest<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<&'a str>,
    input: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAIEmbeddingsResponse {
    data: Vec<OpenAIEmbeddingItem>,
}

#[derive(Debug, Deserialize)]
struct OpenAIEmbeddingItem {
    embedding: Vec<f32>,
}

impl EmbeddingsClient {
    pub fn new(url: String, model: Option<String>, timeout: Duration) -> Result<Self> {
        let http = Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| ExsaError::InternalError(format!("Failed to build HTTP client: {e}")))?;
        Ok(Self { http, url, model })
    }

    pub async fn embed_one(&self, input: &str) -> Result<Vec<f32>> {
        let mut all = self.embed_batch(&[input.to_string()]).await?;
        all.pop().ok_or_else(|| {
            ExsaError::InternalError("Embeddings endpoint returned empty result".to_string())
        })
    }

    pub async fn embed_batch(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>> {
        if inputs.is_empty() {
            return Ok(vec![]);
        }

        let req = OpenAIEmbeddingsRequest {
            model: self.model.as_deref(),
            input: inputs.to_vec(),
        };

        let resp = self
            .http
            .post(&self.url)
            .header("Content-Type", "application/json")
            .json(&req)
            .send()
            .await
            .map_err(|e| ExsaError::InternalError(format!("Embeddings request failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(ExsaError::InternalError(format!(
                "Embeddings endpoint error ({status}): {text}"
            )));
        }

        let parsed: OpenAIEmbeddingsResponse = resp
            .json()
            .await
            .map_err(|e| ExsaError::InternalError(format!("Embeddings decode failed: {e}")))?;

        Ok(parsed.data.into_iter().map(|d| d.embedding).collect())
    }
}
