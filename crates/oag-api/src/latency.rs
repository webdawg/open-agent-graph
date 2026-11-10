//! REST request-duration histogram (spec section 86's `api_latency`).
//!
//! `GraphService` already records `duration_ms` on a `tracing` span for every request, which is
//! right for reading one request in a log and useless for answering "is this peer getting
//! slower?" across a million of them. This is the aggregate half: one atomic increment per
//! request into a fixed-bucket histogram, rendered by `GET /metrics`.
//!
//! Deliberately a layer here rather than inside `GraphService`: this measures the HTTP request as
//! a whole -- routing, auth, handler, response -- which is not something the graph service can
//! see. `search_latency` on `GraphService` measures the search itself; the two overlap for a
//! search request, and that is correct: one is "how long did the request take", the other is "how
//! long did the search part take".

use std::time::Instant;

use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;

use crate::state::AppState;

/// Times one request and records it. Errors are recorded too -- a slow request that fails is
/// still a slow request, and excluding failures would make the histogram an optimism meter.
pub async fn api_latency_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let started = Instant::now();
    let response = next.run(request).await;
    state.api_latency.record(started.elapsed().as_secs_f64() * 1_000.0);
    response
}
