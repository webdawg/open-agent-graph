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
/// Spec section 61 ("disk exhaustion"): the rate limiter's own tracking map
/// is itself an unbounded-growth vector if left unchecked -- confirmed
/// live, 30,000 requests each with a different bogus `Authorization`
/// header (no valid credentials needed at all, since `check()` runs before
/// any handler validates the key) grew this peer's RSS from ~6 MB to
/// ~22.7 MB, strictly increasing forever with no eviction. Once the map
/// grows past this many distinct keys, a sweep removes anything whose
/// window has already expired, bounding steady-state size to roughly
/// "how many distinct keys were active in the last `WINDOW`," not "every
/// distinct key ever seen since process start."
const SWEEP_THRESHOLD: usize = 10_000;

/// Basic per-API-key fixed-window rate limiter (spec sections 58/61/90 —
/// "basic per-key rate limiting"). Anonymous/unauthenticated requests are
/// bucketed together under a single key; they'll also get rejected by auth
/// before doing any real work, so this mainly protects against a single
/// misbehaving key monopolizing the peer.
pub struct RateLimiter {
    windows: Mutex<HashMap<String, (Instant, u32)>>,
    window: Duration,
    max_requests_per_window: u32,
    sweep_threshold: usize,
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new(WINDOW, MAX_REQUESTS_PER_WINDOW, SWEEP_THRESHOLD)
    }
}

impl RateLimiter {
    fn new(window: Duration, max_requests_per_window: u32, sweep_threshold: usize) -> Self {
        Self {
            windows: Mutex::new(HashMap::new()),
            window,
            max_requests_per_window,
            sweep_threshold,
        }
    }

    fn check(&self, key: &str) -> bool {
        let mut windows = self.windows.lock().unwrap();
        let now = Instant::now();

        if windows.len() > self.sweep_threshold {
            windows.retain(|_, (started, _)| now.duration_since(*started) <= self.window);
        }

        let entry = windows.entry(key.to_string()).or_insert((now, 0));
        if now.duration_since(entry.0) > self.window {
            *entry = (now, 0);
        }
        entry.1 += 1;
        entry.1 <= self.max_requests_per_window
    }

    #[cfg(test)]
    fn tracked_key_count(&self) -> usize {
        self.windows.lock().unwrap().len()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_within_the_limit_are_allowed_and_over_the_limit_are_rejected() {
        let limiter = RateLimiter::new(Duration::from_secs(60), 3, SWEEP_THRESHOLD);
        assert!(limiter.check("key-a"));
        assert!(limiter.check("key-a"));
        assert!(limiter.check("key-a"));
        assert!(!limiter.check("key-a"), "4th request within the window must be rejected");
        // A different key has its own independent budget.
        assert!(limiter.check("key-b"));
    }

    /// Regression test for the exact bug found live: the tracked-key map
    /// must not grow forever. Uses a tiny real window (not a mocked clock)
    /// so the test can genuinely sleep past expiry rather than needing
    /// `Instant` to be fakeable.
    #[test]
    fn stale_entries_are_swept_once_the_map_grows_past_the_threshold() {
        let limiter = RateLimiter::new(Duration::from_millis(20), 100, 5);

        for i in 0..6 {
            assert!(limiter.check(&format!("stale-key-{i}")));
        }
        assert_eq!(limiter.tracked_key_count(), 6, "sanity check: all 6 keys tracked so far");

        std::thread::sleep(Duration::from_millis(40));

        // This 7th key arrives when the map already has 6 entries (> the
        // threshold of 5), triggering a sweep that must remove the 6
        // now-expired entries above, leaving only this brand-new one.
        assert!(limiter.check("fresh-key"));
        assert_eq!(
            limiter.tracked_key_count(),
            1,
            "expired entries must be swept once the map exceeds the threshold, not retained forever"
        );
    }
}
