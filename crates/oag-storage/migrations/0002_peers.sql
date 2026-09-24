-- Peer bookkeeping for replication (oag-sync). Event/graph tables are
-- unchanged: event_origins already tracks per-origin chain state generically
-- (spec section 41), so remote origins reuse it as-is.

CREATE TABLE peers (
    peer_id BLOB PRIMARY KEY,
    public_key BLOB NOT NULL,
    name TEXT,
    first_seen INTEGER NOT NULL,
    last_seen INTEGER,
    forked INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE peer_addresses (
    peer_id BLOB NOT NULL REFERENCES peers (peer_id),
    address TEXT NOT NULL,
    PRIMARY KEY (peer_id, address)
);

-- Fork evidence (spec section 55): two different valid-signature events at
-- the same (origin, sequence). Retained, never silently resolved.
CREATE TABLE peer_forks (
    peer_id BLOB NOT NULL,
    sequence INTEGER NOT NULL,
    event_id_a BLOB NOT NULL,
    event_id_b BLOB NOT NULL,
    detected_at INTEGER NOT NULL,
    PRIMARY KEY (peer_id, sequence)
);
