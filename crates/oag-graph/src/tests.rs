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
                extraction_method: None,
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

fn sample_input(subject: &str) -> AssertInput {
    AssertInput {
        subject: subject.to_string(),
        subject_type: None,
        predicate: "related_to".into(),
        object: "https://example.org".into(),
        object_type: None,
        evidence: vec![],
        actor_confidence: None,
        observed_at: None,
        extraction_method: None,
    }
}

async fn declare_actor_with_perms(
    service: &GraphService,
    name: &str,
    permissions: Vec<Permission>,
) -> crate::service::AuthContext {
    let actor_id = service
        .declare_actor(ActorType::Agent, Some(name.into()), None)
        .await
        .unwrap();
    crate::service::AuthContext { actor_id, permissions }
}

#[tokio::test]
async fn non_owner_without_admin_cannot_retract() {
    let (service, owner_auth) = service_with_admin("retract-nonowner").await;
    let assertion_id = service.assert(&owner_auth, sample_input("https://example.com/owned-by-a")).await.unwrap();

    let other = declare_actor_with_perms(&service, "other-actor", vec![Permission::GraphRetractOwn]).await;
    let result = service.retract_assertion(&other, assertion_id, None).await;
    assert!(
        matches!(result, Err(crate::error::GraphError::PermissionDenied(_))),
        "expected PermissionDenied, got {result:?}"
    );

    // The rejected attempt must not have mutated anything.
    let assertion = service.get_assertion(assertion_id).await.unwrap().unwrap();
    assert_eq!(assertion.status, oag_core::AssertionStatus::Active);
}

#[tokio::test]
async fn admin_can_retract_someone_elses_assertion() {
    let (service, owner_auth) = service_with_admin("retract-admin").await;
    let assertion_id = service.assert(&owner_auth, sample_input("https://example.com/owned-by-b")).await.unwrap();

    let admin = declare_actor_with_perms(&service, "root", vec![Permission::Admin]).await;
    service.retract_assertion(&admin, assertion_id, None).await.unwrap();

    let assertion = service.get_assertion(assertion_id).await.unwrap().unwrap();
    assert_eq!(assertion.status, oag_core::AssertionStatus::Retracted);
}

#[tokio::test]
async fn owner_can_retract_own_assertion() {
    let (service, auth) = service_with_admin("retract-owner").await;
    let assertion_id = service.assert(&auth, sample_input("https://example.com/owned-by-self")).await.unwrap();

    service.retract_assertion(&auth, assertion_id, None).await.unwrap();

    let assertion = service.get_assertion(assertion_id).await.unwrap().unwrap();
    assert_eq!(assertion.status, oag_core::AssertionStatus::Retracted);
}

#[tokio::test]
async fn retract_of_missing_assertion_is_not_found() {
    let (service, auth) = service_with_admin("retract-missing").await;
    let bogus_id = oag_core::EventId::derive(b"never-asserted");
    let result = service.retract_assertion(&auth, bogus_id, None).await;
    assert!(matches!(result, Err(crate::error::GraphError::NotFound(_))), "got {result:?}");
}

#[tokio::test]
async fn oversized_subject_is_rejected_before_any_write() {
    let (service, auth) = service_with_admin("validate-oversized-subject").await;
    let huge_subject = "a".repeat(3000);
    let result = service
        .assert(
            &auth,
            AssertInput {
                subject: huge_subject,
                subject_type: None,
                predicate: "related_to".into(),
                object: "https://example.com".into(),
                object_type: None,
                evidence: vec![],
                actor_confidence: None,
                observed_at: None,
                extraction_method: None,
            },
        )
        .await;
    assert!(matches!(result, Err(crate::error::GraphError::InvalidInput(_))), "got {result:?}");

    // Nothing should have been written — this node must not exist.
    let resolved = service.resolve("https://example.com").await.unwrap();
    assert!(matches!(resolved, crate::resolve::ResolveOutcome::NotFound | crate::resolve::ResolveOutcome::Candidates(_)));
}

#[tokio::test]
async fn too_many_evidence_items_is_rejected() {
    let (service, auth) = service_with_admin("validate-evidence-count").await;
    let evidence = (0..25)
        .map(|i| EvidenceInput {
            uri: Some(format!("https://example.com/e{i}")),
            ..Default::default()
        })
        .collect();
    let result = service
        .assert(
            &auth,
            AssertInput {
                subject: "https://example.com/subject".into(),
                subject_type: None,
                predicate: "related_to".into(),
                object: "https://example.com/object".into(),
                object_type: None,
                evidence,
                actor_confidence: None,
                observed_at: None,
                extraction_method: None,
            },
        )
        .await;
    assert!(matches!(result, Err(crate::error::GraphError::InvalidInput(_))), "got {result:?}");
}

#[tokio::test]
async fn oversized_evidence_excerpt_is_rejected() {
    let (service, auth) = service_with_admin("validate-excerpt").await;
    let assertion_id = service
        .assert(
            &auth,
            AssertInput {
                subject: "https://example.com/subject2".into(),
                subject_type: None,
                predicate: "related_to".into(),
                object: "https://example.com/object2".into(),
                object_type: None,
                evidence: vec![],
                actor_confidence: None,
                observed_at: None,
                extraction_method: None,
            },
        )
        .await
        .unwrap();

    let result = service
        .add_evidence(
            &auth,
            assertion_id,
            EvidenceInput {
                excerpt: Some("x".repeat(5000)),
                ..Default::default()
            },
        )
        .await;
    assert!(matches!(result, Err(crate::error::GraphError::InvalidInput(_))), "got {result:?}");
}

#[tokio::test]
async fn oversized_dispute_reason_is_rejected() {
    let (service, auth) = service_with_admin("validate-reason").await;
    let assertion_id = service
        .assert(
            &auth,
            AssertInput {
                subject: "https://example.com/subject3".into(),
                subject_type: None,
                predicate: "related_to".into(),
                object: "https://example.com/object3".into(),
                object_type: None,
                evidence: vec![],
                actor_confidence: None,
                observed_at: None,
                extraction_method: None,
            },
        )
        .await
        .unwrap();

    let result = service.dispute_assertion(&auth, assertion_id, Some("r".repeat(3000))).await;
    assert!(matches!(result, Err(crate::error::GraphError::InvalidInput(_))), "got {result:?}");
}

#[tokio::test]
async fn resolve_with_fts_special_characters_does_not_error() {
    let (service, _auth) = service_with_admin("resolve-fts-safety").await;
    // No exact node exists for any of these, so each falls through to the
    // FTS5 search fallback — colons, quotes, and NEAR/AND-shaped input are
    // all FTS5 query-syntax characters/keywords that previously crashed
    // resolve() with a SQL error instead of returning NotFound.
    for value in [
        "https://example.com/weird:path",
        "\"unterminated quote",
        "NEAR(foo, bar)",
        "col: value",
        "***",
    ] {
        let result = service.resolve(value).await;
        assert!(result.is_ok(), "resolve({value:?}) should not error, got {result:?}");
    }
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
                extraction_method: None,
            },
        )
        .await;
    assert!(matches!(result, Err(crate::error::GraphError::PermissionDenied(_))));
}

#[tokio::test]
async fn declare_alias_attaches_and_lists() {
    let (service, auth) = service_with_admin("declare-alias").await;

    service
        .assert(
            &auth,
            AssertInput {
                subject: "https://example.com/crawled-page".into(),
                subject_type: Some("document".into()),
                predicate: "instance_of".into(),
                object: "concept:document".into(),
                object_type: None,
                evidence: vec![],
                actor_confidence: Some(1.0),
                observed_at: None,
                extraction_method: Some(oag_core::ExtractionMethod::StructuredExtraction),
            },
        )
        .await
        .unwrap();

    let resolved = service.resolve("https://example.com/crawled-page").await.unwrap();
    let ResolveOutcome::Found { node, .. } = resolved else {
        panic!("expected exact resolve match");
    };

    service
        .declare_alias(&auth, node.id, "Example Crawled Page".into(), oag_core::AliasType::Name)
        .await
        .unwrap();

    let aliases = service.list_aliases(node.id).await.unwrap();
    assert_eq!(aliases.len(), 1);
    assert_eq!(aliases[0].alias, "Example Crawled Page");
    assert_eq!(aliases[0].alias_type, oag_core::AliasType::Name);
}

#[tokio::test]
async fn declare_alias_rejects_oversized_alias() {
    let (service, auth) = service_with_admin("declare-alias-oversized").await;
    let bogus_node_id = oag_core::NodeId::from_canonical_identifier("url:https://never-asserted.example.com");

    let result = service
        .declare_alias(&auth, bogus_node_id, "x".repeat(1000), oag_core::AliasType::Name)
        .await;
    assert!(matches!(result, Err(crate::error::GraphError::InvalidInput(_))), "got {result:?}");
}
