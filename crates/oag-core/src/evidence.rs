use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use crate::ids::{AssertionId, EventId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceType {
    WebPage,
    Documentation,
    SourceCode,
    Repository,
    ApiResponse,
    Dataset,
    File,
    Manual,
    Specification,
    UserObservation,
    AgentObservation,
    Other,
}

impl EvidenceType {
    pub fn as_str(&self) -> &'static str {
        match self {
            EvidenceType::WebPage => "web_page",
            EvidenceType::Documentation => "documentation",
            EvidenceType::SourceCode => "source_code",
            EvidenceType::Repository => "repository",
            EvidenceType::ApiResponse => "api_response",
            EvidenceType::Dataset => "dataset",
            EvidenceType::File => "file",
            EvidenceType::Manual => "manual",
            EvidenceType::Specification => "specification",
            EvidenceType::UserObservation => "user_observation",
            EvidenceType::AgentObservation => "agent_observation",
            EvidenceType::Other => "other",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "web_page" => EvidenceType::WebPage,
            "documentation" => EvidenceType::Documentation,
            "source_code" => EvidenceType::SourceCode,
            "repository" => EvidenceType::Repository,
            "api_response" => EvidenceType::ApiResponse,
            "dataset" => EvidenceType::Dataset,
            "file" => EvidenceType::File,
            "manual" => EvidenceType::Manual,
            "specification" => EvidenceType::Specification,
            "user_observation" => EvidenceType::UserObservation,
            "agent_observation" => EvidenceType::AgentObservation,
            "other" => EvidenceType::Other,
            _ => return None,
        })
    }
}

/// Supports or contradicts an assertion (spec section 23). The evidence
/// record itself is a thin index; its id is the id of the `ADD_EVIDENCE`
/// event that created it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub id: EventId,
    pub assertion_id: AssertionId,
    pub evidence_type: EvidenceType,
    pub uri: Option<String>,
    pub title: Option<String>,
    pub excerpt: Option<String>,
    /// `sha256:...` or `blake3:...` — spec section 24 prefers BLAKE3
    /// internally, SHA-256 may additionally be stored for interchange.
    pub content_hash: Option<String>,
    pub observed_at: Option<i64>,
    pub retrieved_at: Option<i64>,
    #[serde(default = "default_metadata")]
    pub metadata: JsonValue,
}

fn default_metadata() -> JsonValue {
    JsonValue::Object(Default::default())
}
