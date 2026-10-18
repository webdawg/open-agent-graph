//! REST surface for spec/17-redaction-and-suppression.md (spec section 85).
//! `GraphService::redact_evidence`/`list_redactions`/`list_suppressed_nodes`/
//! `suppress_node_from_search`/`unsuppress_node_from_search` already took a
//! real `AuthContext` and checked `Permission::Admin` internally from the
//! start -- this file adds no new business logic, only handlers that produce
//! a real authenticated `AuthContext` from a bearer key the way every other
//! REST handler does, closing the "no REST/MCP surface" gap tracked in
//! `OPEN_QUESTIONS.md`. `oag redact *` (CLI) remains available unchanged,
//! using its own synthetic local-operator `AuthContext`.

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::Json;
use serde_json::json;

use crate::dto::RedactEvidenceRequest;
use crate::error::ApiResult;
use crate::handlers::parse_node_id;
use crate::authenticate;
use crate::state::AppState;

fn parse_event_id(s: &str) -> Result<oag_core::EventId, crate::error::ApiError> {
    s.parse().map_err(|_| {
        crate::error::ApiError(oag_graph::GraphError::InvalidInput(format!("invalid evidence id '{s}'")))
    })
}

pub async fn redact_evidence(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<RedactEvidenceRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let auth = authenticate(&headers, &state.graph).await?;
    let evidence_id = parse_event_id(&id)?;
    state.graph.redact_evidence(&auth, evidence_id, body.reason).await?;
    Ok(Json(json!({ "status": "accepted" })))
}

/// Combined view matching `oag redact list`'s existing JSON shape exactly.
pub async fn list_redactions(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    let auth = authenticate(&headers, &state.graph).await?;
    let redactions = state.graph.list_redactions(&auth).await?;
    let suppressed_nodes = state.graph.list_suppressed_nodes(&auth).await?;
    Ok(Json(json!({ "redactions": redactions, "suppressed_nodes": suppressed_nodes })))
}

pub async fn suppress_node(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let auth = authenticate(&headers, &state.graph).await?;
    let node_id = parse_node_id(&id)?;
    state.graph.suppress_node_from_search(&auth, node_id).await?;
    Ok(Json(json!({ "status": "accepted" })))
}

pub async fn unsuppress_node(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let auth = authenticate(&headers, &state.graph).await?;
    let node_id = parse_node_id(&id)?;
    state.graph.unsuppress_node_from_search(&auth, node_id).await?;
    Ok(Json(json!({ "status": "accepted" })))
}
