-- Ephemeral peer trust, Phase 1: presence continuity, tracked separately
-- from `peers.last_seen` (a different, coarser existing signal). One row
-- per `peer_id`'s *current identity lifetime* -- under ephemeral identity,
-- a restart means a brand-new `peer_id` and therefore a brand-new row, by
-- design (see spec/24-ephemeral-peer-trust.md: identity rotation is
-- deliberately not linked across the old and new row).
CREATE TABLE peer_presence (
    peer_id BLOB PRIMARY KEY,
    session_started_at INTEGER NOT NULL,
    last_heartbeat_at INTEGER NOT NULL,
    status TEXT NOT NULL
);

-- The address-correlated "announced restart" grace window (this plan's
-- reading of "voting right" -- see spec/24). Deliberately keyed by network
-- address, not peer_id: when a peer announces it's going offline and a
-- *different* (by design, under ephemeral identity) peer_id's heartbeat
-- next arrives from that same address within the grace window, the
-- receiving peer may treat it more gently. This is a soft, local,
-- unprovable heuristic -- not a cryptographic link between the old and new
-- peer_id, which is deliberately not built (see spec/24's "chain
-- continuity" decision).
CREATE TABLE address_going_offline (
    address TEXT PRIMARY KEY,
    announced_at INTEGER NOT NULL,
    from_peer_id BLOB NOT NULL
);
