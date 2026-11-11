//! Content this peer has recorded holding, keyed by IPFS CID.
//!
//! **Where the data comes from, stated plainly.** The one place this codebase records a CID today
//! is the projector's `{"ipfs_cid": ...}` key in `evidence.metadata` (`crates/oag-events/src/
//! projector.rs`), written when the crawler pins a page. So this module answers "which CIDs has
//! this peer recorded pinning *as evidence*", not "what does this peer's IPFS node hold". Pins
//! made by `oag ipfs pin`, `oag backup --pin`, or `oag ipfs host-page` are not recorded anywhere
//! queryable yet; that gap is recorded rather than papered over here.
//!
//! This is the discovery primitive: a caller that wants a CID can ask a peer whether it holds it
//! instead of being told which peer to ask. Whether it holds it is a *record*, not a live check --
//! nothing here fetches the content to confirm it is still retrievable.

use sqlx::SqliteConnection;

use crate::error::StorageError;

/// How many CIDs `list_pins` returns when the caller does not say.
pub const DEFAULT_PIN_LIST_LIMIT: i64 = 100;

/// Upper bound on `list_pins`, matching `GraphService::search`'s cap: a caller must not be able to
/// make this peer serialize its entire pin set in one response.
pub const MAX_PIN_LIST_LIMIT: i64 = 1000;

/// Distinct CIDs recorded in evidence metadata, most recently pinned first.
///
/// The `json_extract` is over a `TEXT` column with no index, so this is a full scan of `evidence`.
/// That is acceptable for a bounded, operator-driven query and would need a real index if it ever
/// became a hot path -- which is exactly the kind of change that should not be made
/// speculatively.
pub async fn list_pins(conn: &mut SqliteConnection, limit: i64) -> Result<Vec<String>, StorageError> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT DISTINCT json_extract(metadata, '$.ipfs_cid') AS cid \
         FROM evidence \
         WHERE json_extract(metadata, '$.ipfs_cid') IS NOT NULL \
         ORDER BY retrieved_at DESC, id DESC \
         LIMIT ?",
    )
    .bind(limit)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows.into_iter().map(|(cid,)| cid).collect())
}

/// Whether any evidence row records this CID.
pub async fn holds_pin(conn: &mut SqliteConnection, cid: &str) -> Result<bool, StorageError> {
    let (count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM evidence \
         WHERE json_extract(metadata, '$.ipfs_cid') = ? \
         LIMIT 1",
    )
    .bind(cid)
    .fetch_one(&mut *conn)
    .await?;
    Ok(count > 0)
}

/// How many distinct CIDs are recorded.
pub async fn count_pins(conn: &mut SqliteConnection) -> Result<i64, StorageError> {
    let (count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(DISTINCT json_extract(metadata, '$.ipfs_cid')) FROM evidence \
         WHERE json_extract(metadata, '$.ipfs_cid') IS NOT NULL",
    )
    .fetch_one(&mut *conn)
    .await?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pool::open_pool;
    use crate::repo::assertions::{insert, insert_evidence};

    fn temp_db_path(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "oag-storage-pins-test-{name}-{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("oag.sqlite")
    }

    /// Seeds the full chain an evidence row needs -- actor, nodes, edge, assertion, evidence --
    /// because `evidence.assertion_id` and `assertions.edge_id` both have foreign keys. Each call
    /// gets its own actor, nodes, and edge so the ids differ.
    async fn seed(conn: &mut SqliteConnection, cid: Option<&str>) {
        use oag_core::{
            Actor, ActorId, Assertion, AssertionId, AssertionStatus, Edge, EdgeId, Evidence,
            EvidenceType, EventId, ExtractionMethod, Node, NodeType, Predicate,
        };
        use serde_json::json;

        // Every seeded row needs its own ids, even when two calls record the same CID -- that
        // duplicate is exactly what the DISTINCT in `list_pins` has to collapse.
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let tag = format!("{}-{}", cid.unwrap_or("none"), n);
        let now = 1_700_000_000_i64;

        let actor = Actor {
            id: ActorId::derive(format!("pins-actor-{tag}").as_bytes()),
            actor_type: oag_core::ActorType::Agent,
            name: None,
            public_key: None,
            identity_uri: None,
            metadata: json!({}),
            created_at: now,
        };
        crate::repo::actors::insert(conn, &actor).await.unwrap();

        let subject = Node::new(NodeType::repository(), format!("url:https://example.com/{tag}"), now);
        let object = Node::new(NodeType::concept(), format!("concept:{tag}"), now);
        crate::repo::nodes::insert_if_missing(conn, &subject).await.unwrap();
        crate::repo::nodes::insert_if_missing(conn, &object).await.unwrap();
        let edge = Edge::new(subject.id, Predicate::new("implements"), object.id, now);
        crate::repo::edges::insert_if_missing(conn, &edge).await.unwrap();

        let assertion = Assertion {
            id: AssertionId::derive(format!("pins-{tag}").as_bytes()),
            edge_id: edge.id,
            actor_id: actor.id,
            actor_confidence: None,
            observed_at: None,
            asserted_at: now,
            extraction_method: ExtractionMethod::Direct,
            status: AssertionStatus::Active,
        };
        insert(conn, &assertion).await.unwrap();

        let evidence = Evidence {
            id: EventId::derive(format!("ev-{tag}-{}", assertion.id.as_hash().to_string()).as_bytes()),
            assertion_id: assertion.id,
            evidence_type: EvidenceType::Documentation,
            uri: None,
            title: None,
            excerpt: None,
            content_hash: None,
            observed_at: None,
            retrieved_at: Some(now),
            metadata: match cid {
                Some(cid) => json!({ "ipfs_cid": cid }),
                None => json!({}),
            },
        };
        insert_evidence(conn, &evidence).await.unwrap();
    }

    #[tokio::test]
    async fn lists_only_recorded_cids_and_counts_them() {
        let pool = open_pool(&temp_db_path("list")).await.unwrap();
        let mut conn = pool.acquire().await.unwrap();

        assert_eq!(count_pins(&mut conn).await.unwrap(), 0);
        assert!(list_pins(&mut conn, 10).await.unwrap().is_empty());
        assert!(!holds_pin(&mut conn, "QmNothing").await.unwrap());

        seed(&mut conn, Some("QmAlpha")).await;
        seed(&mut conn, Some("QmBeta")).await;
        seed(&mut conn, Some("QmAlpha")).await; // duplicate CID, one distinct entry
        seed(&mut conn, None).await; // no CID recorded at all

        assert_eq!(count_pins(&mut conn).await.unwrap(), 2);
        let pins = list_pins(&mut conn, 10).await.unwrap();
        assert_eq!(pins.len(), 2, "duplicates must collapse: {pins:?}");
        assert!(holds_pin(&mut conn, "QmAlpha").await.unwrap());
        assert!(holds_pin(&mut conn, "QmBeta").await.unwrap());
        assert!(!holds_pin(&mut conn, "QmGamma").await.unwrap());
    }

    #[tokio::test]
    async fn the_limit_is_honoured() {
        let pool = open_pool(&temp_db_path("limit")).await.unwrap();
        let mut conn = pool.acquire().await.unwrap();
        for i in 0..5 {
            seed(&mut conn, Some(&format!("Qm{i}"))).await;
        }
        assert_eq!(list_pins(&mut conn, 2).await.unwrap().len(), 2);
    }
}
