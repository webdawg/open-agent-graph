-- Spec section 65's `authority` ranking signal: a node-level, query-
-- independent PageRank-style score over the local edge graph. Recomputed
-- in full (not incrementally) by GraphService::recompute_authority, so this
-- table is a thin, fully-rebuildable cache, same spirit as every other
-- projection table.

CREATE TABLE node_authority (
    node_id BLOB PRIMARY KEY,
    score REAL NOT NULL,
    computed_at INTEGER NOT NULL
);
