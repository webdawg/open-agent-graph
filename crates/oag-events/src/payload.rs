use serde::{Deserialize, Serialize};

/// The typed body of every distributed mutation (spec section 31, trimmed to
/// the event types this milestone's REST/MCP surface actually emits).
/// `NODE_DECLARE`/`NODE_ALIAS` aren't included: subject/object nodes are
/// resolved-or-created as a side effect of `AssertRelation` (spec section
/// 32), so there's no caller that would ever construct them standalone yet.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event_type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EventPayload {
    AssertRelation(AssertRelationPayload),
    AddEvidence(AddEvidencePayload),
    VerifyAssertion(VerifyAssertionPayload),
    DisputeAssertion(DisputeAssertionPayload),
    RetractAssertion(RetractAssertionPayload),
    SupersedeAssertion(SupersedeAssertionPayload),
    ActorDeclare(ActorDeclarePayload),
    ActorKeyAdd(ActorKeyAddPayload),
}

impl EventPayload {
    pub fn type_str(&self) -> &'static str {
        match self {
            EventPayload::AssertRelation(_) => "ASSERT_RELATION",
            EventPayload::AddEvidence(_) => "ADD_EVIDENCE",
            EventPayload::VerifyAssertion(_) => "VERIFY_ASSERTION",
            EventPayload::DisputeAssertion(_) => "DISPUTE_ASSERTION",
            EventPayload::RetractAssertion(_) => "RETRACT_ASSERTION",
            EventPayload::SupersedeAssertion(_) => "SUPERSEDE_ASSERTION",
            EventPayload::ActorDeclare(_) => "ACTOR_DECLARE",
            EventPayload::ActorKeyAdd(_) => "ACTOR_KEY_ADD",
        }
    }
}

/// Subject/object are always the already-canonicalized identifier strings
/// (e.g. `"url:https://github.com/example/foo"`) that a `NodeId` is derived
/// from, never a raw URL — canonicalization happens once, in the service
/// layer, before the event is built, so replay never depends on
/// canonicalization rules changing between versions (spec invariant 6).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssertRelationPayload {
    pub subject_identifier: String,
    pub subject_type: String,
    pub predicate: String,
    pub object_identifier: String,
    pub object_type: String,
    pub actor_id: String,
    pub actor_confidence: Option<f32>,
    pub observed_at: Option<i64>,
    pub extraction_method: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddEvidencePayload {
    pub assertion_id: String,
    pub evidence_type: String,
    pub uri: Option<String>,
    pub title: Option<String>,
    pub excerpt: Option<String>,
    pub content_hash: Option<String>,
    pub observed_at: Option<i64>,
    pub retrieved_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyAssertionPayload {
    pub assertion_id: String,
    pub observer_actor_id: String,
    pub result: String,
    pub observed_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisputeAssertionPayload {
    pub disputed_assertion_id: String,
    pub disputing_actor_id: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetractAssertionPayload {
    pub retracted_assertion_id: String,
    pub actor_id: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupersedeAssertionPayload {
    pub old_assertion_id: String,
    pub new_assertion_id: String,
    pub actor_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActorDeclarePayload {
    pub actor_type: String,
    pub name: Option<String>,
    pub public_key: Option<String>,
    pub identity_uri: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActorKeyAddPayload {
    pub actor_id: String,
    pub key_hash: String,
    pub permissions: Vec<String>,
}
