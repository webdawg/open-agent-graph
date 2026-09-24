use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use crate::ids::NodeId;

/// A node's type. Open string newtype, not a closed enum — spec section 16
/// requires the vocabulary to remain extensible; unknown types must not be
/// rejected.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeType(String);

impl NodeType {
    pub fn new(raw: impl AsRef<str>) -> Self {
        Self(raw.as_ref().trim().to_lowercase())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for NodeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for NodeType {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

macro_rules! known_node_types {
    ($($konst:ident => $name:literal),+ $(,)?) => {
        impl NodeType {
            $(pub fn $konst() -> Self { Self($name.to_string()) })+
        }

        pub const KNOWN_NODE_TYPES: &[&str] = &[$($name),+];
    };
}

known_node_types! {
    url => "url",
    domain => "domain",
    document => "document",
    website => "website",
    person => "person",
    organization => "organization",
    software => "software",
    repository => "repository",
    package => "package",
    api => "api",
    protocol => "protocol",
    mcp_server => "mcp_server",
    agent => "agent",
    dataset => "dataset",
    product => "product",
    service => "service",
    concept => "concept",
    place => "place",
    event => "event",
    capability => "capability",
    unknown => "unknown",
}

/// Something in the world or on the Internet (spec section 16).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub node_type: NodeType,
    /// The identifier string this node's ID was derived from, e.g.
    /// `"url:https://www.rust-lang.org/"` or `"uuid:<uuidv7>"`.
    pub canonical_identifier: String,
    pub canonical_uri: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    #[serde(default = "default_metadata")]
    pub metadata: JsonValue,
    pub created_at: i64,
}

fn default_metadata() -> JsonValue {
    JsonValue::Object(Default::default())
}

impl Node {
    /// Construct a node, deriving its ID from `canonical_identifier`.
    pub fn new(
        node_type: NodeType,
        canonical_identifier: impl Into<String>,
        created_at: i64,
    ) -> Self {
        let canonical_identifier = canonical_identifier.into();
        let id = NodeId::from_canonical_identifier(&canonical_identifier);
        Self {
            id,
            node_type,
            canonical_identifier,
            canonical_uri: None,
            name: None,
            description: None,
            metadata: default_metadata(),
            created_at,
        }
    }
}

/// An alias for a node — an alternative name, URL, URN, package id, etc.
/// (spec section 17). Stored independently from the node itself.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeAlias {
    pub node_id: NodeId,
    pub alias: String,
    pub alias_type: AliasType,
    pub source_assertion: Option<crate::ids::AssertionId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AliasType {
    Name,
    Url,
    Urn,
    Package,
    ExternalId,
    Acronym,
}

impl AliasType {
    pub fn as_str(&self) -> &'static str {
        match self {
            AliasType::Name => "name",
            AliasType::Url => "url",
            AliasType::Urn => "urn",
            AliasType::Package => "package",
            AliasType::ExternalId => "external_id",
            AliasType::Acronym => "acronym",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "name" => AliasType::Name,
            "url" => AliasType::Url,
            "urn" => AliasType::Urn,
            "package" => AliasType::Package,
            "external_id" => AliasType::ExternalId,
            "acronym" => AliasType::Acronym,
            _ => return None,
        })
    }
}
