# Data Model

This describes the graph's core entities as `oag-graph`/`oag-core` define them, and the ranking
signals computed over them. For how these tables come to exist (projected from the signed event
log), see `docs/event-protocol.md`. For the exact table/column names, see the migrations in
`crates/oag-storage/migrations/`.

## Node

A node is any entity — a URL, a concept, a package, a person, an organization. Nodes are
deliberately *not* asserted directly; they come into existence as the subject or object of an
assertion (spec section 16).

- `id` — a `NodeId`, deterministically derived from its `canonical_identifier` (spec section 14):
  the same identifier string always produces the same node id, on any peer, without coordination.
- `canonical_identifier` — a scheme-prefixed string: `url:https://...`, `concept:model-context-protocol`,
  or a passthrough for an already-scheme-prefixed input (`urn:`, `package:`, `doi:`, `github:`, ...).
  See `crates/oag-graph/src/identifier.rs::canonicalize_value` for the exact rules — free text like
  `"Model Context Protocol"` is lowercased, slugified, and treated as a `concept:` identifier.
- `node_type` — a free-form lowercase string (`website`, `document`, `concept`, ...), not a closed
  enum — spec section 15 is explicit that OAG does not enumerate every entity type up front.
- `name`, `description` — optional, usually filled in later by crawler/enrichment, not present at
  creation time for most nodes (which is why `canonical_identifier` is itself indexed for search —
  see "Search" below).

### Aliases

A node can have any number of aliases (`AliasType`: `Name`, `Url`, `Urn`, `Package`, `ExternalId`,
`Acronym`) — alternate names or identifiers that resolve to the same node without being its
canonical identifier (spec section 17). Aliases never determine `NodeId`; they're purely for
discovery and display.

## Edge

An edge is a `(subject, predicate, object)` triple — but **an edge existing does not mean it's
true** (spec section 19). An edge is a container that one or more assertions attach claims to; the
edge itself carries no confidence or truth value. See `docs/predicates.md` for the predicate
vocabulary.

## Assertion

The actual unit of claimed truth: "actor X claims this edge holds, with this much confidence, as of
this extraction method, as of this time" (spec section 21). Critically, **an assertion is a claim,
not a fact** (spec section 22) — OAG never collapses multiple assertions into one "system-believed"
truth value. Every assertion remains individually inspectable forever.

- `id` — equal to the `event_id` of the `ASSERT_RELATION` event that created it.
- `actor_id`, `actor_confidence` — who claimed it, and how confident *they* say they are (not the
  system's confidence in them).
- `extraction_method` — `direct` (a human/agent asserted it directly via REST/MCP), `structured_extraction`
  (the crawler found it in JSON-LD/ARD/A2A/llms.txt/HTML meta — spec section 73), `llm_extraction`
  (an LLM read the page's own text and produced a candidate — spec section 74), or `verification`.
- `status` — `active`, `disputed`, `retracted`, or `superseded`. **This is a derived, overwritable
  cache**, not the source of truth — the durable history lives in `assertion_disputes`/
  `assertion_retractions`/`assertion_supersessions`, which are append-only and never rewritten. A
  dispute/retraction/supersession never deletes the original assertion; it inserts an audit row and
  flips this one flag.

## Evidence

Attached to an assertion via a separate `ADD_EVIDENCE` event (spec section 33 — evidence is added
independently of the assertion it supports, so new evidence can strengthen an old claim without
re-asserting it). An evidence row's `id` is likewise the `event_id` of the event that created it.

Fields: `evidence_type` (`web_page`, `documentation`, `source_code`, `repository`, `api_response`,
`dataset`, `file`, `manual`, `specification`, `user_observation`, `agent_observation`, `other`),
`uri`, `title`, `excerpt`, `content_hash` (prefixed `blake3:`/`sha256:`, spec section 24), and
timestamps. `title`/`excerpt` are the only fields holding actual free-text content inline (as
opposed to a reference/hash) — see `docs/security.md`'s redaction section for why that matters.

## Actor

Whoever or whatever made a claim: a human, an agent, a model, a crawler, an organization, a domain,
a service, a peer, or anonymous (`ActorType`). An actor is distinct from a peer (spec section 26) —
one peer can host many actors, and the same logical actor's claims can be relayed by peers other
than the one it originally asserted through.

An actor *may* register an Ed25519 public key, proven via a domain-separated signature over
`{actor_type, name, identity_uri}` (spec section 40's actor-key-proof scheme,
`crates/oag-graph/src/service.rs`) — this is what lets an actor's claims be trusted independently of
which peer relayed them, and is the strongest input to the `identity_assurance` signal below.

## Ranking signals (spec section 65)

OAG deliberately never collapses trust into one score. `GraphService::get_edge_corroboration`
(`crates/oag-graph/src/corroboration.rs`) returns every signal separately, so a caller decides how to
weigh them:

| Signal | Meaning |
|---|---|
| `agreement` | Fraction of active assertions on this edge that agree, vs. dispute it |
| `evidence_strength` | How substantial the attached evidence is (type-weighted, averaged per independent source group, not just counted) |
| `source_independence` | How many genuinely distinct sources back this — grouped by registrable domain, with an explicit multi-tenant-host exception (spec section 62: ten GitHub repos under different orgs are independent; ten repos under one org are not) |
| `freshness` | Exponential recency decay (6-month half-life) from the most recent active assertion's observed/asserted time — 0.0 if nothing active backs the edge |
| `identity_assurance` | Mean, across the edge's distinct actors, of how strongly each actor's identity is established: 1.0 (verified public key + identity URI), 0.7 (key only), 0.3 (identity URI only), 0.0 (neither) |

Two related signals live outside `EdgeCorroboration` because they don't fit its single-edge shape:

- **`authority`** (`crates/oag-graph/src/authority.rs`) — a node-level, query-independent
  PageRank-style score ("how many things point to me, weighted by how important those things are"),
  computed by on-demand batch recomputation (`oag authority recompute`), not maintained continuously.
- **`graph_distance`** (`SubgraphNode.distance`) — hops from a traversal's starting node, meaningful
  only in the context of a subgraph walk, not an isolated edge lookup.

## Search

`oag search`/`/api/v1/search` is keyword relevance search over `nodes_fts` (an FTS5 index on
`name`/`description`/`canonical_identifier` — spec section 63). `--semantic` additionally ranks by
embedding cosine similarity if a provider is configured (spec section 64, disabled by default — see
`docs/api.md`). Nodes in `search_suppressions` (spec section 85) are excluded from results
regardless of match quality — see `docs/security.md`.
