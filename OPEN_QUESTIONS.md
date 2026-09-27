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
