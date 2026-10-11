-- Peer clustering, Phase 1 (spec/26): a peer's self-declared, signed claim
-- of which cluster it belongs to. Deliberately an ordinary upsert, not
-- `node_gravity`'s `INSERT OR IGNORE` -- a claimed cluster is current
-- status, not a stable physical trait, so a later declaration legitimately
-- overwrites an earlier one.
--
-- No corroboration, no authorization, no link to anything else: this is a
-- bare signed self-assertion, by explicit design decision (see spec/26's
-- security caveat -- a cluster name is effectively a shared,
-- unauthenticated label, not a verified fact).
CREATE TABLE peer_cluster_claims (
    peer_id BLOB PRIMARY KEY,
    cluster TEXT NOT NULL,
    declared_at INTEGER NOT NULL
);
