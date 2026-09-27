use serde::Deserialize;

use crate::error::EmbeddingError;

/// Provider abstraction for embedding generation (spec section 64). OAG must
/// work with embeddings completely disabled, so `DisabledProvider` is a
/// first-class implementation rather than an `Option<Box<dyn ...>>` threaded
/// through every call site.
#[async_trait::async_trait]
pub trait EmbeddingProvider: Send + Sync {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, EmbeddingError>;

    /// Short identifier persisted alongside each stored embedding (the
    /// `provider` column in `node_embeddings`) so a later config change is
    /// detectable rather than silently mixing incompatible vector spaces.
    fn provider_name(&self) -> &'static str;

    /// Model identifier persisted alongside each stored embedding (the
    /// `model` column in `node_embeddings`).
    fn model_name(&self) -> &str;
}

/// Default provider. Spec section 64: "OAG itself must work with embeddings
/// completely disabled." Every call fails with `EmbeddingError::Disabled`
/// rather than panicking or silently no-op'ing, so callers get a clear typed
/// signal.
#[derive(Debug, Default, Clone, Copy)]
pub struct DisabledProvider;

#[async_trait::async_trait]
impl EmbeddingProvider for DisabledProvider {
    async fn embed(&self, _text: &str) -> Result<Vec<f32>, EmbeddingError> {
        Err(EmbeddingError::Disabled)
    }

    fn provider_name(&self) -> &'static str {
        "disabled"
    }

    fn model_name(&self) -> &str {
        ""
    }
}

/// Talks to any server exposing the OpenAI embeddings HTTP shape
/// (`POST {base_url}/embeddings` with `{"input": ..., "model": ...}`,
/// response `{"data": [{"embedding": [...]}]}`). This covers literal OpenAI
/// as well as Ollama, LM Studio, vLLM, and text-embeddings-inference, all of
/// which implement the same wire shape.
pub struct OpenAiCompatibleProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: Option<String>,
    model: String,
}

impl OpenAiCompatibleProvider {
    pub fn new(base_url: impl Into<String>, api_key: Option<String>, model: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.into(),
            api_key,
            model: model.into(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct EmbeddingsResponse {
    data: Vec<EmbeddingsResponseItem>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingsResponseItem {
    embedding: Vec<f32>,
}

#[async_trait::async_trait]
impl EmbeddingProvider for OpenAiCompatibleProvider {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        let url = format!("{}/embeddings", self.base_url.trim_end_matches('/'));
        let mut request = self
            .client
            .post(url)
            .json(&serde_json::json!({ "input": text, "model": self.model }));
        if let Some(api_key) = &self.api_key {
            request = request.bearer_auth(api_key);
        }
        let response = request.send().await?.error_for_status()?;
        let body: EmbeddingsResponse = response.json().await?;
        body.data
            .into_iter()
            .next()
            .map(|item| item.embedding)
            .ok_or(EmbeddingError::EmptyResponse)
    }

    fn provider_name(&self) -> &'static str {
        "openai-compatible"
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}

#[cfg(test)]
mod tests {
    use axum::{routing::post, Json, Router};
    use serde_json::{json, Value};

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
    async fn embeds_text_against_openai_compatible_endpoint() {
        let app = Router::new().route(
            "/embeddings",
            post(|Json(body): Json<Value>| async move {
                assert_eq!(body["input"], "hello world");
                assert_eq!(body["model"], "test-model");
                Json(json!({ "data": [{ "embedding": [0.1, 0.2, 0.3] }] }))
            }),
        );
        let base_url = spawn_fixture(app).await;

        let provider = OpenAiCompatibleProvider::new(base_url, None, "test-model");
        let embedding = provider.embed("hello world").await.unwrap();

        assert_eq!(embedding, vec![0.1, 0.2, 0.3]);
        assert_eq!(provider.provider_name(), "openai-compatible");
        assert_eq!(provider.model_name(), "test-model");
    }

    #[tokio::test]
    async fn sends_bearer_auth_header_when_api_key_set() {
        let app = Router::new().route(
            "/embeddings",
            post(|headers: axum::http::HeaderMap, Json(_body): Json<Value>| async move {
                assert_eq!(headers.get("authorization").unwrap(), "Bearer secret-key");
                Json(json!({ "data": [{ "embedding": [1.0] }] }))
            }),
        );
        let base_url = spawn_fixture(app).await;

        let provider =
            OpenAiCompatibleProvider::new(base_url, Some("secret-key".to_string()), "test-model");
        let embedding = provider.embed("hi").await.unwrap();

        assert_eq!(embedding, vec![1.0]);
    }

    #[tokio::test]
    async fn empty_data_array_is_a_typed_error() {
        let app = Router::new()
            .route("/embeddings", post(|| async { Json(json!({ "data": [] })) }));
        let base_url = spawn_fixture(app).await;

        let provider = OpenAiCompatibleProvider::new(base_url, None, "test-model");
        let err = provider.embed("hi").await.unwrap_err();

        assert!(matches!(err, EmbeddingError::EmptyResponse));
    }

    #[tokio::test]
    async fn http_error_status_is_a_typed_error() {
        let app = Router::new().route(
            "/embeddings",
            post(|| async { axum::http::StatusCode::INTERNAL_SERVER_ERROR }),
        );
        let base_url = spawn_fixture(app).await;

        let provider = OpenAiCompatibleProvider::new(base_url, None, "test-model");
        let err = provider.embed("hi").await.unwrap_err();

        assert!(matches!(err, EmbeddingError::Http(_)));
    }

    #[tokio::test]
    async fn disabled_provider_always_errors() {
        let provider = DisabledProvider;
        let err = provider.embed("anything").await.unwrap_err();
        assert!(matches!(err, EmbeddingError::Disabled));
        assert_eq!(provider.provider_name(), "disabled");
    }
}
