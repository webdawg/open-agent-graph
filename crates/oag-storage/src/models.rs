//! Raw row shapes as they sit in SQLite. Repository functions convert to/from
//! `oag_core` domain types at the boundary; nothing outside `oag-storage`
//! should need these directly.

use sqlx::FromRow;

#[derive(Debug, FromRow)]
pub struct EventRow {
    pub event_id: Vec<u8>,
    pub origin_peer_id: Vec<u8>,
    pub sequence: i64,
    pub previous_event_id: Option<Vec<u8>>,
    pub event_type: String,
    pub canonical_payload: Vec<u8>,
    pub created_at: i64,
    pub signature: Vec<u8>,
    pub received_at: i64,
}

#[derive(Debug, FromRow)]
pub struct EventOriginRow {
    pub origin_peer_id: Vec<u8>,
    pub highest_contiguous_sequence: i64,
    pub highest_seen_sequence: i64,
    pub head_event_id: Option<Vec<u8>>,
}

#[derive(Debug, FromRow)]
pub struct NodeRow {
    pub node_id: Vec<u8>,
    pub node_type: String,
    pub canonical_identifier: String,
    pub canonical_uri: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub metadata: String,
    pub created_at: i64,
}

#[derive(Debug, FromRow)]
pub struct EdgeRow {
    pub edge_id: Vec<u8>,
    pub subject_node_id: Vec<u8>,
    pub predicate: String,
    pub object_node_id: Vec<u8>,
    pub created_at: i64,
}

#[derive(Debug, FromRow)]
pub struct ActorRow {
    pub actor_id: Vec<u8>,
    pub actor_type: String,
    pub name: Option<String>,
    pub public_key: Option<Vec<u8>>,
    pub identity_uri: Option<String>,
    pub metadata: String,
    pub created_at: i64,
}

#[derive(Debug, FromRow)]
pub struct ActorKeyRow {
    pub key_hash: Vec<u8>,
    pub actor_id: Vec<u8>,
    pub permissions: String,
    pub created_at: i64,
    pub revoked_at: Option<i64>,
}

#[derive(Debug, FromRow)]
pub struct AssertionRow {
    pub id: Vec<u8>,
    pub edge_id: Vec<u8>,
    pub actor_id: Vec<u8>,
    pub actor_confidence: Option<f64>,
    pub observed_at: Option<i64>,
    pub asserted_at: i64,
    pub extraction_method: String,
    pub status: String,
}

#[derive(Debug, FromRow)]
pub struct EvidenceRow {
    pub id: Vec<u8>,
    pub assertion_id: Vec<u8>,
    pub evidence_type: String,
    pub uri: Option<String>,
    pub title: Option<String>,
    pub excerpt: Option<String>,
    pub content_hash: Option<String>,
    pub observed_at: Option<i64>,
    pub retrieved_at: Option<i64>,
    pub metadata: String,
}

#[derive(Debug, FromRow)]
pub struct ObservationRow {
    pub id: Vec<u8>,
    pub assertion_id: Vec<u8>,
    pub observer_actor_id: Vec<u8>,
    pub result: String,
    pub observed_at: i64,
    pub created_at: i64,
}

#[derive(Debug, FromRow)]
pub struct DisputeRow {
    pub id: Vec<u8>,
    pub disputed_assertion_id: Vec<u8>,
    pub disputing_actor_id: Vec<u8>,
    pub reason: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, FromRow)]
pub struct RetractionRow {
    pub id: Vec<u8>,
    pub retracted_assertion_id: Vec<u8>,
    pub actor_id: Vec<u8>,
    pub reason: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, FromRow)]
pub struct SupersessionRow {
    pub id: Vec<u8>,
    pub old_assertion_id: Vec<u8>,
    pub new_assertion_id: Vec<u8>,
    pub actor_id: Vec<u8>,
    pub created_at: i64,
}

#[derive(Debug, FromRow)]
pub struct PeerRow {
    pub peer_id: Vec<u8>,
    pub public_key: Vec<u8>,
    pub name: Option<String>,
    pub first_seen: i64,
    pub last_seen: Option<i64>,
    pub forked: bool,
}

#[derive(Debug, FromRow)]
pub struct PeerAddressRow {
    pub peer_id: Vec<u8>,
    pub address: String,
}

#[derive(Debug, FromRow)]
pub struct PeerForkRow {
    pub peer_id: Vec<u8>,
    pub sequence: i64,
    pub event_id_a: Vec<u8>,
    pub event_id_b: Vec<u8>,
    pub detected_at: i64,
}
