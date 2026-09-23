#[derive(Debug, thiserror::Error)]
pub enum EventsError {
    #[error("canonicalization/serialization failed: {0}")]
    Canonicalize(#[from] serde_json::Error),
    #[error("invalid signature: {0}")]
    InvalidSignature(#[from] oag_crypto::SignatureError),
    #[error("invalid hex in event field: {0}")]
    Hex(#[from] hex::FromHexError),
    #[error("invalid id in event field: {0}")]
    Id(#[from] oag_core::ids::Hash32ParseError),
    #[error("chain error: expected sequence {expected}, got {got}")]
    SequenceGap { expected: u64, got: u64 },
    #[error("chain error: previous_event mismatch (expected head {expected:?}, event declared {declared:?})")]
    PreviousEventMismatch {
        expected: Option<String>,
        declared: Option<String>,
    },
    #[error("unknown extraction_method '{0}' in event payload")]
    UnknownExtractionMethod(String),
    #[error("signature must be 64 bytes, got {0}")]
    BadSignatureLength(usize),
    #[error(transparent)]
    Storage(#[from] oag_storage::StorageError),
    #[error("referenced object not found: {0}")]
    NotFound(String),
}
