use crate::wire::{EventsResponse, HeadsResponse, HelloResponse, PeersResponse};

#[derive(Debug, thiserror::Error)]
pub enum SyncClientError {
    #[error("http request to peer failed: {0}")]
    Http(#[from] reqwest::Error),
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
        Ok(self.http.get(url).send().await?.error_for_status()?.json().await?)
    }

    #[allow(dead_code)]
    pub async fn heads(&self, addr: &str) -> Result<HeadsResponse, SyncClientError> {
        let url = format!("{}/heads", Self::base(addr));
        Ok(self.http.get(url).send().await?.error_for_status()?.json().await?)
    }

    pub async fn fetch_events(
        &self,
        addr: &str,
        origin_hex: &str,
        from: u64,
        to: u64,
    ) -> Result<EventsResponse, SyncClientError> {
        let url = format!("{}/events/{origin_hex}?from={from}&to={to}", Self::base(addr));
        Ok(self.http.get(url).send().await?.error_for_status()?.json().await?)
    }

    pub async fn fetch_peers(&self, addr: &str) -> Result<PeersResponse, SyncClientError> {
        let url = format!("{}/peers", Self::base(addr));
        Ok(self.http.get(url).send().await?.error_for_status()?.json().await?)
    }
}
