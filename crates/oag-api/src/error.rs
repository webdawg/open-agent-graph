use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use oag_graph::GraphError;
use serde_json::json;

pub struct ApiError(pub GraphError);

impl From<GraphError> for ApiError {
    fn from(err: GraphError) -> Self {
        Self(err)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match &self.0 {
            GraphError::InvalidApiKey => (StatusCode::UNAUTHORIZED, self.0.to_string()),
            GraphError::PermissionDenied(_) => (StatusCode::FORBIDDEN, self.0.to_string()),
            GraphError::NotFound(_) => (StatusCode::NOT_FOUND, self.0.to_string()),
            GraphError::InvalidInput(_) => (StatusCode::BAD_REQUEST, self.0.to_string()),
            GraphError::Events(oag_events::EventsError::NotFound(_)) => {
                (StatusCode::NOT_FOUND, self.0.to_string())
            }
            GraphError::Storage(_) | GraphError::Events(_) => {
                tracing::error!(error = %self.0, "internal error");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal error".to_string())
            }
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
}

pub type ApiResult<T> = Result<T, ApiError>;
