use serde::{Deserialize, Serialize};

use crate::ids::{EdgeId, NodeId};
use crate::predicate::Predicate;

/// A semantic relationship between two nodes. Per spec section 19, an Edge is
/// **not** proof that the relationship is true — it only identifies the
/// relationship being discussed. [`crate::assertion::Assertion`] carries the
/// claim.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub id: EdgeId,
    pub subject: NodeId,
    pub predicate: Predicate,
    pub object: NodeId,
    pub created_at: i64,
}

impl Edge {
    pub fn new(subject: NodeId, predicate: Predicate, object: NodeId, created_at: i64) -> Self {
        let id = EdgeId::from_triple(subject, predicate.as_str(), object);
        Self {
            id,
            subject,
            predicate,
            object,
            created_at,
        }
    }
}
