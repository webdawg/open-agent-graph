use std::fmt;

use serde::{Deserialize, Serialize};

/// A predicate name. Deliberately an open string newtype rather than a closed
/// enum — spec section 20 explicitly says not to build a universal ontology.
/// [`KNOWN_PREDICATES`] documents the v0 vocabulary but unknown predicates
/// MUST NOT be rejected.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Predicate(String);

impl Predicate {
    /// Canonicalize a predicate string: trim whitespace, lowercase, collapse
    /// internal whitespace to underscores. Two contributors writing
    /// `"implements"` and `" Implements "` must resolve to the same edge.
    pub fn new(raw: impl AsRef<str>) -> Self {
        let normalized = raw
            .as_ref()
            .trim()
            .to_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join("_");
        Self(normalized)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Predicate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for Predicate {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<String> for Predicate {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

/// The v0 predicate vocabulary (spec section 20). Not exhaustive — new
/// predicates may be asserted freely.
pub const KNOWN_PREDICATES: &[&str] = &[
    "official_source",
    "documentation_for",
    "authored_by",
    "published_by",
    "owned_by",
    "operated_by",
    "maintained_by",
    "implements",
    "supports",
    "does_not_support",
    "requires",
    "depends_on",
    "compatible_with",
    "exposes",
    "references",
    "links_to",
    "derived_from",
    "source_for",
    "cites",
    "describes",
    "explains",
    "example_of",
    "instance_of",
    "part_of",
    "version_of",
    "supersedes",
    "deprecated_by",
    "replaced_by",
    "supports_claim",
    "contradicts",
    "disputes",
    "alternative_to",
    "related_to",
    "provides_capability",
    "uses_protocol",
    "exposed_via",
    "download_at",
    "repository_at",
    "documented_at",
    "homepage",
];
