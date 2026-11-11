//! Content this peer has recorded holding, keyed by IPFS CID.
//!
//! This is the discovery primitive the project's relay and gateway work names as missing: "how
//! would a peer even learn *which* upstream peer has a given CID without being told?" A caller can
//! now ask a peer whether it holds a CID instead of being told whom to ask.
//!
//! **What "holds" means here, stated plainly.** The only CID this codebase records today is the
//! projector's `{"ipfs_cid": ...}` key in `evidence.metadata`, written when the crawler pins a
//! page. So this answers "which CIDs has this peer recorded pinning *as evidence*". Pins made by
//! `oag ipfs pin`, `oag backup --pin`, or `oag ipfs host-page` are not recorded anywhere
//! queryable yet -- that gap is recorded rather than papered over, and until it is closed this
//! surface under-reports what a peer's IPFS node actually holds.
//!
//! **A record, not a live check.** Nothing here fetches the content to confirm it is still
//! retrievable. A peer that once pinned a CID and has since garbage-collected it would still
//! answer "yes". That is the same honesty property as a DNS name: it is a pointer, not a
//! guarantee, and the fetch is where the guarantee is tested.

use oag_storage::repo::{pins as pins_repo, pins::DEFAULT_PIN_LIST_LIMIT};

/// Default page size for `GraphService::list_pins`, for surfaces that should not invent their own.
pub fn pins_limit_default() -> i64 {
    DEFAULT_PIN_LIST_LIMIT
}

use crate::error::GraphError;
use crate::service::GraphService;

impl GraphService {
    /// CIDs this peer has recorded pinning, most recent first, bounded by `limit`.
    ///
    /// `limit` is clamped to `[0, MAX_PIN_LIST_LIMIT]` at this one service layer, matching
    /// `GraphService::search`: a caller must not be able to make this peer serialize its entire
    /// recorded pin set in one response.
    pub async fn list_pins(&self, limit: i64) -> Result<Vec<String>, GraphError> {
        let limit = limit.clamp(0, pins_repo::MAX_PIN_LIST_LIMIT);
        let mut conn = self.pool().acquire().await.map_err(oag_storage::StorageError::from)?;
        Ok(pins_repo::list_pins(&mut conn, limit).await?)
    }

    /// Whether this peer has recorded pinning `cid`. The discovery query: a relay (or any caller)
    /// can ask a peer directly rather than trying peers in configured order.
    pub async fn holds_pin(&self, cid: &str) -> Result<bool, GraphError> {
        let mut conn = self.pool().acquire().await.map_err(oag_storage::StorageError::from)?;
        Ok(pins_repo::holds_pin(&mut conn, cid).await?)
    }

    /// How many distinct CIDs this peer has recorded.
    pub async fn pin_count(&self) -> Result<i64, GraphError> {
        let mut conn = self.pool().acquire().await.map_err(oag_storage::StorageError::from)?;
        Ok(pins_repo::count_pins(&mut conn).await?)
    }
}
