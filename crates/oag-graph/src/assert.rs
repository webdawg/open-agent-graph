use oag_core::{AssertionId, Permission};
use oag_events::payload::{
    AddEvidencePayload, AssertRelationPayload, DisputeAssertionPayload, RetractAssertionPayload,
    SupersedeAssertionPayload, VerifyAssertionPayload,
};
use oag_events::{commit_local_event, EventPayload};

use crate::error::GraphError;
use crate::identifier::canonicalize_value;
use crate::service::{AuthContext, GraphService};

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

impl GraphService {
    /// Assert a relationship, then attach its evidence as separate
    /// `ADD_EVIDENCE` events (spec section 33: evidence is added
    /// separately). Requires `graph:assert`.
    pub async fn assert(&self, auth: &AuthContext, input: AssertInput) -> Result<AssertionId, GraphError> {
        auth.require(Permission::GraphAssert)?;

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
