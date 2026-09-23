use axum::http::HeaderMap;
use oag_core::Permission;
use oag_graph::{AuthContext, GraphError, GraphService};

use crate::error::ApiError;

/// Extract and validate the `Authorization: Bearer <key>` header, using the
/// same API-key auth layer for both REST and MCP (spec section 40).
pub async fn authenticate(headers: &HeaderMap, graph: &GraphService) -> Result<AuthContext, ApiError> {
    let raw = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or(GraphError::InvalidApiKey)?;
    Ok(graph.authenticate(raw).await?)
}

/// Convenience for read endpoints: authenticate, then require `graph:read`.
pub async fn authenticate_read(headers: &HeaderMap, graph: &GraphService) -> Result<AuthContext, ApiError> {
    let auth = authenticate(headers, graph).await?;
    auth.require(Permission::GraphRead)?;
    Ok(auth)
}
