# Open Questions

Design/scope questions that came up during background work. Each one has the default assumption
I'm proceeding with, so work keeps moving instead of blocking. Override any of these any time —
ask me "are there open questions?" or just edit this file directly.

Format: question, assumption I'm running with, status.

## Semantic search (spec section 64)

- **Local embedding model?** Spec section 64 lists "local model" as a possible `EmbeddingProvider`
  implementation. Assumption: deferred entirely for this milestone — an on-device model (ONNX
  Runtime or similar) is a much heavier dependency than an HTTP client, and `OpenAiCompatibleProvider`
  already covers self-hosted/local setups (Ollama, LM Studio, vLLM, text-embeddings-inference all
  speak the same wire shape and can run fully offline on localhost). Status: open.
- **Embedding storage format.** `node_embeddings.embedding` is a raw little-endian `f32` BLOB
  (4 bytes/dimension), not JSON or bincode. Assumption: this is the simplest, most compact
  representation and needs no serde dependency in `oag-storage` for this table. Status: resolved
  (implemented this way; revisit only if cross-language tooling needs to read the column directly).
- **No ANN index / vector extension.** Ranking is brute-force cosine similarity over every stored
  embedding, computed in Rust at query time — no `sqlite-vec`/`vss` extension, no external vector
  database (spec section 64 explicitly prohibits a mandatory external vector DB). Assumption: fine
  for the graph sizes this project targets; swap in a real ANN index (e.g. HNSW) later if
  `node_embeddings` grows large enough that a full scan becomes slow. Status: open (revisit if perf
  becomes an issue).
- **REST/MCP surface: extend vs. new endpoint.** Followed the exact precedent set by `authority`
  (spec section 65): recomputation (`recompute_embeddings`) is CLI-only (`oag embeddings recompute`),
  with no REST/MCP endpoint at all — same as `oag authority recompute` has none. The read path
  (`semantic_search`) extends the *existing* `/api/v1/search` REST endpoint and `graph_search` MCP
  tool with a `semantic: bool` parameter, rather than adding new endpoints — same as how `authority`
  became an extra field on the existing `get_node` response instead of its own route. Status: resolved.
- **Config shape for `[search]`.** Added `embedding_provider` ("disabled" | "openai_compatible"),
  `embedding_base_url`, `embedding_api_key`, `embedding_model` fields alongside the existing
  `semantic_enabled`. Assumption: `semantic_enabled` controls whether `oag search` ranks
  semantically *by default* without `--semantic`; the CLI flag can still force it on for one
  invocation. Either way, if the resolved provider ends up being `DisabledProvider` (no
  `embedding_base_url` configured, or `embedding_provider` left as "disabled"), the search errors
  clearly (`embeddings are disabled`) rather than silently falling back to keyword FTS — consistent
  with spec section 64's "typed error, never panic" requirement, and avoids a caller mistaking a
  degraded keyword result for a real semantic one. Status: resolved.

## Metrics (spec section 86)

- **Scope: cheap current-state snapshot only, not full spec compliance.** Implemented
  `events_total`, `nodes_total`, `edges_total`, `assertions_total`, `evidence_total`, `peer_count`,
  and `sqlite_size_bytes` -- the subset of the spec's suggested measurements that are simple
  `COUNT(*)`/`PRAGMA` queries against tables that already exist. Deliberately NOT implemented this
  milestone: `events_by_origin` (a labeled/vector metric -- needs a GROUP BY and a decision on label
  cardinality), `events_pending_chain`, `peer_sync_lag`, `peer_sync_errors`,
  `replication_bytes_in`/`out`, `search_latency`, `api_latency` (all need request-duration
  instrumentation -- a middleware/histogram layer, not a snapshot query), `crawl_jobs`/
  `crawl_failures`, `verification_jobs`/`verification_failures` (need counters wired through the
  crawler and verifier code paths), and `blob_store_size` (no blob store exists yet -- spec section
  84's `blobs/<hash>` is itself unimplemented). Assumption: shipping the correct, cheap subset now is
  better than blocking the whole endpoint on the request-timing/counter-wiring work the rest needs.
  Status: open (revisit alongside whichever of request tracing, crawler/verifier job counters, or
  the blob store lands first).
- **Output format: hand-rolled Prometheus text exposition, no external crate.** `GET /metrics`
  returns `text/plain; version=0.0.4` built by a small pure `render()` function in `oag-api`, rather
  than pulling in `metrics`/`metrics-exporter-prometheus`. Assumption: spec section 86 explicitly
  says no external metrics platform is required to operate OAG, and seven `# HELP`/`# TYPE`/value
  line groups don't justify a dependency -- consistent with this project's general preference for a
  small hand-rolled implementation over a heavyweight crate when the surface is this small (see
  `oag-embeddings` only pulling in `reqwest`, not a full SDK). Status: resolved.
- **Unauthenticated endpoint.** `/metrics` needs no bearer token, same as `/api/v1/status`.
  Assumption: metrics endpoints are conventionally open so a scraper doesn't need a credential
  provisioned, and nothing in the seven exposed metrics is sensitive (all are aggregate counts/
  sizes, no per-actor or per-peer breakdown). Status: resolved.
- **Route placement: top-level `/metrics`, not under `/api/v1`.** Matches the spec's literal path
  and Prometheus's own scrape-path convention, rather than being namespaced with the rest of the
  REST API. Status: resolved.

## LLM extraction (spec section 74)

- **Runs unconditionally as a fallback pass, not gated behind a CLI flag.** Spec section 73's
  priority order is "prefer deterministic, then use LLM extraction" -- read as a default pipeline
  order, not an opt-in the operator has to remember per crawl. A disabled extractor
  (`DisabledExtractor`, the default) returns zero candidates and makes no network request, so this
  is a true no-op when unconfigured -- unlike `oag search --semantic`, there's no separate flag to
  turn it on for one invocation, since there's no meaningful "run LLM extraction just this once"
  use case for a background/batch crawl. Status: open (revisit if a real use case for per-invocation
  override shows up).
- **No threshold on candidate confidence -- every candidate is asserted.** Low-confidence
  LLM-extracted assertions still go through `assert()` at whatever confidence the model reported,
  rather than being dropped below some cutoff. Assumption: the corroboration/ranking layer (spec
  section 65 -- `EdgeCorroboration`, already complete) is the intended place low-confidence,
  single-source claims get down-weighted, not the extraction layer itself; filtering here would
  hide information ranking could otherwise use. Status: open (revisit if low-confidence LLM noise
  turns out to be a real problem in practice).
- **Extractor identity/model info live entirely in the actor name, not a new field.** Every
  candidate is asserted by a `Model`-type actor named `llm-extractor:<model>` (e.g.
  `llm-extractor:gpt-4o-mini`), reusing the identical pattern the "crawler" actor already
  establishes for structured extraction -- no new schema field for "extractor identity" or "model
  information" (spec section 74's wording) since `assert()`'s existing actor/evidence/confidence/
  timestamp machinery already covers all of it once the actor itself encodes the model. Status:
  resolved.
- **Malformed LLM responses are treated as zero candidates, not a crawl failure.** A non-JSON or
  wrong-shape chat completion logs a `tracing::warn!` and the crawl continues with whatever
  deterministic facts it already found. Assumption: an occasional bad LLM response is expected
  behavior, not a bug -- failing the whole crawl over it would make LLM extraction net-negative for
  reliability. Status: resolved.
- **Config shape mirrors `[search]`'s embedding fields exactly, under `[crawler]`.** Added
  `llm_provider` ("disabled" | "openai_compatible"), `llm_base_url`, `llm_api_key`, `llm_model` to
  `CrawlerSection` rather than inventing a new `[llm]` table -- LLM extraction is crawler-specific
  (unlike embeddings, which search also needs), so it belongs in the section that already owns
  crawl-time behavior. Status: resolved.
