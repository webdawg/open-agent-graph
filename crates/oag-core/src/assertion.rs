use serde::{Deserialize, Serialize};

use crate::ids::{ActorId, AssertionId, EdgeId};

/// How a candidate relationship was produced. Machine extraction is a
/// candidate assertion, not a fact (spec sections 47, 74) — this field is
/// what lets downstream consumers tell the difference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtractionMethod {
    /// A human or agent directly asserted this relationship.
    Direct,
    /// Produced by structured/deterministic extraction (JSON-LD, schema.org,
    /// ARD, A2A, package metadata, etc).
    StructuredExtraction,
    /// Produced by an LLM reading unstructured content.
    LlmExtraction,
    /// Produced by an automated verification/observation pass.
    Verification,
}

impl ExtractionMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            ExtractionMethod::Direct => "direct",
            ExtractionMethod::StructuredExtraction => "structured_extraction",
            ExtractionMethod::LlmExtraction => "llm_extraction",
            ExtractionMethod::Verification => "verification",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "direct" => ExtractionMethod::Direct,
            "structured_extraction" => ExtractionMethod::StructuredExtraction,
            "llm_extraction" => ExtractionMethod::LlmExtraction,
            "verification" => ExtractionMethod::Verification,
            _ => return None,
        })
    }
}

/// The lifecycle state a projected assertion is currently believed to be in.
/// This is a derived, overwritable cache — the event log and the dedicated
/// `assertion_disputes`/`assertion_retractions`/`assertion_supersessions`
/// tables remain the authoritative history (see plan's "projection tables
/// stay thin" note).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssertionStatus {
    Active,
    Disputed,
    Retracted,
    Superseded,
}

impl AssertionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            AssertionStatus::Active => "active",
            AssertionStatus::Disputed => "disputed",
            AssertionStatus::Retracted => "retracted",
            AssertionStatus::Superseded => "superseded",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "active" => AssertionStatus::Active,
            "disputed" => AssertionStatus::Disputed,
            "retracted" => AssertionStatus::Retracted,
            "superseded" => AssertionStatus::Superseded,
            _ => return None,
        })
    }
}

/// An Actor claims an Edge is valid (spec section 21). This is the
/// fundamental knowledge contribution: it means "Actor A states relationship
/// R", never "Relationship R is globally true" (spec section 22).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Assertion {
    /// Equal to the ID of the `ASSERT_RELATION` event that created this
    /// assertion (spec section 21).
    pub id: AssertionId,
    pub edge_id: EdgeId,
    pub actor_id: ActorId,
    /// The submitting actor's own confidence in the claim. This is NOT the
    /// system's confidence (spec section 13/22) — no derived system-wide
    /// score is computed in this milestone.
    pub actor_confidence: Option<f32>,
    pub observed_at: Option<i64>,
    pub asserted_at: i64,
    pub extraction_method: ExtractionMethod,
    pub status: AssertionStatus,
}
