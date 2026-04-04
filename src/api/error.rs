use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;

/// API-layer error type
#[derive(Debug)]
pub enum ApiError {
    /// 400 - Bad request (invalid input)
    BadRequest(String),

    /// 500 - Internal error
    Internal(String),

    /// 503 - Service unavailable (e.g., no data ingested yet)
    Unavailable(String),
}

/// JSON error response body
#[derive(Serialize)]
struct ErrorBody {
    error: String,
    message: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, error_type, message) = match self {
            ApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, "bad_request", msg),
            ApiError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error", msg),
            ApiError::Unavailable(msg) => (StatusCode::SERVICE_UNAVAILABLE, "unavailable", msg),
        };

        let body = ErrorBody {
            error: error_type.into(),
            message,
        };

        (status, Json(body)).into_response()
    }
}

// Convert engine errors to API errors
impl From<crate::engine::EngineError> for ApiError {
    fn from(err: crate::engine::EngineError) -> Self {
        use crate::engine::EngineError;
        match err {
            EngineError::Store(e) => {
                // Table not found usually means no data ingested
                if e.to_string().contains("not found") {
                    ApiError::Unavailable(
                        "No data ingested yet. Run code-raptor to ingest first.".into(),
                    )
                } else {
                    ApiError::Internal(e.to_string())
                }
            }
            EngineError::Embedding(e) => ApiError::Internal(e.to_string()),
            EngineError::Generation(e) => ApiError::Internal(format!("LLM error: {}", e)),
            EngineError::Rerank(e) => ApiError::Internal(format!("Reranking error: {}", e)),
        }
    }
}

// Convert embed errors
impl From<code_rag_store::EmbedError> for ApiError {
    fn from(err: code_rag_store::EmbedError) -> Self {
        ApiError::Internal(format!("Embedding error: {}", err))
    }
}

// Convert store errors
impl From<crate::store::StoreError> for ApiError {
    fn from(err: crate::store::StoreError) -> Self {
        if err.to_string().contains("not found") {
            ApiError::Unavailable("No data ingested yet. Run code-raptor to ingest first.".into())
        } else {
            ApiError::Internal(err.to_string())
        }
    }
}
