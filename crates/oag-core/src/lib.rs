pub mod actor;
pub mod assertion;
pub mod canonicalize;
pub mod edge;
pub mod evidence;
pub mod ids;
pub mod node;
pub mod predicate;

pub use actor::{Actor, ActorType, Permission};
pub use assertion::{Assertion, AssertionStatus, ExtractionMethod, VerifyResult};
pub use canonicalize::{canonical_identifier, canonicalize_url, generate_uuid_identifier};
pub use edge::Edge;
pub use evidence::{Evidence, EvidenceType};
pub use ids::{ActorId, AssertionId, EdgeId, EventId, Hash32, NodeId};
pub use node::{AliasType, Node, NodeAlias, NodeType};
pub use predicate::{Predicate, KNOWN_PREDICATES};

/// Serialize a value to RFC 8785 canonical JSON bytes. This is the single
/// place `serde_jcs` gets called from domain code, so canonicalization stays
/// consistent between node/edge identifier hashing call sites.
pub fn canonical_json_bytes<T: serde::Serialize>(value: &T) -> Result<Vec<u8>, serde_json::Error> {
    serde_jcs::to_vec(value)
}
