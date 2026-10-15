//! Native IPFS pinning/retrieval for evidence content (see
//! `spec/29-ipfs-pinning.md`). Kept as its own leaf crate — like
//! `oag-embeddings` — so callers that don't need IPFS (the default) don't
//! pay for the HTTP client dependency, and so `oag-cli` can use it directly
//! for `oag ipfs cat` without depending on all of `oag-crawler`.

pub mod embedded;
pub mod error;
pub mod provider;

pub use embedded::EmbeddedIpfsProvider;
pub use error::IpfsError;
pub use provider::{DisabledProvider, KuboHttpProvider, PinningProvider};
