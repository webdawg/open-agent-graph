-- CIDs this peer has pinned, from every path that records one. One row per CID.
--
-- Before this table the only recorded CID lived in `evidence.metadata` under an `ipfs_cid` key,
-- written when the crawler pinned a page -- so the discovery surface (`GET /api/v1/pins`) could
-- only answer "which CIDs did this peer pin as evidence", not "what does it hold". Pins made by
-- `oag ipfs pin`, `oag backup --pin`, and `oag ipfs host-page` left no queryable trace at all.
--
-- `source` records which path recorded it, so an operator can tell an evidence pin from an
-- operator's explicit `oag ipfs pin`. It is descriptive, not a permission or trust signal.
CREATE TABLE pinned_cids (
    cid TEXT PRIMARY KEY,
    pinned_at INTEGER NOT NULL,
    source TEXT NOT NULL
);

-- Backfill from the one place a CID was already recorded, so a peer that has been running has its
-- history preserved rather than silently reported as holding nothing. `MAX(retrieved_at)` gives the
-- most recent evidence retrieval for each CID; `COALESCE` guards a NULL timestamp.
INSERT OR IGNORE INTO pinned_cids (cid, pinned_at, source)
SELECT json_extract(metadata, '$.ipfs_cid'),
       MAX(COALESCE(retrieved_at, 0)),
       'evidence'
FROM evidence
WHERE json_extract(metadata, '$.ipfs_cid') IS NOT NULL
GROUP BY json_extract(metadata, '$.ipfs_cid');
