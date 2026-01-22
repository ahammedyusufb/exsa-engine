use crate::api::schema::AppState;
use crate::rag::models::{RagIngestResponse, RagSearchRequest, RagStatusResponse};
use crate::utils::error::{ExsaError, Result};
use axum::{
    extract::{Multipart, Path, Query, State},
    Json,
};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct ListDocsQuery {
    pub kb: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct IngestQuery {
    pub kb: Option<String>,
    pub title: Option<String>,
}

pub async fn rag_status(State(state): State<AppState>) -> Json<RagStatusResponse> {
    if let Some(rag) = &state.rag {
        return Json(RagStatusResponse {
            enabled: true,
            default_kb: rag.cfg().default_kb.clone(),
            qdrant_collection: rag.cfg().qdrant_collection.clone(),
        });
    }

    Json(RagStatusResponse {
        enabled: false,
        default_kb: "default".to_string(),
        qdrant_collection: "exsa_rag_chunks".to_string(),
    })
}

pub async fn list_documents(
    State(state): State<AppState>,
    Query(q): Query<ListDocsQuery>,
) -> Result<Json<serde_json::Value>> {
    let Some(rag) = &state.rag else {
        return Err(ExsaError::ServiceUnavailable(
            "RAG is not enabled".to_string(),
        ));
    };

    let kb = q.kb.unwrap_or_else(|| rag.cfg().default_kb.clone());
    let limit = q.limit.unwrap_or(200).clamp(1, 1000);

    let docs = rag.list_documents(&kb, limit).await?;
    Ok(Json(serde_json::json!({ "kb": kb, "documents": docs })))
}

pub async fn delete_document(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let Some(rag) = &state.rag else {
        return Err(ExsaError::ServiceUnavailable(
            "RAG is not enabled".to_string(),
        ));
    };

    let document_id = Uuid::parse_str(&id)
        .map_err(|_| ExsaError::InvalidParameters("Invalid document id".to_string()))?;

    rag.delete_document(document_id).await?;
    Ok(Json(serde_json::json!({ "success": true })))
}

pub async fn rag_search(
    State(state): State<AppState>,
    Json(req): Json<RagSearchRequest>,
) -> Result<Json<serde_json::Value>> {
    let Some(rag) = &state.rag else {
        return Err(ExsaError::ServiceUnavailable(
            "RAG is not enabled".to_string(),
        ));
    };

    let kb = req.kb.unwrap_or_else(|| rag.cfg().default_kb.clone());
    let top_k = req.top_k.unwrap_or(rag.cfg().retrieve_top_k).clamp(1, 20);

    let results = rag.search(&kb, &req.query, top_k).await?;
    Ok(Json(serde_json::json!({ "kb": kb, "results": results })))
}

/// Ingest a document into RAG.
///
/// Multipart fields supported:
/// - file: uploaded file (text/plain or markdown). For binary types, the bytes are stored as UTF-8 lossily.
/// - text: raw text content
/// - source_name: optional display name
pub async fn ingest_document_multipart(
    State(state): State<AppState>,
    Query(q): Query<IngestQuery>,
    mut multipart: Multipart,
) -> Result<Json<RagIngestResponse>> {
    let Some(rag) = &state.rag else {
        return Err(ExsaError::ServiceUnavailable(
            "RAG is not enabled".to_string(),
        ));
    };

    let kb = q.kb.clone().unwrap_or_else(|| rag.cfg().default_kb.clone());
    let title_provided = q.title.is_some();
    let mut title = q.title.unwrap_or_else(|| "Untitled".to_string());
    let mut source_name: Option<String> = None;
    let mut text: Option<String> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ExsaError::InvalidParameters(format!("Invalid multipart: {e}")))?
    {
        let name = field.name().unwrap_or("").to_string();

        match name.as_str() {
            "file" => {
                let file_name = field.file_name().map(|s| s.to_string());
                if let Some(f) = file_name.clone() {
                    // If user did not provide a title explicitly, use filename
                    if !title_provided {
                        title = f.clone();
                    }
                    if source_name.is_none() {
                        source_name = Some(f);
                    }
                }

                let bytes = field
                    .bytes()
                    .await
                    .map_err(|e| ExsaError::InvalidParameters(format!("File read failed: {e}")))?;

                let content = String::from_utf8_lossy(&bytes).to_string();
                text = Some(content);
            }
            "text" => {
                text =
                    Some(field.text().await.map_err(|e| {
                        ExsaError::InvalidParameters(format!("Text read failed: {e}"))
                    })?);
            }
            "source_name" => {
                source_name = Some(field.text().await.map_err(|e| {
                    ExsaError::InvalidParameters(format!("source_name read failed: {e}"))
                })?);
            }
            _ => {}
        }
    }

    let text =
        text.ok_or_else(|| ExsaError::InvalidParameters("No document text provided".to_string()))?;
    let source_name = source_name.unwrap_or_else(|| title.clone());

    let resp = rag.ingest_text(&kb, &title, &source_name, &text).await?;

    Ok(Json(resp))
}
