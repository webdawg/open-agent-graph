//! Spec section 74 (LLM Extraction): after deterministic extraction (JSON-LD,
//! ARD, A2A, llms.txt, HTML meta -- spec section 73's priority order) runs,
//! this is the fallback pass over a page's own text. It produces *candidate*
//! assertions, not facts -- the orchestrator in `crawler.rs` records them
//! with `ExtractionMethod::LlmExtraction`, a per-candidate confidence, and a
//! distinct extractor actor (named after the model) so the required
//! provenance (extractor identity, model, source, evidence, timestamp,
//! confidence) is exactly the same machinery `assert()` already provides for
//! every other extraction method -- not a parallel bookkeeping path.
//!
//! Disabled by default (spec section 11: OAG requires no external service).
//! `DisabledExtractor` returns no candidates rather than an error: the
//! crawler calls this unconditionally as an optional extra pass on every
//! crawl, not something a caller opts into per-call.

use async_trait::async_trait;
use serde::Deserialize;

use crate::error::CrawlError;

/// One candidate relationship an LLM found in a page's text. Never asserted
/// directly -- `crawler.rs` turns each into an `AssertInput` with
/// `extraction_method: LlmExtraction` and `actor_confidence: confidence`.
#[derive(Debug, Clone, PartialEq)]
pub struct CandidateAssertion {
    pub subject: String,
    pub predicate: String,
    pub object: String,
    pub confidence: f32,
}

#[async_trait]
pub trait LlmExtractor: Send + Sync {
    async fn extract(&self, page_text: &str, source_url: &str) -> Result<Vec<CandidateAssertion>, CrawlError>;

    /// Short identifier for the actor declared to own whatever candidates
    /// this extractor produces (spec section 74's "extractor identity").
    /// `""` (as `DisabledExtractor` returns) means no actor is ever needed,
    /// since `extract` never returns a non-empty candidate list.
    fn model_name(&self) -> &str;
}

/// Default extractor. Always returns zero candidates rather than an error
/// -- unlike `oag_embeddings::DisabledProvider`, this is called
/// unconditionally on every crawl, not from an explicit opt-in command, so
/// "disabled" must mean "silently contributes nothing" rather than a typed
/// failure the caller has to handle.
#[derive(Debug, Default, Clone, Copy)]
pub struct DisabledExtractor;

#[async_trait]
impl LlmExtractor for DisabledExtractor {
    async fn extract(&self, _page_text: &str, _source_url: &str) -> Result<Vec<CandidateAssertion>, CrawlError> {
        Ok(Vec::new())
    }

    fn model_name(&self) -> &str {
        ""
    }
}

/// Talks to any server exposing the OpenAI chat-completions HTTP shape --
/// covers literal OpenAI as well as Ollama, LM Studio, vLLM, and other
/// self-hosted servers implementing the same wire shape (same reasoning as
/// `oag_embeddings::OpenAiCompatibleProvider`).
pub struct OpenAiCompatibleExtractor {
    client: reqwest::Client,
    base_url: String,
    api_key: Option<String>,
    model: String,
}

impl OpenAiCompatibleExtractor {
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
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Debug, Deserialize)]
struct ChatMessage {
    content: String,
}

#[derive(Debug, Deserialize)]
struct RawCandidate {
    subject: String,
    predicate: String,
    object: String,
    #[serde(default)]
    confidence: f32,
}

fn build_prompt(page_text: &str, source_url: &str) -> String {
    format!(
        "You extract factual subject-predicate-object relationships from web page \
         content. The page is at {source_url}.\n\n\
         Respond with ONLY a JSON array (no markdown fences, no commentary). Each \
         element must be an object with exactly these keys: \"subject\" (string), \
         \"predicate\" (a short snake_case verb phrase), \"object\" (string), and \
         \"confidence\" (a number from 0.0 to 1.0 reflecting how directly the text \
         states this). If nothing can be confidently extracted, respond with [].\n\n\
         Page content:\n{page_text}"
    )
}

/// Best-effort parse of a chat model's response content into candidates.
/// A malformed or non-JSON response is an expected outcome of talking to a
/// language model (not a bug) -- treated the same as "found nothing" rather
/// than a hard error, so one bad response doesn't fail the whole crawl.
fn parse_candidates(content: &str) -> Vec<CandidateAssertion> {
    let trimmed = content.trim().trim_start_matches("```json").trim_start_matches("```").trim_end_matches("```").trim();
    let raw: Vec<RawCandidate> = match serde_json::from_str(trimmed) {
        Ok(candidates) => candidates,
        Err(e) => {
            tracing::warn!(error = %e, "llm extraction response was not valid JSON, treating as no candidates");
            return Vec::new();
        }
    };
    raw.into_iter()
        .filter(|c| !c.subject.trim().is_empty() && !c.predicate.trim().is_empty() && !c.object.trim().is_empty())
        .map(|c| CandidateAssertion {
            subject: c.subject,
            predicate: c.predicate,
            object: c.object,
            confidence: c.confidence.clamp(0.0, 1.0),
        })
        .collect()
}

#[async_trait]
impl LlmExtractor for OpenAiCompatibleExtractor {
    async fn extract(&self, page_text: &str, source_url: &str) -> Result<Vec<CandidateAssertion>, CrawlError> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let mut request = self.client.post(url).json(&serde_json::json!({
            "model": self.model,
            "temperature": 0.0,
            "messages": [{"role": "user", "content": build_prompt(page_text, source_url)}],
        }));
        if let Some(api_key) = &self.api_key {
            request = request.bearer_auth(api_key);
        }
        let response = request.send().await?.error_for_status()?;
        let body: ChatResponse = response.json().await?;
        let content = body.choices.into_iter().next().map(|c| c.message.content).unwrap_or_default();
        Ok(parse_candidates(&content))
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
    async fn disabled_extractor_always_returns_empty() {
        let extractor = DisabledExtractor;
        let candidates = extractor.extract("some text", "https://example.com").await.unwrap();
        assert!(candidates.is_empty());
        assert_eq!(extractor.model_name(), "");
    }

    #[test]
    fn parse_candidates_reads_plain_json_array() {
        let content = r#"[{"subject": "https://example.com", "predicate": "made_by", "object": "Example Corp", "confidence": 0.8}]"#;
        let candidates = parse_candidates(content);
        assert_eq!(
            candidates,
            vec![CandidateAssertion {
                subject: "https://example.com".to_string(),
                predicate: "made_by".to_string(),
                object: "Example Corp".to_string(),
                confidence: 0.8,
            }]
        );
    }

    #[test]
    fn parse_candidates_strips_markdown_code_fences() {
        let content = "```json\n[{\"subject\": \"a\", \"predicate\": \"b\", \"object\": \"c\", \"confidence\": 1.5}]\n```";
        let candidates = parse_candidates(content);
        // confidence clamped into [0.0, 1.0]
        assert_eq!(candidates[0].confidence, 1.0);
    }

    #[test]
    fn parse_candidates_treats_malformed_json_as_empty() {
        assert!(parse_candidates("not json at all").is_empty());
    }

    #[test]
    fn parse_candidates_drops_entries_with_empty_fields() {
        let content = r#"[{"subject": "", "predicate": "b", "object": "c", "confidence": 0.5}]"#;
        assert!(parse_candidates(content).is_empty());
    }

    #[test]
    fn parse_candidates_empty_array_is_empty() {
        assert!(parse_candidates("[]").is_empty());
    }

    #[tokio::test]
    async fn openai_compatible_extractor_parses_real_http_response() {
        let app = Router::new().route(
            "/chat/completions",
            post(|Json(body): Json<Value>| async move {
                assert_eq!(body["model"], "test-model");
                Json(json!({
                    "choices": [{
                        "message": {
                            "content": "[{\"subject\": \"https://example.com\", \"predicate\": \"instance_of\", \"object\": \"concept:website\", \"confidence\": 0.6}]"
                        }
                    }]
                }))
            }),
        );
        let base_url = spawn_fixture(app).await;

        let extractor = OpenAiCompatibleExtractor::new(base_url, None, "test-model");
        let candidates = extractor.extract("hello world", "https://example.com").await.unwrap();

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].subject, "https://example.com");
        assert_eq!(extractor.model_name(), "test-model");
    }

    #[tokio::test]
    async fn openai_compatible_extractor_sends_bearer_auth_header_when_api_key_set() {
        let app = Router::new().route(
            "/chat/completions",
            post(|headers: axum::http::HeaderMap, Json(_body): Json<Value>| async move {
                assert_eq!(headers.get("authorization").unwrap(), "Bearer secret-key");
                Json(json!({"choices": [{"message": {"content": "[]"}}]}))
            }),
        );
        let base_url = spawn_fixture(app).await;

        let extractor = OpenAiCompatibleExtractor::new(base_url, Some("secret-key".to_string()), "test-model");
        extractor.extract("hi", "https://example.com").await.unwrap();
    }

    #[tokio::test]
    async fn openai_compatible_extractor_http_error_status_is_a_typed_error() {
        let app = Router::new().route(
            "/chat/completions",
            post(|| async { axum::http::StatusCode::INTERNAL_SERVER_ERROR }),
        );
        let base_url = spawn_fixture(app).await;

        let extractor = OpenAiCompatibleExtractor::new(base_url, None, "test-model");
        let err = extractor.extract("hi", "https://example.com").await.unwrap_err();
        assert!(matches!(err, CrawlError::Http(_)));
    }
}
