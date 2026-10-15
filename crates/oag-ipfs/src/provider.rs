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

    /// Pin a whole directory as a single UnixFS tree, returning the root
    /// directory's CID. `files` is `(relative_path, content)` pairs — e.g.
    /// `("index.html", ...)`, `("css/style.css", ...)`. Distinct from `pin`
    /// (one file, one CID) rather than a loop calling it once per file,
    /// because a directory needs one UnixFS tree with a single root CID a
    /// browser can resolve `/index.html`, `/css/style.css`, etc. under —
    /// seeing this as "the next step on a future-work list," see
    /// `spec/29-ipfs-pinning.md`.
    async fn pin_directory(&self, files: &[(String, Vec<u8>)]) -> Result<String, IpfsError>;

    /// This provider's own real, dialable multiaddrs for a browser to
    /// connect directly to (e.g. for `GET /ipfs-site/{cid}`'s bootstrap
    /// page) — empty for every provider except `EmbeddedIpfsProvider`,
    /// which is the only one that *is* a directly-dialable libp2p node
    /// itself (`KuboHttpProvider` talks to a separate Kubo process over
    /// plain HTTP, which has no bearing on what a browser can dial).
    /// Default empty rather than required, so adding this didn't need to
    /// touch `DisabledProvider`/`KuboHttpProvider`.
    async fn dialable_multiaddrs(&self) -> Vec<String> {
        Vec::new()
    }

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

    async fn pin_directory(&self, _files: &[(String, Vec<u8>)]) -> Result<String, IpfsError> {
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

    async fn pin_directory(&self, files: &[(String, Vec<u8>)]) -> Result<String, IpfsError> {
        let url = format!(
            "{}/api/v0/add?recursive=true&wrap-with-directory=true&cid-version=1",
            self.base_url.trim_end_matches('/')
        );
        let mut form = reqwest::multipart::Form::new();
        for (relative_path, content) in files {
            // Same "a part needs an explicit filename or it can be silently
            // dropped" care as the single-file `pin` above -- here the
            // filename is also load-bearing in a second way: kubo uses each
            // part's filename as that file's path *inside* the resulting
            // UnixFS directory, not just as a label.
            let part = reqwest::multipart::Part::bytes(content.clone()).file_name(relative_path.clone());
            form = form.part("file", part);
        }
        let mut request = self.client.post(url).multipart(form);
        if let Some(api_key) = &self.api_key {
            request = request.bearer_auth(api_key);
        }
        let response = request.send().await?.error_for_status()?;
        let body = response.text().await?;
        // Unlike single-file `pin`'s first line, a directory add returns one
        // JSON object per file *and* per intermediate directory, with the
        // wrapping root directory's own object always last -- that line's
        // Hash is the one CID a caller can resolve every uploaded path
        // under.
        let last_line = body.lines().last().unwrap_or("");
        let parsed: AddResponse =
            serde_json::from_str(last_line).map_err(|_| IpfsError::EmptyResponse)?;
        Ok(parsed.hash)
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
        assert!(matches!(
            provider.pin_directory(&[]).await,
            Err(IpfsError::Disabled)
        ));
        assert_eq!(provider.provider_name(), "disabled");
    }

    #[tokio::test]
    async fn pins_a_directory_and_returns_the_root_cid() {
        let app = Router::new().route(
            "/api/v0/add",
            post(|body: axum::body::Bytes| async move {
                let raw = String::from_utf8_lossy(&body);
                // Both files' relative paths must survive as part filenames
                // -- kubo uses them as the path *inside* the resulting
                // UnixFS directory, not just as a label.
                assert!(raw.contains(r#"filename="index.html""#), "missing index.html part: {raw}");
                assert!(
                    raw.contains(r#"filename="css/style.css""#),
                    "missing css/style.css part: {raw}"
                );
                // kubo's real response is one JSON object per file *plus*
                // the wrapping directory, newline-separated, root last.
                concat!(
                    r#"{"Name":"index.html","Hash":"QmFile1","Size":"5"}"#, "\n",
                    r#"{"Name":"css/style.css","Hash":"QmFile2","Size":"5"}"#, "\n",
                    r#"{"Name":"","Hash":"QmRootDir","Size":"10"}"#,
                )
            }),
        );
        let base_url = spawn_fixture(app).await;

        let provider = KuboHttpProvider::new(base_url, None);
        let cid = provider
            .pin_directory(&[
                ("index.html".to_string(), b"<html>".to_vec()),
                ("css/style.css".to_string(), b"body{}".to_vec()),
            ])
            .await
            .unwrap();

        // The root directory's CID (the last line), not either file's own.
        assert_eq!(cid, "QmRootDir");
    }

    #[tokio::test]
    async fn malformed_directory_add_response_is_a_typed_error() {
        let app = Router::new().route("/api/v0/add", post(|| async { "not json" }));
        let base_url = spawn_fixture(app).await;

        let provider = KuboHttpProvider::new(base_url, None);
        let err = provider
            .pin_directory(&[("index.html".to_string(), b"<html>".to_vec())])
            .await
            .unwrap_err();

        assert!(matches!(err, IpfsError::EmptyResponse));
    }
}
