-- Durability visibility (no single peer's storage/RAM is ever assumed
-- reliable — durability comes from how many peers are known to have a
-- given origin's data). Every `hello` response already carries the
-- responding peer's own heads for every origin it knows about; this table
-- is where that gets persisted instead of discarded after each sync.

CREATE TABLE peer_known_heads (
    peer_id BLOB NOT NULL,
    origin_peer_id BLOB NOT NULL,
    sequence INTEGER NOT NULL,
    observed_at INTEGER NOT NULL,
    PRIMARY KEY (peer_id, origin_peer_id)
);

CREATE INDEX idx_peer_known_heads_origin ON peer_known_heads (origin_peer_id);
