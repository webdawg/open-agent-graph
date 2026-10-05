use oag_core::{Actor, ActorId, ActorType, Permission};
use oag_crypto::{verify_with_domain, PeerIdentity, Signature, VerifyingKey};
use oag_events::payload::{ActorDeclarePayload, ActorKeyAddPayload};
use oag_events::{commit_local_event, EventPayload, ProjectionOutcome};
use oag_storage::repo::actors;
use oag_storage::SqlitePool;
use serde::Serialize;

use crate::assert::check_opt_len;
use crate::error::GraphError;

/// Domain prefix for the message an actor signs to prove possession of the
/// private key behind a `PublicKeyProof` (spec section 13's domain
/// separation, same pattern as event signing).
pub(crate) const ACTOR_KEY_PROOF_DOMAIN: &str = "OAG:ACTOR_KEY_PROOF:v1:";

// Same spec section 61 rationale as assert.rs's MAX_* constants: an
// unbounded `name`/`identity_uri` would otherwise permanently bloat the
// event log with no guard at all, unlike every other text field this
// service layer accepts.
const MAX_ACTOR_NAME_LEN: usize = 512;
const MAX_IDENTITY_URI_LEN: usize = 2048;

/// `pub(crate)` (not just `pub`) so tests can construct the exact same
/// message a real caller's tooling would sign, without duplicating and
/// risking drift from the shape actually verified below.
#[derive(Serialize)]
pub(crate) struct ActorKeyProofMessage<'a> {
    pub actor_type: &'a str,
    pub name: Option<&'a str>,
    pub identity_uri: Option<&'a str>,
}

/// Proof that the caller controls the private key behind `public_key`:
/// `signature` must be `sign_with_domain(signing_key, "OAG:ACTOR_KEY_PROOF:v1:",
/// canonical_json_bytes(&ActorKeyProofMessage { actor_type, name, identity_uri }))`
/// — i.e. a signature over the *exact* `actor_type`/`name`/`identity_uri`
/// being declared, binding the proof to this one declaration rather than
/// letting a signature be replayed onto a different label for the same key.
/// The private key itself never needs to touch this peer — the proof is
/// produced entirely by the caller's own tooling.
pub struct PublicKeyProof {
    pub public_key: [u8; 32],
    pub signature: [u8; 64],
}

/// The single service layer REST and MCP both call into (spec section 90) —
/// no business logic lives in either transport's handlers.
pub struct GraphService {
    pool: SqlitePool,
    identity: PeerIdentity,
    /// Serializes every local commit (spec section 38's one-transaction
    /// guarantee is necessary but not sufficient on its own): confirmed
    /// live that concurrent calls into `commit_local_event` for the same
    /// peer -- e.g. two simultaneous REST requests -- can hit SQLite's
    /// `SQLITE_BUSY_SNAPSHOT` in WAL mode, which `busy_timeout` does not
    /// resolve by waiting (a stale read snapshot can't be fixed by
    /// retrying the same write; the whole read-then-write needs to
    /// restart). A given peer has exactly one identity and therefore
    /// exactly one logical writer of its own event chain ever -- there's
    /// no reason local commits should ever race at the SQLite level at
    /// all, so this makes that true in-process rather than hoping the
    /// database's own locking happens to cover it.
    commit_lock: tokio::sync::Mutex<()>,
}

impl GraphService {
    pub fn new(pool: SqlitePool, identity: PeerIdentity) -> Self {
        Self { pool, identity, commit_lock: tokio::sync::Mutex::new(()) }
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

    /// The only path any `GraphService` method should use to commit a
    /// local event -- see `commit_lock`'s own doc comment for why.
    pub(crate) async fn commit_event(
        &self,
        payload: EventPayload,
        created_at: i64,
    ) -> Result<(oag_core::EventId, ProjectionOutcome), oag_events::EventsError> {
        let _guard = self.commit_lock.lock().await;
        commit_local_event(&self.pool, &self.identity, payload, created_at).await
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

    /// Find an actor by exact `name` + `actor_type` — used to reuse a
    /// stable local actor (e.g. "the crawler actor") across separate
    /// process invocations instead of minting a fresh one each time.
    pub async fn find_actor_by_name(
        &self,
        name: &str,
        actor_type: ActorType,
    ) -> Result<Option<Actor>, GraphError> {
        let mut conn = self.pool.acquire().await.map_err(oag_storage::StorageError::from)?;
        Ok(actors::find_by_name_and_type(&mut conn, name, actor_type).await?)
    }

    /// Declare a new actor (spec section 25). Returns its (deterministic,
    /// possibly-deduplicated) id. `public_key_proof`, if given, must verify
    /// against the exact `actor_type`/`name`/`identity_uri` supplied here
    /// (see [`PublicKeyProof`]) — an invalid or mismatched proof is rejected
    /// before anything is committed, exactly like the field-length checks in
    /// `assert.rs`. A verified public key raises this actor's
    /// `identity_assurance` ranking signal (spec section 65) and gives it a
    /// stable, key-derived `ActorId` that dedups across repeat declarations.
    pub async fn declare_actor(
        &self,
        actor_type: ActorType,
        name: Option<String>,
        identity_uri: Option<String>,
        public_key_proof: Option<PublicKeyProof>,
    ) -> Result<ActorId, GraphError> {
        check_opt_len("name", &name, MAX_ACTOR_NAME_LEN)?;
        check_opt_len("identity_uri", &identity_uri, MAX_IDENTITY_URI_LEN)?;

        let public_key_hex = match &public_key_proof {
            Some(proof) => {
                let verifying_key = VerifyingKey::from_bytes(&proof.public_key)
                    .map_err(|_| GraphError::InvalidInput("invalid public key bytes".to_string()))?;
                let message = ActorKeyProofMessage {
                    actor_type: actor_type.as_str(),
                    name: name.as_deref(),
                    identity_uri: identity_uri.as_deref(),
                };
                let canonical = oag_core::canonical_json_bytes(&message)
                    .map_err(|e| GraphError::InvalidInput(format!("failed to canonicalize proof message: {e}")))?;
                let signature = Signature::from_bytes(&proof.signature);
                verify_with_domain(&verifying_key, ACTOR_KEY_PROOF_DOMAIN, &canonical, &signature)
                    .map_err(|_| GraphError::InvalidInput("public key proof signature does not verify".to_string()))?;
                Some(hex::encode(proof.public_key))
            }
            None => None,
        };

        let now = self.now();
        let (_, outcome) = self
            .commit_event(
                EventPayload::ActorDeclare(ActorDeclarePayload {
                    actor_type: actor_type.as_str().to_string(),
                    name,
                    public_key: public_key_hex,
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

        self.commit_event(
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

    /// Every key ever issued on this peer, active and revoked alike (never
    /// the raw key itself -- only its hash is ever stored). No `AuthContext`
    /// parameter, matching `create_key`'s own existing convention: key
    /// management is a data-dir-filesystem-trust CLI operation today, same
    /// as `oag backup`/`oag rebuild`, not something REST/MCP exposes.
    pub async fn list_keys(&self) -> Result<Vec<oag_storage::repo::actors::ApiKeyInfo>, GraphError> {
        let mut conn = self.pool.acquire().await.map_err(oag_storage::StorageError::from)?;
        Ok(actors::list_keys(&mut conn).await?)
    }

    /// Revoke a previously-issued key by its hex-encoded hash (as shown by
    /// `list_keys`), via an `ACTOR_KEY_REVOKE` event (spec section 106
    /// invariant 1 -- revocation is a mutation like any other, not a raw
    /// `UPDATE`). Errors with `GraphError::NotFound` if `key_hash_hex`
    /// doesn't match any currently-active key, rather than silently
    /// succeeding on a typo or an already-revoked key.
    pub async fn revoke_key(&self, key_hash_hex: &str) -> Result<(), GraphError> {
        let key_hash_bytes = hex::decode(key_hash_hex)
            .map_err(|e| GraphError::InvalidInput(format!("invalid key hash '{key_hash_hex}': {e}")))?;
        let key_hash: [u8; 32] = key_hash_bytes
            .try_into()
            .map_err(|_| GraphError::InvalidInput(format!("key hash '{key_hash_hex}' must be 32 bytes")))?;

        let mut conn = self.pool.acquire().await.map_err(oag_storage::StorageError::from)?;
        let existing = actors::find_active_key(&mut conn, &key_hash).await?;
        let Some(existing) = existing else {
            return Err(GraphError::NotFound(format!("active key {key_hash_hex}")));
        };

        self.commit_event(
            EventPayload::ActorKeyRevoke(oag_events::payload::ActorKeyRevokePayload {
                actor_id: existing.actor_id.to_hex(),
                key_hash: key_hash_hex.to_string(),
            }),
            self.now(),
        )
        .await?;
        Ok(())
    }
}
