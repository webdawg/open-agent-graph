use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use crate::ids::ActorId;

/// Who submitted information (spec section 25). Distinct from a network
/// peer (spec section 26) — a single peer serves many actors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorType {
    Human,
    Agent,
    Model,
    Crawler,
    Organization,
    Domain,
    Service,
    Peer,
    Anonymous,
}

impl ActorType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ActorType::Human => "human",
            ActorType::Agent => "agent",
            ActorType::Model => "model",
            ActorType::Crawler => "crawler",
            ActorType::Organization => "organization",
            ActorType::Domain => "domain",
            ActorType::Service => "service",
            ActorType::Peer => "peer",
            ActorType::Anonymous => "anonymous",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "human" => ActorType::Human,
            "agent" => ActorType::Agent,
            "model" => ActorType::Model,
            "crawler" => ActorType::Crawler,
            "organization" => ActorType::Organization,
            "domain" => ActorType::Domain,
            "service" => ActorType::Service,
            "peer" => ActorType::Peer,
            "anonymous" => ActorType::Anonymous,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Actor {
    pub id: ActorId,
    pub actor_type: ActorType,
    pub name: Option<String>,
    /// Ed25519 public key, if this actor signs its own assertions
    /// independently of the peer that relays them.
    pub public_key: Option<[u8; 32]>,
    pub identity_uri: Option<String>,
    #[serde(default = "default_metadata")]
    pub metadata: JsonValue,
    pub created_at: i64,
}

fn default_metadata() -> JsonValue {
    JsonValue::Object(Default::default())
}

/// Permissions an API key can carry (spec section 58, trimmed to what this
/// milestone can actually enforce — no crawler/peer subsystems yet).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Permission {
    GraphRead,
    GraphAssert,
    GraphVerify,
    GraphRetractOwn,
    Admin,
}

impl Permission {
    pub fn as_str(&self) -> &'static str {
        match self {
            Permission::GraphRead => "graph:read",
            Permission::GraphAssert => "graph:assert",
            Permission::GraphVerify => "graph:verify",
            Permission::GraphRetractOwn => "graph:retract-own",
            Permission::Admin => "admin",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "graph:read" => Permission::GraphRead,
            "graph:assert" => Permission::GraphAssert,
            "graph:verify" => Permission::GraphVerify,
            "graph:retract-own" => Permission::GraphRetractOwn,
            "admin" => Permission::Admin,
            _ => return None,
        })
    }
}
