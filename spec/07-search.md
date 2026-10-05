# Search

**Source**: `crates/oag-graph/src/search.rs`, `crates/oag-graph/src/semantic_search.rs`,
`crates/oag-embeddings/src/*.rs`.

## Keyword search (`GraphService::search`)

FTS5 relevance search over `nodes.name`/`description`/`canonical_identifier`. Free text is run
through `sanitize_fts_query` before being used as the `MATCH` pattern: every character that isn't
alphanumeric or whitespace is replaced with a space, each resulting word is individually
double-quoted (forcing phrase-match, not operator-match), so arbitrary user input can never produce
an FTS5 syntax error or an unintended column-filter/boolean-operator query. This also closes any
SQL-injection surface — the sanitized string is still passed as a bound parameter, never
interpolated into the SQL text. Confirmed by test with deliberately hostile input (unbalanced
quotes, `NEAR(...)`, `***`, column-filter syntax).

Nodes in `search_suppressions` are excluded from results via a live `NOT IN` subquery, not a cached
flag — this stays correct across `oag rebuild` without special-casing.

### The `limit` parameter — found and fixed

SQLite treats a negative `LIMIT` as "no limit at all," not as zero or an error. Confirmed live:
`GET /api/v1/search?q=...&limit=-1` returned *every* matching row rather than being rejected or
clamped — any `graph:read` caller (the lowest-privilege read permission in the system) could force
this peer to serialize its entire matching node set in one response, with no cap on the way out to
bound it either.

`limit` is now clamped to `[0, 1000]` before it ever reaches a query — `limit=0` still legitimately
means "zero results" (SQLite's `LIMIT 0` behaves normally; only *negative* values are
special-cased as unlimited), and 1000 follows the same precedent as other per-call caps elsewhere
in the system (`MAX_EVENTS_PER_FETCH`, etc.). The clamp lives in `GraphService::search`, the one
service layer REST/MCP/CLI all call through, so all three surfaces are protected uniformly.

## Semantic search (`GraphService::semantic_search`)

Brute-force cosine similarity over embeddings already loaded into memory — no mandatory external
vector database or SQLite vector extension (a deliberate v1 scope choice; see
[22-future-work.md](22-future-work.md) for ANN-index deferral). Disabled by default
(`oag_embeddings::DisabledProvider`); returns `GraphError::Embedding(EmbeddingError::Disabled)`
rather than silently returning nothing, so a caller gets a clear signal semantic search isn't
configured rather than mistaking it for "no matches."

`cosine_similarity(a, b)` returns `0.0` for mismatched lengths or either vector having zero
magnitude — never divides by zero, never panics. Ranking sorts with `f32::total_cmp`, which gives a
total order even if a NaN somehow entered the vector data, rather than the panic risk a naive
`partial_cmp`-based sort would carry.

`semantic_search`'s `limit` is clamped the same way as keyword search's (`[0, 1000]`) — its
negative-limit case was already safe before the fix (`.max(0)`), but the upper bound wasn't, so a
huge positive limit was previously bounded only by the total number of embedded nodes rather than
by an explicit cap.

### Recomputing embeddings

`oag embeddings recompute` wipes and regenerates the whole `node_embeddings` table from the
currently-configured `EmbeddingProvider` — same rebuildable-projection shape as
`recompute_authority`, so a provider or model change doesn't leave stale vectors from a different
vector space mixed in with new ones.

## CLI surface

| Command | Behavior |
|---|---|
| `oag search <query> [--semantic] [--limit]` | Keyword or (with `--semantic`) embedding-ranked search |
| `oag embeddings recompute` | Batch-recompute every node's embedding |

## Tests

`crates/oag-graph/src/tests.rs` (`resolve_with_fts_special_characters_does_not_error`,
`search_with_a_negative_limit_returns_nothing_not_everything`,
`semantic_search_ranks_topically_similar_nodes_above_unrelated_ones`,
`semantic_search_with_default_disabled_provider_is_a_typed_error_not_a_panic`);
`crates/oag-embeddings/src/similarity.rs` (zero-vector/mismatched-length/NaN-safety unit tests).
