use oag_core::{Actor, ActorId, ActorType, Permission};
use oag_crypto::PeerIdentity;
use oag_events::payload::{ActorDeclarePayload, ActorKeyAddPayload};
use oag_events::{commit_local_event, EventPayload, ProjectionOutcome};
use oag_storage::repo::actors;
use oag_storage::SqlitePool;

use crate::error::GraphError;

/// The single service layer REST and MCP both call into (spec section 90) —
/// no business logic lives in either transport's handlers.
pub struct GraphService {
    pool: SqlitePool,
    identity: PeerIdentity,
}

impl GraphService {
    pub fn new(pool: SqlitePool, identity: PeerIdentity) -> Self {
        Self { pool, identity }
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub fn identity(&self) -> &PeerIdentity {
        &self.identity
    }

    pub(crate) fn now(&self) -> i64 {
        chrono_now()
    }
}

fn chrono_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// An authenticated caller's identity and permissions, resolved from an API
/// key (spec section 57).
#[derive(Debug, Clone)]
pub struct AuthContext {
    pub actor_id: ActorId,
    pub permissions: Vec<Permission>,
}

impl AuthContext {
    pub fn require(&self, permission: Permission) -> Result<(), GraphError> {
        if self.permissions.contains(&permission) || self.permissions.contains(&Permission::Admin) {
            Ok(())
        } else {
            Err(GraphError::PermissionDenied(permission.as_str()))
        }
    }
}

impl GraphService {
    /// Resolve a raw `Authorization: Bearer <key>` value to its actor and
    /// permissions. Only the BLAKE3 hash of the key is ever compared against
    /// storage (spec section 57 — raw keys are never persisted).
    pub async fn authenticate(&self, raw_api_key: &str) -> Result<AuthContext, GraphError> {
        let key_hash = *blake3::hash(raw_api_key.as_bytes()).as_bytes();
        let mut conn = self.pool.acquire().await.map_err(oag_storage::StorageError::from)?;
        let found = actors::find_active_key(&mut conn, &key_hash).await?;
        match found {
            Some(key) => Ok(AuthContext {
                actor_id: key.actor_id,
                permissions: key.permissions,
            }),
            None => Err(GraphError::InvalidApiKey),
        }
    }

    /// True if no actor exists yet in this peer's database — used to decide
    /// whether `oag serve` needs to mint the one-time bootstrap admin key.
    pub async fn has_any_actor(&self) -> Result<bool, GraphError> {
        let mut conn = self.pool.acquire().await.map_err(oag_storage::StorageError::from)?;
        Ok(actors::any_actor_exists(&mut conn).await?)
    }

    /// Declare a new actor (spec section 25). Returns its (deterministic,
    /// possibly-deduplicated) id.
    pub async fn declare_actor(
        &self,
        actor_type: ActorType,
        name: Option<String>,
        identity_uri: Option<String>,
    ) -> Result<ActorId, GraphError> {
        let now = self.now();
        let (_, outcome) = commit_local_event(
            &self.pool,
            &self.identity,
            EventPayload::ActorDeclare(ActorDeclarePayload {
                actor_type: actor_type.as_str().to_string(),
                name,
                public_key: None,
                identity_uri,
            }),
            now,
        )
        .await?;
        match outcome {
            ProjectionOutcome::ActorDeclared { actor_id } => Ok(actor_id),
            _ => unreachable!("ActorDeclare always yields ActorDeclared"),
        }
    }

    /// Mint a fresh API key for `actor_id` and attach it via an
    /// `ACTOR_KEY_ADD` event. Returns the raw key exactly once — only its
    /// hash is ever persisted (spec section 57).
    pub async fn create_key(
        &self,
        actor_id: ActorId,
        permissions: Vec<Permission>,
    ) -> Result<String, GraphError> {
        let raw = oag_crypto::random_bytes_32();
        let raw_key = format!("oagk_{}", hex::encode(raw));
        let key_hash = blake3::hash(raw_key.as_bytes());

        commit_local_event(
            &self.pool,
            &self.identity,
            EventPayload::ActorKeyAdd(ActorKeyAddPayload {
                actor_id: actor_id.to_hex(),
                key_hash: key_hash.to_hex().to_string(),
                permissions: permissions.iter().map(|p| p.as_str().to_string()).collect(),
            }),
            self.now(),
        )
        .await?;

        Ok(raw_key)
    }

    pub async fn get_actor(&self, actor_id: ActorId) -> Result<Option<Actor>, GraphError> {
        let mut conn = self.pool.acquire().await.map_err(oag_storage::StorageError::from)?;
        Ok(actors::get_by_id(&mut conn, actor_id).await?)
    }
}
