-- OAG single-peer core schema.
--
-- Every table here is either the event log itself or a thin, rebuildable
-- projection index over it (see plan: "projection tables stay thin"). Full
-- fidelity for any event always lives in events.canonical_payload.

CREATE TABLE events (
    event_id BLOB PRIMARY KEY,
    origin_peer_id BLOB NOT NULL,
    sequence INTEGER NOT NULL,
    previous_event_id BLOB,
    event_type TEXT NOT NULL,
    canonical_payload BLOB NOT NULL,
    created_at INTEGER NOT NULL,
    signature BLOB NOT NULL,
    received_at INTEGER NOT NULL,
    UNIQUE (origin_peer_id, sequence)
);

CREATE INDEX idx_events_event_type ON events (event_type);
CREATE INDEX idx_events_created_at ON events (created_at);

-- Lets `/history/{object_type}/{id}` find every event that touched a given
-- node/edge/assertion without scanning canonical_payload blobs.
CREATE TABLE event_refs (
    event_id BLOB NOT NULL REFERENCES events (event_id),
    ref_type TEXT NOT NULL,
    ref_id BLOB NOT NULL,
    PRIMARY KEY (ref_type, ref_id, event_id)
);

CREATE INDEX idx_event_refs_ref ON event_refs (ref_type, ref_id);

CREATE TABLE event_origins (
    origin_peer_id BLOB PRIMARY KEY,
    highest_contiguous_sequence INTEGER NOT NULL DEFAULT 0,
    highest_seen_sequence INTEGER NOT NULL DEFAULT 0,
    head_event_id BLOB
);

CREATE TABLE nodes (
    node_id BLOB PRIMARY KEY,
    node_type TEXT NOT NULL,
    canonical_identifier TEXT NOT NULL UNIQUE,
    canonical_uri TEXT,
    name TEXT,
    description TEXT,
    metadata TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL
);

CREATE INDEX idx_nodes_node_type ON nodes (node_type);

CREATE TABLE node_aliases (
    node_id BLOB NOT NULL REFERENCES nodes (node_id),
    alias TEXT NOT NULL,
    alias_type TEXT NOT NULL,
    source_assertion BLOB,
    PRIMARY KEY (node_id, alias)
);

CREATE TABLE edges (
    edge_id BLOB PRIMARY KEY,
    subject_node_id BLOB NOT NULL REFERENCES nodes (node_id),
    predicate TEXT NOT NULL,
    object_node_id BLOB NOT NULL REFERENCES nodes (node_id),
    created_at INTEGER NOT NULL
);

CREATE INDEX idx_edges_subject ON edges (subject_node_id);
CREATE INDEX idx_edges_object ON edges (object_node_id);
CREATE INDEX idx_edges_predicate ON edges (predicate);

CREATE TABLE actors (
    actor_id BLOB PRIMARY KEY,
    actor_type TEXT NOT NULL,
    name TEXT,
    public_key BLOB,
    identity_uri TEXT,
    metadata TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL
);

CREATE TABLE actor_keys (
    key_hash BLOB PRIMARY KEY,
    actor_id BLOB NOT NULL REFERENCES actors (actor_id),
    permissions TEXT NOT NULL DEFAULT '[]',
    created_at INTEGER NOT NULL,
    revoked_at INTEGER
);

CREATE INDEX idx_actor_keys_actor_id ON actor_keys (actor_id);

CREATE TABLE assertions (
    id BLOB PRIMARY KEY,
    edge_id BLOB NOT NULL REFERENCES edges (edge_id),
    actor_id BLOB NOT NULL REFERENCES actors (actor_id),
    actor_confidence REAL,
    observed_at INTEGER,
    asserted_at INTEGER NOT NULL,
    extraction_method TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active'
);

CREATE INDEX idx_assertions_edge_id ON assertions (edge_id);
CREATE INDEX idx_assertions_actor_id ON assertions (actor_id);
CREATE INDEX idx_assertions_status ON assertions (status);

CREATE TABLE evidence (
    id BLOB PRIMARY KEY,
    assertion_id BLOB NOT NULL REFERENCES assertions (id),
    evidence_type TEXT NOT NULL,
    uri TEXT,
    title TEXT,
    excerpt TEXT,
    content_hash TEXT,
    observed_at INTEGER,
    retrieved_at INTEGER,
    metadata TEXT NOT NULL DEFAULT '{}'
);

CREATE INDEX idx_evidence_assertion_id ON evidence (assertion_id);

CREATE TABLE observations (
    id BLOB PRIMARY KEY,
    assertion_id BLOB NOT NULL REFERENCES assertions (id),
    observer_actor_id BLOB NOT NULL REFERENCES actors (actor_id),
    result TEXT NOT NULL,
    observed_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE INDEX idx_observations_assertion_id ON observations (assertion_id);

CREATE TABLE assertion_disputes (
    id BLOB PRIMARY KEY,
    disputed_assertion_id BLOB NOT NULL REFERENCES assertions (id),
    disputing_actor_id BLOB NOT NULL REFERENCES actors (actor_id),
    reason TEXT,
    created_at INTEGER NOT NULL
);

CREATE INDEX idx_disputes_disputed_assertion ON assertion_disputes (disputed_assertion_id);

CREATE TABLE assertion_retractions (
    id BLOB PRIMARY KEY,
    retracted_assertion_id BLOB NOT NULL REFERENCES assertions (id),
    actor_id BLOB NOT NULL REFERENCES actors (actor_id),
    reason TEXT,
    created_at INTEGER NOT NULL
);

CREATE INDEX idx_retractions_retracted_assertion ON assertion_retractions (retracted_assertion_id);

CREATE TABLE assertion_supersessions (
    id BLOB PRIMARY KEY,
    old_assertion_id BLOB NOT NULL REFERENCES assertions (id),
    new_assertion_id BLOB NOT NULL REFERENCES assertions (id),
    actor_id BLOB NOT NULL REFERENCES actors (actor_id),
    created_at INTEGER NOT NULL
);

CREATE INDEX idx_supersessions_old ON assertion_supersessions (old_assertion_id);

-- Full-text search (spec section 63): node name/description and evidence
-- title/excerpt/uri. `content='nodes'`/`content='evidence'` makes these
-- external-content tables so we don't duplicate the data; triggers below
-- keep them in sync.
CREATE VIRTUAL TABLE nodes_fts USING fts5(
    name,
    description,
    content = 'nodes',
    content_rowid = 'rowid'
);

CREATE TRIGGER nodes_fts_ai AFTER INSERT ON nodes BEGIN
    INSERT INTO nodes_fts (rowid, name, description)
    VALUES (new.rowid, new.name, new.description);
END;

CREATE TRIGGER nodes_fts_ad AFTER DELETE ON nodes BEGIN
    INSERT INTO nodes_fts (nodes_fts, rowid, name, description)
    VALUES ('delete', old.rowid, old.name, old.description);
END;

CREATE TRIGGER nodes_fts_au AFTER UPDATE ON nodes BEGIN
    INSERT INTO nodes_fts (nodes_fts, rowid, name, description)
    VALUES ('delete', old.rowid, old.name, old.description);
    INSERT INTO nodes_fts (rowid, name, description)
    VALUES (new.rowid, new.name, new.description);
END;

CREATE VIRTUAL TABLE evidence_fts USING fts5(
    title,
    excerpt,
    uri,
    content = 'evidence',
    content_rowid = 'rowid'
);

CREATE TRIGGER evidence_fts_ai AFTER INSERT ON evidence BEGIN
    INSERT INTO evidence_fts (rowid, title, excerpt, uri)
    VALUES (new.rowid, new.title, new.excerpt, new.uri);
END;

CREATE TRIGGER evidence_fts_ad AFTER DELETE ON evidence BEGIN
    INSERT INTO evidence_fts (evidence_fts, rowid, title, excerpt, uri)
    VALUES ('delete', old.rowid, old.title, old.excerpt, old.uri);
END;

CREATE TRIGGER evidence_fts_au AFTER UPDATE ON evidence BEGIN
    INSERT INTO evidence_fts (evidence_fts, rowid, title, excerpt, uri)
    VALUES ('delete', old.rowid, old.title, old.excerpt, old.uri);
    INSERT INTO evidence_fts (rowid, title, excerpt, uri)
    VALUES (new.rowid, new.title, new.excerpt, new.uri);
END;
