use std::sync::Arc;

use oag_core::Permission;
use oag_graph::{AssertInput, AuthContext, EvidenceInput, GraphError, GraphService};
use rmcp::handler::server::tool::Extension;
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::model::{ServerCapabilities, ServerConfig};
use rmcp::{tool, tool_router, ErrorData, ServerHandler};
use serde_json::json;

use crate::params::{
    AddEvidenceParams, AssertParams, DisputeParams, EvidenceParam, HistoryParams, NodeIdParams,
    ResolveParams, RetractParams, SearchParams, SubgraphParams, VerifyParams,
};

fn map_err(e: GraphError) -> ErrorData {
    match e {
        GraphError::InvalidApiKey => ErrorData::invalid_params("invalid API key", None),
        GraphError::PermissionDenied(p) => {
            ErrorData::invalid_params(format!("permission denied: requires {p}"), None)
        }
        GraphError::NotFound(what) => ErrorData::invalid_params(format!("not found: {what}"), None),
        GraphError::InvalidInput(msg) => ErrorData::invalid_params(msg, None),
        other => ErrorData::internal_error(other.to_string(), None),
    }
}

fn extract_bearer(parts: &http::request::Parts) -> Result<&str, ErrorData> {
    parts
        .headers
        .get(http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(|| ErrorData::invalid_params("missing 'Authorization: Bearer <key>' header", None))
}

fn evidence_input(p: EvidenceParam) -> EvidenceInput {
    EvidenceInput {
        evidence_type: p.evidence_type,
        uri: p.uri,
        title: p.title,
        excerpt: p.excerpt,
        content_hash: p.content_hash,
        observed_at: None,
        retrieved_at: None,
    }
}

/// Every peer exposes the same graph through MCP (spec section 68) — tool
/// bodies here are thin adapters over [`GraphService`], the same service
/// layer the REST API (`oag-api`) calls. No business logic is duplicated.
#[derive(Clone)]
pub struct OagMcpServer {
    graph: Arc<GraphService>,
}

impl OagMcpServer {
    pub fn new(graph: Arc<GraphService>) -> Self {
        Self { graph }
    }

    async fn authenticate(&self, parts: &http::request::Parts) -> Result<AuthContext, ErrorData> {
        let raw = extract_bearer(parts)?;
        self.graph.authenticate(raw).await.map_err(map_err)
    }

    async fn authenticate_read(&self, parts: &http::request::Parts) -> Result<AuthContext, ErrorData> {
        let auth = self.authenticate(parts).await?;
        auth.require(Permission::GraphRead).map_err(map_err)?;
        Ok(auth)
    }
}

#[tool_router]
impl OagMcpServer {
    #[tool(description = "Search the graph for nodes matching a free-text query. Returns concise results, not full graph internals.")]
    async fn graph_search(
        &self,
        Extension(parts): Extension<http::request::Parts>,
        Parameters(p): Parameters<SearchParams>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.authenticate_read(&parts).await?;
        let results = self.graph.search(&p.query, p.limit.unwrap_or(20)).await.map_err(map_err)?;
        Ok(Json(json!({ "results": results })))
    }

    #[tool(description = "Resolve a URL or name to its graph node, if one exists. Returns an exact match, candidate matches, or nothing found.")]
    async fn graph_resolve(
        &self,
        Extension(parts): Extension<http::request::Parts>,
        Parameters(p): Parameters<ResolveParams>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.authenticate_read(&parts).await?;
        let outcome = self.graph.resolve(&p.value).await.map_err(map_err)?;
        Ok(Json(match outcome {
            oag_graph::ResolveOutcome::Found { node, confidence } => json!({
                "node_id": node.id.to_hex(),
                "canonical_identifier": node.canonical_identifier,
                "type": node.node_type.as_str(),
                "confidence": confidence,
            }),
            oag_graph::ResolveOutcome::Candidates(nodes) => json!({ "candidates": nodes }),
            oag_graph::ResolveOutcome::NotFound => json!({ "candidates": [] }),
        }))
    }

    #[tool(description = "Get a single node by its id.")]
    async fn graph_get_node(
        &self,
        Extension(parts): Extension<http::request::Parts>,
        Parameters(p): Parameters<NodeIdParams>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.authenticate_read(&parts).await?;
        let node_id = p.node_id.parse().map_err(|_| ErrorData::invalid_params("invalid node_id", None))?;
        let node = self.graph.get_node(node_id).await.map_err(map_err)?;
        Ok(Json(json!({ "node": node })))
    }

    #[tool(description = "List all edges touching a node, in either direction.")]
    async fn graph_get_edges(
        &self,
        Extension(parts): Extension<http::request::Parts>,
        Parameters(p): Parameters<NodeIdParams>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.authenticate_read(&parts).await?;
        let node_id = p.node_id.parse().map_err(|_| ErrorData::invalid_params("invalid node_id", None))?;
        let edges = self.graph.get_edges(node_id).await.map_err(map_err)?;
        Ok(Json(json!({ "edges": edges })))
    }

    #[tool(description = "Get a compact semantic neighborhood (nodes, edges, assertions) around a node, out to a given depth.")]
    async fn graph_get_subgraph(
        &self,
        Extension(parts): Extension<http::request::Parts>,
        Parameters(p): Parameters<SubgraphParams>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.authenticate_read(&parts).await?;
        let node_id = p.node_id.parse().map_err(|_| ErrorData::invalid_params("invalid node_id", None))?;
        let sg = self
            .graph
            .get_subgraph(node_id, p.depth.unwrap_or(1), p.max_nodes.unwrap_or(200))
            .await
            .map_err(map_err)?;
        let value = serde_json::to_value(&sg)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        Ok(Json(value))
    }

    #[tool(description = "Find evidence sources backing any assertion whose edge touches this node.")]
    async fn graph_find_sources(
        &self,
        Extension(parts): Extension<http::request::Parts>,
        Parameters(p): Parameters<NodeIdParams>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.authenticate_read(&parts).await?;
        let node_id = p.node_id.parse().map_err(|_| ErrorData::invalid_params("invalid node_id", None))?;
        let sources = self.graph.find_sources(node_id).await.map_err(map_err)?;
        Ok(Json(json!({ "sources": sources })))
    }

    #[tool(description = "Assert that a relationship holds between a subject and an object, optionally with supporting evidence. Creates a signed, evidence-backed claim — not a declaration of global truth.")]
    async fn graph_assert(
        &self,
        Extension(parts): Extension<http::request::Parts>,
        Parameters(p): Parameters<AssertParams>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let auth = self.authenticate(&parts).await?;
        let input = AssertInput {
            subject: p.subject,
            subject_type: p.subject_type,
            predicate: p.predicate,
            object: p.object,
            object_type: p.object_type,
            evidence: p.evidence.into_iter().map(evidence_input).collect(),
            actor_confidence: p.confidence,
            observed_at: None,
        };
        let assertion_id = self.graph.assert(&auth, input).await.map_err(map_err)?;
        Ok(Json(json!({
            "assertion_id": assertion_id.to_hex(),
            "status": "accepted_pending_verification",
        })))
    }

    #[tool(description = "Attach a piece of evidence to an existing assertion.")]
    async fn graph_add_evidence(
        &self,
        Extension(parts): Extension<http::request::Parts>,
        Parameters(p): Parameters<AddEvidenceParams>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let auth = self.authenticate(&parts).await?;
        let assertion_id = p
            .assertion_id
            .parse()
            .map_err(|_| ErrorData::invalid_params("invalid assertion_id", None))?;
        self.graph
            .add_evidence(&auth, assertion_id, evidence_input(p.evidence))
            .await
            .map_err(map_err)?;
        Ok(Json(json!({ "status": "accepted" })))
    }

    #[tool(description = "Record a verification observation for an assertion (e.g. confirmed, contradicted, unreachable).")]
    async fn graph_verify_assertion(
        &self,
        Extension(parts): Extension<http::request::Parts>,
        Parameters(p): Parameters<VerifyParams>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let auth = self.authenticate(&parts).await?;
        let assertion_id = p
            .assertion_id
            .parse()
            .map_err(|_| ErrorData::invalid_params("invalid assertion_id", None))?;
        let observed_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        self.graph
            .verify_assertion(&auth, assertion_id, p.result, observed_at)
            .await
            .map_err(map_err)?;
        Ok(Json(json!({ "status": "accepted" })))
    }

    #[tool(description = "Dispute an existing assertion. Disagreement is first-class data — this does not delete or hide the original claim.")]
    async fn graph_dispute_assertion(
        &self,
        Extension(parts): Extension<http::request::Parts>,
        Parameters(p): Parameters<DisputeParams>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let auth = self.authenticate(&parts).await?;
        let assertion_id = p
            .assertion_id
            .parse()
            .map_err(|_| ErrorData::invalid_params("invalid assertion_id", None))?;
        self.graph
            .dispute_assertion(&auth, assertion_id, p.reason)
            .await
            .map_err(map_err)?;
        Ok(Json(json!({ "status": "accepted" })))
    }

    #[tool(description = "Retract an assertion. History is preserved — the original assertion remains inspectable.")]
    async fn graph_retract_assertion(
        &self,
        Extension(parts): Extension<http::request::Parts>,
        Parameters(p): Parameters<RetractParams>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let auth = self.authenticate(&parts).await?;
        let assertion_id = p
            .assertion_id
            .parse()
            .map_err(|_| ErrorData::invalid_params("invalid assertion_id", None))?;
        self.graph
            .retract_assertion(&auth, assertion_id, p.reason)
            .await
            .map_err(map_err)?;
        Ok(Json(json!({ "status": "accepted" })))
    }

    #[tool(description = "Get the full event history for a node, edge, or assertion — every assert/verify/dispute/retract event that touched it, oldest first.")]
    async fn graph_get_history(
        &self,
        Extension(parts): Extension<http::request::Parts>,
        Parameters(p): Parameters<HistoryParams>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.authenticate_read(&parts).await?;
        let entries = self.graph.get_history(&p.object_type, &p.id).await.map_err(map_err)?;
        Ok(Json(json!({ "history": entries })))
    }
}

#[rmcp::tool_handler]
impl ServerHandler for OagMcpServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(
                "Open Agent Graph: an evidence-backed semantic graph. Claims are assertions, \
                 not truth — always check disputes/history before treating a result as settled.",
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts_with_auth(value: Option<&str>) -> http::request::Parts {
        let mut builder = http::Request::builder().uri("/mcp");
        if let Some(v) = value {
            builder = builder.header(http::header::AUTHORIZATION, v);
        }
        let (parts, _) = builder.body(()).unwrap().into_parts();
        parts
    }

    #[test]
    fn extract_bearer_strips_prefix() {
        let parts = parts_with_auth(Some("Bearer oagk_abc123"));
        assert_eq!(extract_bearer(&parts).unwrap(), "oagk_abc123");
    }

    #[test]
    fn extract_bearer_rejects_missing_header() {
        let parts = parts_with_auth(None);
        assert!(extract_bearer(&parts).is_err());
    }

    #[test]
    fn extract_bearer_rejects_non_bearer_scheme() {
        let parts = parts_with_auth(Some("Basic dXNlcjpwYXNz"));
        assert!(extract_bearer(&parts).is_err());
    }
}
