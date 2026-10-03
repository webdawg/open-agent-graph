-- Spec section 55: `peer_forks` was meant to be fork *evidence* (see
-- 0002_peers.sql's own comment), but only ever stored the two conflicting
-- events' bare ids. `event_id_a` (the already-accepted event) is still
-- retrievable from `events` by that id, but `event_id_b` (the incoming
-- event that triggered fork detection) is never inserted anywhere else --
-- its content was simply lost, leaving only a hash an operator can't do
-- anything with. Storing the incoming event's full signed JSON here is
-- what actually makes this evidence rather than just a flag.
ALTER TABLE peer_forks ADD COLUMN event_b_signed_json TEXT NOT NULL DEFAULT '';
