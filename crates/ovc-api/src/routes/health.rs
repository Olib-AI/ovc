//! Health check endpoint.

use axum::Json;

use crate::models::HealthResponse;

/// Handler: `GET /api/v1/health`
pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
    })
}
