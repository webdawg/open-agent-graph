//! Minimal standalone HTTP surface for the evolution layer: one endpoint,
//! `POST /brain/exchange`, that runs attention over this node's own pad and
//! the caller's pad, programs this node's own stored pad from the result,
//! and hands the caller back the row it should program *its* pad with.
//! Deliberately not mounted on `oag serve` -- see `oag-brain/Cargo.toml`'s
//! top comment for why this crate stays outside the main workspace.

use std::sync::Arc;

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use oag_storage::repo::tensor_pads as tensor_pads_repo;
use oag_storage::SqlitePool;
use oag_tensor::TensorPad;
use serde::{Deserialize, Serialize};

use crate::brain::Brain;

pub struct AppState {
    pub pool: SqlitePool,
    pub self_peer_id: [u8; 32],
    pub brain: Brain,
}

#[derive(Deserialize)]
struct ExchangeRequest {
    pad: Vec<f32>,
}

#[derive(Serialize)]
struct ExchangeResponse {
    pad: Vec<f32>,
}

fn now_ts() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64
}

async fn exchange(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ExchangeRequest>,
) -> Result<Json<ExchangeResponse>, axum::http::StatusCode> {
    let mut conn = state.pool.acquire().await.map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let own_pad = tensor_pads_repo::get(&mut conn, &state.self_peer_id)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?
        .unwrap_or_default();
    let own_pad = TensorPad::from_values(own_pad).values;
    let caller_pad = TensorPad::from_values(req.pad).values;

    let (own_out, peer_out) =
        state.brain.attend(&own_pad, &caller_pad).map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    tensor_pads_repo::upsert(&mut conn, &state.self_peer_id, &own_out, now_ts())
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(ExchangeResponse { pad: peer_out }))
}

pub fn build_app(state: Arc<AppState>) -> Router {
    Router::new().route("/brain/exchange", post(exchange)).with_state(state)
}
