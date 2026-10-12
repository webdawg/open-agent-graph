use serde::Deserialize;

use crate::error::IpfsError;

/// Provider abstraction for IPFS content pinning/retrieval. OAG must work
/// with IPFS integration completely disabled, so `DisabledProvider` is a
/// first-class implementation rather than an `Option<Box<dyn ...>>` threaded
/// through every call site — same shape as `oag_embeddings::EmbeddingProvider`.
#[async_trait::async_trait]
pub trait PinningProvider: Send + Sync {
    /// Pin raw content, returning its CID.
    async fn pin(&self, content: &[u8]) -> Result<String, IpfsError>;

    /// Retrieve previously-pinned content by CID.
    async fn fetch(&self, cid: &str) -> Result<Vec<u8>, IpfsError>;

    fn provider_name(&self) -> &'static str;
}

/// Default provider. IPFS integration is opt-in; every call fails with
/// `IpfsError::Disabled` rather than panicking or silently no-op'ing, so
/// callers (the crawler, `oag ipfs cat`) get a clear typed signal.
#[derive(Debug, Default, Clone, Copy)]
pub struct DisabledProvider;

#[async_trait::async_trait]
impl PinningProvider for DisabledProvider {
    async fn pin(&self, _content: &[u8]) -> Result<String, IpfsError> {
        Err(IpfsError::Disabled)
    }

    async fn fetch(&self, _cid: &str) -> Result<Vec<u8>, IpfsError> {
        Err(IpfsError::Disabled)
    }

    fn provider_name(&self) -> &'static str {
        "disabled"
    }
}

/// Talks to a kubo-compatible IPFS HTTP RPC API (a local daemon, or a remote
/// pinning service fronting the same API shape). Hand-rolled over `reqwest`
/// rather than pulling in `rust-ipfs` (alpha-quality, far heavier than this
/// narrow need justifies) or `ipfs-api` (a thin wrapper over the same two
/// HTTP calls this makes directly) — same "small hand-rolled implementation
/// over a heavyweight crate" posture already used elsewhere in this project
/// (the Prometheus metrics endpoint, `OpenAiCompatibleProvider`).
pub struct KuboHttpProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: Option<String>,
}

impl KuboHttpProvider {
    pub fn new(base_url: impl Into<String>, api_key: Option<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.into(),
            api_key,
        }
    }
}

#[derive(Debug, Deserialize)]
struct AddResponse {
    #[serde(rename = "Hash")]
    hash: String,
}

#[async_trait::async_trait]
impl PinningProvider for KuboHttpProvider {
    async fn pin(&self, content: &[u8]) -> Result<String, IpfsError> {
        let url = format!("{}/api/v0/add", self.base_url.trim_end_matches('/'));
        // `file_name` matters here, not just cosmetically: kubo (and some
        // intermediary proxies fronting a pinning service) identify the
        // uploaded part by its filename, and a part with no filename at all
        // can be silently treated as an empty/ignored field by a stricter
        // multipart parser -- confirmed live against a fixture during this
        // feature's own verification (spec/29-ipfs-pinning.md).
        let part = reqwest::multipart::Part::bytes(content.to_vec()).file_name("content");
        let form = reqwest::multipart::Form::new().part("file", part);
        let mut request = self.client.post(url).multipart(form);
        if let Some(api_key) = &self.api_key {
            request = request.bearer_auth(api_key);
        }
        let response = request.send().await?.error_for_status()?;
        let body = response.text().await?;
        // kubo returns one JSON object per added file, newline-separated;
        // a single-file add is exactly one line.
        let first_line = body.lines().next().unwrap_or("");
        let parsed: AddResponse =
            serde_json::from_str(first_line).map_err(|_| IpfsError::EmptyResponse)?;
        Ok(parsed.hash)
    }

    async fn fetch(&self, cid: &str) -> Result<Vec<u8>, IpfsError> {
        let url = format!("{}/api/v0/cat", self.base_url.trim_end_matches('/'));
        let mut request = self.client.post(&url).query(&[("arg", cid)]);
        if let Some(api_key) = &self.api_key {
            request = request.bearer_auth(api_key);
        }
        let response = request.send().await?.error_for_status()?;
        let bytes = response.bytes().await?;
        Ok(bytes.to_vec())
    }

    fn provider_name(&self) -> &'static str {
        "kubo"
    }
}

#[cfg(test)]
mod tests {
    use axum::{
        extract::Query,
        routing::post,
        Router,
    };
    use std::collections::HashMap;

    use super::*;

    async fn spawn_fixture(router: Router) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn pins_content_and_returns_the_cid() {
        let app = Router::new().route(
            "/api/v0/add",
            post(|body: axum::body::Bytes| async move {
                let raw = String::from_utf8_lossy(&body);
                // Regression guard: a part with no filename can be silently
                // dropped by a stricter multipart parser (found live against
                // a fixture kubo stand-in during this feature's own
                // verification) -- confirm the real content actually made
                // it into the request body, not just that *a* request
                // arrived.
                assert!(raw.contains("filename="), "multipart part must carry a filename: {raw}");
                assert!(raw.contains("hello"), "multipart body must carry the pinned content: {raw}");
                r#"{"Name":"file","Hash":"QmTestCid","Size":"5"}"#
            }),
        );
        let base_url = spawn_fixture(app).await;

        let provider = KuboHttpProvider::new(base_url, None);
        let cid = provider.pin(b"hello").await.unwrap();

        assert_eq!(cid, "QmTestCid");
        assert_eq!(provider.provider_name(), "kubo");
    }

    #[tokio::test]
    async fn fetches_content_by_cid() {
        let app = Router::new().route(
            "/api/v0/cat",
            post(|Query(params): Query<HashMap<String, String>>| async move {
                assert_eq!(params.get("arg").map(String::as_str), Some("QmTestCid"));
                "hello world"
            }),
        );
        let base_url = spawn_fixture(app).await;

        let provider = KuboHttpProvider::new(base_url, None);
        let bytes = provider.fetch("QmTestCid").await.unwrap();

        assert_eq!(bytes, b"hello world");
    }

    #[tokio::test]
    async fn sends_bearer_auth_header_when_api_key_set() {
        let app = Router::new().route(
            "/api/v0/add",
            post(|headers: axum::http::HeaderMap| async move {
                assert_eq!(headers.get("authorization").unwrap(), "Bearer secret-key");
                r#"{"Name":"file","Hash":"QmTestCid","Size":"5"}"#
            }),
        );
        let base_url = spawn_fixture(app).await;

        let provider = KuboHttpProvider::new(base_url, Some("secret-key".to_string()));
        let cid = provider.pin(b"hello").await.unwrap();

        assert_eq!(cid, "QmTestCid");
    }

    #[tokio::test]
    async fn malformed_add_response_is_a_typed_error() {
        let app = Router::new().route("/api/v0/add", post(|| async { "not json" }));
        let base_url = spawn_fixture(app).await;

        let provider = KuboHttpProvider::new(base_url, None);
        let err = provider.pin(b"hello").await.unwrap_err();

        assert!(matches!(err, IpfsError::EmptyResponse));
    }

    #[tokio::test]
    async fn http_error_status_is_a_typed_error() {
        let app = Router::new().route(
            "/api/v0/add",
            post(|| async { axum::http::StatusCode::INTERNAL_SERVER_ERROR }),
        );
        let base_url = spawn_fixture(app).await;

        let provider = KuboHttpProvider::new(base_url, None);
        let err = provider.pin(b"hello").await.unwrap_err();

        assert!(matches!(err, IpfsError::Http(_)));
    }

    #[tokio::test]
    async fn disabled_provider_always_errors() {
        let provider = DisabledProvider;
        assert!(matches!(provider.pin(b"x").await, Err(IpfsError::Disabled)));
        assert!(matches!(provider.fetch("cid").await, Err(IpfsError::Disabled)));
        assert_eq!(provider.provider_name(), "disabled");
    }
}
