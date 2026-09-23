#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
    #[error(transparent)]
    Migrate(#[from] sqlx::migrate::MigrateError),
    #[error("corrupt id column: expected 32 bytes, got {0}")]
    BadIdLength(usize),
    #[error("corrupt metadata JSON: {0}")]
    BadMetadata(#[from] serde_json::Error),
    #[error("unknown {0} value in database: {1}")]
    UnknownEnumValue(&'static str, String),
    #[error("row not found")]
    NotFound,
}

pub fn bytes_to_array(bytes: &[u8]) -> Result<[u8; 32], StorageError> {
    bytes
        .try_into()
        .map_err(|_| StorageError::BadIdLength(bytes.len()))
}
