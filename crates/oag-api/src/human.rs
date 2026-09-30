//! Spec section 80 (Human Interface): a Node page and an Assertion page,
//! secondary to the REST/MCP APIs but making provenance visually
//! inspectable (spec section 81). Reuses the exact same `graph:read` API
//! keys REST already requires -- accepted via a `?key=` query parameter
//! since a plain browser link can't set an `Authorization` header -- so
//! this never makes previously key-gated graph content newly public.
//!
//! Every field below is pre-flattened into plain `String`/`Option<String>`/
//! `bool`/`i64` before reaching a template. Several source fields (crawled
//! page titles, evidence excerpts, actor names) are attacker/crawler-
//! controlled text; askama HTML-escapes every `{{ }}` interpolation by
//! default, and nothing here is ever marked `|safe`.

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use oag_graph::GraphError;
use serde::Deserialize;

use crate::handlers::{parse_assertion_id, parse_node_id};
use crate::state::AppState;

pub struct HumanError(StatusCode, String);

/// Only ever applied to short, server-generated error strings (never to
/// graph content, which always goes through an askama template instead) --
/// still escaped rather than assumed safe, since `GraphError::NotFound`
/// messages echo back caller-supplied ids.
fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

impl IntoResponse for HumanError {
    fn into_response(self) -> Response {
        (self.0, Html(format!("<p>{}</p>", escape_html(&self.1)))).into_response()
    }
}

impl From<GraphError> for HumanError {
    fn from(e: GraphError) -> Self {
        match e {
            GraphError::InvalidApiKey => Self(StatusCode::UNAUTHORIZED, "invalid API key".to_string()),
            GraphError::PermissionDenied(p) => Self(StatusCode::FORBIDDEN, format!("permission denied: requires {p}")),
            GraphError::NotFound(what) => Self(StatusCode::NOT_FOUND, format!("not found: {what}")),
            other => Self(StatusCode::INTERNAL_SERVER_ERROR, other.to_string()),
        }
    }
}

impl From<crate::error::ApiError> for HumanError {
    fn from(e: crate::error::ApiError) -> Self {
        Self::from(e.0)
    }
}

#[derive(Deserialize)]
pub struct KeyQuery {
    key: Option<String>,
}

async fn authenticate_query(key: &Option<String>, graph: &oag_graph::GraphService) -> Result<(), HumanError> {
    let key = key.as_deref().ok_or_else(|| HumanError(StatusCode::UNAUTHORIZED, "missing ?key=<api-key>".to_string()))?;
    let auth = graph.authenticate(key).await?;
    auth.require(oag_core::Permission::GraphRead)?;
    Ok(())
}

fn node_link(id: oag_core::NodeId, key: &str) -> String {
    format!("/ui/nodes/{id}?key={key}")
}

fn assertion_link(id: oag_core::AssertionId, key: &str) -> String {
    format!("/ui/assertions/{id}?key={key}")
}

struct NodeView {
    id: String,
    node_type: String,
    canonical_identifier: String,
    name: Option<String>,
}

struct AliasView {
    alias: String,
    alias_type: String,
}

struct RelationshipView {
    predicate: String,
    other_node_identifier: String,
    direction: &'static str,
    link: String,
}

struct EvidenceView {
    evidence_type: String,
    uri: Option<String>,
    title: Option<String>,
    excerpt: Option<String>,
    content_hash: Option<String>,
}

fn evidence_view(e: &oag_core::Evidence) -> EvidenceView {
    EvidenceView {
        evidence_type: e.evidence_type.as_str().to_string(),
        uri: e.uri.clone(),
        title: e.title.clone(),
        excerpt: e.excerpt.clone(),
        content_hash: e.content_hash.clone(),
    }
}

struct DisputeView {
    reason: Option<String>,
    created_at: i64,
}

fn dispute_view(d: &oag_storage::repo::assertions::Dispute) -> DisputeView {
    DisputeView { reason: d.reason.clone(), created_at: d.created_at }
}

struct ObservationView {
    result: String,
    observed_at: i64,
}

fn observation_view(o: &oag_storage::repo::assertions::Observation) -> ObservationView {
    ObservationView { result: o.result.clone(), observed_at: o.observed_at }
}

struct AssertionSummaryView {
    link: String,
    subject_identifier: String,
    predicate: String,
    object_identifier: String,
    status: String,
    actor_confidence: Option<f32>,
    evidence: Vec<EvidenceView>,
    disputes: Vec<DisputeView>,
    last_observation: Option<ObservationView>,
}

struct HistoryEntryView {
    event_id: String,
    event_type: String,
    created_at: i64,
}

struct SearchResultView {
    canonical_identifier: String,
    name: Option<String>,
    link: String,
}

#[derive(Template)]
#[template(path = "search.html")]
struct SearchTemplate {
    key: String,
    query: String,
    has_searched: bool,
    results: Vec<SearchResultView>,
}

#[derive(Deserialize)]
pub struct SearchQuery {
    key: Option<String>,
    q: Option<String>,
}

/// A landing/browse page (deferred at first alongside the Node/Assertion
/// pages, per `OPEN_QUESTIONS.md`'s "Human Interface" section -- direct-link
/// pages don't need a search box, but they're much less discoverable
/// without one). Reuses `GraphService::search` exactly like `oag search`/
/// `GET /api/v1/search`, just rendered as clickable links instead of JSON.
pub async fn search_page(State(state): State<AppState>, Query(q): Query<SearchQuery>) -> Result<Response, HumanError> {
    authenticate_query(&q.key, &state.graph).await?;
    let key = q.key.unwrap_or_default();
    let query = q.q.unwrap_or_default();
    let has_searched = !query.trim().is_empty();

    let results = if has_searched {
        state
            .graph
            .search(&query, 50)
            .await?
            .into_iter()
            .map(|n| SearchResultView { canonical_identifier: n.canonical_identifier, name: n.name, link: node_link(n.id, &key) })
            .collect()
    } else {
        Vec::new()
    };

    let template = SearchTemplate { key, query, has_searched, results };
    Ok(Html(template.render().map_err(|e| HumanError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?).into_response())
}

#[derive(Template)]
#[template(path = "node.html")]
struct NodeTemplate {
    key: String,
    node: NodeView,
    aliases: Vec<AliasView>,
    relationships: Vec<RelationshipView>,
    assertions: Vec<AssertionSummaryView>,
    history: Vec<HistoryEntryView>,
}

pub async fn node_page(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<KeyQuery>,
) -> Result<Response, HumanError> {
    authenticate_query(&q.key, &state.graph).await?;
    let key = q.key.unwrap_or_default();
    let node_id = parse_node_id(&id)?;
    let node = state.graph.get_node(node_id).await?.ok_or_else(|| GraphError::NotFound(format!("node {id}")))?;

    let aliases = state
        .graph
        .list_aliases(node_id)
        .await?
        .into_iter()
        .map(|a| AliasView { alias: a.alias, alias_type: a.alias_type.as_str().to_string() })
        .collect();

    let mut relationships = Vec::new();
    for edge in state.graph.get_edges(node_id).await? {
        let (direction, other_id) =
            if edge.subject == node_id { ("outgoing", edge.object) } else { ("incoming", edge.subject) };
        let other_identifier = state
            .graph
            .get_node(other_id)
            .await?
            .map(|n| n.canonical_identifier)
            .unwrap_or_else(|| other_id.to_hex());
        relationships.push(RelationshipView {
            predicate: edge.predicate.as_str().to_string(),
            other_node_identifier: other_identifier,
            direction,
            link: node_link(other_id, &key),
        });
    }

    let mut assertions = Vec::new();
    for a in state.graph.list_assertions_for_node(node_id).await? {
        let Some(edge) = state.graph.get_edge(a.edge_id).await? else { continue };
        let subject_identifier =
            state.graph.get_node(edge.subject).await?.map(|n| n.canonical_identifier).unwrap_or_else(|| edge.subject.to_hex());
        let object_identifier =
            state.graph.get_node(edge.object).await?.map(|n| n.canonical_identifier).unwrap_or_else(|| edge.object.to_hex());
        let evidence = state.graph.list_evidence(a.id).await?.iter().map(evidence_view).collect();
        let disputes = state.graph.list_disputes(a.id).await?.iter().map(dispute_view).collect();
        let last_observation = state
            .graph
            .list_observations(a.id)
            .await?
            .iter()
            .max_by_key(|o| o.observed_at)
            .map(observation_view);
        assertions.push(AssertionSummaryView {
            link: assertion_link(a.id, &key),
            subject_identifier,
            predicate: edge.predicate.as_str().to_string(),
            object_identifier,
            status: a.status.as_str().to_string(),
            actor_confidence: a.actor_confidence,
            evidence,
            disputes,
            last_observation,
        });
    }

    let history = state
        .graph
        .get_history("node", &id)
        .await?
        .into_iter()
        .map(|h| HistoryEntryView { event_id: h.event_id.to_hex(), event_type: h.event_type, created_at: h.created_at })
        .collect();

    let template = NodeTemplate {
        key,
        node: NodeView {
            id: node.id.to_hex(),
            node_type: node.node_type.as_str().to_string(),
            canonical_identifier: node.canonical_identifier,
            name: node.name,
        },
        aliases,
        relationships,
        assertions,
        history,
    };
    Ok(Html(template.render().map_err(|e| HumanError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?).into_response())
}

struct SupersessionView {
    new_assertion_id: String,
    created_at: i64,
    link: String,
}

#[derive(Template)]
#[template(path = "assertion.html")]
struct AssertionTemplate {
    key: String,
    assertion_id: String,
    subject_link: String,
    subject_identifier: String,
    predicate: String,
    object_link: String,
    object_identifier: String,
    actor_name: Option<String>,
    actor_type: String,
    actor_id: String,
    provenance: Option<oag_graph::EventProvenance>,
    evidence: Vec<EvidenceView>,
    created_at: i64,
    observed_at: Option<i64>,
    observations: Vec<ObservationView>,
    disputes: Vec<DisputeView>,
    supersession: Option<SupersessionView>,
    status: String,
}

pub async fn assertion_page(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<KeyQuery>,
) -> Result<Response, HumanError> {
    authenticate_query(&q.key, &state.graph).await?;
    let key = q.key.unwrap_or_default();
    let assertion_id = parse_assertion_id(&id)?;
    let assertion =
        state.graph.get_assertion(assertion_id).await?.ok_or_else(|| GraphError::NotFound(format!("assertion {id}")))?;
    let edge = state
        .graph
        .get_edge(assertion.edge_id)
        .await?
        .ok_or_else(|| GraphError::NotFound(format!("edge for assertion {id}")))?;
    let subject =
        state.graph.get_node(edge.subject).await?.map(|n| n.canonical_identifier).unwrap_or_else(|| edge.subject.to_hex());
    let object =
        state.graph.get_node(edge.object).await?.map(|n| n.canonical_identifier).unwrap_or_else(|| edge.object.to_hex());
    let actor = state.graph.get_actor(assertion.actor_id).await?;

    let event_id = oag_core::EventId::from_hash(assertion_id.as_hash());
    let provenance = state.graph.get_event_provenance(event_id).await?;

    let evidence = state.graph.list_evidence(assertion_id).await?.iter().map(evidence_view).collect();
    let observations = state.graph.list_observations(assertion_id).await?.iter().map(observation_view).collect();
    let disputes = state.graph.list_disputes(assertion_id).await?.iter().map(dispute_view).collect();

    let supersession = state
        .graph
        .get_history("assertion", &id)
        .await?
        .into_iter()
        .find(|h| h.event_type == "SUPERSEDE_ASSERTION")
        .and_then(|h| {
            let new_id_str = h.payload.get("new_assertion_id")?.as_str()?.to_string();
            let new_assertion_id: oag_core::AssertionId = new_id_str.parse().ok()?;
            Some(SupersessionView {
                new_assertion_id: new_id_str,
                created_at: h.created_at,
                link: assertion_link(new_assertion_id, &key),
            })
        });

    let (actor_name, actor_type, actor_id) = match actor {
        Some(a) => (a.name, a.actor_type.as_str().to_string(), a.id.to_hex()),
        None => (None, "unknown".to_string(), assertion.actor_id.to_hex()),
    };

    let template = AssertionTemplate {
        key: key.clone(),
        assertion_id: id,
        subject_link: node_link(edge.subject, &key),
        subject_identifier: subject,
        predicate: edge.predicate.as_str().to_string(),
        object_link: node_link(edge.object, &key),
        object_identifier: object,
        actor_name,
        actor_type,
        actor_id,
        provenance,
        evidence,
        created_at: assertion.asserted_at,
        observed_at: assertion.observed_at,
        observations,
        disputes,
        supersession,
        status: assertion.status.as_str().to_string(),
    };
    Ok(Html(template.render().map_err(|e| HumanError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?).into_response())
}
