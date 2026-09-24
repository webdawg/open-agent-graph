use oag_core::{AssertionId, Permission};
use oag_events::payload::{
    AddEvidencePayload, AssertRelationPayload, DisputeAssertionPayload, RetractAssertionPayload,
    SupersedeAssertionPayload, VerifyAssertionPayload,
};
use oag_events::{commit_local_event, EventPayload};

use crate::error::GraphError;
use crate::identifier::canonicalize_value;
use crate::service::{AuthContext, GraphService};

// Spec section 61 (Public-Network Abuse): "gigantic evidence payloads" and
// "event flooding" are guarded here, at the one service layer both REST and
// MCP call through — an HTTP-level body-size cap (see oag-api/oag-sync)
// bounds total request size, but a single field within an otherwise-small
// request could still be absurd, and evidence count isn't bounded by bytes
// at all (many tiny evidence entries still cost one signed event each).
const MAX_IDENTIFIER_LEN: usize = 2048;
const MAX_TYPE_LEN: usize = 64;
const MAX_PREDICATE_LEN: usize = 128;
const MAX_URI_LEN: usize = 2048;
const MAX_TITLE_LEN: usize = 512;
const MAX_EXCERPT_LEN: usize = 4096;
const MAX_CONTENT_HASH_LEN: usize = 128;
const MAX_REASON_LEN: usize = 2048;
const MAX_EVIDENCE_PER_ASSERTION: usize = 20;

fn check_len(field: &'static str, value: &str, max: usize) -> Result<(), GraphError> {
    if value.len() > max {
        Err(GraphError::InvalidInput(format!(
            "{field} is {} bytes, exceeds the {max}-byte limit",
            value.len()
        )))
    } else {
        Ok(())
    }
}

fn check_opt_len(field: &'static str, value: &Option<String>, max: usize) -> Result<(), GraphError> {
    match value {
        Some(v) => check_len(field, v, max),
        None => Ok(()),
    }
}

#[derive(Debug, Default, Clone)]
pub struct EvidenceInput {
    pub evidence_type: Option<String>,
    pub uri: Option<String>,
    pub title: Option<String>,
    pub excerpt: Option<String>,
    pub content_hash: Option<String>,
    pub observed_at: Option<i64>,
    pub retrieved_at: Option<i64>,
}

impl EvidenceInput {
    fn validate(&self) -> Result<(), GraphError> {
        check_opt_len("evidence.uri", &self.uri, MAX_URI_LEN)?;
        check_opt_len("evidence.title", &self.title, MAX_TITLE_LEN)?;
        check_opt_len("evidence.excerpt", &self.excerpt, MAX_EXCERPT_LEN)?;
        check_opt_len("evidence.content_hash", &self.content_hash, MAX_CONTENT_HASH_LEN)?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct AssertInput {
    pub subject: String,
    pub subject_type: Option<String>,
    pub predicate: String,
    pub object: String,
    pub object_type: Option<String>,
    pub evidence: Vec<EvidenceInput>,
    pub actor_confidence: Option<f32>,
    pub observed_at: Option<i64>,
}

impl AssertInput {
    fn validate(&self) -> Result<(), GraphError> {
        check_len("subject", &self.subject, MAX_IDENTIFIER_LEN)?;
        check_len("object", &self.object, MAX_IDENTIFIER_LEN)?;
        check_len("predicate", &self.predicate, MAX_PREDICATE_LEN)?;
        check_opt_len("subject_type", &self.subject_type, MAX_TYPE_LEN)?;
        check_opt_len("object_type", &self.object_type, MAX_TYPE_LEN)?;
        if self.evidence.len() > MAX_EVIDENCE_PER_ASSERTION {
            return Err(GraphError::InvalidInput(format!(
                "{} evidence items exceeds the {MAX_EVIDENCE_PER_ASSERTION}-item limit per assertion",
                self.evidence.len()
            )));
        }
        for e in &self.evidence {
            e.validate()?;
        }
        Ok(())
    }
}

impl GraphService {
    /// Assert a relationship, then attach its evidence as separate
    /// `ADD_EVIDENCE` events (spec section 33: evidence is added
    /// separately). Requires `graph:assert`.
    pub async fn assert(&self, auth: &AuthContext, input: AssertInput) -> Result<AssertionId, GraphError> {
        auth.require(Permission::GraphAssert)?;
        input.validate()?;

        let (subject_identifier, subject_kind) = canonicalize_value(&input.subject);
        let (object_identifier, object_kind) = canonicalize_value(&input.object);
        let subject_type = input
            .subject_type
            .or_else(|| subject_kind.map(String::from))
            .unwrap_or_else(|| "unknown".to_string());
        let object_type = input
            .object_type
            .or_else(|| object_kind.map(String::from))
            .unwrap_or_else(|| "unknown".to_string());

        let now = self.now();
        let (assertion_id, _) = commit_local_event(
            self.pool(),
            self.identity(),
            EventPayload::AssertRelation(AssertRelationPayload {
                subject_identifier,
                subject_type,
                predicate: input.predicate,
                object_identifier,
                object_type,
                actor_id: auth.actor_id.to_hex(),
                actor_confidence: input.actor_confidence,
                observed_at: input.observed_at,
                extraction_method: "direct".to_string(),
            }),
            now,
        )
        .await?;

        for evidence in input.evidence {
            self.add_evidence_internal(assertion_id, evidence).await?;
        }

        Ok(assertion_id)
    }

    /// Attach a piece of evidence to an existing assertion. Requires
    /// `graph:assert` (evidence is part of building the case for a claim,
    /// same permission as making one).
    pub async fn add_evidence(
        &self,
        auth: &AuthContext,
        assertion_id: AssertionId,
        evidence: EvidenceInput,
    ) -> Result<(), GraphError> {
        auth.require(Permission::GraphAssert)?;
        evidence.validate()?;
        self.add_evidence_internal(assertion_id, evidence).await
    }

    async fn add_evidence_internal(
        &self,
        assertion_id: AssertionId,
        evidence: EvidenceInput,
    ) -> Result<(), GraphError> {
        commit_local_event(
            self.pool(),
            self.identity(),
            EventPayload::AddEvidence(AddEvidencePayload {
                assertion_id: assertion_id.to_hex(),
                evidence_type: evidence.evidence_type.unwrap_or_else(|| "other".to_string()),
                uri: evidence.uri,
                title: evidence.title,
                excerpt: evidence.excerpt,
                content_hash: evidence.content_hash,
                observed_at: evidence.observed_at,
                retrieved_at: evidence.retrieved_at,
            }),
            self.now(),
        )
        .await?;
        Ok(())
    }

    /// Requires `graph:verify`.
    pub async fn verify_assertion(
        &self,
        auth: &AuthContext,
        assertion_id: AssertionId,
        result: String,
        observed_at: i64,
    ) -> Result<(), GraphError> {
        auth.require(Permission::GraphVerify)?;
        commit_local_event(
            self.pool(),
            self.identity(),
            EventPayload::VerifyAssertion(VerifyAssertionPayload {
                assertion_id: assertion_id.to_hex(),
                observer_actor_id: auth.actor_id.to_hex(),
                result,
                observed_at,
            }),
            self.now(),
        )
        .await?;
        Ok(())
    }

    /// Requires `graph:assert` (disputing is itself a claim about a claim).
    pub async fn dispute_assertion(
        &self,
        auth: &AuthContext,
        disputed_assertion_id: AssertionId,
        reason: Option<String>,
    ) -> Result<(), GraphError> {
        auth.require(Permission::GraphAssert)?;
        check_opt_len("reason", &reason, MAX_REASON_LEN)?;
        commit_local_event(
            self.pool(),
            self.identity(),
            EventPayload::DisputeAssertion(DisputeAssertionPayload {
                disputed_assertion_id: disputed_assertion_id.to_hex(),
                disputing_actor_id: auth.actor_id.to_hex(),
                reason,
            }),
            self.now(),
        )
        .await?;
        Ok(())
    }

    /// Requires `graph:retract-own`, and the caller must be the assertion's
    /// original asserting actor — "own" is enforced, not just checked for
    /// presence. Callers holding `admin` may retract any assertion.
    pub async fn retract_assertion(
        &self,
        auth: &AuthContext,
        retracted_assertion_id: AssertionId,
        reason: Option<String>,
    ) -> Result<(), GraphError> {
        auth.require(Permission::GraphRetractOwn)?;
        check_opt_len("reason", &reason, MAX_REASON_LEN)?;

        let assertion = self
            .get_assertion(retracted_assertion_id)
            .await?
            .ok_or_else(|| GraphError::NotFound(format!("assertion {retracted_assertion_id}")))?;
        if assertion.actor_id != auth.actor_id && !auth.permissions.contains(&Permission::Admin) {
            return Err(GraphError::PermissionDenied(
                "graph:retract-own requires being the original asserting actor (or admin)",
            ));
        }

        commit_local_event(
            self.pool(),
            self.identity(),
            EventPayload::RetractAssertion(RetractAssertionPayload {
                retracted_assertion_id: retracted_assertion_id.to_hex(),
                actor_id: auth.actor_id.to_hex(),
                reason,
            }),
            self.now(),
        )
        .await?;
        Ok(())
    }

    /// Requires `graph:assert`.
    pub async fn supersede_assertion(
        &self,
        auth: &AuthContext,
        old_assertion_id: AssertionId,
        new_assertion_id: AssertionId,
    ) -> Result<(), GraphError> {
        auth.require(Permission::GraphAssert)?;
        commit_local_event(
            self.pool(),
            self.identity(),
            EventPayload::SupersedeAssertion(SupersedeAssertionPayload {
                old_assertion_id: old_assertion_id.to_hex(),
                new_assertion_id: new_assertion_id.to_hex(),
                actor_id: auth.actor_id.to_hex(),
            }),
            self.now(),
        )
        .await?;
        Ok(())
    }

    pub async fn get_assertion(
        &self,
        id: AssertionId,
    ) -> Result<Option<oag_core::Assertion>, GraphError> {
        let mut conn = self
            .pool()
            .acquire()
            .await
            .map_err(oag_storage::StorageError::from)?;
        Ok(oag_storage::repo::assertions::get_by_id(&mut conn, id).await?)
    }

    pub async fn list_evidence(
        &self,
        assertion_id: AssertionId,
    ) -> Result<Vec<oag_core::Evidence>, GraphError> {
        let mut conn = self
            .pool()
            .acquire()
            .await
            .map_err(oag_storage::StorageError::from)?;
        Ok(oag_storage::repo::assertions::list_evidence(&mut conn, assertion_id).await?)
    }
}
