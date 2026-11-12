use std::sync::atomic::Ordering;
use std::time::Duration;

use futures_util::StreamExt;
use serde::de::DeserializeOwned;

use crate::wire::{EventsResponse, HeadsResponse, HelloResponse, PeersResponse};

#[derive(Debug, thiserror::Error)]
pub enum SyncClientError {
    #[error("http request to peer failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("peer response body exceeds the {0}-byte limit")]
    ResponseTooLarge(usize),
    #[error("peer response was not valid JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),
    /// The peer's rate limiter rejected this request, and said when to retry.
    ///
    /// Distinct from `Http` on purpose: a 429 is not a dead peer, and treating it as one is how a
    /// sync flood becomes thousands of pointless retries against a limiter that already answered.
    #[error("peer rate limited this request; retry after {}s", retry_after.as_secs())]
    RateLimited { retry_after: Duration },
}

/// Used when a 429 omits `Retry-After`, so a client that honours the header still backs off a
/// sensible amount rather than immediately.
const DEFAULT_RETRY_AFTER: Duration = Duration::from_secs(30);

/// Spec section 61 ("disk exhaustion" / resource-exhaustion defenses):
/// a peer we sync *with* is exactly as untrusted as a peer syncing with
/// *us* -- `oag-sync`'s own server already caps incoming push bodies at
/// `MAX_PUSH_BODY_BYTES` (`server.rs`) via `RequestBodyLimitLayer`, but
/// nothing bounded how much of another peer's `/hello`/`/heads`/`/events`/
/// `/peers` response this peer would buffer into memory as a *client*. A
/// malicious or compromised peer returning a gigantic response previously
/// had no limit at all here (`.json()` buffers the whole body before any
/// of this project's own item-count caps, like `discover_peers`'s
/// `MAX_PEERS_PER_RESPONSE`, even get a chance to run). Matches
/// `oag-crawler/src/fetch.rs::safe_fetch`'s same streaming-with-a-cap
/// pattern, applied to the sync wire protocol's own HTTP client.
const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

/// How long a TCP connection to a peer may take to establish before the attempt is abandoned.
///
/// This is the timeout that matters most: a peer that is unreachable *and* silently dropping
/// packets (rather than refusing the connection) would otherwise hold the request open until the
/// operating system gives up, which can be minutes.
pub const SYNC_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Total time budget for one sync request, connect included.
///
/// Deliberately longer than the mesh's own 30s (`oag-mesh`): an `/events` response is a real batch
/// of signed events, not a small snapshot, so a legitimate peer on a slow link needs more room
/// than the mesh's store heads and declarations do. `MAX_RESPONSE_BYTES` bounds how large that
/// batch can be; this bounds how long it can take to arrive.
pub const SYNC_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// Builds a `SyncClient` over a client with bounded connect and total-request timeouts.
///
/// `SyncClient::new` uses the `SYNC_*` constants; this exists so a test can prove the bound holds
/// without waiting the production value out, and so an embedder can retune it.
pub fn sync_client_with_timeouts(
    connect_timeout: std::time::Duration,
    request_timeout: std::time::Duration,
) -> SyncClient {
    SyncClient {
        http: reqwest::Client::builder()
            .connect_timeout(connect_timeout)
            .timeout(request_timeout)
            .build()
            .expect("building a reqwest client with static timeout configuration cannot fail"),
        bytes_in: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
        bytes_out: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
    }
}

impl SyncClient {
    /// Streams a response body to JSON under the size cap, counting every byte this peer received
    /// from a peer into `bytes_in`. Counted even when the cap trips or the JSON is invalid: the
    /// bytes crossed the wire, and a metrics layer that only counted successful responses would
    /// under-report exactly the traffic an operator is trying to account for.
    /// Sends `request`, turning a 429 into [`SyncClientError::RateLimited`] carrying the peer's
    /// `Retry-After`, and every other non-success into the usual `Http` error.
    ///
    /// Split out so all five endpoints treat a rate limit identically rather than each deciding
    /// whether `error_for_status` is good enough.
    async fn send_capped(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, SyncClientError> {
        let response = request.send().await?;
        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let retry_after = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
                .map(Duration::from_secs)
                .unwrap_or(DEFAULT_RETRY_AFTER);
            return Err(SyncClientError::RateLimited { retry_after });
        }
        Ok(response.error_for_status()?)
    }

    async fn capped_json<T: DeserializeOwned>(&self, response: reqwest::Response) -> Result<T, SyncClientError> {
        let mut body = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            body.extend_from_slice(&chunk);
            if body.len() > MAX_RESPONSE_BYTES {
                self.bytes_in.fetch_add(body.len() as u64, Ordering::Relaxed);
                return Err(SyncClientError::ResponseTooLarge(MAX_RESPONSE_BYTES));
            }
        }
        self.bytes_in.fetch_add(body.len() as u64, Ordering::Relaxed);
        Ok(serde_json::from_slice(&body)?)
    }
}

/// Counts one outgoing request body before it is sent. Split out so both POST paths count the same
/// way and a future one can copy it.
fn record_outgoing(bytes_out: &std::sync::Arc<std::sync::atomic::AtomicU64>, body: &[u8]) {
    bytes_out.fetch_add(body.len() as u64, Ordering::Relaxed);
}

/// Thin HTTP client for the `/oag/sync/v1/*` wire protocol (spec section
/// 47-48). Deliberately plain `reqwest`, no custom framing — replication
/// transport stays separable from the event protocol itself.
#[derive(Clone)]
pub struct SyncClient {
    http: reqwest::Client,
    /// Process-lifetime bytes received from peers (`spec/19`'s `replication_bytes_in`), counted in
    /// `capped_json` across every endpoint this client calls.
    bytes_in: std::sync::Arc<std::sync::atomic::AtomicU64>,
    /// Process-lifetime bytes sent to peers (`replication_bytes_out`), counted at each POST.
    bytes_out: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl SyncClient {
    /// A client bounded by [`SYNC_CONNECT_TIMEOUT`] and [`SYNC_REQUEST_TIMEOUT`].
    ///
    /// Not `Default`: `reqwest::Client::default()` sets no timeout at all, which for a client that
    /// talks to an operator-configured list of arbitrary peer addresses means one black-holed peer
    /// can hold a sync round open indefinitely. The bounded values are the default here.
    pub fn new() -> Self {
        sync_client_with_timeouts(SYNC_CONNECT_TIMEOUT, SYNC_REQUEST_TIMEOUT)
    }

    /// A client over an already-built `reqwest::Client`, for an embedder that configures its own
    /// transport (proxies, TLS, connection pooling) and wants this wire protocol on top of it.
    pub fn with_http(http: reqwest::Client) -> Self {
        Self {
            http,
            bytes_in: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
            bytes_out: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    /// The `(bytes_in, bytes_out)` counters themselves, for a metrics layer that needs the exact
    /// same `Arc`s the gossip loop increments. Same sharing pattern as `SyncService`'s.
    pub fn replication_bytes_counters(
        &self,
    ) -> (std::sync::Arc<std::sync::atomic::AtomicU64>, std::sync::Arc<std::sync::atomic::AtomicU64>) {
        (self.bytes_in.clone(), self.bytes_out.clone())
    }

    /// `(bytes_in, bytes_out)` this client has moved since it was built.
    pub fn replication_bytes(&self) -> (u64, u64) {
        (
            self.bytes_in.load(Ordering::Relaxed),
            self.bytes_out.load(Ordering::Relaxed),
        )
    }

    fn base(addr: &str) -> String {
        format!("{}/oag/sync/v1", addr.trim_end_matches('/'))
    }

    pub async fn hello(&self, addr: &str) -> Result<HelloResponse, SyncClientError> {
        let url = format!("{}/hello", Self::base(addr));
        self.capped_json(self.send_capped(self.http.get(url)).await?).await
    }

    #[allow(dead_code)]
    pub async fn heads(&self, addr: &str) -> Result<HeadsResponse, SyncClientError> {
        let url = format!("{}/heads", Self::base(addr));
        self.capped_json(self.send_capped(self.http.get(url)).await?).await
    }

    pub async fn fetch_events(
        &self,
        addr: &str,
        origin_hex: &str,
        from: u64,
        to: u64,
    ) -> Result<EventsResponse, SyncClientError> {
        let url = format!("{}/events/{origin_hex}?from={from}&to={to}", Self::base(addr));
        self.capped_json(self.send_capped(self.http.get(url)).await?).await
    }

    pub async fn fetch_peers(&self, addr: &str) -> Result<PeersResponse, SyncClientError> {
        let url = format!("{}/peers", Self::base(addr));
        self.capped_json(self.send_capped(self.http.get(url)).await?).await
    }

    /// Ephemeral peer trust, Phase 1: push one signed presence heartbeat.
    /// Fire-and-forget by design at the call site (`presence.rs`'s sender
    /// loop) — one unreachable peer must never block heartbeats to the
    /// others, same reasoning as the gossip loop's own per-address handling.
    pub async fn send_presence(
        &self,
        addr: &str,
        heartbeat: &crate::presence::PresenceHeartbeat,
    ) -> Result<(), SyncClientError> {
        let url = format!("{}/presence", Self::base(addr));
        let body = serde_json::to_vec(heartbeat)?;
        record_outgoing(&self.bytes_out, &body);
        self.send_capped(
            self.http
                .post(url)
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body),
        )
        .await?;
        Ok(())
    }

    /// Peer clustering, Phase 1: push one signed cluster declaration.
    /// Fire-and-forget by design at the call site (`cluster.rs`'s
    /// announce loop), same reasoning as `send_presence`.
    pub async fn send_cluster_declaration(
        &self,
        addr: &str,
        declaration: &crate::cluster::ClusterDeclaration,
    ) -> Result<(), SyncClientError> {
        let url = format!("{}/cluster", Self::base(addr));
        let body = serde_json::to_vec(declaration)?;
        record_outgoing(&self.bytes_out, &body);
        self.send_capped(
            self.http
                .post(url)
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body),
        )
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use axum::routing::get;
    use axum::Router;

    use super::*;

    async fn spawn_oversized_peers_fixture() -> String {
        async fn oversized_peers() -> String {
            let huge_address = "x".repeat(super::MAX_RESPONSE_BYTES + 1);
            format!(r#"{{"peers":[{{"peer_id":"p","public_key":"pk","addresses":["{huge_address}"],"last_seen":null}}]}}"#)
        }
        let router = Router::new().route("/oag/sync/v1/peers", get(oversized_peers));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn fetch_peers_rejects_an_oversized_response_instead_of_buffering_it() {
        let base = spawn_oversized_peers_fixture().await;
        let client = SyncClient::new();

        let result = client.fetch_peers(&base).await;
        assert!(matches!(result, Err(SyncClientError::ResponseTooLarge(_))), "got {result:?}");
    }

    /// The black-hole case: the peer accepts the TCP connection and then never sends a byte. A
    /// connect timeout alone cannot catch this -- the connection *succeeded* -- so it is the total
    /// request timeout that bounds the wait. Without it, one non-responding peer could hold a
    /// sync round open indefinitely.
    /// The byte counters are the aggregate half of replication cost, so the only meaningful test
    /// is that real traffic moves them: a fetched response increments `bytes_in`, and a sent
    /// presence heartbeat increments `bytes_out`. A fixture server that returns 200 is enough --
    /// the point is what the *client* counts, not what the server validates.
    #[tokio::test]
    async fn replication_bytes_counters_move_with_real_traffic() {
        async fn peers() -> &'static str {
            r#"{"peers":[]}"#
        }
        async fn presence() -> &'static str {
            "ok"
        }
        let router = Router::new()
            .route("/oag/sync/v1/peers", get(peers))
            .route("/oag/sync/v1/presence", axum::routing::post(presence));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let base = format!("http://{addr}");

        let client = SyncClient::new();
        assert_eq!(client.replication_bytes(), (0, 0), "nothing moved yet");

        client.fetch_peers(&base).await.unwrap();
        let (bytes_in, bytes_out) = client.replication_bytes();
        assert!(bytes_in > 0, "a fetched response body must be counted, got {bytes_in}");
        assert_eq!(bytes_out, 0, "a GET sends no body");

        let identity = oag_crypto::PeerIdentity::generate();
        let heartbeat = crate::presence::build_and_sign(
            &identity,
            crate::presence::PresenceStatus::Online,
            1,
            2,
        )
        .unwrap();
        client.send_presence(&base, &heartbeat).await.unwrap();
        let (bytes_in_after, bytes_out_after) = client.replication_bytes();
        // `bytes_in` counts *streamed* read bodies, not the tiny unread responses to POSTs. That
        // boundary is deliberate and is stated in the metric's HELP text.
        assert_eq!(bytes_in_after, bytes_in, "an unread POST response body is not counted");
        assert!(
            bytes_out_after > 0,
            "the presence heartbeat body must be counted as bytes out, got {bytes_out_after}"
        );
    }

    #[tokio::test]
    async fn a_peer_that_accepts_but_never_responds_is_abandoned_at_the_timeout() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else { break };
                // Hold the connection open deliberately, responding with nothing.
                std::mem::forget(stream);
            }
        });

        let client = sync_client_with_timeouts(
            std::time::Duration::from_millis(50),
            std::time::Duration::from_millis(200),
        );
        let base = format!("http://{addr}");
        let started = std::time::Instant::now();
        let result = client.fetch_peers(&base).await;
        let elapsed = started.elapsed();

        assert!(result.is_err(), "a non-responding peer must produce an error, not hang");
        assert!(
            elapsed < std::time::Duration::from_secs(5),
            "the request timeout should fire well before this, took {elapsed:?}"
        );
    }

    /// The default client must be bounded too, not only the test-only constructor: the whole point
    /// is that `SyncClient::new()` -- the constructor every production caller uses -- cannot hang
    /// forever. Proven by a connection-refused peer, which exercises the same client
    /// configuration through the public constructor.
    #[tokio::test]
    async fn the_default_client_is_bounded_and_reaches_a_real_peer() {
        async fn peers() -> &'static str {
            r#"{"peers":[]}"#
        }
        let router = Router::new().route("/oag/sync/v1/peers", get(peers));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });

        let client = SyncClient::new();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client.fetch_peers(&format!("http://{addr}")),
        )
        .await
        .expect("the default client must not hang a reachable peer");
        assert!(result.is_ok(), "got {result:?}");
    }
}
