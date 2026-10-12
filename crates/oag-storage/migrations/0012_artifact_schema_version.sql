-- spec/27-artifact-versioning.md: the per-artifact version number the project's
-- "legacy must flow" principle requires. `NOT NULL DEFAULT 1` so every existing
-- row (and every row inserted by code that doesn't yet know this column exists)
-- is correctly marked "current" -- there is no second schema shape yet for any
-- of these four tables, so this is a true no-op today. Same additive-column
-- shape as 0007_fork_evidence.sql.
ALTER TABLE nodes ADD COLUMN schema_version INTEGER NOT NULL DEFAULT 1;
ALTER TABLE edges ADD COLUMN schema_version INTEGER NOT NULL DEFAULT 1;
ALTER TABLE assertions ADD COLUMN schema_version INTEGER NOT NULL DEFAULT 1;
ALTER TABLE evidence ADD COLUMN schema_version INTEGER NOT NULL DEFAULT 1;
