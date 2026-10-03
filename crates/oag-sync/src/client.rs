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
#[derive(Clone, Default)]
pub struct SyncClient {
    http: reqwest::Client,
}

impl SyncClient {
    pub fn new() -> Self {
        Self::default()
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
}
