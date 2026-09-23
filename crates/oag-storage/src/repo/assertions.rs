use oag_core::{
    ActorId, Assertion, AssertionId, AssertionStatus, EdgeId, Evidence, EvidenceType,
    ExtractionMethod,
};
use sqlx::SqliteConnection;

use crate::error::{bytes_to_array, StorageError};
use crate::models::{
    AssertionRow, DisputeRow, EvidenceRow, ObservationRow, RetractionRow,
};

fn row_to_assertion(row: AssertionRow) -> Result<Assertion, StorageError> {
    let extraction_method = ExtractionMethod::parse(&row.extraction_method).ok_or_else(|| {
        StorageError::UnknownEnumValue("extraction_method", row.extraction_method.clone())
    })?;
    let status = AssertionStatus::parse(&row.status)
        .ok_or_else(|| StorageError::UnknownEnumValue("status", row.status.clone()))?;
    Ok(Assertion {
        id: AssertionId::from_hash(oag_core::Hash32::from_bytes(bytes_to_array(&row.id)?)),
        edge_id: EdgeId::from_hash(oag_core::Hash32::from_bytes(bytes_to_array(&row.edge_id)?)),
        actor_id: ActorId::from_hash(oag_core::Hash32::from_bytes(bytes_to_array(&row.actor_id)?)),
        actor_confidence: row.actor_confidence.map(|c| c as f32),
        observed_at: row.observed_at,
        asserted_at: row.asserted_at,
        extraction_method,
        status,
    })
}

pub async fn insert(conn: &mut SqliteConnection, assertion: &Assertion) -> Result<(), StorageError> {
    sqlx::query(
        "INSERT INTO assertions \
         (id, edge_id, actor_id, actor_confidence, observed_at, asserted_at, extraction_method, status) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(assertion.id.as_hash().as_bytes().to_vec())
    .bind(assertion.edge_id.as_hash().as_bytes().to_vec())
    .bind(assertion.actor_id.as_hash().as_bytes().to_vec())
    .bind(assertion.actor_confidence.map(|c| c as f64))
    .bind(assertion.observed_at)
    .bind(assertion.asserted_at)
    .bind(assertion.extraction_method.as_str())
    .bind(assertion.status.as_str())
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn get_by_id(
    conn: &mut SqliteConnection,
    id: AssertionId,
) -> Result<Option<Assertion>, StorageError> {
    let row: Option<AssertionRow> = sqlx::query_as("SELECT * FROM assertions WHERE id = ?")
        .bind(id.as_hash().as_bytes().to_vec())
        .fetch_optional(&mut *conn)
        .await?;
    row.map(row_to_assertion).transpose()
}

pub async fn list_by_edge(
    conn: &mut SqliteConnection,
    edge_id: EdgeId,
) -> Result<Vec<Assertion>, StorageError> {
    let rows: Vec<AssertionRow> = sqlx::query_as("SELECT * FROM assertions WHERE edge_id = ?")
        .bind(edge_id.as_hash().as_bytes().to_vec())
        .fetch_all(&mut *conn)
        .await?;
    rows.into_iter().map(row_to_assertion).collect()
}

pub async fn set_status(
    conn: &mut SqliteConnection,
    id: AssertionId,
    status: AssertionStatus,
) -> Result<(), StorageError> {
    sqlx::query("UPDATE assertions SET status = ? WHERE id = ?")
        .bind(status.as_str())
        .bind(id.as_hash().as_bytes().to_vec())
        .execute(&mut *conn)
        .await?;
    Ok(())
}

fn row_to_evidence(row: EvidenceRow) -> Result<Evidence, StorageError> {
    let evidence_type = EvidenceType::parse(&row.evidence_type)
        .ok_or_else(|| StorageError::UnknownEnumValue("evidence_type", row.evidence_type.clone()))?;
    Ok(Evidence {
        id: oag_core::EventId::from_hash(oag_core::Hash32::from_bytes(bytes_to_array(&row.id)?)),
        assertion_id: AssertionId::from_hash(oag_core::Hash32::from_bytes(bytes_to_array(
            &row.assertion_id,
        )?)),
        evidence_type,
        uri: row.uri,
        title: row.title,
        excerpt: row.excerpt,
        content_hash: row.content_hash,
        observed_at: row.observed_at,
        retrieved_at: row.retrieved_at,
        metadata: serde_json::from_str(&row.metadata)?,
    })
}

pub async fn insert_evidence(
    conn: &mut SqliteConnection,
    evidence: &Evidence,
) -> Result<(), StorageError> {
    let metadata = serde_json::to_string(&evidence.metadata)?;
    sqlx::query(
        "INSERT INTO evidence \
         (id, assertion_id, evidence_type, uri, title, excerpt, content_hash, observed_at, retrieved_at, metadata) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(evidence.id.as_hash().as_bytes().to_vec())
    .bind(evidence.assertion_id.as_hash().as_bytes().to_vec())
    .bind(evidence.evidence_type.as_str())
    .bind(&evidence.uri)
    .bind(&evidence.title)
    .bind(&evidence.excerpt)
    .bind(&evidence.content_hash)
    .bind(evidence.observed_at)
    .bind(evidence.retrieved_at)
    .bind(metadata)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn list_evidence(
    conn: &mut SqliteConnection,
    assertion_id: AssertionId,
) -> Result<Vec<Evidence>, StorageError> {
    let rows: Vec<EvidenceRow> = sqlx::query_as("SELECT * FROM evidence WHERE assertion_id = ?")
        .bind(assertion_id.as_hash().as_bytes().to_vec())
        .fetch_all(&mut *conn)
        .await?;
    rows.into_iter().map(row_to_evidence).collect()
}

pub struct Dispute {
    pub id: oag_core::EventId,
    pub disputed_assertion_id: AssertionId,
    pub disputing_actor_id: ActorId,
    pub reason: Option<String>,
    pub created_at: i64,
}

pub async fn insert_dispute(conn: &mut SqliteConnection, d: &Dispute) -> Result<(), StorageError> {
    sqlx::query(
        "INSERT INTO assertion_disputes (id, disputed_assertion_id, disputing_actor_id, reason, created_at) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(d.id.as_hash().as_bytes().to_vec())
    .bind(d.disputed_assertion_id.as_hash().as_bytes().to_vec())
    .bind(d.disputing_actor_id.as_hash().as_bytes().to_vec())
    .bind(&d.reason)
    .bind(d.created_at)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn list_disputes(
    conn: &mut SqliteConnection,
    assertion_id: AssertionId,
) -> Result<Vec<DisputeRow>, StorageError> {
    let rows: Vec<DisputeRow> =
        sqlx::query_as("SELECT * FROM assertion_disputes WHERE disputed_assertion_id = ?")
            .bind(assertion_id.as_hash().as_bytes().to_vec())
            .fetch_all(&mut *conn)
            .await?;
    Ok(rows)
}

pub struct Retraction {
    pub id: oag_core::EventId,
    pub retracted_assertion_id: AssertionId,
    pub actor_id: ActorId,
    pub reason: Option<String>,
    pub created_at: i64,
}

pub async fn insert_retraction(
    conn: &mut SqliteConnection,
    r: &Retraction,
) -> Result<(), StorageError> {
    sqlx::query(
        "INSERT INTO assertion_retractions (id, retracted_assertion_id, actor_id, reason, created_at) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(r.id.as_hash().as_bytes().to_vec())
    .bind(r.retracted_assertion_id.as_hash().as_bytes().to_vec())
    .bind(r.actor_id.as_hash().as_bytes().to_vec())
    .bind(&r.reason)
    .bind(r.created_at)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn list_retractions(
    conn: &mut SqliteConnection,
    assertion_id: AssertionId,
) -> Result<Vec<RetractionRow>, StorageError> {
    let rows: Vec<RetractionRow> =
        sqlx::query_as("SELECT * FROM assertion_retractions WHERE retracted_assertion_id = ?")
            .bind(assertion_id.as_hash().as_bytes().to_vec())
            .fetch_all(&mut *conn)
            .await?;
    Ok(rows)
}

pub struct Supersession {
    pub id: oag_core::EventId,
    pub old_assertion_id: AssertionId,
    pub new_assertion_id: AssertionId,
    pub actor_id: ActorId,
    pub created_at: i64,
}

pub async fn insert_supersession(
    conn: &mut SqliteConnection,
    s: &Supersession,
) -> Result<(), StorageError> {
    sqlx::query(
        "INSERT INTO assertion_supersessions (id, old_assertion_id, new_assertion_id, actor_id, created_at) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(s.id.as_hash().as_bytes().to_vec())
    .bind(s.old_assertion_id.as_hash().as_bytes().to_vec())
    .bind(s.new_assertion_id.as_hash().as_bytes().to_vec())
    .bind(s.actor_id.as_hash().as_bytes().to_vec())
    .bind(s.created_at)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub struct Observation {
    pub id: oag_core::EventId,
    pub assertion_id: AssertionId,
    pub observer_actor_id: ActorId,
    pub result: String,
    pub observed_at: i64,
    pub created_at: i64,
}

pub async fn insert_observation(
    conn: &mut SqliteConnection,
    o: &Observation,
) -> Result<(), StorageError> {
    sqlx::query(
        "INSERT INTO observations (id, assertion_id, observer_actor_id, result, observed_at, created_at) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(o.id.as_hash().as_bytes().to_vec())
    .bind(o.assertion_id.as_hash().as_bytes().to_vec())
    .bind(o.observer_actor_id.as_hash().as_bytes().to_vec())
    .bind(&o.result)
    .bind(o.observed_at)
    .bind(o.created_at)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn list_observations(
    conn: &mut SqliteConnection,
    assertion_id: AssertionId,
) -> Result<Vec<ObservationRow>, StorageError> {
    let rows: Vec<ObservationRow> =
        sqlx::query_as("SELECT * FROM observations WHERE assertion_id = ?")
            .bind(assertion_id.as_hash().as_bytes().to_vec())
            .fetch_all(&mut *conn)
            .await?;
    Ok(rows)
}
