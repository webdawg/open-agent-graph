use oag_graph::{AssertInput, EvidenceInput};
use serde::Deserialize;

use crate::error::ApiError;
use oag_graph::GraphError;

fn parse_timestamp(s: &str) -> Result<i64, ApiError> {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.timestamp())
        .map_err(|e| ApiError(GraphError::InvalidInput(format!("invalid RFC3339 timestamp '{s}': {e}"))))
}

#[derive(Debug, Deserialize)]
pub struct EvidenceRequest {
    #[serde(rename = "type")]
    pub evidence_type: Option<String>,
    pub uri: Option<String>,
    pub title: Option<String>,
    pub excerpt: Option<String>,
    pub content_hash: Option<String>,
    pub observed_at: Option<String>,
    pub retrieved_at: Option<String>,
}

impl EvidenceRequest {
    pub(crate) fn into_input(self) -> Result<EvidenceInput, ApiError> {
        Ok(EvidenceInput {
            evidence_type: self.evidence_type,
            uri: self.uri,
            title: self.title,
            excerpt: self.excerpt,
            content_hash: self.content_hash,
            observed_at: self.observed_at.as_deref().map(parse_timestamp).transpose()?,
            retrieved_at: self.retrieved_at.as_deref().map(parse_timestamp).transpose()?,
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct AssertRequest {
    pub subject: String,
    pub subject_type: Option<String>,
    pub predicate: String,
    pub object: String,
    pub object_type: Option<String>,
    #[serde(default)]
    pub evidence: Vec<EvidenceRequest>,
    pub actor_confidence: Option<f32>,
    pub observed_at: Option<String>,
}

impl AssertRequest {
    pub fn into_input(self) -> Result<AssertInput, ApiError> {
        let evidence = self
            .evidence
            .into_iter()
            .map(EvidenceRequest::into_input)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(AssertInput {
            subject: self.subject,
            subject_type: self.subject_type,
            predicate: self.predicate,
            object: self.object,
            object_type: self.object_type,
            evidence,
            actor_confidence: self.actor_confidence,
            observed_at: self.observed_at.as_deref().map(parse_timestamp).transpose()?,
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct VerifyRequest {
    pub result: String,
    pub observed_at: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct DisputeRequest {
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RetractRequest {
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ResolveRequest {
    pub value: String,
}

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub q: String,
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct SubgraphQuery {
    pub node: String,
    pub depth: Option<u32>,
    pub max_nodes: Option<usize>,
}

pub(crate) fn now_or(observed_at: Option<String>) -> Result<i64, ApiError> {
    match observed_at {
        Some(s) => parse_timestamp(&s),
        None => Ok(std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64),
    }
}
