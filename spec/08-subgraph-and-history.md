# Subgraph, History, and Explainability

**Source**: `crates/oag-graph/src/subgraph.rs`, `crates/oag-graph/src/sources.rs`,
`crates/oag-graph/src/provenance.rs`, `crates/oag-storage/src/repo/events.rs` (`history_for`).

## Subgraph traversal (`get_subgraph`)

Breadth-first walk out to `depth` hops from a root node, capped at `max_nodes`. Correctly
short-circuits on both bounds: the outer hop loop checks `visited_nodes.len() >= max_nodes` before
starting each hop, and the inner edge loop checks it again before processing each edge — so even
an enormous `depth` (e.g. `u32::MAX`) terminates quickly once `max_nodes` is reached, rather than
attempting the full walk first and truncating after. `max_nodes` itself isn't additionally capped
by the service layer, but this was checked and judged acceptable: the real graph's own size is a
natural ceiling regardless of what a caller requests, unlike the `search` `limit` bug (see
[07-search.md](07-search.md)), where a negative value bypassed any real-data ceiling entirely.

Each returned node carries `distance` — hops from the root, correct because BFS guarantees the
first time a node is reached is via a shortest path (later rounds never revisit it).

## Explainability (`find_sources`)

All evidence backing any assertion whose edge touches a given node — "why is this relationship
here" starts from a node's edges. Exposed as MCP's `graph_find_sources` and REST's
`GET /nodes/{id}/sources` — the REST route was added after being found missing: `docs/api.md` had
explicitly flagged the gap ("No REST equivalent") rather than it being an undocumented oversight,
but a REST-only caller doing the same explainability work genuinely couldn't do what an MCP agent
could. Deliberately not added to the CLI, matching the existing precedent that `subgraph`/`history`
are also REST/MCP-only with no CLI equivalent.

## Event provenance (`get_event_provenance`)

Given an assertion id (which *is* the id of the `ASSERT_RELATION` event that created it), resolves
and re-verifies that event's origin peer, signature validity, and whether this peer even knows the
origin's public key (`origin_key_known: false` if not). Backs the Human Interface's "origin peer /
event id / signature status" fields (spec section 80/81's explainability chain, last two hops).

## History

`get_history(object_type, id)` returns every event that ever referenced a given node/edge/
assertion, via `event_refs` — a secondary index populated by the projector specifically so history
lookups don't require scanning the whole event log. Used directly by REST/MCP's `history` endpoint
and by the Human Interface to render an assertion's full lifecycle (assert → evidence → dispute →
retract, etc.) and to derive supersession status (filtering history for a `SUPERSEDE_ASSERTION`
event, rather than adding a dedicated storage accessor for just that one field).

## CLI surface

Subgraph, `find_sources`, and `history` have no CLI surface — REST/MCP only, consistent with the
project's established "mutations are sometimes CLI-or-REST/MCP but never both; multi-hop/aggregate
reads are REST/MCP-only" pattern.

## Tests

`crates/oag-graph/src/tests.rs` (`subgraph_reports_correct_hop_distance_along_a_chain`,
`subgraph_keeps_the_shortest_distance_when_a_node_is_reachable_two_ways`,
`end_to_end_assert_search_subgraph_history`); `crates/oag-graph/src/provenance.rs`'s own tests;
`crates/oag-api/src/tests.rs`'s `get_node_sources_returns_evidence_from_touching_edges`.
