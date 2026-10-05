# Corroboration and Authority

**Source**: `crates/oag-graph/src/corroboration.rs`, `crates/oag-graph/src/authority.rs`.

## Philosophy

OAG deliberately never collapses trust into one score. `GraphService::get_edge_corroboration`
returns every signal separately, so a caller decides how to weigh them.

## Per-edge signals (`EdgeCorroboration`)

| Signal | Meaning |
|---|---|
| `agreement` | Fraction of active assertions on this edge that agree, vs. dispute it |
| `evidence_strength` | How substantial the attached evidence is (type-weighted, averaged per independent source group, not just counted) |
| `source_independence` | How many genuinely distinct sources back this — grouped by registrable domain, with an explicit multi-tenant-host exception (ten GitHub repos under different orgs are independent; ten repos under one org are not) |
| `freshness` | Exponential recency decay (6-month half-life) from the most recent active assertion's observed/asserted time |
| `identity_assurance` | Mean, across the edge's distinct actors, of each actor's identity strength: 1.0 (verified key + identity URI), 0.7 (key only), 0.3 (identity URI only), 0.0 (neither) |

Also reported: `total_assertions`, `active_assertions`, `disputed_assertions`, `distinct_actors`,
and `source_groups` (sorted, deduplicated control-group keys backing the active assertions, for
explainability — not just a count).

### Freshness and timestamp manipulation

`freshness_from_age(now, timestamp)` clamps `(now - timestamp)` to a minimum of 0 before computing
decay. This was specifically checked (not assumed) against the obvious attack of claiming a
timestamp far in the future to inflate freshness: a future timestamp is clamped to exactly the same
maximum (1.0) an honestly-current claim already gets — there is no way to exceed what an honest
claim would already achieve. Confirmed by test: `freshness_clamps_future_timestamps_to_full_strength`.

## Node-level and traversal-relative signals

These don't fit `EdgeCorroboration`'s single-edge shape, so they live separately:

- **`authority`** (`crates/oag-graph/src/authority.rs`): a node-level, query-independent
  PageRank-style score ("how many things point to me, weighted by how important those things
  are"). Pure power-iteration over the plain edge list (damping 0.85, max 50 iterations,
  convergence epsilon 1e-6), computed by on-demand batch recomputation (`oag authority recompute`),
  not maintained continuously. Dangling nodes (out-degree zero) redistribute their score uniformly
  each iteration, the standard PageRank fix for rank otherwise leaking out of the system.
- **`graph_distance`** (`SubgraphNode.distance`): hops from a traversal's starting node, meaningful
  only in the context of a subgraph walk. See [08-subgraph-and-history.md](08-subgraph-and-history.md).

## CLI surface

| Command | Behavior |
|---|---|
| `oag authority recompute` | Batch-recompute the PageRank-style `authority` signal for every node |
| `oag edge corroboration <id>` | Print all corroboration signals for one edge |

REST/MCP expose per-edge corroboration (`GET /edges/{id}/corroboration` / `graph_get_corroboration`)
but not a direct authority-recompute trigger — that's a CLI-only batch operation, same posture as
`oag rebuild`.

## Tests

`crates/oag-graph/src/corroboration.rs` (per-signal unit tests, including the GitHub-multi-tenant
control-group cases, freshness clamping, identity-assurance tiers);
`crates/oag-graph/src/authority.rs` (`recompute_authority_ranks_a_real_hub_above_a_real_leaf`).
