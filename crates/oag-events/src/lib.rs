pub mod builder;
pub mod commit;
pub mod envelope;
pub mod error;
pub mod payload;
pub mod projector;
#[cfg(test)]
mod tests;
pub mod validate;

pub use builder::build_and_sign;
pub use commit::commit_local_event;
pub use envelope::{SignedEvent, UnsignedEvent};
pub use error::EventsError;
pub use payload::*;
pub use projector::ProjectionOutcome;
pub use validate::{validate_chain, verify_and_derive_id};
