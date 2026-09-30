use oag_core::{
    Actor, ActorType, Assertion, AssertionStatus, Edge, Evidence, EvidenceType, ExtractionMethod,
    Node, NodeType, Permission, Predicate,
};

use crate::pool::open_pool;
use crate::repo;

fn temp_db_path(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "oag-storage-test-{name}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("oag.sqlite")
}

#[tokio::test]
async fn migrations_create_expected_tables() {
    let path = temp_db_path("migrations");
    let pool = open_pool(&path).await.unwrap();
    let tables: Vec<(String,)> = sqlx::query_as(
        "SELECT name FROM sqlite_master WHERE type IN ('table', 'view') ORDER BY name",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    let names: Vec<&str> = tables.iter().map(|(n,)| n.as_str()).collect();
    for expected in [
        "events",
        "event_origins",
        "event_refs",
        "nodes",
        "node_aliases",
        "edges",
        "actors",
        "actor_keys",
        "assertions",
        "evidence",
        "observations",
        "assertion_disputes",
        "assertion_retractions",
        "assertion_supersessions",
        "peers",
        "peer_addresses",
        "peer_forks",
        "peer_known_heads",
        "node_authority",
    ] {
        assert!(names.contains(&expected), "missing table {expected}");
    }
}

#[tokio::test]
async fn peer_repo_round_trip_and_fork_recording() {
    let path = temp_db_path("peers");
    let pool = open_pool(&path).await.unwrap();
    let mut conn = pool.acquire().await.unwrap();

    let peer_id = [7u8; 32];
    let public_key = [9u8; 32];

    repo::peers::upsert_peer(&mut conn, &peer_id, &public_key, Some("peer-a"), 100)
        .await
        .unwrap();
    repo::peers::add_address(&mut conn, &peer_id, "http://127.0.0.1:7443").await.unwrap();

    let info = repo::peers::get_peer(&mut conn, &peer_id).await.unwrap().unwrap();
    assert_eq!(info.public_key, public_key);
    assert_eq!(info.name.as_deref(), Some("peer-a"));
    assert!(!info.forked);

    let addrs = repo::peers::list_addresses(&mut conn, &peer_id).await.unwrap();
    assert_eq!(addrs, vec!["http://127.0.0.1:7443".to_string()]);

    // re-upsert with no name shouldn't clobber the existing name, but should bump last_seen
    repo::peers::upsert_peer(&mut conn, &peer_id, &public_key, None, 200).await.unwrap();
    let info = repo::peers::get_peer(&mut conn, &peer_id).await.unwrap().unwrap();
    assert_eq!(info.name.as_deref(), Some("peer-a"));
    assert_eq!(info.last_seen, Some(200));

    repo::peers::record_fork(&mut conn, &peer_id, 5, &[1u8; 32], &[2u8; 32], 300)
        .await
        .unwrap();
    repo::peers::mark_forked(&mut conn, &peer_id).await.unwrap();

    let info = repo::peers::get_peer(&mut conn, &peer_id).await.unwrap().unwrap();
    assert!(info.forked);
    let forks = repo::peers::list_forks(&mut conn, &peer_id).await.unwrap();
    assert_eq!(forks.len(), 1);
}

#[tokio::test]
async fn assert_evidence_dispute_retract_round_trip() {
    let path = temp_db_path("vertical-slice");
    let pool = open_pool(&path).await.unwrap();
    let mut conn = pool.acquire().await.unwrap();

    let now = 1_700_000_000_i64;

    let actor = Actor {
        id: oag_core::ActorId::derive(b"test-actor"),
        actor_type: ActorType::Agent,
        name: Some("test agent".into()),
        public_key: None,
        identity_uri: None,
        metadata: serde_json::json!({}),
        created_at: now,
    };
    repo::actors::insert(&mut conn, &actor).await.unwrap();

    let subject = Node::new(NodeType::repository(), "url:https://github.com/example/foo", now);
    let object = Node::new(NodeType::concept(), "concept:model-context-protocol", now);
    repo::nodes::insert_if_missing(&mut conn, &subject).await.unwrap();
    repo::nodes::insert_if_missing(&mut conn, &object).await.unwrap();

    let edge = Edge::new(subject.id, Predicate::new("implements"), object.id, now);
    repo::edges::insert_if_missing(&mut conn, &edge).await.unwrap();

    let assertion = Assertion {
        id: oag_core::EventId::derive(b"fake-event-1"),
        edge_id: edge.id,
        actor_id: actor.id,
        actor_confidence: Some(0.97),
        observed_at: Some(now),
        asserted_at: now,
        extraction_method: ExtractionMethod::Direct,
        status: AssertionStatus::Active,
    };
    repo::assertions::insert(&mut conn, &assertion).await.unwrap();

    let evidence = Evidence {
        id: oag_core::EventId::derive(b"fake-event-2"),
        assertion_id: assertion.id,
        evidence_type: EvidenceType::Documentation,
        uri: Some("https://github.com/example/foo/blob/main/README.md".into()),
        title: Some("README".into()),
        excerpt: None,
        content_hash: None,
        observed_at: Some(now),
        retrieved_at: Some(now),
        metadata: serde_json::json!({}),
    };
    repo::assertions::insert_evidence(&mut conn, &evidence).await.unwrap();

    let fetched = repo::assertions::get_by_id(&mut conn, assertion.id)
        .await
        .unwrap()
        .expect("assertion round-trips");
    assert_eq!(fetched.status, AssertionStatus::Active);

    let ev_list = repo::assertions::list_evidence(&mut conn, assertion.id).await.unwrap();
    assert_eq!(ev_list.len(), 1);

    // dispute
    repo::assertions::insert_dispute(
        &mut conn,
        &repo::assertions::Dispute {
            id: oag_core::EventId::derive(b"fake-event-3"),
            disputed_assertion_id: assertion.id,
            disputing_actor_id: actor.id,
            reason: Some("no longer true".into()),
            created_at: now + 1,
        },
    )
    .await
    .unwrap();
    repo::assertions::set_status(&mut conn, assertion.id, AssertionStatus::Disputed)
        .await
        .unwrap();

    let disputes = repo::assertions::list_disputes(&mut conn, assertion.id).await.unwrap();
    assert_eq!(disputes.len(), 1);

    // retract
    repo::assertions::insert_retraction(
        &mut conn,
        &repo::assertions::Retraction {
            id: oag_core::EventId::derive(b"fake-event-4"),
            retracted_assertion_id: assertion.id,
            actor_id: actor.id,
            reason: None,
            created_at: now + 2,
        },
    )
    .await
    .unwrap();
    repo::assertions::set_status(&mut conn, assertion.id, AssertionStatus::Retracted)
        .await
        .unwrap();

    let refetched = repo::assertions::get_by_id(&mut conn, assertion.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(refetched.status, AssertionStatus::Retracted);

    // original assertion is still resolvable — history preserved, not deleted
    let retractions = repo::assertions::list_retractions(&mut conn, assertion.id).await.unwrap();
    assert_eq!(retractions.len(), 1);
}

#[tokio::test]
async fn fts_search_finds_node_after_update() {
    let path = temp_db_path("fts");
    let pool = open_pool(&path).await.unwrap();
    let mut conn = pool.acquire().await.unwrap();

    let mut node = Node::new(NodeType::software(), "url:https://example.com/proj", 1);
    node.name = Some("Example Project".into());
    node.description = Some("a rust implementation of a graph".into());
    repo::nodes::insert_if_missing(&mut conn, &node).await.unwrap();

    let found = repo::nodes::search(&mut conn, "graph", 10).await.unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id, node.id);

    // FTS5 external-content triggers only fire on writes through the base
    // table (UPDATE/INSERT/DELETE) — confirm the AFTER UPDATE trigger keeps
    // the index in sync.
    sqlx::query("UPDATE nodes SET description = ? WHERE node_id = ?")
        .bind("an unrelated rewritten description")
        .bind(node.id.as_hash().as_bytes().to_vec())
        .execute(&mut *conn)
        .await
        .unwrap();

    let stale = repo::nodes::search(&mut conn, "graph", 10).await.unwrap();
    assert!(stale.is_empty(), "FTS index should no longer match old text");

    let fresh = repo::nodes::search(&mut conn, "unrelated", 10).await.unwrap();
    assert_eq!(fresh.len(), 1);
}

#[tokio::test]
async fn known_head_upsert_round_trips_and_overwrites() {
    let path = temp_db_path("known-heads");
    let pool = open_pool(&path).await.unwrap();
    let mut conn = pool.acquire().await.unwrap();

    let peer_id = [1u8; 32];
    let origin_a = [2u8; 32];
    let origin_b = [3u8; 32];

    repo::replication::upsert_known_head(&mut conn, &peer_id, &origin_a, 5, 100).await.unwrap();
    repo::replication::upsert_known_head(&mut conn, &peer_id, &origin_b, 9, 100).await.unwrap();

    let heads_a = repo::replication::list_known_heads_for_origin(&mut conn, &origin_a).await.unwrap();
    assert_eq!(heads_a.len(), 1);
    assert_eq!(heads_a[0].peer_id, peer_id);
    assert_eq!(heads_a[0].sequence, 5);

    // A later report for the same (peer, origin) overwrites rather than duplicating.
    repo::replication::upsert_known_head(&mut conn, &peer_id, &origin_a, 12, 200).await.unwrap();
    let heads_a = repo::replication::list_known_heads_for_origin(&mut conn, &origin_a).await.unwrap();
    assert_eq!(heads_a.len(), 1, "same (peer, origin) must overwrite, not duplicate");
    assert_eq!(heads_a[0].sequence, 12);
    assert_eq!(heads_a[0].observed_at, 200);

    // origin_b's row is untouched by origin_a's updates.
    let heads_b = repo::replication::list_known_heads_for_origin(&mut conn, &origin_b).await.unwrap();
    assert_eq!(heads_b.len(), 1);
    assert_eq!(heads_b[0].sequence, 9);
}

#[tokio::test]
async fn node_authority_upsert_get_and_clear_round_trip() {
    let path = temp_db_path("node-authority");
    let pool = open_pool(&path).await.unwrap();
    let mut conn = pool.acquire().await.unwrap();

    let node = Node::new(NodeType::concept(), "concept:authority-test", 1);
    repo::nodes::insert_if_missing(&mut conn, &node).await.unwrap();

    assert_eq!(repo::node_authority::get(&mut conn, node.id).await.unwrap(), None);

    repo::node_authority::upsert(&mut conn, node.id, 0.42, 100).await.unwrap();
    let score = repo::node_authority::get(&mut conn, node.id).await.unwrap();
    assert!((score.unwrap() - 0.42).abs() < 1e-6);

    // Overwrite, not duplicate.
    repo::node_authority::upsert(&mut conn, node.id, 0.9, 200).await.unwrap();
    let score = repo::node_authority::get(&mut conn, node.id).await.unwrap();
    assert!((score.unwrap() - 0.9).abs() < 1e-6);

    repo::node_authority::clear_all(&mut conn).await.unwrap();
    assert_eq!(repo::node_authority::get(&mut conn, node.id).await.unwrap(), None);
}

#[tokio::test]
async fn clear_projection_tables_wipes_graph_data_but_not_events() {
    let path = temp_db_path("rebuild-clear");
    let pool = open_pool(&path).await.unwrap();
    let mut conn = pool.acquire().await.unwrap();

    let actor = Actor {
        id: oag_core::ActorId::derive(b"rebuild-test-actor"),
        actor_type: ActorType::Agent,
        name: Some("rebuild tester".into()),
        public_key: None,
        identity_uri: None,
        metadata: serde_json::json!({}),
        created_at: 1,
    };
    repo::actors::insert(&mut conn, &actor).await.unwrap();
    let subject = Node::new(NodeType::repository(), "url:https://example.com/rebuild-subject", 1);
    let object = Node::new(NodeType::concept(), "concept:rebuild-object", 1);
    repo::nodes::insert_if_missing(&mut conn, &subject).await.unwrap();
    repo::nodes::insert_if_missing(&mut conn, &object).await.unwrap();
    let edge = Edge::new(subject.id, Predicate::new("related_to"), object.id, 1);
    repo::edges::insert_if_missing(&mut conn, &edge).await.unwrap();

    assert!(repo::nodes::get_by_id(&mut conn, subject.id).await.unwrap().is_some());

    repo::rebuild::clear_projection_tables(&mut conn).await.unwrap();

    assert!(repo::nodes::get_by_id(&mut conn, subject.id).await.unwrap().is_none());
    assert!(repo::edges::get_by_id(&mut conn, edge.id).await.unwrap().is_none());
    let actors_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM actors").fetch_one(&mut *conn).await.unwrap();
    assert_eq!(actors_count.0, 0);

    // Events themselves are a completely separate concern this function
    // never touches -- there are none in this test, but the table itself
    // and event_origins must still exist and be queryable.
    let events_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM events").fetch_one(&mut *conn).await.unwrap();
    assert_eq!(events_count.0, 0);
}

#[tokio::test]
async fn list_all_in_insertion_order_returns_events_oldest_first() {
    let path = temp_db_path("rebuild-order");
    let pool = open_pool(&path).await.unwrap();
    let mut conn = pool.acquire().await.unwrap();

    for i in 0..3u8 {
        let event = repo::events::StoredEvent {
            event_id: oag_core::EventId::derive(&[i]),
            origin_peer_id: [i; 32],
            sequence: 1,
            previous_event_id: None,
            event_type: "TEST".into(),
            canonical_payload: format!("{{\"n\":{i}}}").into_bytes(),
            created_at: 100 + i as i64,
            signature: vec![0u8; 64],
        };
        repo::events::insert_event(&mut conn, &event, 100 + i as i64).await.unwrap();
    }

    let rows = repo::events::list_all_in_insertion_order(&mut conn).await.unwrap();
    assert_eq!(rows.len(), 3);
    let created_ats: Vec<i64> = rows.iter().map(|r| r.created_at).collect();
    assert_eq!(created_ats, vec![100, 101, 102], "must come back in original insertion order");
}

/// `ApiKeyInfo.permissions` must round-trip in `Permission::as_str()` form
/// (`"graph:read"`, colon-separated) -- not `Permission`'s own derived
/// kebab-case Serialize (`"graph-read"`), which `Permission::parse` and
/// `oag key create --permission` don't understand. A caller pasting
/// `oag key list`'s own output back into `--permission` must work.
#[tokio::test]
async fn list_keys_reports_colon_separated_permission_strings() {
    let pool = open_pool(&temp_db_path("list-keys-format")).await.unwrap();
    let mut conn = pool.acquire().await.unwrap();

    let actor = Actor {
        id: oag_core::ActorId::derive(b"list-keys-format-actor"),
        actor_type: ActorType::Agent,
        name: Some("test".into()),
        public_key: None,
        identity_uri: None,
        metadata: serde_json::json!({}),
        created_at: 1,
    };
    repo::actors::insert(&mut conn, &actor).await.unwrap();
    repo::actors::create_key(&mut conn, &[7u8; 32], actor.id, &[Permission::GraphRead, Permission::GraphCrawl], 1)
        .await
        .unwrap();

    let keys = repo::actors::list_keys(&mut conn).await.unwrap();
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].permissions, vec!["graph:read".to_string(), "graph:crawl".to_string()]);
    for p in &keys[0].permissions {
        assert!(Permission::parse(p).is_some(), "{p} must round-trip through Permission::parse");
    }
}
