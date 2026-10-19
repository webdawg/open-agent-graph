-- Spec section 85 (Deletion and Redaction), extending it to one more field:
-- an actor's `identity_uri` (could carry personal data, e.g. an
-- email-shaped URI) can now be blanked the same way evidence `title`/
-- `excerpt` already could.
--
-- Deliberately no `REFERENCES actors (actor_id)`, matching
-- `search_suppressions`' reasoning (`0006_redactions.sql`) rather than
-- `redactions`' own FK-to-`events`: `actors` IS wiped and replayed by
-- `oag rebuild` (`clear_projection_tables` deletes it before replay
-- reinserts it), and a foreign key here would make that delete fail with a
-- constraint violation for as long as any tombstone row survives -- which
-- it must, by design. `oag_graph::rebuild::rebuild_projection` reapplies
-- outstanding tombstones here after every replay, the same way it already
-- does for `redactions`.
--
-- This table is excluded from `clear_projection_tables`'s wipe list for the
-- same reason `redactions`/`search_suppressions` are: it's the durable
-- record of an operator's redaction decision, not a cache of the event log.
CREATE TABLE actor_identity_uri_redactions (
    actor_id BLOB PRIMARY KEY,
    redacted_at INTEGER NOT NULL,
    reason TEXT
);
