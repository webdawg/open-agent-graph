use oag_core::{ActorType, Permission};
use oag_crypto::PeerIdentity;
use oag_storage::pool::open_pool;

use crate::assert::{AssertInput, EvidenceInput};
use crate::resolve::ResolveOutcome;
use crate::service::GraphService;

fn temp_db_path(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "oag-graph-test-{name}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("oag.sqlite")
}

async fn service_with_admin(name: &str) -> (GraphService, crate::service::AuthContext) {
    let pool = open_pool(&temp_db_path(name)).await.unwrap();
    let identity = PeerIdentity::generate();
    let service = GraphService::new(pool, identity);

    let actor_id = service
        .declare_actor(ActorType::Agent, Some("admin".into()), None)
        .await
        .unwrap();
    service
        .create_key(
            actor_id,
            vec![
                Permission::GraphAssert,
                Permission::GraphVerify,
                Permission::GraphRetractOwn,
            ],
        )
        .await
        .unwrap();

    (
        service,
        crate::service::AuthContext {
            actor_id,
            permissions: vec![Permission::GraphAssert, Permission::GraphVerify, Permission::GraphRetractOwn],
        },
    )
}

#[tokio::test]
async fn end_to_end_assert_search_subgraph_history() {
    let (service, auth) = service_with_admin("e2e").await;

    let assertion_id = service
        .assert(
            &auth,
            AssertInput {
                subject: "https://github.com/example/foo".into(),
                subject_type: Some("repository".into()),
                predicate: "implements".into(),
                object: "Model Context Protocol".into(),
                object_type: None,
                evidence: vec![EvidenceInput {
                    evidence_type: Some("documentation".into()),
                    uri: Some("https://github.com/example/foo/blob/main/README.md".into()),
                    title: Some("README".into()),
                    ..Default::default()
                }],
                actor_confidence: Some(0.9),
                observed_at: None,
            },
        )
        .await
        .unwrap();

    let assertion = service.get_assertion(assertion_id).await.unwrap().unwrap();
    assert_eq!(assertion.status, oag_core::AssertionStatus::Active);

    let evidence = service.list_evidence(assertion_id).await.unwrap();
    assert_eq!(evidence.len(), 1);

    // resolve() should find the subject by its canonicalized URL identifier.
    let resolved = service.resolve("https://github.com/example/foo").await.unwrap();
    let ResolveOutcome::Found { node, confidence } = resolved else {
        panic!("expected exact resolve match");
    };
    assert_eq!(confidence, 1.0);

    let subgraph = service.get_subgraph(node.id, 2, 100).await.unwrap();
    assert_eq!(subgraph.nodes.len(), 2);
    assert_eq!(subgraph.edges.len(), 1);
    assert_eq!(subgraph.assertions.len(), 1);

    let sources = service.find_sources(node.id).await.unwrap();
    assert_eq!(sources.len(), 1);

    // dispute + retract, then confirm history captures the full lifecycle.
    service
        .dispute_assertion(&auth, assertion_id, Some("outdated".into()))
        .await
        .unwrap();
    service
        .retract_assertion(&auth, assertion_id, None)
        .await
        .unwrap();

    let history = service
        .get_history("assertion", &assertion_id.to_hex())
        .await
        .unwrap();
    assert_eq!(history.len(), 4); // assert, evidence, dispute, retract

    let refetched = service.get_assertion(assertion_id).await.unwrap().unwrap();
    assert_eq!(refetched.status, oag_core::AssertionStatus::Retracted);
}

#[tokio::test]
async fn unauthenticated_actor_cannot_assert() {
    let (service, _) = service_with_admin("permission-check").await;
    let no_permissions = crate::service::AuthContext {
        actor_id: oag_core::ActorId::derive(b"someone-else"),
        permissions: vec![],
    };
    let result = service
        .assert(
            &no_permissions,
            AssertInput {
                subject: "https://example.com".into(),
                subject_type: None,
                predicate: "related_to".into(),
                object: "https://example.org".into(),
                object_type: None,
                evidence: vec![],
                actor_confidence: None,
                observed_at: None,
            },
        )
        .await;
    assert!(matches!(result, Err(crate::error::GraphError::PermissionDenied(_))));
}
