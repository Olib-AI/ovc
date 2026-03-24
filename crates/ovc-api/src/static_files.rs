//! Embedded frontend static file handler.
//!
//! Serves the compiled frontend SPA from files embedded at compile time
//! via `rust-embed`. Falls back to `index.html` for client-side routing.

use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "../../frontend/dist/"]
struct FrontendAssets;

/// Serves embedded frontend files, falling back to `index.html` for SPA routing.
pub async fn serve_frontend(uri: axum::http::Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    // Try the exact path first.
    if let Some(file) = FrontendAssets::get(path) {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        // Hashed assets (in assets/) are immutable — cache forever.
        // Other files (index.html) must not be cached to pick up new deploys.
        let cache = if path.starts_with("assets/") {
            "public, max-age=31536000, immutable"
        } else {
            "no-cache, no-store, must-revalidate"
        };
        return (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, mime.as_ref().to_owned()),
                (header::CACHE_CONTROL, cache.to_owned()),
            ],
            file.data.to_vec(),
        )
            .into_response();
    }

    // Fallback to index.html for SPA client-side routing.
    if let Some(index) = FrontendAssets::get("index.html") {
        return (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "text/html".to_owned()),
                (
                    header::CACHE_CONTROL,
                    "no-cache, no-store, must-revalidate".to_owned(),
                ),
            ],
            index.data.to_vec(),
        )
            .into_response();
    }

    StatusCode::NOT_FOUND.into_response()
}
