-- Spec section 85 (Deletion and Redaction). Both tables are deliberately
-- excluded from `oag_storage::repo::rebuild::clear_projection_tables`'s wipe
-- list -- they are the durable record of an operator's redaction/suppression
-- decisions, not a cache derived from the event log, so `oag rebuild` must
-- never touch them.

-- Tombstone for evidence content that has been scrubbed locally (title/
-- excerpt blanked on the `evidence` row). `event_id` doubles as the
-- evidence row's own id (`Evidence.id` is the `ADD_EVIDENCE` event's id),
-- so this table only ever needs one column to look either up by the other.
-- The signed event itself in `events.canonical_payload` is never modified --
-- this table exists purely so the locally materialized `evidence` row stays
-- redacted across every future `oag rebuild`.
CREATE TABLE redactions (
    event_id BLOB PRIMARY KEY REFERENCES events (event_id),
    redacted_at INTEGER NOT NULL,
    reason TEXT
);

-- Hides a node from `oag search`/`/api/v1/search` without touching the node,
-- its edges, or its assertions -- a lesser, fully reversible intervention
-- than evidence redaction (spec section 85's "search suppression").
--
-- Deliberately no `REFERENCES nodes (node_id)`: unlike `events`, `nodes` IS
-- wiped and replayed by `oag rebuild` (`clear_projection_tables` deletes it
-- before replay reinserts it), and a foreign key here would make that
-- delete fail with a constraint violation for as long as any suppression
-- row survives rebuild -- which it must, by design.
CREATE TABLE search_suppressions (
    node_id BLOB PRIMARY KEY,
    suppressed_at INTEGER NOT NULL
);
