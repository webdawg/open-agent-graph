//! `POST /api/v1/crawl` (spec section 71): lets a remote REST/MCP caller
//! trigger this peer's crawler, rather than crawling being CLI-only. Gated
//! behind `Permission::GraphCrawl`, not `graph:assert` -- crawling makes
//! *this peer* issue outbound HTTP requests to a caller-supplied URL, a
//! meaningfully different risk than authoring a claim, so an operator must
//! grant it explicitly. The server's own SSRF/private-network policy
//! (`[crawler]` in config.toml) is never overridable per-request -- there is
//! deliberately no `allow_private_networks` field on `CrawlRequest`.

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use oag_core::Permission;
use oag_graph::GraphError;
use serde::Deserialize;
use serde_json::json;

use crate::auth::authenticate;
use crate::state::AppState;

pub struct CrawlApiError(StatusCode, String);

impl IntoResponse for CrawlApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

impl From<GraphError> for CrawlApiError {
    fn from(e: GraphError) -> Self {
        match e {
            GraphError::InvalidApiKey => Self(StatusCode::UNAUTHORIZED, e.to_string()),
            GraphError::PermissionDenied(_) => Self(StatusCode::FORBIDDEN, e.to_string()),
            other => Self(StatusCode::INTERNAL_SERVER_ERROR, other.to_string()),
        }
    }
}

impl From<crate::error::ApiError> for CrawlApiError {
    fn from(e: crate::error::ApiError) -> Self {
        Self::from(e.0)
    }
}

impl From<oag_crawler::error::CrawlError> for CrawlApiError {
    fn from(e: oag_crawler::error::CrawlError) -> Self {
        use oag_crawler::error::CrawlError;
        match e {
            CrawlError::UnsupportedScheme(_) | CrawlError::NoHost => Self(StatusCode::BAD_REQUEST, e.to_string()),
            CrawlError::BlockedAddress(_) | CrawlError::RobotsDisallowed => {
                Self(StatusCode::FORBIDDEN, e.to_string())
            }
            CrawlError::DnsResolutionFailed(..)
            | CrawlError::NoAddresses(_)
            | CrawlError::TooManyRedirects(_)
            | CrawlError::RedirectWithoutLocation
            | CrawlError::ResponseTooLarge(_)
            | CrawlError::UnsupportedContentType(_)
            | CrawlError::Http(_) => Self(StatusCode::BAD_GATEWAY, e.to_string()),
            CrawlError::ClientBuild(_) | CrawlError::Graph(_) => {
                tracing::error!(error = %e, "internal error during REST-triggered crawl");
                Self(StatusCode::INTERNAL_SERVER_ERROR, "internal error".to_string())
            }
        }
    }
}

#[derive(Deserialize)]
pub struct CrawlRequest {
    pub url: String,
}

pub async fn crawl(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CrawlRequest>,
) -> Result<Json<serde_json::Value>, CrawlApiError> {
    let auth = authenticate(&headers, &state.graph).await?;
    auth.require(Permission::GraphCrawl)?;

    let url = url::Url::parse(&body.url)
        .map_err(|e| CrawlApiError(StatusCode::BAD_REQUEST, format!("invalid url '{}': {e}", body.url)))?;

    let summary = state.crawler.crawl(&url).await?;
    Ok(Json(serde_json::to_value(summary).map_err(|e| CrawlApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?))
}
