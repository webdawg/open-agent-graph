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
permissions. Only a key's BLAKE3 hash is ever stored — the raw key is shown once, at creation, and
cannot be recovered or re-displayed later.

`oag key list` shows every key ever issued (by hash, active and revoked alike) and `oag key revoke
<hash>` deactivates one immediately — useful if a key leaks or an integration is decommissioned.
Revocation is itself a signed `ACTOR_KEY_REVOKE` event (spec section 106 invariant 1: every mutation
is a signed event, not a silent database edit), so it's auditable and survives `oag rebuild` exactly
like any other mutation. Like `oag key create`, both are CLI-only, gated by filesystem access to the
data directory — there is no REST/MCP surface for key management.

Permissions (`Permission` enum, `crates/oag-core/src/actor.rs`):

| Permission | Grants |
|---|---|
| `graph:read` | Search, resolve, get nodes/edges/assertions/subgraphs/history/corroboration |
| `graph:assert` | Create assertions, attach evidence |
| `graph:verify` | Record verification observations |
| `graph:retract-own` | Retract an assertion your own actor created |
| `graph:crawl` | Trigger this peer's crawler against a caller-supplied URL (spec section 71) -- separate from `graph:assert` since it makes this peer issue outbound HTTP requests, not just author a claim |
| `admin` | Evidence redaction, search suppression (spec section 85 — CLI-only today, see below) |

**`admin` is a superuser bypass, not just its own scoped permission**: `AuthContext::require`
(`crates/oag-graph/src/service.rs`) checks `self.permissions.contains(&permission) ||
self.permissions.contains(&Permission::Admin)` -- a key holding `admin` satisfies *every* permission
check in the system, including ones granted after `admin` was introduced (like `graph:crawl` above).
The bootstrap key `oag serve` prints on first run is `admin`-only for exactly this reason: it's meant
to be the one credential an operator uses locally to mint every other, narrower-scoped key via
`oag key create`, not a key to hand out. Treat it accordingly — it is not "just" a redaction/
suppression credential.

A missing or invalid bearer token, or a token whose actor lacks the required permission, is
rejected before any handler logic runs (`GraphError::InvalidApiKey` / `GraphError::PermissionDenied`).
A handful of routes need no auth at all by design: `GET /api/v1/status` (so a peer can be health-
checked without provisioning a credential) and `GET /metrics` (conventional for a scrape target —
see `docs/api.md`).

Peer-to-peer replication (`oag-sync`) is a *separate* trust boundary from actor API keys — see
[Federation trust](#federation-trust-vs-data-trust) below.

## Rate limiting

A basic per-API-key fixed-window limiter (`crates/oag-api/src/rate_limit.rs`) caps each key to 120
requests per 60-second window (spec sections 58/61/90), applied identically to REST and MCP (same
key, same shared counter — `crates/oag-cli/src/serve.rs::build_app` gives `/mcp` its own copy of
this layer rather than relying on `rest_router`'s own, which — found and fixed live — never actually
covered a route merged in after `build_router` already returned). Requests with *no* `Authorization`
header at all share a single `"anonymous"` bucket; a request *with* a header — even a garbage,
never-valid one — gets its own bucket keyed by that exact string, since the limiter runs before any
handler validates the key. This is deliberately simple (in-memory, per-process, not distributed) —
it is not a substitute for a real edge/WAF rate limiter on a publicly exposed deployment.

That per-string keying was itself an unbounded-growth vector, found and fixed live: the tracking
map previously never evicted anything, so a caller sending a different bogus `Authorization` value
on every request — no valid credentials needed at all — grew it forever. Confirmed: 30,000 requests
each with a unique garbage key grew this peer's RSS from ~6 MB to ~22.7 MB with no bound in sight.
Fixed with a threshold-triggered sweep — once the map exceeds 10,000 tracked keys, entries whose
window has already expired are purged — confirmed live that doubling the request count (60,000,
well past the threshold) added no further measurable growth (~23.5 MB, essentially flat).

`oag-sync`'s own HTTP endpoints (`/oag/sync/v1/*`, deliberately unauthenticated — see "Federation
trust vs. data trust" below) have a separate, global 600-requests/60s counter
(`SyncRateLimiter`, `crates/oag-sync/src/rate_limit.rs`), since there's no per-caller identity to
key by. `oag-reticulum`'s listener (`crates/oag-reticulum/src/listener.rs`) answers the identical
three calls over a Reticulum `Link` instead of HTTP, and — also found and fixed live — initially had
no rate limiting on that path at all. `listen_tcp` means this transport isn't inherently
bandwidth-capped by radio the way a pure mesh-radio deployment might be, so it now reuses the exact
same `SyncRateLimiter` directly rather than reimplementing the logic.

## Response/request size caps

Every HTTP boundary in this codebase caps body size before fully buffering it, so no single
request or response can exhaust memory regardless of what the other side claims (spec section 61 —
"disk exhaustion," "event flooding"): REST request bodies are capped at 4 MiB
(`MAX_REQUEST_BODY_BYTES`, `crates/oag-api/src/lib.rs`); incoming `oag-sync` event pushes likewise
(`MAX_PUSH_BODY_BYTES`, `crates/oag-sync/src/server.rs`); the crawler streams a fetched page with
an explicit cap rather than buffering it whole (`crates/oag-crawler/src/fetch.rs`). `oag-sync`'s
own HTTP *client* (`crates/oag-sync/src/client.rs`) gets the identical treatment for the other
direction — a peer this node syncs *with* is exactly as untrusted as a peer syncing with it, so a
malicious or compromised peer's `/hello`/`/heads`/`/events`/`/peers` response is streamed with the
same 4 MiB cap rather than buffered unbounded before `discover_peers`'s own item-count caps
(`MAX_PEERS_PER_RESPONSE`/`MAX_ADDRESSES_PER_PEER`) ever get a chance to run.

Those per-response caps bound one gossip round, not the *total* `peers` table size on their own —
the gossip loop re-syncs with every known address forever (`crates/oag-sync/src/gossip.rs`), so a
single malicious relay feeding 200 fresh identities every round would otherwise grow `peers`/
`peer_addresses` on disk without bound. Generating a real Ed25519 keypair costs an attacker nothing,
so the peer_id/public_key consistency check above doesn't stop this either — it only catches lying
about an *existing* pairing, not minting endless new, individually-valid ones. `discover_peers` now
also checks a global cap (`MAX_TOTAL_KNOWN_PEERS`, 10,000) before learning any new peer from a
gossip response at all, confirmed with a test that fills a peer's table to the cap and checks a
real, legitimate peer introduced via gossip is still correctly rejected once full.

A caller's own `limit` parameter is the same class of boundary, from the other direction: SQLite
treats a negative `LIMIT` as "no limit at all," not as zero or an error — confirmed live, `GET
/api/v1/search?q=...&limit=-1` returned *every* matching row rather than being rejected or clamped,
with no cap on the way out to bound the response either. `GraphService::search` and
`::semantic_search` (`crates/oag-graph/src/search.rs`/`semantic_search.rs`) now clamp `limit` to
`[0, 1000]` before it ever reaches a query, at the one service layer REST/MCP/CLI all call through.

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

`PeerId` being *derived* from a public key rather than independently chosen (spec section 12) is
only as strong as every path that stores a `(peer_id, public_key)` pairing actually checking the two
derive from each other. `sync_with_peer`'s own `hello` handshake always has; `discover_peers` (the
`/peers`-gossip path, used to learn a peer transitively through a relay) didn't until found and
fixed here — a malicious relay could offer a real third party's `peer_id` paired with the
*attacker's own* public key, and this peer would store that pairing with nothing to say otherwise.

That pairing alone does **not** let the attacker forge an *accepted* event under the victim's
identity — `ingest_remote_event`'s own `OriginKeyMismatch` check derives the verifying peer id
purely from the key's own bytes, never from what a local table claims it maps to, so an event lying
about its `origin_peer` relative to whoever actually signed it is rejected regardless of which key
was looked up to check it. What the bad pairing actually breaks is this peer's ability to accept the
*real* victim's own legitimate events going forward — they'd fail signature verification against
the wrong key — a denial-of-replication effect against whichever peer's identity got poisoned, not
an impersonation one. Still a real bug (an untrusted relay should never get to break a third party's
replication reaching you, and a local identity cache should never adopt an internally-inconsistent
entry at all), just not the more severe impersonation issue an earlier draft of this note claimed.
Both paths now validate the pairing identically (`crates/oag-sync/src/service.rs`), confirmed with
three tests: one plays the relay role and checks a poisoned pairing is never adopted; the other two
pin down exactly what a bad pairing does and doesn't enable, directly (`crates/oag-sync/src/tests.rs`).

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
control, and should not be relied on to keep anything actually private. `oag redact list` shows
both audit views together — every redaction tombstone and every currently-suppressed node — rather
than just the former.

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
