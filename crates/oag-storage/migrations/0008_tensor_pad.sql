-- The "ant memory node": one small, persistent tensor per peer, keyed by
-- that peer's own `peer_id` (not a remote peer's -- today exactly one row
-- ever exists, this peer's own pad; keying by peer_id rather than a
-- singleton row leaves room for a future per-remote-peer pad without a
-- schema change). `pad_values` is a raw little-endian f32 blob, same
-- encoding as `node_embeddings.embedding` -- decoded by the storage layer,
-- not SQL. (Named `pad_values`, not `values`, since the latter collides
-- with the SQL keyword used in INSERT statements.)

CREATE TABLE tensor_pads (
    peer_id BLOB PRIMARY KEY,
    pad_values BLOB NOT NULL,
    updated_at INTEGER NOT NULL
);
