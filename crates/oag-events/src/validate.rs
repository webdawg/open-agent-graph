use oag_core::EventId;
use oag_crypto::{verify_with_domain, Signature, VerifyingKey};

use crate::envelope::{SignedEvent, EVENT_ID_DOMAIN};
use crate::error::EventsError;

/// Verify a signed event's signature and recompute its `EventId` from the
/// canonical bytes of the whole envelope (spec section 28 — never trust a
/// caller-supplied id). Returns the recomputed id on success.
pub fn verify_and_derive_id(
    signed: &SignedEvent,
    verifying_key: &VerifyingKey,
) -> Result<EventId, EventsError> {
    let canonical_unsigned = oag_core::canonical_json_bytes(&signed.unsigned)?;
    let decoded = hex::decode(&signed.signature)?;
    let decoded_len = decoded.len();
    let signature_bytes: [u8; 64] = decoded
        .try_into()
        .map_err(|_| EventsError::BadSignatureLength(decoded_len))?;
    let signature = Signature::from_bytes(&signature_bytes);
    verify_with_domain(verifying_key, EVENT_ID_DOMAIN, &canonical_unsigned, &signature)?;

    let canonical_signed = oag_core::canonical_json_bytes(signed)?;
    Ok(EventId::derive(&canonical_signed))
}

/// Check that `sequence`/`previous_event` continue this origin's chain
/// without a gap (spec sections 29, 54). `expected_next_sequence` and
/// `expected_head` come from the origin's current `event_origins` row (both
/// are `1`/`None` for a brand-new origin).
pub fn validate_chain(
    expected_next_sequence: u64,
    expected_head: Option<EventId>,
    sequence: u64,
    previous_event: Option<EventId>,
) -> Result<(), EventsError> {
    if sequence != expected_next_sequence {
        return Err(EventsError::SequenceGap {
            expected: expected_next_sequence,
            got: sequence,
        });
    }
    if previous_event != expected_head {
        return Err(EventsError::PreviousEventMismatch {
            expected: expected_head.map(|id| id.to_hex()),
            declared: previous_event.map(|id| id.to_hex()),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::build_and_sign;
    use crate::payload::{AssertRelationPayload, EventPayload};
    use oag_crypto::PeerIdentity;

    fn sample_payload() -> EventPayload {
        EventPayload::AssertRelation(AssertRelationPayload {
            subject_identifier: "url:https://github.com/example/foo".into(),
            subject_type: "repository".into(),
            predicate: "implements".into(),
            object_identifier: "concept:model-context-protocol".into(),
            object_type: "concept".into(),
            actor_id: "deadbeef".into(),
            actor_confidence: Some(0.9),
            observed_at: None,
            extraction_method: "direct".into(),
        })
    }

    #[test]
    fn signed_event_verifies_and_id_is_deterministic() {
        let identity = PeerIdentity::generate();
        let (signed, event_id) =
            build_and_sign(&identity, 1, None, 1_700_000_000, sample_payload()).unwrap();

        let recomputed = verify_and_derive_id(&signed, &identity.verifying_key()).unwrap();
        assert_eq!(event_id, recomputed);

        // Rebuilding from the same signed bytes must yield the same id again
        // (spec invariant 9: projection/derivation is deterministic).
        let recomputed_again = verify_and_derive_id(&signed, &identity.verifying_key()).unwrap();
        assert_eq!(recomputed, recomputed_again);
    }

    #[test]
    fn tampered_payload_fails_verification() {
        let identity = PeerIdentity::generate();
        let (mut signed, _) =
            build_and_sign(&identity, 1, None, 1_700_000_000, sample_payload()).unwrap();
        if let EventPayload::AssertRelation(ref mut p) = signed.unsigned.payload {
            p.predicate = "contradicts".into();
        }
        assert!(verify_and_derive_id(&signed, &identity.verifying_key()).is_err());
    }

    #[test]
    fn chain_rejects_sequence_gap() {
        assert!(validate_chain(5, None, 6, None).is_err());
    }

    #[test]
    fn chain_rejects_wrong_previous() {
        let identity = PeerIdentity::generate();
        let (_, event_id) =
            build_and_sign(&identity, 1, None, 1_700_000_000, sample_payload()).unwrap();
        assert!(validate_chain(2, Some(event_id), 2, None).is_err());
        assert!(validate_chain(2, Some(event_id), 2, Some(event_id)).is_ok());
    }
}
