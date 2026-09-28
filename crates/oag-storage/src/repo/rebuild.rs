//! Spec section 83: every table here is a derived, rebuildable projection
//! of the immutable event log — never `events`/`event_origins`/`event_refs`
//! themselves (the source of truth and its own bookkeeping), and never the
//! peer-network tables (`peers`/`peer_addresses`/`peer_forks`/
//! `peer_known_heads`, unrelated to graph projection) or `node_authority`
//! (its own separately-recomputed cache — `edges` reconstructs byte-
//! identical, so existing authority scores stay valid without a recompute).
//! Also never `redactions`/`search_suppressions` (spec section 85) — those
//! are durable operator decisions, not a cache of the event log, and must
//! survive every rebuild; `oag_graph::rebuild::rebuild_projection` reapplies
//! outstanding redactions after replaying, precisely because this function
//! never wipes them in the first place.
use sqlx::SqliteConnection;

use crate::error::StorageError;

/// Delete order matters: children before parents, so `foreign_keys=ON`
/// (spec section 9) doesn't reject a delete against a still-referenced row.
const TABLES_CHILDREN_FIRST: &[&str] = &[
    "evidence",
    "observations",
    "assertion_disputes",
    "assertion_retractions",
    "assertion_supersessions",
    "assertions",
    "actor_keys",
    "node_aliases",
    "edges",
    "actors",
    "nodes",
];

pub async fn clear_projection_tables(conn: &mut SqliteConnection) -> Result<(), StorageError> {
    for table in TABLES_CHILDREN_FIRST {
        // Safe to build the statement dynamically here: `table` only ever
        // comes from the fixed, hardcoded `&'static str` list above, never
        // from any external input.
        sqlx::query(sqlx::AssertSqlSafe(format!("DELETE FROM {table}")))
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}
