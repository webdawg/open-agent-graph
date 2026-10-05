use oag_core::{ActorType, Permission};
use oag_crypto::PeerIdentity;
use oag_storage::pool::open_pool;

use crate::assert::{AssertInput, EvidenceInput};
use crate::resolve::ResolveOutcome;
use crate::service::{AuthContext, GraphService};

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
        .declare_actor(ActorType::Agent, Some("admin".into()), None, None)
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
        .declare_actor(ActorType::Agent, Some(name.into()), None, None)
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

async fn edge_id_of(service: &GraphService, assertion_id: oag_core::AssertionId) -> oag_core::EdgeId {
    service.get_assertion(assertion_id).await.unwrap().unwrap().edge_id
}

#[tokio::test]
async fn corroboration_on_unknown_edge_is_not_found() {
    let (service, _auth) = service_with_admin("corroboration-not-found").await;
    let bogus_edge_id = oag_core::EdgeId::derive(b"never-created");
    let result = service.get_edge_corroboration(bogus_edge_id).await;
    assert!(matches!(result, Err(crate::error::GraphError::NotFound(_))), "got {result:?}");
}

#[tokio::test]
async fn single_unevidenced_assertion_gives_baseline_corroboration() {
    let (service, auth) = service_with_admin("corroboration-baseline").await;
    let assertion_id = service.assert(&auth, sample_input("https://example.com/solo-claim")).await.unwrap();
    let edge_id = edge_id_of(&service, assertion_id).await;

    let c = service.get_edge_corroboration(edge_id).await.unwrap();
    assert_eq!(c.active_assertions, 1);
    assert_eq!(c.disputed_assertions, 0);
    assert_eq!(c.distinct_actors, 1);
    assert_eq!(c.source_groups.len(), 1, "a bare claim still counts as one (weak) source group");
    assert_eq!(c.source_independence, 0.0, "one source group is not independent corroboration");
    assert_eq!(c.evidence_strength, 0.0, "no evidence backs the claim");
    assert_eq!(c.agreement, 1.0, "nothing disputes or contradicts it");
}

#[tokio::test]
async fn two_actors_without_evidence_give_two_source_groups() {
    let (service, auth_a) = service_with_admin("corroboration-two-actors").await;
    let auth_b = declare_actor_with_perms(&service, "actor-b", vec![Permission::GraphAssert]).await;

    let a1 = service.assert(&auth_a, sample_input("https://example.com/two-actors")).await.unwrap();
    service.assert(&auth_b, sample_input("https://example.com/two-actors")).await.unwrap();
    let edge_id = edge_id_of(&service, a1).await;

    let c = service.get_edge_corroboration(edge_id).await.unwrap();
    assert_eq!(c.active_assertions, 2);
    assert_eq!(c.distinct_actors, 2);
    assert_eq!(c.source_groups.len(), 2);
    assert_eq!(c.source_independence, 0.5);
}

fn evidenced_input(subject: &str, uri: &str, evidence_type: &str) -> AssertInput {
    AssertInput {
        evidence: vec![EvidenceInput {
            uri: Some(uri.to_string()),
            evidence_type: Some(evidence_type.to_string()),
            ..Default::default()
        }],
        ..sample_input(subject)
    }
}

#[tokio::test]
async fn common_control_evidence_collapses_source_independence() {
    // The literal spec section 62 scenario: two actors, two evidence items,
    // both controlled by the same GitHub repo owner — not two independent
    // confirmations.
    let (service, auth_a) = service_with_admin("corroboration-common-control").await;
    let auth_b = declare_actor_with_perms(&service, "actor-b", vec![Permission::GraphAssert]).await;

    let a1 = service
        .assert(
            &auth_a,
            evidenced_input(
                "https://example.com/common-control",
                "https://github.com/same-owner/repo-a",
                "repository",
            ),
        )
        .await
        .unwrap();
    service
        .assert(
            &auth_b,
            evidenced_input(
                "https://example.com/common-control",
                "https://github.com/same-owner/repo-b",
                "repository",
            ),
        )
        .await
        .unwrap();
    let edge_id = edge_id_of(&service, a1).await;

    let c = service.get_edge_corroboration(edge_id).await.unwrap();
    assert_eq!(c.distinct_actors, 2);
    assert_eq!(c.source_groups, vec!["github.com/same-owner".to_string()]);
    assert_eq!(c.source_independence, 0.0, "one controller behind both sources");
}

#[tokio::test]
async fn independent_evidence_domains_raise_source_independence() {
    let (service, auth_a) = service_with_admin("corroboration-independent").await;
    let auth_b = declare_actor_with_perms(&service, "actor-b", vec![Permission::GraphAssert]).await;

    let a1 = service
        .assert(
            &auth_a,
            evidenced_input(
                "https://example.com/independent",
                "https://github.com/owner-a/repo",
                "repository",
            ),
        )
        .await
        .unwrap();
    service
        .assert(
            &auth_b,
            evidenced_input("https://example.com/independent", "https://github.com/owner-b/repo", "repository"),
        )
        .await
        .unwrap();
    let edge_id = edge_id_of(&service, a1).await;

    let c = service.get_edge_corroboration(edge_id).await.unwrap();
    assert_eq!(c.source_groups.len(), 2);
    assert_eq!(c.source_independence, 0.5);
}

#[tokio::test]
async fn specification_evidence_scores_higher_than_web_page() {
    let (service, auth_a) = service_with_admin("corroboration-evidence-strength").await;
    let auth_b = declare_actor_with_perms(&service, "actor-b", vec![Permission::GraphAssert]).await;

    let a1 = service
        .assert(
            &auth_a,
            evidenced_input(
                "https://example.com/evidence-strength",
                "https://spec.example.org/doc",
                "specification",
            ),
        )
        .await
        .unwrap();
    service
        .assert(
            &auth_b,
            evidenced_input("https://example.com/evidence-strength", "https://blog.example.net/post", "web_page"),
        )
        .await
        .unwrap();
    let edge_id = edge_id_of(&service, a1).await;

    let c = service.get_edge_corroboration(edge_id).await.unwrap();
    // mean of the Specification (1.0) and WebPage (0.3) weights.
    assert!((c.evidence_strength - 0.65).abs() < 1e-6, "got {}", c.evidence_strength);
}

#[tokio::test]
async fn dispute_lowers_agreement_via_active_disputed_ratio() {
    let (service, auth_a) = service_with_admin("corroboration-dispute").await;
    let auth_b = declare_actor_with_perms(&service, "actor-b", vec![Permission::GraphAssert]).await;

    let a1 = service.assert(&auth_a, sample_input("https://example.com/disputed-edge")).await.unwrap();
    service.assert(&auth_b, sample_input("https://example.com/disputed-edge")).await.unwrap();
    let edge_id = edge_id_of(&service, a1).await;

    let before = service.get_edge_corroboration(edge_id).await.unwrap();
    assert_eq!(before.agreement, 1.0);

    service.dispute_assertion(&auth_b, a1, Some("outdated".into())).await.unwrap();

    let after = service.get_edge_corroboration(edge_id).await.unwrap();
    assert_eq!(after.active_assertions, 1);
    assert_eq!(after.disputed_assertions, 1);
    assert_eq!(after.agreement, 0.5);
}

#[tokio::test]
async fn confirmed_observation_keeps_agreement_high() {
    let (service, auth) = service_with_admin("corroboration-confirmed").await;
    let verifier = declare_actor_with_perms(&service, "verifier", vec![Permission::GraphVerify]).await;

    let assertion_id = service.assert(&auth, sample_input("https://example.com/confirmed-edge")).await.unwrap();
    let edge_id = edge_id_of(&service, assertion_id).await;

    service
        .verify_assertion(&verifier, assertion_id, oag_core::VerifyResult::Confirmed, 0)
        .await
        .unwrap();

    let c = service.get_edge_corroboration(edge_id).await.unwrap();
    assert_eq!(c.agreement, 1.0);
}

#[tokio::test]
async fn contradicted_observation_lowers_agreement() {
    let (service, auth) = service_with_admin("corroboration-contradicted").await;
    let verifier = declare_actor_with_perms(&service, "verifier", vec![Permission::GraphVerify]).await;

    let assertion_id = service.assert(&auth, sample_input("https://example.com/contradicted-edge")).await.unwrap();
    let edge_id = edge_id_of(&service, assertion_id).await;

    service
        .verify_assertion(&verifier, assertion_id, oag_core::VerifyResult::Contradicted, 0)
        .await
        .unwrap();

    let c = service.get_edge_corroboration(edge_id).await.unwrap();
    assert!((c.agreement - 0.5).abs() < 1e-6, "got {}", c.agreement);
}

#[tokio::test]
async fn recent_assertion_is_nearly_fully_fresh() {
    let (service, auth) = service_with_admin("corroboration-fresh").await;
    let assertion_id = service.assert(&auth, sample_input("https://example.com/fresh-edge")).await.unwrap();
    let edge_id = edge_id_of(&service, assertion_id).await;

    let c = service.get_edge_corroboration(edge_id).await.unwrap();
    assert!(c.freshness > 0.999, "got {}", c.freshness);
}

#[tokio::test]
async fn old_observed_at_decays_freshness() {
    let (service, auth) = service_with_admin("corroboration-stale").await;
    let year_ago = service.now() - 365 * 86_400;
    let assertion_id = service
        .assert(
            &auth,
            AssertInput {
                observed_at: Some(year_ago),
                ..sample_input("https://example.com/stale-edge")
            },
        )
        .await
        .unwrap();
    let edge_id = edge_id_of(&service, assertion_id).await;

    let c = service.get_edge_corroboration(edge_id).await.unwrap();
    // One year is roughly two half-lives (180 days each) out.
    assert!(c.freshness < 0.3, "got {}", c.freshness);
    assert!(c.freshness > 0.0);
}

#[tokio::test]
async fn no_active_assertions_gives_zero_freshness() {
    let (service, auth) = service_with_admin("corroboration-no-active-freshness").await;
    let assertion_id = service.assert(&auth, sample_input("https://example.com/retracted-edge")).await.unwrap();
    let edge_id = edge_id_of(&service, assertion_id).await;
    service.retract_assertion(&auth, assertion_id, None).await.unwrap();

    let c = service.get_edge_corroboration(edge_id).await.unwrap();
    assert_eq!(c.active_assertions, 0);
    assert_eq!(c.freshness, 0.0);
}

fn sign_actor_key_proof(
    signing_key: &oag_crypto::SigningKey,
    actor_type: oag_core::ActorType,
    name: Option<&str>,
    identity_uri: Option<&str>,
) -> crate::service::PublicKeyProof {
    let message = crate::service::ActorKeyProofMessage { actor_type: actor_type.as_str(), name, identity_uri };
    let canonical = oag_core::canonical_json_bytes(&message).unwrap();
    let signature =
        oag_crypto::sign_with_domain(signing_key, crate::service::ACTOR_KEY_PROOF_DOMAIN, &canonical);
    crate::service::PublicKeyProof {
        public_key: signing_key.verifying_key().to_bytes(),
        signature: signature.to_bytes(),
    }
}

#[tokio::test]
async fn declare_actor_with_valid_public_key_proof_succeeds() {
    let (service, _) = service_with_admin("actor-proof-valid").await;
    let signing_key = oag_crypto::SigningKey::from_bytes(&oag_crypto::random_bytes_32());
    let proof = sign_actor_key_proof(
        &signing_key,
        oag_core::ActorType::Agent,
        Some("verified-actor"),
        Some("https://example.com/verified-actor"),
    );
    let expected_public_key = signing_key.verifying_key().to_bytes();

    let actor_id = service
        .declare_actor(
            oag_core::ActorType::Agent,
            Some("verified-actor".into()),
            Some("https://example.com/verified-actor".into()),
            Some(proof),
        )
        .await
        .unwrap();

    let actor = service.get_actor(actor_id).await.unwrap().unwrap();
    assert_eq!(actor.public_key, Some(expected_public_key));
    assert_eq!(actor.identity_uri.as_deref(), Some("https://example.com/verified-actor"));
}

#[tokio::test]
async fn declare_actor_rejects_tampered_signature() {
    let (service, _) = service_with_admin("actor-proof-tampered").await;
    let signing_key = oag_crypto::SigningKey::from_bytes(&oag_crypto::random_bytes_32());
    let mut proof = sign_actor_key_proof(&signing_key, oag_core::ActorType::Agent, Some("actor-a"), None);
    proof.signature[0] ^= 0xFF;

    let result = service
        .declare_actor(oag_core::ActorType::Agent, Some("actor-a".into()), None, Some(proof))
        .await;
    assert!(matches!(result, Err(crate::error::GraphError::InvalidInput(_))), "got {result:?}");
}

#[tokio::test]
async fn declare_actor_rejects_proof_signed_for_a_different_label() {
    let (service, _) = service_with_admin("actor-proof-mismatch").await;
    let signing_key = oag_crypto::SigningKey::from_bytes(&oag_crypto::random_bytes_32());
    // Proof is signed for "actor-a"...
    let proof = sign_actor_key_proof(&signing_key, oag_core::ActorType::Agent, Some("actor-a"), None);

    // ...but the declaration claims a different name -- the signature must
    // not transfer to a label it was never signed for.
    let result = service
        .declare_actor(oag_core::ActorType::Agent, Some("actor-b".into()), None, Some(proof))
        .await;
    assert!(matches!(result, Err(crate::error::GraphError::InvalidInput(_))), "got {result:?}");
}

#[tokio::test]
async fn declare_actor_rejects_a_public_key_that_did_not_produce_the_signature() {
    let (service, _) = service_with_admin("actor-proof-wrong-key").await;
    let signing_key = oag_crypto::SigningKey::from_bytes(&oag_crypto::random_bytes_32());
    let mut proof = sign_actor_key_proof(&signing_key, oag_core::ActorType::Agent, Some("actor-c"), None);
    proof.public_key = oag_crypto::random_bytes_32(); // claim someone else's key entirely

    let result = service
        .declare_actor(oag_core::ActorType::Agent, Some("actor-c".into()), None, Some(proof))
        .await;
    assert!(matches!(result, Err(crate::error::GraphError::InvalidInput(_))), "got {result:?}");
}

#[tokio::test]
async fn declare_actor_rejects_an_oversized_name() {
    let (service, _) = service_with_admin("actor-oversized-name").await;
    let huge_name = "x".repeat(10_000);

    let result = service.declare_actor(oag_core::ActorType::Agent, Some(huge_name), None, None).await;
    assert!(matches!(result, Err(crate::error::GraphError::InvalidInput(_))), "got {result:?}");
}

#[tokio::test]
async fn declare_actor_rejects_an_oversized_identity_uri() {
    let (service, _) = service_with_admin("actor-oversized-identity-uri").await;
    let huge_uri = format!("https://example.com/{}", "x".repeat(10_000));

    let result = service
        .declare_actor(oag_core::ActorType::Agent, None, Some(huge_uri), None)
        .await;
    assert!(matches!(result, Err(crate::error::GraphError::InvalidInput(_))), "got {result:?}");
}

/// The real regression test for the race `oag_events`' own
/// `commit_local_event_itself_is_not_safe_for_concurrent_same_peer_calls`
/// documents at the function level: `GraphService::commit_event`'s
/// serialization lock is what actually has to make concurrent same-peer
/// commits safe, since every `GraphService` method goes through it.
/// Confirmed live before this lock existed: 10 of 20 concurrent calls here
/// failed outright with a raw "database is locked" SQLite error.
#[tokio::test]
async fn concurrent_graph_service_commits_from_the_same_peer_all_succeed() {
    let (service, _) = service_with_admin("concurrent-graph-service-commits").await;
    let service = std::sync::Arc::new(service);

    let mut tasks = Vec::new();
    for i in 0..20 {
        let service = service.clone();
        tasks.push(tokio::spawn(async move {
            service.declare_actor(oag_core::ActorType::Agent, Some(format!("concurrent-actor-{i}")), None, None).await
        }));
    }

    let mut errors = Vec::new();
    for task in tasks {
        if let Err(e) = task.await.unwrap() {
            errors.push(format!("{e}"));
        }
    }
    assert!(errors.is_empty(), "every concurrent GraphService commit should succeed, got errors: {errors:?}");
}

#[tokio::test]
async fn same_public_key_dedups_to_the_same_actor_id() {
    let (service, _) = service_with_admin("actor-proof-dedup").await;
    let signing_key = oag_crypto::SigningKey::from_bytes(&oag_crypto::random_bytes_32());

    let first = service
        .declare_actor(
            oag_core::ActorType::Agent,
            Some("dedup-actor".into()),
            None,
            Some(sign_actor_key_proof(&signing_key, oag_core::ActorType::Agent, Some("dedup-actor"), None)),
        )
        .await
        .unwrap();
    let second = service
        .declare_actor(
            oag_core::ActorType::Agent,
            Some("dedup-actor".into()),
            None,
            Some(sign_actor_key_proof(&signing_key, oag_core::ActorType::Agent, Some("dedup-actor"), None)),
        )
        .await
        .unwrap();
    assert_eq!(first, second, "re-declaring with the same key should dedup to the same ActorId");
}

#[tokio::test]
async fn identity_assurance_is_the_mean_across_distinct_actors_not_the_max() {
    let (service, auth_a) = service_with_admin("corroboration-identity-mean").await;

    let signing_key = oag_crypto::SigningKey::from_bytes(&oag_crypto::random_bytes_32());
    let proof = sign_actor_key_proof(&signing_key, oag_core::ActorType::Agent, Some("verified-b"), None);
    let actor_b = service
        .declare_actor(oag_core::ActorType::Agent, Some("verified-b".into()), None, Some(proof))
        .await
        .unwrap();
    let auth_b = crate::service::AuthContext { actor_id: actor_b, permissions: vec![Permission::GraphAssert] };

    let a1 = service.assert(&auth_a, sample_input("https://example.com/identity-mean")).await.unwrap();
    service.assert(&auth_b, sample_input("https://example.com/identity-mean")).await.unwrap();
    let edge_id = edge_id_of(&service, a1).await;

    let c = service.get_edge_corroboration(edge_id).await.unwrap();
    // bare admin actor (0.0) + verified-key-only actor (0.7) -> mean 0.35, not 0.7.
    assert!((c.identity_assurance - 0.35).abs() < 1e-6, "got {}", c.identity_assurance);
}

#[tokio::test]
async fn recompute_authority_ranks_a_real_hub_above_a_real_leaf() {
    let (service, auth) = service_with_admin("authority-hub").await;

    // Three different subjects all assert a relationship to the same
    // "hub" object -- the hub should end up with the highest authority.
    for subject in ["https://example.com/spoke-a", "https://example.com/spoke-b", "https://example.com/spoke-c"] {
        service
            .assert(&auth, related_to_input(subject, "https://example.com/hub"))
            .await
            .unwrap();
    }

    let summary = service.recompute_authority().await.unwrap();
    assert!(summary.nodes_scored >= 4, "expected at least the hub + 3 spokes, got {summary:?}");
    assert!(summary.edges_considered >= 3);

    let hub = service.resolve("https://example.com/hub").await.unwrap();
    let ResolveOutcome::Found { node: hub_node, .. } = hub else { panic!("hub should resolve") };
    let spoke = service.resolve("https://example.com/spoke-a").await.unwrap();
    let ResolveOutcome::Found { node: spoke_node, .. } = spoke else { panic!("spoke should resolve") };

    let hub_authority = service.get_node_authority(hub_node.id).await.unwrap().unwrap();
    let spoke_authority = service.get_node_authority(spoke_node.id).await.unwrap().unwrap();
    assert!(hub_authority > spoke_authority, "hub ({hub_authority}) should outrank spoke ({spoke_authority})");
}

#[tokio::test]
async fn node_with_no_edges_has_no_authority_score() {
    let (service, auth) = service_with_admin("authority-none").await;
    service.assert(&auth, sample_input("https://example.com/authority-scope-check")).await.unwrap();
    service.recompute_authority().await.unwrap();

    let bogus_node_id = oag_core::NodeId::from_canonical_identifier("url:https://never-in-any-edge.example.com");
    assert_eq!(service.get_node_authority(bogus_node_id).await.unwrap(), None);
}

fn related_to_input(subject: &str, object: &str) -> AssertInput {
    AssertInput {
        subject: subject.to_string(),
        subject_type: None,
        predicate: "related_to".into(),
        object: object.to_string(),
        object_type: None,
        evidence: vec![],
        actor_confidence: None,
        observed_at: None,
        extraction_method: None,
    }
}

#[tokio::test]
async fn subgraph_reports_correct_hop_distance_along_a_chain() {
    let (service, auth) = service_with_admin("subgraph-distance-chain").await;

    service.assert(&auth, related_to_input("https://example.com/chain-root", "https://example.com/chain-a")).await.unwrap();
    service.assert(&auth, related_to_input("https://example.com/chain-a", "https://example.com/chain-b")).await.unwrap();
    service.assert(&auth, related_to_input("https://example.com/chain-b", "https://example.com/chain-c")).await.unwrap();

    let ResolveOutcome::Found { node: root, .. } = service.resolve("https://example.com/chain-root").await.unwrap() else {
        panic!("root should resolve");
    };

    let subgraph = service.get_subgraph(root.id, 3, 100).await.unwrap();
    let distance_of = |identifier: &str| -> u32 {
        subgraph
            .nodes
            .iter()
            .find(|n| n.node.canonical_identifier == identifier)
            .unwrap_or_else(|| panic!("{identifier} missing from subgraph"))
            .distance
    };

    assert_eq!(distance_of("url:https://example.com/chain-root"), 0);
    assert_eq!(distance_of("url:https://example.com/chain-a"), 1);
    assert_eq!(distance_of("url:https://example.com/chain-b"), 2);
    assert_eq!(distance_of("url:https://example.com/chain-c"), 3);
}

#[tokio::test]
async fn subgraph_keeps_the_shortest_distance_when_a_node_is_reachable_two_ways() {
    let (service, auth) = service_with_admin("subgraph-distance-diamond").await;

    // root -> c directly (distance 1), *and* root -> a -> c / root -> b -> c
    // (distance 2 via either detour). The direct edge must win.
    service.assert(&auth, related_to_input("https://example.com/diamond-root", "https://example.com/diamond-c")).await.unwrap();
    service.assert(&auth, related_to_input("https://example.com/diamond-root", "https://example.com/diamond-a")).await.unwrap();
    service.assert(&auth, related_to_input("https://example.com/diamond-root", "https://example.com/diamond-b")).await.unwrap();
    service.assert(&auth, related_to_input("https://example.com/diamond-a", "https://example.com/diamond-c")).await.unwrap();
    service.assert(&auth, related_to_input("https://example.com/diamond-b", "https://example.com/diamond-c")).await.unwrap();

    let ResolveOutcome::Found { node: root, .. } = service.resolve("https://example.com/diamond-root").await.unwrap() else {
        panic!("root should resolve");
    };

    let subgraph = service.get_subgraph(root.id, 3, 100).await.unwrap();
    let c_distance = subgraph
        .nodes
        .iter()
        .find(|n| n.node.canonical_identifier == "url:https://example.com/diamond-c")
        .unwrap()
        .distance;
    assert_eq!(c_distance, 1, "the direct edge's distance must win over the longer detour");
}

#[tokio::test]
async fn rebuild_projection_reproduces_identical_graph_state() {
    let (service, auth) = service_with_admin("rebuild-projection").await;

    // Exercise several event types at once, including a real cross-event
    // ordering dependency (ADD_EVIDENCE against an already-created
    // assertion, and a NODE_ALIAS against an already-created node).
    let assertion_id = service
        .assert(
            &auth,
            AssertInput {
                subject: "https://example.com/rebuild-subject".into(),
                subject_type: None,
                predicate: "implements".into(),
                object: "https://example.com/rebuild-object".into(),
                object_type: None,
                evidence: vec![EvidenceInput {
                    evidence_type: Some("documentation".into()),
                    uri: Some("https://example.com/rebuild-subject/docs".into()),
                    title: Some("Docs".into()),
                    ..Default::default()
                }],
                actor_confidence: Some(0.8),
                observed_at: None,
                extraction_method: None,
            },
        )
        .await
        .unwrap();

    let ResolveOutcome::Found { node: subject_node, .. } = service.resolve("https://example.com/rebuild-subject").await.unwrap() else {
        panic!("subject should resolve");
    };
    service.declare_alias(&auth, subject_node.id, "Rebuild Subject".into(), oag_core::AliasType::Name).await.unwrap();

    let other = declare_actor_with_perms(&service, "rebuild-disputer", vec![Permission::GraphAssert]).await;
    service.dispute_assertion(&other, assertion_id, Some("checking this".into())).await.unwrap();

    // Snapshot every read path before rebuilding.
    let before_assertion = service.get_assertion(assertion_id).await.unwrap().unwrap();
    let before_evidence = service.list_evidence(assertion_id).await.unwrap();
    let before_aliases = service.list_aliases(subject_node.id).await.unwrap();
    let before_resolve = service.resolve("https://example.com/rebuild-subject").await.unwrap();
    let before_actor = service.get_actor(before_assertion.actor_id).await.unwrap().unwrap();

    let summary = service.rebuild_projection().await.unwrap();
    assert!(summary.events_replayed >= 5, "expected at least declare_actor+assert+evidence+alias+actor+dispute, got {summary:?}");

    let after_assertion = service.get_assertion(assertion_id).await.unwrap().unwrap();
    assert_eq!(after_assertion.status, before_assertion.status);
    assert_eq!(after_assertion.status, oag_core::AssertionStatus::Disputed);

    let after_evidence = service.list_evidence(assertion_id).await.unwrap();
    assert_eq!(after_evidence.len(), before_evidence.len());
    assert_eq!(after_evidence[0].uri, before_evidence[0].uri);

    let after_aliases = service.list_aliases(subject_node.id).await.unwrap();
    assert_eq!(after_aliases.len(), before_aliases.len());
    assert_eq!(after_aliases[0].alias, before_aliases[0].alias);

    let after_resolve = service.resolve("https://example.com/rebuild-subject").await.unwrap();
    let (ResolveOutcome::Found { node: before_node, .. }, ResolveOutcome::Found { node: after_node, .. }) =
        (before_resolve, after_resolve)
    else {
        panic!("both resolves should find the node");
    };
    assert_eq!(before_node.id, after_node.id);

    let after_actor = service.get_actor(before_assertion.actor_id).await.unwrap().unwrap();
    assert_eq!(after_actor.name, before_actor.name);
}

/// Deterministic, hash-derived bag-of-words embedding -- no network calls,
/// no external model. Text sharing words lands in the same buckets and so
/// scores similarly under cosine similarity, which is enough to exercise
/// real ranking behavior without a real embedding model in the test suite.
struct FakeEmbeddingProvider;

// Large relative to the handful of distinct words in these tests, so two
// unrelated words landing in the same bucket (and spuriously inflating
// similarity) is vanishingly unlikely.
const FAKE_EMBEDDING_DIM: usize = 4096;

#[async_trait::async_trait]
impl oag_embeddings::EmbeddingProvider for FakeEmbeddingProvider {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, oag_embeddings::EmbeddingError> {
        let mut vector = vec![0.0f32; FAKE_EMBEDDING_DIM];
        for word in text.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()) {
            let hash = blake3::hash(word.to_lowercase().as_bytes());
            let bytes = hash.as_bytes();
            let bucket = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize % FAKE_EMBEDDING_DIM;
            vector[bucket] += 1.0;
        }
        Ok(vector)
    }

    fn provider_name(&self) -> &'static str {
        "fake-test-provider"
    }

    fn model_name(&self) -> &str {
        "fake-v1"
    }
}

#[tokio::test]
async fn search_with_a_negative_limit_returns_nothing_not_everything() {
    // SQLite treats a negative SQL LIMIT as "no limit at all" -- confirmed
    // live over REST before this was fixed: `limit=-1` returned every
    // matching row instead of being clamped. Regression test: seed several
    // matching nodes, confirm a negative limit returns none of them rather
    // than all of them.
    let (service, auth) = service_with_admin("search-negative-limit").await;
    for i in 0..5 {
        service.assert(&auth, sample_input(&format!("Negative Limit Target {i}"))).await.unwrap();
    }

    let normal = service.search("Negative Limit Target", 20).await.unwrap();
    assert_eq!(normal.len(), 5, "sanity check: all 5 should be findable with a normal limit");

    let negative = service.search("Negative Limit Target", -1).await.unwrap();
    assert!(negative.is_empty(), "a negative limit must not bypass the cap and return everything, got {negative:?}");
}

#[tokio::test]
async fn semantic_search_ranks_topically_similar_nodes_above_unrelated_ones() {
    let (service, auth) = service_with_admin("semantic-search-ranking").await;

    for subject in ["Rust Programming Language", "Rust Async Runtime", "Banana Smoothie Recipe"] {
        service.assert(&auth, sample_input(subject)).await.unwrap();
    }

    let summary = service.recompute_embeddings(&FakeEmbeddingProvider).await.unwrap();
    assert!(summary.nodes_embedded >= 3, "expected at least the 3 subjects embedded, got {summary:?}");
    assert_eq!(summary.nodes_skipped, 0);

    let results = service.semantic_search(&FakeEmbeddingProvider, "Rust Programming", 2).await.unwrap();
    assert_eq!(results.len(), 2);
    let identifiers: Vec<&str> = results.iter().map(|(node, _)| node.canonical_identifier.as_str()).collect();
    assert!(identifiers.contains(&"concept:rust-programming-language"));
    assert!(identifiers.contains(&"concept:rust-async-runtime"));
    assert!(!identifiers.contains(&"concept:banana-smoothie-recipe"));
}

#[tokio::test]
async fn semantic_search_with_default_disabled_provider_is_a_typed_error_not_a_panic() {
    let (service, auth) = service_with_admin("semantic-search-disabled").await;
    service.assert(&auth, sample_input("Some Topic")).await.unwrap();

    let disabled = oag_embeddings::DisabledProvider;
    let result = service.semantic_search(&disabled, "some topic", 5).await;
    assert!(
        matches!(result, Err(crate::error::GraphError::Embedding(oag_embeddings::EmbeddingError::Disabled))),
        "got {result:?}"
    );

    let recompute_result = service.recompute_embeddings(&disabled).await;
    assert!(
        matches!(
            recompute_result,
            Err(crate::error::GraphError::Embedding(oag_embeddings::EmbeddingError::Disabled))
        ),
        "got {recompute_result:?}"
    );
}

/// Spec section 104's "crash recovery" storage test. A real process crash
/// can't be simulated in-process, but the property that actually matters is
/// testable without one: nothing about a peer's committed data should
/// depend on that process staying alive. Close every handle to the pool
/// (dropping `GraphService`/`SqlitePool` is exactly what a killed process
/// does -- no graceful shutdown hook runs either way), then reopen a
/// completely fresh `SqlitePool`/`GraphService` against the same on-disk
/// file and confirm every committed event's projection is still there,
/// byte-for-byte queryable the same as before.
#[tokio::test]
async fn committed_data_survives_a_full_pool_close_and_reopen() {
    let db_path = temp_db_path("crash-recovery");

    let assertion_id = {
        let pool = open_pool(&db_path).await.unwrap();
        let service = GraphService::new(pool, PeerIdentity::generate());
        let actor_id =
            service.declare_actor(ActorType::Agent, Some("admin".into()), None, None).await.unwrap();
        let auth = AuthContext { actor_id, permissions: vec![Permission::GraphAssert] };

        let assertion_id = service
            .assert(
                &auth,
                AssertInput {
                    subject: "https://example.com/crash-recovery-test".into(),
                    subject_type: None,
                    predicate: "instance_of".into(),
                    object: "concept:crash-recovery-test".into(),
                    object_type: None,
                    evidence: vec![EvidenceInput {
                        evidence_type: Some("documentation".into()),
                        title: Some("Survives a restart".into()),
                        ..Default::default()
                    }],
                    actor_confidence: Some(0.9),
                    observed_at: None,
                    extraction_method: None,
                },
            )
            .await
            .unwrap();

        // `service` and its `SqlitePool` go out of scope here with no
        // explicit close/flush call -- the same abrupt end a `kill -9`
        // leaves behind. (A real peer's signing identity lives in a
        // separate `identity.key` file outside the database and survives
        // independently of it -- already covered by `oag-sync`'s
        // `peer_restart_resumes_and_converges`; this test is specifically
        // about the *data*.)
        assertion_id
    };

    // Reopen a completely fresh pool and service against the same file.
    let pool = open_pool(&db_path).await.unwrap();
    let service = GraphService::new(pool, PeerIdentity::generate());

    let assertion = service.get_assertion(assertion_id).await.unwrap();
    assert!(assertion.is_some(), "assertion committed before the simulated crash must survive it");
    assert_eq!(assertion.unwrap().status, oag_core::AssertionStatus::Active);

    let evidence = service.list_evidence(assertion_id).await.unwrap();
    assert_eq!(evidence.len(), 1);
    assert_eq!(evidence[0].title.as_deref(), Some("Survives a restart"));

    let resolved = service.resolve("https://example.com/crash-recovery-test").await.unwrap();
    assert!(matches!(resolved, ResolveOutcome::Found { confidence, .. } if confidence == 1.0));
}
