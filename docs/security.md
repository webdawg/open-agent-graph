# Security

This document exists because spec section 85 (Deletion and Redaction) explicitly requires
"explicit documentation before public federation." It covers OAG's identity, authentication,
authorization, network-abuse defenses, and the real limits of local data redaction — the things an
operator needs to understand *before* peering this software with anyone they don't already trust.

## Identity

Every peer generates a persistent Ed25519 keypair on first startup, stored at
`<data-dir>/identity.key` with owner-only (`0600`) file permissions
(`crates/oag-crypto/src/identity.rs`). The private key never leaves the node except through an
explicit `oag identity backup` — there is no automatic export, sync, or cloud storage of it.

A peer's public identity, `PeerId`, is `BLAKE3(public_key)`, displayed as lowercase Base32 with an
`oagp_` prefix (spec section 12). It is derived, not chosen — you cannot pick your own peer id, and
two different keys cannot collide onto the same one except by a BLAKE3 preimage, which is
computationally infeasible.

**Losing `identity.key` means losing that peer's identity permanently.** There is no recovery
mechanism beyond a prior `oag identity backup`. A new `identity.key` means a new peer id, which
desyncs from every event previously signed under the old key (see `oag identity restore`'s
`--force` guard, which exists specifically to prevent this from happening by accident).

## Domain-separated hashing and signing

Every identifier (`NodeId`, `EdgeId`, `EventId`, `ActorId`) is a BLAKE3 hash computed as
`BLAKE3("OAG:<KIND>:v1:" + input)` (`crates/oag-core/src/ids.rs`). The domain prefix means a
`NodeId` and an `EventId` can never collide even if the same raw bytes were hashed for both — an
attacker can't construct input that hashes to a valid-looking id of the wrong kind.

Signatures follow the same pattern (`crates/oag-crypto/src/signing.rs::sign_with_domain`/
`verify_with_domain`): every signed structure (events, actor key-possession proofs) is signed over
`domain_prefix + canonical_bytes`, so a signature valid for one purpose can never be replayed as
valid for another.

Event payloads are serialized via RFC 8785 JSON Canonicalization (JCS, `serde_jcs`) before signing
(spec section 28) — this guarantees the *exact same* logical event always produces the *exact same*
bytes to sign and verify, regardless of which peer or language produced it, eliminating an entire
class of signature-malleability bugs that ad-hoc JSON serialization would introduce.

## Authentication and authorization

REST and MCP share one auth path (`crates/oag-api/src/auth.rs`): `Authorization: Bearer <api-key>`.
API keys are created locally via `oag key create` and bound to one actor and an explicit list of
permissions — there is no separate "admin" superuser bypass; every credential is scoped to exactly
the permissions it was issued.

Permissions (`Permission` enum, `crates/oag-core/src/actor.rs`):

| Permission | Grants |
|---|---|
| `graph:read` | Search, resolve, get nodes/edges/assertions/subgraphs/history/corroboration |
| `graph:assert` | Create assertions, attach evidence |
| `graph:verify` | Record verification observations |
| `graph:retract-own` | Retract an assertion your own actor created |
| `admin` | Evidence redaction, search suppression (spec section 85 — CLI-only today, see below) |

A missing or invalid bearer token, or a token whose actor lacks the required permission, is
rejected before any handler logic runs (`GraphError::InvalidApiKey` / `GraphError::PermissionDenied`).
A handful of routes need no auth at all by design: `GET /api/v1/status` (so a peer can be health-
checked without provisioning a credential) and `GET /metrics` (conventional for a scrape target —
see `docs/api.md`).

Peer-to-peer replication (`oag-sync`) is a *separate* trust boundary from actor API keys — see
[Federation trust](#federation-trust-vs-data-trust) below.

## Rate limiting

A basic per-API-key fixed-window limiter (`crates/oag-api/src/rate_limit.rs`) caps each key to 120
requests per 60-second window (spec sections 58/61/90). Anonymous/unauthenticated requests share a
single bucket — they're rejected by auth before doing real work regardless, so this mainly exists
to stop one misbehaving credential from monopolizing a peer. This is deliberately simple (in-memory,
per-process, not distributed) — it is not a substitute for a real edge/WAF rate limiter on a
publicly exposed deployment.

## Crawler / SSRF defenses

`oag crawl` fetches arbitrary operator-supplied URLs, which is exactly the shape of request that
enables server-side request forgery against internal infrastructure. `crates/oag-crawler/src/ssrf.rs`
blocks, by default: loopback, RFC 1918 private ranges, link-local (which covers the
`169.254.169.254` cloud-metadata endpoint for free, since that address is itself link-local),
unspecified, broadcast, documentation, and multicast ranges, on both IPv4 and IPv6 (including
IPv4-mapped IPv6 addresses, a common bypass vector). This check runs against the *resolved* IP, not
the hostname string, so DNS rebinding to a blocked address is caught. An operator can explicitly
opt in to crawling private networks with `--allow-private-networks`, for deliberately internal
targets — off by default, loud to turn on.

The crawler also enforces: a redirect cap, a response-size cap, a request timeout, and a
robots.txt check before fetching (`crates/oag-crawler/src/fetch.rs`, `robots.rs`).

LLM extraction (spec section 74, `crates/oag-crawler/src/llm_extract.rs`) never talks to anything
unless explicitly configured (`[crawler].llm_provider = "openai_compatible"` in `config.toml`) —
disabled by default, matching semantic search's `DisabledProvider` pattern.

## Federation trust vs. data trust

Accepting an event into local storage is a different question from trusting that event's *content*
(spec section 60). `oag-sync`'s `FederationPolicy` (`crates/oag-sync/src/federation.rs`) controls
the first question only:

- `Open` (the default) — accept replicated events from any peer that can produce a validly signed
  event.
- `Allowlist(peers)` — only accept events originating from an explicit set of peer ids.

Either way, an *accepted* event is still just a signed claim from whatever actor asserted it — the
corroboration signals (`docs/data-model.md`'s "Ranking signals" section) are what determine how much
weight a reader should give it, not federation policy. A wide-open federation policy plus careful
corroboration-aware reading is a deliberate design choice (spec section 61: public-network abuse is
expected, not prevented by gatekeeping who can talk to you).

Every signed event carries its own origin peer and a per-origin hash chain
(`previous_event`/`sequence`, see `docs/event-protocol.md`). A peer that tries to rewrite its own
history is caught by fork detection (`crates/oag-events/src/ingest.rs`) — two different events
claiming the same `(origin_peer, sequence)` are both recorded, flagged in `peer_forks`, and neither
is silently trusted over the other.

## Deletion and redaction — what it actually does, and doesn't (spec section 85)

**This is the single most important thing to understand before running a public-facing peer.**

> Distributed immutable systems cannot promise that data already replicated to independent peers can
> be physically erased everywhere. — spec section 85, verbatim

`oag redact evidence <id>` (see `crates/oag-graph/src/redaction.rs`) blanks an evidence record's
`title`/`excerpt` text **on this one peer's own local database only**. It:

- Does **not** reach any peer this event has already been replicated to. There is no mechanism
  anywhere in `oag-sync` — and none is planned — to instruct another peer to delete or stop serving
  a copy it already has. An immutable, gossip-replicated log cannot make that promise, and this
  project does not pretend otherwise.
- Does **not** modify the underlying signed event. `events.canonical_payload` and its Ed25519
  signature are untouched forever — the original bytes remain, byte-for-byte, exactly as originally
  signed and received. Only the *locally materialized, queryable copy* (the `evidence` row) is
  scrubbed. This is intentional: it keeps the event log's own cryptographic integrity fully intact
  (spec section 85's "retaining enough cryptographic metadata to preserve event integrity") while
  still removing the sensitive content from what `oag search`/REST/MCP will ever surface locally.
- Is **irreversible at the tooling level**. There is no "unredact" command. A tombstone (the
  `redactions` table) records that a redaction happened, when, and why, for audit purposes — it does
  not carry a way back.
- **Cannot be undone by `--force` overwriting it either** — a second `oag redact evidence` call on
  an already-redacted id is a typed error (`GraphError::AlreadyRedacted`), not a silent no-op or a
  way to "re-redact" with different content.
- Refuses to run at all without `--force`, and prints this exact limitation as a warning every time,
  so it's never invoked by muscle memory without the operator seeing it.

`oag redact suppress-node`/`unsuppress-node` is a *much* weaker, fully reversible intervention: it
hides a node from search results only. The node, its edges, and every assertion about it remain
fully intact and directly retrievable by id — this is a UI-layer omission, not a data-minimization
control, and should not be relied on to keep anything actually private.

**Bottom line**: OAG's redaction tooling minimizes what *this* peer keeps and serves going forward.
It is not, and cannot be, a mechanism for retracting data from a network you don't fully control —
plan what you assert and what evidence you attach accordingly, especially before federating
publicly.

## Backups contain everything

`oag backup` (spec section 82) snapshots the *entire* local database, including any evidence that
hasn't been redacted, via SQLite's `VACUUM INTO` (safe against a live, WAL-mode database). Treat
backup files with the same sensitivity as the live database — redacting evidence after taking a
backup does not retroactively redact the backup.

## Reporting a vulnerability

This is a young, single-maintainer project without a formal disclosure program yet. Please open an
issue at the project's GitHub repository, or reach out directly, rather than exploiting anything you
find against a real deployment.
