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

### The `offset` parameter — pagination

A `limit` without an `offset` means a caller can only ever reach the first page of a result set.
Until this was added, the Human Interface's search page hard-coded a page size of 50 with no way
forward — the first 50 matches were reachable and nothing past them was. That was a scope call
(`OPEN_QUESTIONS.md`: "correct at today's scale"), not an oversight; it stops being correct as
soon as a graph holds more matching nodes than one page.

`offset` skips that many results before returning any, and is exposed on all four surfaces: REST
(`offset=`), MCP (`offset`), CLI (`--offset`), and the Human Interface (`offset=`). The SQL is
`LIMIT ? OFFSET ?`; SQLite rejects `OFFSET` without `LIMIT`, so the service layer always pairs
them, and it clamps `offset` at 0 for the same reason it clamps `limit` — a caller passing a
negative value almost certainly meant "from the start," and the four surfaces should behave
identically rather than three clamping and one erroring.

The Human Interface's page size is **fixed at 50**, not caller-controlled: it is a browsing
surface, and a caller-chosen page size would let one request serialize a whole matching node set
into HTML. Only the offset is caller-controlled. "Next" is offered whenever the page came back
full, which is the cheapest available signal short of fetching `limit+1` rows; "Previous" is
offered whenever the offset is greater than zero.

**Stable ordering is not what `offset` needs.** Pagination is only meaningful if the order does
not change between pages, and the order does not: FTS5 `rank` is a function of the *corpus*, not
just the row, so inserting a document moves every existing row's score even when their relative
order is preserved. Measured: after one insertion the eleven seeded matches kept their exact
relative order while every one of their ranks moved (for example `-2.0000e-6` became `-1.9650e-6`).

That distinction is the whole reason `offset` cannot be made safe. `OFFSET` counts rows from the
start, so a document arriving that outranks the current page shifts every later offset and a walk
repeats a row -- asserted by `search_cursor_walk_covers_every_match_once_across_an_insertion` at
the service layer and `search_cursor_walk_does_not_repeat_a_row_when_a_match_arrives_mid_walk`
over REST. Making the order total (`ORDER BY rank, node_id`, both paths) removes the tie-breaking
ambiguity but not the shift.

### The `cursor` parameter — keyset pagination

`cursor` is the contract for walking a result set that may change between requests. It is
exposed on REST (`cursor=`), MCP (`cursor`), and the CLI (`--cursor`); the Human Interface keeps
`offset` alone, being a browsing surface with a fixed page size. It takes the
`next_cursor` value from the previous response, or is omitted for the first page, and returns the
rows that follow the last row the caller actually saw. When both `cursor` and `offset` are sent
the cursor wins: they are different contracts and applying both would describe a page neither
promises.

**The cursor names a row, not a rank.** An earlier version stored the last row's `rank` and
compared it against the next page's ranks. That is wrong for the reason above -- the stored score
was computed under the old corpus and the next page's scores under the new one, so the boundary
row repeated. The cursor is now the last row's `node_id`, and the query re-resolves that row's
*current* rank before applying the boundary, which makes it exact under any absolute shift.

**What it still cannot do.** If a corpus change *reorders* rows across the boundary -- rather
than moving all their scores together -- a row can still repeat or skip. No pagination scheme
survives that, because the ordering itself changed. What the cursor does guarantee is that a
walk never repeats a row it has already seen, which `offset` does not.

**A stale cursor starts over.** If the cursor's node no longer matches the query (removed, or its
text edited), the boundary cannot be resolved and the walk restarts from the beginning rather than
erroring -- a caller cannot recover from a 500 mid-walk, and the alternative is a silent wrong
page. A cursor from a different query is not detected: FTS5 computes `rank` per query, so such a
row may not match at all, and the result is the same restart.

### Recomputing embeddings

`oag embeddings recompute` wipes and regenerates the whole `node_embeddings` table from the
currently-configured `EmbeddingProvider` — same rebuildable-projection shape as
`recompute_authority`, so a provider or model change doesn't leave stale vectors from a different
vector space mixed in with new ones.

## CLI surface

| Command | Behavior |
|---|---|
| `oag search <query> [--semantic] [--limit] [--offset]` | Keyword or (with `--semantic`) embedding-ranked search; `--offset` pages through results |
| `oag embeddings recompute` | Batch-recompute every node's embedding |

## Tests

`crates/oag-graph/src/tests.rs` (`resolve_with_fts_special_characters_does_not_error`,
`search_with_a_negative_limit_returns_nothing_not_everything`,
`search_pages_through_results_with_an_offset`,
`semantic_search_ranks_topically_similar_nodes_above_unrelated_ones`,
`semantic_search_with_default_disabled_provider_is_a_typed_error_not_a_panic`);
`crates/oag-embeddings/src/similarity.rs` (zero-vector/mismatched-length/NaN-safety unit tests).
