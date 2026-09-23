#[derive(Debug, thiserror::Error)]
pub enum GraphError {
    #[error(transparent)]
    Storage(#[from] oag_storage::StorageError),
    #[error(transparent)]
    Events(#[from] oag_events::EventsError),
    #[error("permission denied: requires {0}")]
    PermissionDenied(&'static str),
    #[error("invalid api key")]
    InvalidApiKey,
    #[error("not found: {0}")]
    NotFound(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
}
