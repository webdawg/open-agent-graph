use oag_core::{
    Actor, ActorId, ActorType, AliasType, Assertion, AssertionId, AssertionStatus, Edge, Evidence,
    EvidenceType, ExtractionMethod, Node, NodeAlias, NodeId, NodeType, Permission, Predicate,
};
use oag_storage::repo::{actors, assertions, edges, events, nodes};
use oag_storage::SqliteConnection;

use crate::error::EventsError;
use crate::payload::EventPayload;

/// Most event types don't need to hand anything back to the caller beyond
/// the `EventId` `commit_local_event` already returns. `ActorDeclare` is the
/// exception: the actor's id is derived inside projection (from its public
/// key, or from the event id when it has none), so the caller — which needs
/// the id to immediately attach a key — can't compute it up front.
#[derive(Debug)]
pub enum ProjectionOutcome {
    Unit,
    ActorDeclared { actor_id: ActorId },
}

/// Apply a validated event's payload to the projection tables, inside the
/// same transaction the raw event row was inserted in. Idempotent: the
/// caller is expected to have already checked `event_id` wasn't already
/// present in `events` before calling this (spec section 53 — duplicate
/// delivery must be a no-op, which `commit_local_event`'s "insert event,
/// bail if not newly inserted" guard provides).
pub async fn project(
    conn: &mut SqliteConnection,
    event_id: oag_core::EventId,
    created_at: i64,
    payload: &EventPayload,
) -> Result<ProjectionOutcome, EventsError> {
    match payload {
        EventPayload::AssertRelation(p) => {
            project_assert_relation(conn, event_id, created_at, p).await?;
            Ok(ProjectionOutcome::Unit)
        }
        EventPayload::AddEvidence(p) => {
            project_add_evidence(conn, event_id, p).await?;
            Ok(ProjectionOutcome::Unit)
        }
        EventPayload::VerifyAssertion(p) => {
            project_verify_assertion(conn, event_id, created_at, p).await?;
            Ok(ProjectionOutcome::Unit)
        }
        EventPayload::DisputeAssertion(p) => {
            project_dispute_assertion(conn, event_id, created_at, p).await?;
            Ok(ProjectionOutcome::Unit)
        }
        EventPayload::RetractAssertion(p) => {
            project_retract_assertion(conn, event_id, created_at, p).await?;
            Ok(ProjectionOutcome::Unit)
        }
        EventPayload::SupersedeAssertion(p) => {
            project_supersede_assertion(conn, event_id, created_at, p).await?;
            Ok(ProjectionOutcome::Unit)
        }
        EventPayload::ActorDeclare(p) => {
            let actor_id = project_actor_declare(conn, event_id, created_at, p).await?;
            Ok(ProjectionOutcome::ActorDeclared { actor_id })
        }
        EventPayload::ActorKeyAdd(p) => {
            project_actor_key_add(conn, created_at, p).await?;
            Ok(ProjectionOutcome::Unit)
        }
        EventPayload::NodeAlias(p) => {
            project_node_alias(conn, p).await?;
            Ok(ProjectionOutcome::Unit)
        }
    }
}

async fn resolve_or_create_node(
    conn: &mut SqliteConnection,
    identifier: &str,
    node_type: &str,
    created_at: i64,
) -> Result<NodeId, EventsError> {
    let node_id = NodeId::from_canonical_identifier(identifier);
    if nodes::get_by_id(conn, node_id).await?.is_none() {
        let node = Node::new(NodeType::new(node_type), identifier, created_at);
        nodes::insert_if_missing(conn, &node).await?;
    }
    Ok(node_id)
}

async fn project_assert_relation(
    conn: &mut SqliteConnection,
    event_id: oag_core::EventId,
    created_at: i64,
    p: &crate::payload::AssertRelationPayload,
) -> Result<(), EventsError> {
    let subject_id = resolve_or_create_node(conn, &p.subject_identifier, &p.subject_type, created_at).await?;
    let object_id = resolve_or_create_node(conn, &p.object_identifier, &p.object_type, created_at).await?;

    let predicate = Predicate::new(&p.predicate);
    let edge = Edge::new(subject_id, predicate, object_id, created_at);
    edges::insert_if_missing(conn, &edge).await?;

    let actor_id: ActorId = p.actor_id.parse()?;
    let extraction_method = ExtractionMethod::parse(&p.extraction_method)
        .ok_or_else(|| EventsError::UnknownExtractionMethod(p.extraction_method.clone()))?;

    let assertion = Assertion {
        id: event_id,
        edge_id: edge.id,
        actor_id,
        actor_confidence: p.actor_confidence,
        observed_at: p.observed_at,
        asserted_at: created_at,
        extraction_method,
        status: AssertionStatus::Active,
    };
    assertions::insert(conn, &assertion).await?;

    events::add_ref(conn, event_id, "assertion", event_id.as_hash().as_bytes()).await?;
    events::add_ref(conn, event_id, "edge", edge.id.as_hash().as_bytes()).await?;
    events::add_ref(conn, event_id, "node", subject_id.as_hash().as_bytes()).await?;
    events::add_ref(conn, event_id, "node", object_id.as_hash().as_bytes()).await?;

    Ok(())
}

async fn project_add_evidence(
    conn: &mut SqliteConnection,
    event_id: oag_core::EventId,
    p: &crate::payload::AddEvidencePayload,
) -> Result<(), EventsError> {
    let assertion_id: AssertionId = p.assertion_id.parse()?;
    if assertions::get_by_id(conn, assertion_id).await?.is_none() {
        return Err(EventsError::NotFound(format!("assertion {}", p.assertion_id)));
    }
    let evidence_type = EvidenceType::parse(&p.evidence_type)
        .ok_or_else(|| EventsError::UnknownExtractionMethod(p.evidence_type.clone()))?;

    let evidence = Evidence {
        id: event_id,
        assertion_id,
        evidence_type,
        uri: p.uri.clone(),
        title: p.title.clone(),
        excerpt: p.excerpt.clone(),
        content_hash: p.content_hash.clone(),
        observed_at: p.observed_at,
        retrieved_at: p.retrieved_at,
        metadata: serde_json::json!({}),
    };
    assertions::insert_evidence(conn, &evidence).await?;
    events::add_ref(conn, event_id, "assertion", assertion_id.as_hash().as_bytes()).await?;
    Ok(())
}

async fn project_verify_assertion(
    conn: &mut SqliteConnection,
    event_id: oag_core::EventId,
    created_at: i64,
    p: &crate::payload::VerifyAssertionPayload,
) -> Result<(), EventsError> {
    let assertion_id: AssertionId = p.assertion_id.parse()?;
    let observer_actor_id: ActorId = p.observer_actor_id.parse()?;
    if assertions::get_by_id(conn, assertion_id).await?.is_none() {
        return Err(EventsError::NotFound(format!("assertion {}", p.assertion_id)));
    }
    assertions::insert_observation(
        conn,
        &assertions::Observation {
            id: event_id,
            assertion_id,
            observer_actor_id,
            result: p.result.clone(),
            observed_at: p.observed_at,
            created_at,
        },
    )
    .await?;
    events::add_ref(conn, event_id, "assertion", assertion_id.as_hash().as_bytes()).await?;
    Ok(())
}

async fn project_dispute_assertion(
    conn: &mut SqliteConnection,
    event_id: oag_core::EventId,
    created_at: i64,
    p: &crate::payload::DisputeAssertionPayload,
) -> Result<(), EventsError> {
    let disputed_assertion_id: AssertionId = p.disputed_assertion_id.parse()?;
    let disputing_actor_id: ActorId = p.disputing_actor_id.parse()?;
    if assertions::get_by_id(conn, disputed_assertion_id).await?.is_none() {
        return Err(EventsError::NotFound(format!(
            "assertion {}",
            p.disputed_assertion_id
        )));
    }
    assertions::insert_dispute(
        conn,
        &assertions::Dispute {
            id: event_id,
            disputed_assertion_id,
            disputing_actor_id,
            reason: p.reason.clone(),
            created_at,
        },
    )
    .await?;
    assertions::set_status(conn, disputed_assertion_id, AssertionStatus::Disputed).await?;
    events::add_ref(
        conn,
        event_id,
        "assertion",
        disputed_assertion_id.as_hash().as_bytes(),
    )
    .await?;
    Ok(())
}

async fn project_retract_assertion(
    conn: &mut SqliteConnection,
    event_id: oag_core::EventId,
    created_at: i64,
    p: &crate::payload::RetractAssertionPayload,
) -> Result<(), EventsError> {
    let retracted_assertion_id: AssertionId = p.retracted_assertion_id.parse()?;
    let actor_id: ActorId = p.actor_id.parse()?;
    if assertions::get_by_id(conn, retracted_assertion_id).await?.is_none() {
        return Err(EventsError::NotFound(format!(
            "assertion {}",
            p.retracted_assertion_id
        )));
    }
    assertions::insert_retraction(
        conn,
        &assertions::Retraction {
            id: event_id,
            retracted_assertion_id,
            actor_id,
            reason: p.reason.clone(),
            created_at,
        },
    )
    .await?;
    assertions::set_status(conn, retracted_assertion_id, AssertionStatus::Retracted).await?;
    events::add_ref(
        conn,
        event_id,
        "assertion",
        retracted_assertion_id.as_hash().as_bytes(),
    )
    .await?;
    Ok(())
}

async fn project_supersede_assertion(
    conn: &mut SqliteConnection,
    event_id: oag_core::EventId,
    created_at: i64,
    p: &crate::payload::SupersedeAssertionPayload,
) -> Result<(), EventsError> {
    let old_assertion_id: AssertionId = p.old_assertion_id.parse()?;
    let new_assertion_id: AssertionId = p.new_assertion_id.parse()?;
    let actor_id: ActorId = p.actor_id.parse()?;
    for (label, id) in [("old", old_assertion_id), ("new", new_assertion_id)] {
        if assertions::get_by_id(conn, id).await?.is_none() {
            return Err(EventsError::NotFound(format!("{label} assertion {id}")));
        }
    }
    assertions::insert_supersession(
        conn,
        &assertions::Supersession {
            id: event_id,
            old_assertion_id,
            new_assertion_id,
            actor_id,
            created_at,
        },
    )
    .await?;
    assertions::set_status(conn, old_assertion_id, AssertionStatus::Superseded).await?;
    events::add_ref(conn, event_id, "assertion", old_assertion_id.as_hash().as_bytes()).await?;
    events::add_ref(conn, event_id, "assertion", new_assertion_id.as_hash().as_bytes()).await?;
    Ok(())
}

async fn project_actor_declare(
    conn: &mut SqliteConnection,
    event_id: oag_core::EventId,
    created_at: i64,
    p: &crate::payload::ActorDeclarePayload,
) -> Result<ActorId, EventsError> {
    let actor_type = ActorType::parse(&p.actor_type)
        .ok_or_else(|| EventsError::UnknownExtractionMethod(p.actor_type.clone()))?;
    let public_key = p
        .public_key
        .as_deref()
        .map(hex::decode)
        .transpose()?
        .map(|bytes| {
            let len = bytes.len();
            bytes
                .try_into()
                .map_err(|_| EventsError::BadSignatureLength(len))
        })
        .transpose()?;

    // Deterministic identity: an actor with a known public key always gets
    // the same ActorId (dedup across repeated declarations); one with no key
    // is scoped to the declaring event.
    let seed: String = p
        .public_key
        .clone()
        .unwrap_or_else(|| event_id.to_hex());
    let actor_id = ActorId::derive(seed.as_bytes());

    let actor = Actor {
        id: actor_id,
        actor_type,
        name: p.name.clone(),
        public_key,
        identity_uri: p.identity_uri.clone(),
        metadata: serde_json::json!({}),
        created_at,
    };
    actors::insert(conn, &actor).await?;
    Ok(actor_id)
}

fn parse_alias_type(s: &str) -> Result<AliasType, EventsError> {
    Ok(match s {
        "name" => AliasType::Name,
        "url" => AliasType::Url,
        "urn" => AliasType::Urn,
        "package" => AliasType::Package,
        "external_id" => AliasType::ExternalId,
        "acronym" => AliasType::Acronym,
        other => return Err(EventsError::UnknownExtractionMethod(other.to_string())),
    })
}

async fn project_node_alias(
    conn: &mut SqliteConnection,
    p: &crate::payload::NodeAliasPayload,
) -> Result<(), EventsError> {
    let node_id: NodeId = p.node_id.parse()?;
    if nodes::get_by_id(conn, node_id).await?.is_none() {
        return Err(EventsError::NotFound(format!("node {}", p.node_id)));
    }
    let alias_type = parse_alias_type(&p.alias_type)?;
    nodes::insert_alias(
        conn,
        &NodeAlias {
            node_id,
            alias: p.alias.clone(),
            alias_type,
            source_assertion: None,
        },
    )
    .await?;
    Ok(())
}

async fn project_actor_key_add(
    conn: &mut SqliteConnection,
    created_at: i64,
    p: &crate::payload::ActorKeyAddPayload,
) -> Result<(), EventsError> {
    let actor_id: ActorId = p.actor_id.parse()?;
    let key_hash_bytes = hex::decode(&p.key_hash)?;
    let key_hash_len = key_hash_bytes.len();
    let key_hash: [u8; 32] = key_hash_bytes
        .try_into()
        .map_err(|_| EventsError::BadSignatureLength(key_hash_len))?;
    let permissions: Vec<Permission> = p
        .permissions
        .iter()
        .filter_map(|s| Permission::parse(s))
        .collect();
    actors::create_key(conn, &key_hash, actor_id, &permissions, created_at).await?;
    Ok(())
}
