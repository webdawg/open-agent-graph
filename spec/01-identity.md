# Peer Identity

**Source**: `crates/oag-crypto/src/identity.rs`, `crates/oag-cli/src/commands.rs` (backup/restore).

## What it is

Every OAG peer is identified by a persistent Ed25519 keypair, generated on first run and stored at
`<data-dir>/identity.key` with owner-only (`0600`) file permissions. There is no registration step,
no central authority, and no way to pick your own identity.

- **`PeerId`** = `BLAKE3("OAG:PEER:v1:" + public_key)`, displayed as lowercase Base32 with an
  `oagp_` prefix. It is *derived*, never independently chosen — two different keys cannot collide
  onto the same `PeerId` except by a BLAKE3 preimage, which is computationally infeasible.
- `PeerIdentity::load_or_generate(path)` is the only way a peer's identity comes into being: if
  `identity.key` exists, load it; otherwise generate a fresh keypair and write it.

## Guarantees

- The private key never leaves the node except through an explicit `oag identity backup`. There is
  no automatic export, sync, or cloud storage of it.
- **Losing `identity.key` means losing that peer's identity permanently.** There is no recovery
  mechanism beyond a prior backup. A new `identity.key` produces a new `PeerId`, which desyncs from
  every event this peer previously signed under the old key.
- `oag identity restore` refuses to overwrite an existing `identity.key` without `--force`,
  specifically to prevent this loss from happening by accident.
- **Every path that stores a `(peer_id, public_key)` pairing learned from another peer must verify
  the pairing is internally consistent** (`peer_id == PeerId::from_public_key(public_key)`) before
  storing it. This is checked in two places: `SyncService::sync_with_peer`'s `hello` handshake, and
  `SyncService::discover_peers`'s `/peers`-gossip path. Both were found and fixed in the same
  session that produced this spec — `discover_peers` initially lacked this check, which would have
  let a malicious relay poison a peer's identity cache for a third party. See
  [14-replication-sync.md](14-replication-sync.md) for the consequences of a bad pairing and why
  they're a denial-of-replication effect, not an impersonation one (`ingest_remote_event`'s
  `OriginKeyMismatch` check is an independent second backstop regardless).

## CLI surface

| Command | Behavior |
|---|---|
| `oag identity show` | Print this peer's `peer_id` and deterministic petname (see [21-petname.md](21-petname.md)) |
| `oag identity backup <path>` | Copy `identity.key` to `path` |
| `oag identity restore <path>` | Copy `path` over this peer's `identity.key`; refuses without `--force` if one already exists |

Identity has no REST or MCP surface — it is a local, filesystem-trust operation, consistent with
every other credential-lifecycle command in this project (API keys, backups).

## Tests

`crates/oag-crypto/src/identity.rs`'s own unit tests; `oag-cli/src/commands.rs`'s
`identity_restore_refuses_to_overwrite_without_force` / `identity_restore_round_trips_the_same_peer_id`
/ `identity_restore_overwrites_with_force`.
