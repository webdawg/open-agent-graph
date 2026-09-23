use oag_core::EventId;
use oag_crypto::{sign_with_domain, PeerIdentity};

use crate::envelope::{SignedEvent, UnsignedEvent, CURRENT_VERSION, EVENT_ID_DOMAIN};
use crate::error::EventsError;
use crate::payload::EventPayload;

/// Build, canonicalize, and sign a new event originating from this peer
/// (spec sections 27-28's exact pipeline, pinned in the plan):
/// 1. build the unsigned event
/// 2. canonicalize it (JCS)
/// 3. sign `"OAG:EVENT:v1:" || canonical_unsigned`
/// 4. attach the signature
/// 5. `event_id = BLAKE3("OAG:EVENT:v1:" || JCS(signed_event))`
pub fn build_and_sign(
    identity: &PeerIdentity,
    sequence: u64,
    previous_event: Option<EventId>,
    created_at: i64,
    payload: EventPayload,
) -> Result<(SignedEvent, EventId), EventsError> {
    let unsigned = UnsignedEvent {
        version: CURRENT_VERSION,
        origin_peer: identity.peer_id().to_string(),
        sequence,
        previous_event: previous_event.map(|id| id.to_hex()),
        created_at,
        payload,
    };

    let canonical_unsigned = oag_core::canonical_json_bytes(&unsigned)?;
    let signature = sign_with_domain(identity.signing_key(), EVENT_ID_DOMAIN, &canonical_unsigned);

    let signed = SignedEvent {
        unsigned,
        signature: hex::encode(signature.to_bytes()),
    };

    let canonical_signed = oag_core::canonical_json_bytes(&signed)?;
    let event_id = EventId::derive(&canonical_signed);

    Ok((signed, event_id))
}
