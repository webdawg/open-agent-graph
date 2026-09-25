use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::Json;
use oag_core::{AssertionId, EdgeId, NodeId};
use serde_json::json;

use crate::dto::{
    now_or, AssertRequest, DisputeRequest, EvidenceRequest, ResolveRequest, RetractRequest,
    SearchQuery, SubgraphQuery, VerifyRequest,
};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::{authenticate, authenticate_read};

pub async fn search(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<SearchQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    authenticate_read(&headers, &state.graph).await?;
    let results = state.graph.search(&q.q, q.limit.unwrap_or(20)).await?;
    Ok(Json(json!({ "results": results })))
}

pub async fn get_node(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    authenticate_read(&headers, &state.graph).await?;
    let node_id = parse_node_id(&id)?;
    let node = state
        .graph
        .get_node(node_id)
        .await?
        .ok_or_else(|| ApiError(oag_graph::GraphError::NotFound(format!("node {id}"))))?;
    let aliases = state.graph.list_aliases(node_id).await?;
    Ok(Json(json!({ "node": node, "aliases": aliases })))
}

pub async fn get_node_edges(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    authenticate_read(&headers, &state.graph).await?;
    let node_id = parse_node_id(&id)?;
    let edges = state.graph.get_edges(node_id).await?;
    Ok(Json(json!({ "edges": edges })))
}

pub async fn get_node_assertions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    authenticate_read(&headers, &state.graph).await?;
    let node_id = parse_node_id(&id)?;
    let assertions = state.graph.list_assertions_for_node(node_id).await?;
    Ok(Json(json!({ "assertions": assertions })))
}

pub async fn get_assertion(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    authenticate_read(&headers, &state.graph).await?;
    let assertion_id = parse_assertion_id(&id)?;
    let assertion = state
        .graph
        .get_assertion(assertion_id)
        .await?
        .ok_or_else(|| ApiError(oag_graph::GraphError::NotFound(format!("assertion {id}"))))?;
    let evidence = state.graph.list_evidence(assertion_id).await?;
    let observations = state.graph.list_observations(assertion_id).await?;
    let disputes = state.graph.list_disputes(assertion_id).await?;
    Ok(Json(json!({
        "assertion": assertion,
        "evidence": evidence,
        "observations": observations,
        "disputes": disputes,
    })))
}

pub async fn get_edge_corroboration(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<oag_graph::EdgeCorroboration>> {
    authenticate_read(&headers, &state.graph).await?;
    let edge_id = parse_edge_id(&id)?;
    let corroboration = state.graph.get_edge_corroboration(edge_id).await?;
    Ok(Json(corroboration))
}

pub async fn create_assertion(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<AssertRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let auth = authenticate(&headers, &state.graph).await?;
    let input = body.into_input()?;
    let assertion_id = state.graph.assert(&auth, input).await?;
    Ok(Json(json!({
        "assertion_id": assertion_id.to_hex(),
        "status": "accepted",
    })))
}

pub async fn add_evidence(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<EvidenceRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let auth = authenticate(&headers, &state.graph).await?;
    let assertion_id = parse_assertion_id(&id)?;
    state.graph.add_evidence(&auth, assertion_id, body.into_input()?).await?;
    Ok(Json(json!({ "status": "accepted" })))
}

pub async fn verify_assertion(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<VerifyRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let auth = authenticate(&headers, &state.graph).await?;
    let assertion_id = parse_assertion_id(&id)?;
    let observed_at = now_or(body.observed_at)?;
    let result = oag_core::VerifyResult::parse(&body.result).ok_or_else(|| {
        ApiError(oag_graph::GraphError::InvalidInput(format!(
            "unknown verify result '{}', expected one of confirmed/not_confirmed/changed/contradicted/unreachable/unknown",
            body.result
        )))
    })?;
    state
        .graph
        .verify_assertion(&auth, assertion_id, result, observed_at)
        .await?;
    Ok(Json(json!({ "status": "accepted" })))
}

pub async fn dispute_assertion(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<DisputeRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let auth = authenticate(&headers, &state.graph).await?;
    let assertion_id = parse_assertion_id(&id)?;
    state.graph.dispute_assertion(&auth, assertion_id, body.reason).await?;
    Ok(Json(json!({ "status": "accepted" })))
}

pub async fn retract_assertion(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<RetractRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let auth = authenticate(&headers, &state.graph).await?;
    let assertion_id = parse_assertion_id(&id)?;
    state.graph.retract_assertion(&auth, assertion_id, body.reason).await?;
    Ok(Json(json!({ "status": "accepted" })))
}

pub async fn resolve(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ResolveRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    authenticate_read(&headers, &state.graph).await?;
    let outcome = state.graph.resolve(&body.value).await?;
    Ok(Json(resolve_outcome_to_json(outcome)))
}

fn resolve_outcome_to_json(outcome: oag_graph::ResolveOutcome) -> serde_json::Value {
    match outcome {
        oag_graph::ResolveOutcome::Found { node, confidence } => json!({
            "node_id": node.id.to_hex(),
            "canonical_uri": node.canonical_uri,
            "canonical_identifier": node.canonical_identifier,
            "type": node.node_type.as_str(),
            "confidence": confidence,
        }),
        oag_graph::ResolveOutcome::Candidates(nodes) => json!({ "candidates": nodes }),
        oag_graph::ResolveOutcome::NotFound => json!({ "candidates": [] }),
    }
}

pub async fn subgraph(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<SubgraphQuery>,
) -> ApiResult<Json<oag_graph::Subgraph>> {
    authenticate_read(&headers, &state.graph).await?;
    let node_id = parse_node_id(&q.node)?;
    let sg = state
        .graph
        .get_subgraph(node_id, q.depth.unwrap_or(1), q.max_nodes.unwrap_or(200))
        .await?;
    Ok(Json(sg))
}

pub async fn history(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((object_type, id)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    authenticate_read(&headers, &state.graph).await?;
    let entries = state.graph.get_history(&object_type, &id).await?;
    Ok(Json(json!({ "history": entries })))
}

pub async fn status(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(json!({
        "peer_id": state.graph.identity().peer_id().to_string(),
        "status": "running",
        "replication": null,
    }))
}

fn parse_node_id(s: &str) -> Result<NodeId, ApiError> {
    s.parse()
        .map_err(|_| ApiError(oag_graph::GraphError::InvalidInput(format!("invalid node id '{s}'"))))
}

fn parse_assertion_id(s: &str) -> Result<AssertionId, ApiError> {
    s.parse()
        .map_err(|_| ApiError(oag_graph::GraphError::InvalidInput(format!("invalid assertion id '{s}'"))))
}

fn parse_edge_id(s: &str) -> Result<EdgeId, ApiError> {
    s.parse()
        .map_err(|_| ApiError(oag_graph::GraphError::InvalidInput(format!("invalid edge id '{s}'"))))
}
