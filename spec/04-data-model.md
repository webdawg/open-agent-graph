# Data Model

**Source**: `crates/oag-core/src/*.rs`, `crates/oag-graph/src/identifier.rs`,
`crates/oag-storage/migrations/0001_init.sql`. See also `docs/predicates.md` for the full predicate
vocabulary.

## Node

Any entity — a URL, a concept, a package, a person, an organization. Nodes are never asserted
directly; they come into existence as the subject or object of an assertion.

- `id`: a `NodeId`, deterministically derived from `canonical_identifier` — the same identifier
  string always produces the same node id, on any peer, without coordination.
- `canonical_identifier`: a scheme-prefixed string (`url:https://...`, `concept:model-context-protocol`,
  or a passthrough for an already-scheme-prefixed input like `urn:`/`package:`/`doi:`/`github:`).
  See `canonicalize_value` below for the exact rules.
- `node_type`: a free-form lowercase string (`website`, `document`, `concept`, ...) — deliberately
  not a closed enum, so a future type never requires a schema migration.
- `name`/`description`: optional, usually filled in later by crawler/enrichment.

### Identifier canonicalization (`canonicalize_value`)

1. Looks like an `http(s)` URL → `url:<canonicalized>`. URL canonicalization
   (`oag_core::canonicalize_url`) lowercases scheme/host, strips default ports, resolves
   dot-segments, strips known tracking params (`utm_*`, `gclid`, `fbclid`), and strips an *empty*
   fragment (`#` with nothing after it) — found and fixed: `url::Url` keeps a bare trailing `#` as
   `Some("")` rather than `None`, which previously made `https://x.com/page` and
   `https://x.com/page#` canonicalize to two different strings, hence two different `NodeId`s, for
   the same resource. A *non-empty* fragment (`#/route`-style client routing) is left alone —
   deliberately conservative, since the project's own stated principle is not to aggressively
   collapse URLs when semantic equivalence is uncertain. Query parameter *order* is similarly left
   alone for the same reason (a design choice, not a bug — confirmed by checking it's consistent
   with the stated philosophy rather than assuming).
2. Already has an identifier-scheme prefix (`concept:`, `urn:`, `package:`, ...) with no spaces →
   used as-is (lowercased).
3. Otherwise → slugified and treated as a `concept:` identifier (`"Model Context Protocol"` →
   `concept:model-context-protocol`).

### Aliases

A node can have any number of aliases (`AliasType`: `Name`, `Url`, `Urn`, `Package`, `ExternalId`,
`Acronym`) — alternate names/identifiers that resolve to the same node without being canonical.
Aliases never determine `NodeId`; they exist purely for discovery and display.

## Edge

A `(subject, predicate, object)` triple. **An edge existing does not mean it's true** — an edge is
a container that one or more assertions attach claims to; the edge itself carries no confidence or
truth value. Predicates are an open vocabulary (`docs/predicates.md`), not a closed enum.

## Assertion

The actual unit of claimed truth: "actor X claims this edge holds, with this much confidence, as of
this extraction method, as of this time." **An assertion is a claim, not a fact** — OAG never
collapses multiple assertions into one "system-believed" truth value.

- `id` = the `event_id` of the `ASSERT_RELATION` event that created it.
- `actor_id`, `actor_confidence`: who claimed it, and how confident *they* say they are.
- `extraction_method`: `direct` (asserted via REST/MCP), `structured_extraction` (crawler found it
  in JSON-LD/ARD/A2A/llms.txt/HTML meta), `llm_extraction` (an LLM read the page's own text), or
  `verification`.
- `status`: `active`, `disputed`, `retracted`, or `superseded` — **a derived, overwritable cache**,
  not the source of truth. The durable history lives in `assertion_disputes`/`_retractions`/
  `_supersessions`, which are append-only. A dispute/retraction/supersession never deletes the
  original assertion; it inserts an audit row and flips this one flag. See
  [05-assertions-lifecycle.md](05-assertions-lifecycle.md).

## Evidence

Attached to an assertion via a separate `ADD_EVIDENCE` event — evidence is added independently of
the assertion it supports, so new evidence can strengthen an old claim without re-asserting it.

Fields: `evidence_type` (`web_page`, `documentation`, `source_code`, `repository`, `api_response`,
`dataset`, `file`, `manual`, `specification`, `user_observation`, `agent_observation`, `other`),
`uri`, `title`, `excerpt`, `content_hash` (prefixed `blake3:`/`sha256:`), timestamps. `title`/
`excerpt` are the only fields holding free-text content inline — see
[17-redaction-and-suppression.md](17-redaction-and-suppression.md) for why that matters.

## Actor

Whoever or whatever made a claim: human, agent, model, crawler, organization, domain, service,
peer, or anonymous (`ActorType`). An actor is distinct from a peer — one peer can host many actors,
and the same logical actor's claims can be relayed by peers other than the one it originally
asserted through.

An actor *may* register an Ed25519 public key, proven via a domain-separated signature over
`{actor_type, name, identity_uri}` — this lets an actor's claims be trusted independently of which
peer relayed them, and is the strongest input to the `identity_assurance` signal (see
[06-corroboration-and-authority.md](06-corroboration-and-authority.md)). See
[09-authentication-and-authorization.md](09-authentication-and-authorization.md) for the
key-proof mechanism itself.

## Input validation (every mutation's text fields)

Every text-bearing mutation is length-capped at the one service layer REST/MCP/CLI all call through
(`crates/oag-graph/src/assert.rs`'s `MAX_*` constants, `check_len`/`check_opt_len`) — subject/
predicate/object, evidence fields, aliases, dispute/retract/redaction reasons, and actor
name/identity_uri. This was incomplete in two places, found and fixed: `declare_actor`'s
`name`/`identity_uri` had no cap at all despite its own doc comment claiming otherwise, and
`redact_evidence`'s `reason` had the same gap. Both now go through the same `check_opt_len` every
other field uses.

## Storage model

One SQLite database per peer, WAL mode, `foreign_keys=ON`. Two categories of table:

- **Source of truth**: `events`, `event_origins`, `event_refs` — append-only, never wiped except by
  a fresh local commit or validated remote ingest.
- **Derived projections**: everything else graph-shaped — fully reconstructible from the event log
  by `oag rebuild` (see [03-rebuild-and-backup.md](03-rebuild-and-backup.md)).

Two tables sit outside both categories — `redactions` and `search_suppressions` — durable operator
decisions that `oag rebuild` must never wipe. See
[17-redaction-and-suppression.md](17-redaction-and-suppression.md).

## Tests

`crates/oag-core/src/canonicalize.rs` (URL canonicalization, including the empty-fragment
regression tests); `crates/oag-graph/src/identifier.rs` (value canonicalization); `crates/oag-graph/
src/tests.rs` (`declare_actor_rejects_an_oversized_name`/`_identity_uri`,
`redacting_with_an_oversized_reason_is_rejected`).
