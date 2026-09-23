use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;

use crate::state::AppState;

const WINDOW: Duration = Duration::from_secs(60);
const MAX_REQUESTS_PER_WINDOW: u32 = 120;

/// Basic per-API-key fixed-window rate limiter (spec sections 58/61/90 —
/// "basic per-key rate limiting"). Anonymous/unauthenticated requests are
/// bucketed together under a single key; they'll also get rejected by auth
/// before doing any real work, so this mainly protects against a single
/// misbehaving key monopolizing the peer.
pub struct RateLimiter {
    windows: Mutex<HashMap<String, (Instant, u32)>>,
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self {
            windows: Mutex::new(HashMap::new()),
        }
    }
}

impl RateLimiter {
    fn check(&self, key: &str) -> bool {
        let mut windows = self.windows.lock().unwrap();
        let now = Instant::now();
        let entry = windows.entry(key.to_string()).or_insert((now, 0));
        if now.duration_since(entry.0) > WINDOW {
            *entry = (now, 0);
        }
        entry.1 += 1;
        entry.1 <= MAX_REQUESTS_PER_WINDOW
    }
}

pub async fn rate_limit_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let key = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("anonymous")
        .to_string();

    if !state.rate_limiter.check(&key) {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }

    Ok(next.run(request).await)
}
