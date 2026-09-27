//! Embedding generation and vector ranking for semantic search (spec section
//! 64). Kept as its own leaf crate — like `oag-crawler`/`oag-petname` — so
//! `oag-graph` doesn't pay for an HTTP client dependency when embeddings are
//! disabled, which is the default and fully-supported state.

pub mod error;
pub mod provider;
pub mod similarity;

pub use error::EmbeddingError;
pub use provider::{DisabledProvider, EmbeddingProvider, OpenAiCompatibleProvider};
pub use similarity::{cosine_similarity, rank};
