-- The gravity layer (spec/25-gravity.md): one small, truly random "how
-- much gravity is at this node" value per peer, rolled once with a strong
-- RNG and persisted -- not regenerated every call, so it behaves like the
-- stand-in for a future hardware-measured physical trait that it is,
-- rather than a fresh coin flip each time anyone asks. Stable across
-- restarts for a permanent identity; rolled fresh each time for an
-- ephemeral one, same as its peer_id (see spec/24).
CREATE TABLE node_gravity (
    peer_id BLOB PRIMARY KEY,
    gravity_level REAL NOT NULL,
    generated_at INTEGER NOT NULL
);
