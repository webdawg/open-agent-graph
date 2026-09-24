use oag_core::{
    Actor, ActorType, Assertion, AssertionStatus, Edge, Evidence, EvidenceType, ExtractionMethod,
    Node, NodeType, Predicate,
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
