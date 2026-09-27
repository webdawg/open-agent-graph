-- Spec section 64's semantic search: embeddings stored locally, no
-- mandatory external vector database. This is a thin, fully-rebuildable
-- cache -- oag_graph::semantic_search::recompute_embeddings is the only
-- writer, replacing the whole table each time it runs (same shape as
-- node_authority). `embedding` is a raw little-endian f32 blob decoded by
-- the storage layer; brute-force cosine similarity happens in Rust at
-- query time, not in SQL -- v1 scope deliberately skips a native ANN
-- index/extension (see OPEN_QUESTIONS.md).

CREATE TABLE node_embeddings (
    node_id BLOB PRIMARY KEY,
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    dim INTEGER NOT NULL,
    embedding BLOB NOT NULL,
    computed_at INTEGER NOT NULL
);
