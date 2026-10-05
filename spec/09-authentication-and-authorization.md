# Authentication and Authorization

**Source**: `crates/oag-api/src/auth.rs`, `crates/oag-graph/src/service.rs`,
`crates/oag-core/src/actor.rs`.

## API keys

REST and MCP share one auth path: `Authorization: Bearer <api-key>`. Keys are created locally via
`oag key create`, bound to one actor and an explicit list of permissions. Only a key's BLAKE3 hash
is ever stored — the raw key is shown once, at creation, and cannot be recovered or re-displayed.

- `oag key list` shows every key ever issued (by hash, active and revoked alike).
- `oag key revoke <hash>` deactivates one immediately via a signed `ACTOR_KEY_REVOKE` event — a
  mutation like any other, auditable and rebuild-safe, not a silent database edit.
- Both are CLI-only, gated by filesystem access to the data directory — there is no REST/MCP
  surface for key management, same posture as identity and backup.

## Permissions

| Permission | Grants |
|---|---|
| `graph:read` | Search, resolve, get nodes/edges/assertions/subgraphs/history/corroboration |
| `graph:assert` | Create assertions, attach evidence, dispute, supersede |
| `graph:verify` | Record verification observations |
| `graph:retract-own` | Retract an assertion your own actor created |
| `graph:crawl` | Trigger this peer's crawler against a caller-supplied URL — separate from `graph:assert` since it makes this peer issue outbound HTTP requests, not just author a claim |
| `admin` | Evidence redaction, search suppression (CLI-only) |

**`admin` is a true superuser bypass, not just its own scoped permission**:
`AuthContext::require` checks `self.permissions.contains(&permission) ||
self.permissions.contains(&Permission::Admin)` — a key holding `admin` satisfies *every*
permission check in the system, including ones added after `admin` was introduced. The bootstrap
key `oag serve` prints on first run is `admin`-only for exactly this reason: it's meant to be the
one credential an operator uses locally to mint every other, narrower-scoped key, not a key to hand
out.

A missing or invalid bearer token, or a token whose actor lacks the required permission, is
rejected before any handler logic runs. Routes needing no auth at all, by design:
`GET /api/v1/status` and `GET /metrics`.

## Actor key proofs

An actor may register an Ed25519 public key, proven via `sign_with_domain(signing_key,
"OAG:ACTOR_KEY_PROOF:v1:", canonical_json_bytes({actor_type, name, identity_uri}))` — a signature
over the *exact* `actor_type`/`name`/`identity_uri` being declared, so a proof can't be replayed
onto a different label for the same key. The private key never needs to touch this peer; the proof
is produced entirely by the caller's own tooling. A verified key raises the actor's
`identity_assurance` signal (see [06-corroboration-and-authority.md](06-corroboration-and-authority.md))
and gives it a stable, key-derived `ActorId` that dedups across repeat declarations with the same
key.

## Rate limiting on this boundary

See [18-rate-limiting-and-resource-limits.md](18-rate-limiting-and-resource-limits.md) for the
per-key limiter and the serious bug found and fixed in its own tracking map.

## Tests

`crates/oag-graph/src/tests.rs` (`declare_actor_with_valid_public_key_proof_succeeds`,
`declare_actor_rejects_tampered_signature`, `declare_actor_rejects_proof_signed_for_a_different_label`,
`declare_actor_rejects_a_public_key_that_did_not_produce_the_signature`,
`same_public_key_dedups_to_the_same_actor_id`, `unauthenticated_actor_cannot_assert`);
`crates/oag-cli/src/commands.rs` (`key_revoke_deactivates_the_key_and_survives_rebuild`,
`key_revoke_unknown_hash_is_not_found`).
