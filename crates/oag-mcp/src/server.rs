use std::sync::Arc;

use oag_core::Permission;
use oag_crawler::CrawlerService;
use oag_graph::{AssertInput, AuthContext, EvidenceInput, GraphError, GraphService};
use rmcp::handler::server::tool::Extension;
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::model::{ServerCapabilities, ServerConfig};
use rmcp::{tool, tool_router, ErrorData, ServerHandler};
use serde_json::json;
use tracing::Instrument;

use crate::params::{
    ActorIdParams, AddEvidenceParams, AssertParams, CrawlParams, DisputeParams, EdgeIdParams,
    EvidenceParam, HistoryParams, NodeIdParams, ResolveParams, RetractParams, SearchParams,
    SubgraphParams, VerifyParams,
};

fn map_err(e: GraphError) -> ErrorData {
    match e {
        GraphError::InvalidApiKey => ErrorData::invalid_params("invalid API key", None),
        GraphError::PermissionDenied(p) => {
            ErrorData::invalid_params(format!("permission denied: requires {p}"), None)
        }
        GraphError::NotFound(what) => ErrorData::invalid_params(format!("not found: {what}"), None),
        GraphError::InvalidInput(msg) => ErrorData::invalid_params(msg, None),
        GraphError::Embedding(oag_embeddings::EmbeddingError::Disabled) => {
            ErrorData::invalid_params(e.to_string(), None)
        }
        other => ErrorData::internal_error(other.to_string(), None),
    }
}

fn map_crawl_err(e: oag_crawler::error::CrawlError) -> ErrorData {
    use oag_crawler::error::CrawlError;
    match e {
        CrawlError::UnsupportedScheme(_) | CrawlError::NoHost => ErrorData::invalid_params(e.to_string(), None),
        CrawlError::BlockedAddress(_) | CrawlError::RobotsDisallowed => ErrorData::invalid_params(e.to_string(), None),
        CrawlError::Graph(inner) => map_err(inner),
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
    embedding_provider: Arc<dyn oag_embeddings::EmbeddingProvider>,
    crawler: Arc<CrawlerService>,
}

impl OagMcpServer {
    pub fn new(
        graph: Arc<GraphService>,
        embedding_provider: Arc<dyn oag_embeddings::EmbeddingProvider>,
        crawler: Arc<CrawlerService>,
    ) -> Self {
        Self { graph, embedding_provider, crawler }
    }

    async fn authenticate(&self, parts: &http::request::Parts) -> Result<AuthContext, ErrorData> {
        let raw = extract_bearer(parts)?;
        let auth = self.graph.authenticate(raw).await.map_err(map_err)?;
        // Spec section 87: every relevant request's logs should include
        // `actor_id`. Recorded here, the one place every tool's auth path
        // funnels through, onto whatever span `Self::traced` already opened
        // for this call.
        tracing::Span::current().record("actor_id", tracing::field::display(auth.actor_id));
        Ok(auth)
    }

    async fn authenticate_read(&self, parts: &http::request::Parts) -> Result<AuthContext, ErrorData> {
        let auth = self.authenticate(parts).await?;
        auth.require(Permission::GraphRead).map_err(map_err)?;
        Ok(auth)
    }

    /// Spec section 87: a structured `tracing` span per MCP tool call,
    /// mirroring `oag_api::logging`'s REST request spans exactly —
    /// `tool`/`peer_id` known up front, `actor_id` recorded from inside
    /// `Self::authenticate` once auth resolves it, `duration_ms`/`result`
    /// logged on completion. Every `#[tool]` method has the identical
    /// `Result<Json<serde_json::Value>, ErrorData>` shape, so one generic
    /// wrapper covers all of them.
    async fn traced<F>(&self, tool: &'static str, fut: F) -> Result<Json<serde_json::Value>, ErrorData>
    where
        F: std::future::Future<Output = Result<Json<serde_json::Value>, ErrorData>>,
    {
        let start = std::time::Instant::now();
        let span = tracing::info_span!(
            "mcp_tool",
            tool,
            peer_id = %self.graph.identity().peer_id(),
            actor_id = tracing::field::Empty,
        );
        let result = fut.instrument(span.clone()).await;
        let _entered = span.enter();
        tracing::info!(
            duration_ms = start.elapsed().as_millis() as u64,
            result = if result.is_ok() { "ok" } else { "error" },
            "mcp tool call completed"
        );
        result
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
        self.traced("graph_search", async {
            self.authenticate_read(&parts).await?;
            let results = if p.semantic.unwrap_or(false) {
                let ranked = self
                    .graph
                    .semantic_search(self.embedding_provider.as_ref(), &p.query, p.limit.unwrap_or(20))
                    .await
                    .map_err(map_err)?;
                json!(ranked)
            } else {
                let results = self.graph.search(&p.query, p.limit.unwrap_or(20)).await.map_err(map_err)?;
                json!(results)
            };
            Ok(Json(json!({ "results": results })))
        })
        .await
    }

    #[tool(description = "Resolve a URL or name to its graph node, if one exists. Returns an exact match, candidate matches, or nothing found.")]
    async fn graph_resolve(
        &self,
        Extension(parts): Extension<http::request::Parts>,
        Parameters(p): Parameters<ResolveParams>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.traced("graph_resolve", async {
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
        })
        .await
    }

    #[tool(description = "Get a single node by its id.")]
    async fn graph_get_node(
        &self,
        Extension(parts): Extension<http::request::Parts>,
        Parameters(p): Parameters<NodeIdParams>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.traced("graph_get_node", async {
            self.authenticate_read(&parts).await?;
            let node_id = p.node_id.parse().map_err(|_| ErrorData::invalid_params("invalid node_id", None))?;
            let node = self.graph.get_node(node_id).await.map_err(map_err)?;
            let aliases = self.graph.list_aliases(node_id).await.map_err(map_err)?;
            let authority = self.graph.get_node_authority(node_id).await.map_err(map_err)?;
            Ok(Json(json!({ "node": node, "aliases": aliases, "authority": authority })))
        })
        .await
    }

    #[tool(description = "Get a single actor by its id -- who or what made a claim (human, agent, model, crawler, organization, domain, service, peer, or anonymous), and its identity_uri/public key if it has one.")]
    async fn graph_get_actor(
        &self,
        Extension(parts): Extension<http::request::Parts>,
        Parameters(p): Parameters<ActorIdParams>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.traced("graph_get_actor", async {
            self.authenticate_read(&parts).await?;
            let actor_id = p.actor_id.parse().map_err(|_| ErrorData::invalid_params("invalid actor_id", None))?;
            let actor = self.graph.get_actor(actor_id).await.map_err(map_err)?;
            Ok(Json(json!({ "actor": actor })))
        })
        .await
    }

    #[tool(description = "List all edges touching a node, in either direction.")]
    async fn graph_get_edges(
        &self,
        Extension(parts): Extension<http::request::Parts>,
        Parameters(p): Parameters<NodeIdParams>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.traced("graph_get_edges", async {
            self.authenticate_read(&parts).await?;
            let node_id = p.node_id.parse().map_err(|_| ErrorData::invalid_params("invalid node_id", None))?;
            let edges = self.graph.get_edges(node_id).await.map_err(map_err)?;
            Ok(Json(json!({ "edges": edges })))
        })
        .await
    }

    #[tool(description = "Get a single edge by its id -- the subject/predicate/object triple. Remember an edge is not a truth declaration by itself; see graph_get_corroboration for how well-supported it is.")]
    async fn graph_get_edge(
        &self,
        Extension(parts): Extension<http::request::Parts>,
        Parameters(p): Parameters<EdgeIdParams>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.traced("graph_get_edge", async {
            self.authenticate_read(&parts).await?;
            let edge_id = p.edge_id.parse().map_err(|_| ErrorData::invalid_params("invalid edge_id", None))?;
            let edge = self.graph.get_edge(edge_id).await.map_err(map_err)?;
            Ok(Json(json!({ "edge": edge })))
        })
        .await
    }

    #[tool(description = "Get corroboration signals for one edge: how many independent sources (not just how many assertions) back it, how strong the evidence is, and how much verification/dispute agreement it has. Never collapsed into one score — inspect each signal.")]
    async fn graph_get_corroboration(
        &self,
        Extension(parts): Extension<http::request::Parts>,
        Parameters(p): Parameters<EdgeIdParams>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.traced("graph_get_corroboration", async {
            self.authenticate_read(&parts).await?;
            let edge_id = p.edge_id.parse().map_err(|_| ErrorData::invalid_params("invalid edge_id", None))?;
            let corroboration = self.graph.get_edge_corroboration(edge_id).await.map_err(map_err)?;
            let value = serde_json::to_value(&corroboration)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
            Ok(Json(value))
        })
        .await
    }

    #[tool(description = "Get a compact semantic neighborhood (nodes, edges, assertions) around a node, out to a given depth.")]
    async fn graph_get_subgraph(
        &self,
        Extension(parts): Extension<http::request::Parts>,
        Parameters(p): Parameters<SubgraphParams>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.traced("graph_get_subgraph", async {
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
        })
        .await
    }

    #[tool(description = "Find evidence sources backing any assertion whose edge touches this node.")]
    async fn graph_find_sources(
        &self,
        Extension(parts): Extension<http::request::Parts>,
        Parameters(p): Parameters<NodeIdParams>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.traced("graph_find_sources", async {
            self.authenticate_read(&parts).await?;
            let node_id = p.node_id.parse().map_err(|_| ErrorData::invalid_params("invalid node_id", None))?;
            let sources = self.graph.find_sources(node_id).await.map_err(map_err)?;
            Ok(Json(json!({ "sources": sources })))
        })
        .await
    }

    #[tool(description = "Assert that a relationship holds between a subject and an object, optionally with supporting evidence. Creates a signed, evidence-backed claim — not a declaration of global truth.")]
    async fn graph_assert(
        &self,
        Extension(parts): Extension<http::request::Parts>,
        Parameters(p): Parameters<AssertParams>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.traced("graph_assert", async {
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
                extraction_method: None,
            };
            let assertion_id = self.graph.assert(&auth, input).await.map_err(map_err)?;
            Ok(Json(json!({
                "assertion_id": assertion_id.to_hex(),
                "status": "accepted_pending_verification",
            })))
        })
        .await
    }

    #[tool(description = "Attach a piece of evidence to an existing assertion.")]
    async fn graph_add_evidence(
        &self,
        Extension(parts): Extension<http::request::Parts>,
        Parameters(p): Parameters<AddEvidenceParams>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.traced("graph_add_evidence", async {
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
        })
        .await
    }

    #[tool(description = "Record a verification observation for an assertion (e.g. confirmed, contradicted, unreachable).")]
    async fn graph_verify_assertion(
        &self,
        Extension(parts): Extension<http::request::Parts>,
        Parameters(p): Parameters<VerifyParams>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.traced("graph_verify_assertion", async {
            let auth = self.authenticate(&parts).await?;
            let assertion_id = p
                .assertion_id
                .parse()
                .map_err(|_| ErrorData::invalid_params("invalid assertion_id", None))?;
            let observed_at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            let result = oag_core::VerifyResult::parse(&p.result)
                .ok_or_else(|| ErrorData::invalid_params("unknown verify result, expected one of confirmed/not_confirmed/changed/contradicted/unreachable/unknown", None))?;
            self.graph
                .verify_assertion(&auth, assertion_id, result, observed_at)
                .await
                .map_err(map_err)?;
            Ok(Json(json!({ "status": "accepted" })))
        })
        .await
    }

    #[tool(description = "Dispute an existing assertion. Disagreement is first-class data — this does not delete or hide the original claim.")]
    async fn graph_dispute_assertion(
        &self,
        Extension(parts): Extension<http::request::Parts>,
        Parameters(p): Parameters<DisputeParams>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.traced("graph_dispute_assertion", async {
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
        })
        .await
    }

    #[tool(description = "Retract an assertion. History is preserved — the original assertion remains inspectable.")]
    async fn graph_retract_assertion(
        &self,
        Extension(parts): Extension<http::request::Parts>,
        Parameters(p): Parameters<RetractParams>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.traced("graph_retract_assertion", async {
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
        })
        .await
    }

    #[tool(description = "Get the full event history for a node, edge, or assertion — every assert/verify/dispute/retract event that touched it, oldest first.")]
    async fn graph_get_history(
        &self,
        Extension(parts): Extension<http::request::Parts>,
        Parameters(p): Parameters<HistoryParams>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.traced("graph_get_history", async {
            self.authenticate_read(&parts).await?;
            let entries = self.graph.get_history(&p.object_type, &p.id).await.map_err(map_err)?;
            Ok(Json(json!({ "history": entries })))
        })
        .await
    }

    #[tool(description = "Crawl one URL: fetch it safely (SSRF-guarded), extract structured facts (JSON-LD, llms.txt, ARD, A2A, HTML meta), and assert them as evidence-backed claims. Requires the graph:crawl permission, distinct from graph:assert -- this makes the peer itself issue an outbound HTTP request to the given URL.")]
    async fn graph_crawl(
        &self,
        Extension(parts): Extension<http::request::Parts>,
        Parameters(p): Parameters<CrawlParams>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.traced("graph_crawl", async {
            let auth = self.authenticate(&parts).await?;
            auth.require(Permission::GraphCrawl).map_err(map_err)?;
            let url = url::Url::parse(&p.url).map_err(|e| ErrorData::invalid_params(format!("invalid url '{}': {e}", p.url), None))?;
            let summary = self.crawler.crawl(&url).await.map_err(map_crawl_err)?;
            let value = serde_json::to_value(summary).map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
            Ok(Json(value))
        })
        .await
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
