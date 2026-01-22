//! Axum server: router, handlers, and application state.

use crate::auth::TokenManager;
use crate::error::Error;
use crate::proxy::{forward_response, ProxyClient};
use axum::body::Bytes;
use axum::extract::{OriginalUri, Path, State};
use axum::http::{HeaderMap, Method};
use axum::response::Response;
use axum::routing::any;
use axum::Router;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    proxy: Arc<ProxyClient>,
}

impl AppState {
    pub async fn new() -> Result<Self, Error> {
        let token = crate::config::load_github_token()?;
        let manager = Arc::new(TokenManager::new(token).await?);
        let proxy = Arc::new(ProxyClient::new(manager)?);
        Ok(Self { proxy })
    }
}

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/v1/{*path}", any(proxy_handler))
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .layer(tower_http::limit::RequestBodyLimitLayer::new(10 * 1024 * 1024))
        .with_state(state)
}

async fn proxy_handler(
    State(state): State<AppState>,
    method: Method,
    Path(path): Path<String>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, Error> {
    let content_type = headers.get("content-type").and_then(|v| v.to_str().ok());
    let query = uri.query().map(|q| format!("?{}", q)).unwrap_or_default();

    let resp = state.proxy.forward(&format!("/{}{}", path, query), method, body, content_type).await?;
    forward_response(resp).await
}
