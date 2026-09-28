//! Spec section 87 (Logging): structured `tracing` fields on every REST
//! request. Scope for this milestone is the fields a single HTTP-layer span
//! can cheaply and correctly carry: `request_id`, `peer_id`, `route`,
//! `method`, `actor_id` (recorded once auth resolves it), `duration`, and
//! `result`. `event_id`/`origin_peer` (per-event fields, relevant to
//! `oag-sync`'s replication/gossip paths rather than a single REST request)
//! and MCP-side request spans are deferred -- see the repo root's
//! `OPEN_QUESTIONS.md` "Logging" section.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use axum::body::Body;
use axum::extract::Request;
use axum::response::Response;
use oag_crypto::PeerId;
use tower_http::trace::MakeSpan;
use tracing::Span;

static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Unique enough for correlating log lines within one peer process: this
/// process's OS pid plus a monotonic per-process counter. Not a UUID --
/// avoids a new dependency for a value that only needs to be unique within
/// one running peer's own logs, never compared across peers.
fn next_request_id() -> String {
    let n = REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}-{n}", std::process::id())
}

#[derive(Clone)]
pub struct ApiMakeSpan {
    peer_id: PeerId,
}

impl ApiMakeSpan {
    pub fn new(peer_id: PeerId) -> Self {
        Self { peer_id }
    }
}

impl MakeSpan<Body> for ApiMakeSpan {
    fn make_span(&mut self, request: &Request<Body>) -> Span {
        tracing::info_span!(
            "http_request",
            request_id = %next_request_id(),
            peer_id = %self.peer_id,
            route = %request.uri().path(),
            method = %request.method(),
            actor_id = tracing::field::Empty,
            status = tracing::field::Empty,
        )
    }
}

/// Records `duration`/`result`/`status` once the handler has produced a
/// response -- `actor_id` is recorded separately, from inside
/// `auth::authenticate`, the moment it's known (before the handler body
/// even runs), so it's present on the span regardless of how the request
/// finishes.
pub fn on_response(response: &Response, latency: Duration, span: &Span) {
    let status = response.status();
    span.record("status", status.as_u16());
    tracing::info!(
        duration_ms = latency.as_millis() as u64,
        result = if status.is_success() { "ok" } else { "error" },
        "request completed"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_ids_are_unique_and_stable_shape() {
        let a = next_request_id();
        let b = next_request_id();
        assert_ne!(a, b);
        assert!(a.contains('-'));
    }
}
