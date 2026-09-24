use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;

use crate::service::SyncService;

const WINDOW: Duration = Duration::from_secs(60);
/// Generous relative to legitimate traffic: the default gossip interval is
/// 30s per configured peer (spec section 94), so even a peer with several
/// dozen bootstrap/discovered addresses stays far under this. Sized to
/// still bound a genuine flood (spec section 61 — "event flooding",
/// "signature spam") rather than to fingerprint normal usage precisely.
const MAX_REQUESTS_PER_WINDOW: u32 = 600;

/// A single global fixed-window counter, not per-caller. The sync endpoints
/// are deliberately unauthenticated (see `server::router`'s doc comment —
/// every event is self-authenticating via its own signature), so there is
/// no natural per-caller identity to key by short of the source IP; keying
/// by IP would require switching every `axum::serve` call site (the CLI's
/// `serve.rs` and both integration test files) to
/// `into_make_service_with_connect_info`, which is a larger, riskier change
/// than this hardening pass warrants. A global cap still directly satisfies
/// spec section 61 ("a valid signature alone must never guarantee unlimited
/// replication") — per-source fairness is a reasonable future refinement.
pub struct SyncRateLimiter {
    window: Mutex<(Instant, u32)>,
}

impl Default for SyncRateLimiter {
    fn default() -> Self {
        Self {
            window: Mutex::new((Instant::now(), 0)),
        }
    }
}

impl SyncRateLimiter {
    fn check(&self) -> bool {
        let mut window = self.window.lock().unwrap();
        let now = Instant::now();
        if now.duration_since(window.0) > WINDOW {
            *window = (now, 0);
        }
        window.1 += 1;
        window.1 <= MAX_REQUESTS_PER_WINDOW
    }
}

pub async fn rate_limit_middleware(
    State(service): State<SyncService>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if !service.rate_limiter().check() {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }
    Ok(next.run(request).await)
}
