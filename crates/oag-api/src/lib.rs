pub mod auth;
pub mod dto;
pub mod error;
pub mod handlers;
pub mod rate_limit;
pub mod state;
#[cfg(test)]
mod tests;

pub use auth::{authenticate, authenticate_read};
pub use state::AppState;

use axum::middleware;
use axum::routing::{get, post};
use axum::Router;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::trace::TraceLayer;

/// Spec section 61 (Public-Network Abuse — "gigantic evidence payloads"):
/// reject oversized bodies before they're buffered/parsed at all, matching
/// the same limit `oag-sync`'s replication endpoints already enforce.
const MAX_REQUEST_BODY_BYTES: usize = 4 * 1024 * 1024;

/// Build the full `/api/v1` REST router (spec section 66, minus `/peers` —
/// no replication in this milestone). MCP (`oag-mcp`) is mounted separately
/// on the same outer Axum app by the caller, both sharing this `AppState`'s
/// `GraphService`.
pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/search", get(handlers::search))
        .route("/api/v1/nodes/{id}", get(handlers::get_node))
        .route("/api/v1/nodes/{id}/edges", get(handlers::get_node_edges))
        .route(
            "/api/v1/nodes/{id}/assertions",
            get(handlers::get_node_assertions),
        )
        .route("/api/v1/assertions/{id}", get(handlers::get_assertion))
        .route("/api/v1/assertions", post(handlers::create_assertion))
        .route(
            "/api/v1/assertions/{id}/evidence",
            post(handlers::add_evidence),
        )
        .route(
            "/api/v1/assertions/{id}/verify",
            post(handlers::verify_assertion),
        )
        .route(
            "/api/v1/assertions/{id}/dispute",
            post(handlers::dispute_assertion),
        )
        .route(
            "/api/v1/assertions/{id}/retract",
            post(handlers::retract_assertion),
        )
        .route("/api/v1/resolve", post(handlers::resolve))
        .route("/api/v1/subgraph", get(handlers::subgraph))
        .route("/api/v1/history/{object_type}/{id}", get(handlers::history))
        .route("/api/v1/status", get(handlers::status))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            rate_limit::rate_limit_middleware,
        ))
        .layer(TraceLayer::new_for_http())
        .layer(RequestBodyLimitLayer::new(MAX_REQUEST_BODY_BYTES))
        .with_state(state)
}
