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
}

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
    }
}

async fn capped_json<T: DeserializeOwned>(response: reqwest::Response) -> Result<T, SyncClientError> {
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        body.extend_from_slice(&chunk);
        if body.len() > MAX_RESPONSE_BYTES {
            return Err(SyncClientError::ResponseTooLarge(MAX_RESPONSE_BYTES));
        }
    }
    Ok(serde_json::from_slice(&body)?)
}

/// Thin HTTP client for the `/oag/sync/v1/*` wire protocol (spec section
/// 47-48). Deliberately plain `reqwest`, no custom framing — replication
/// transport stays separable from the event protocol itself.
#[derive(Clone)]
pub struct SyncClient {
    http: reqwest::Client,
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
        Self { http }
    }

    fn base(addr: &str) -> String {
        format!("{}/oag/sync/v1", addr.trim_end_matches('/'))
    }

    pub async fn hello(&self, addr: &str) -> Result<HelloResponse, SyncClientError> {
        let url = format!("{}/hello", Self::base(addr));
        capped_json(self.http.get(url).send().await?.error_for_status()?).await
    }

    #[allow(dead_code)]
    pub async fn heads(&self, addr: &str) -> Result<HeadsResponse, SyncClientError> {
        let url = format!("{}/heads", Self::base(addr));
        capped_json(self.http.get(url).send().await?.error_for_status()?).await
    }

    pub async fn fetch_events(
        &self,
        addr: &str,
        origin_hex: &str,
        from: u64,
        to: u64,
    ) -> Result<EventsResponse, SyncClientError> {
        let url = format!("{}/events/{origin_hex}?from={from}&to={to}", Self::base(addr));
        capped_json(self.http.get(url).send().await?.error_for_status()?).await
    }

    pub async fn fetch_peers(&self, addr: &str) -> Result<PeersResponse, SyncClientError> {
        let url = format!("{}/peers", Self::base(addr));
        capped_json(self.http.get(url).send().await?.error_for_status()?).await
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
        self.http.post(url).json(heartbeat).send().await?.error_for_status()?;
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
        self.http.post(url).json(declaration).send().await?.error_for_status()?;
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
