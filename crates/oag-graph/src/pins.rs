//! Content this peer has pinned, keyed by IPFS CID.
//!
//! This is the discovery primitive the project's relay and gateway work names as missing: "how
//! would a peer even learn *which* upstream peer has a given CID without being told?" A caller can
//! now ask a peer whether it holds a CID instead of being told whom to ask.
//!
//! **Every recorded pin path lands here**, so the answer is not limited to one source:
//!
//! - `evidence` -- the assert/evidence path, which is how the crawler's page pin lands.
//! - `cli-pin` -- `oag ipfs pin`.
//! - `backup-pin` -- `oag backup --pin`.
//! - `host-page` -- `oag ipfs host-page`.
//!
//! Pins made before the `pinned_cids` table existed are backfilled by migration `0015` from the one
//! place a CID was already recorded (`evidence.metadata`'s `ipfs_cid` key), so a long-running peer
//! does not report an empty pin list after upgrading.
//!
//! **A record, not a live check.** Nothing here fetches the content to confirm it is still
//! retrievable. A peer that once pinned a CID and has since garbage-collected it would still
//! answer "yes". That is the same honesty property as a DNS name: it is a pointer, not a
//! guarantee, and the fetch is where the guarantee is tested.

use oag_storage::repo::pinned_cids::{self, DEFAULT_LIMIT};

use crate::assert::{check_len, MAX_PINNED_CID_LEN};
use crate::error::GraphError;
use crate::service::GraphService;

/// Default page size for `list_pins`, for surfaces that should not invent their own.
pub fn pins_limit_default() -> i64 {
    DEFAULT_LIMIT
}

impl GraphService {
    /// Records that this peer pinned `cid`. Idempotent per CID, so a re-pin updates the timestamp
    /// rather than creating a second entry.
    pub async fn record_pin(&self, cid: &str, source: &str) -> Result<(), GraphError> {
        check_len("cid", cid, MAX_PINNED_CID_LEN)?;
        let mut conn = self.pool().acquire().await.map_err(oag_storage::StorageError::from)?;
        pinned_cids::record(&mut conn, cid, source, self.now()).await?;
        Ok(())
    }

    /// CIDs this peer has pinned, most recent first, bounded by `limit`.
    ///
    /// `limit` is clamped to `[0, MAX_LIMIT]` at this one service layer, matching
    /// `GraphService::search`: a caller must not be able to make this peer serialize its entire
    /// recorded pin set in one response.
    pub async fn list_pins(&self, limit: i64) -> Result<Vec<String>, GraphError> {
        let limit = limit.clamp(0, pinned_cids::MAX_LIMIT);
        let mut conn = self.pool().acquire().await.map_err(oag_storage::StorageError::from)?;
        Ok(pinned_cids::list(&mut conn, limit).await?.into_iter().map(|(cid, _)| cid).collect())
    }

    /// Whether this peer has recorded pinning `cid`. The discovery query: a relay (or any caller)
    /// can ask a peer directly rather than trying peers in configured order.
    pub async fn holds_pin(&self, cid: &str) -> Result<bool, GraphError> {
        let mut conn = self.pool().acquire().await.map_err(oag_storage::StorageError::from)?;
        Ok(pinned_cids::holds(&mut conn, cid).await?)
    }

    /// How many distinct CIDs this peer has pinned.
    pub async fn pin_count(&self) -> Result<i64, GraphError> {
        let mut conn = self.pool().acquire().await.map_err(oag_storage::StorageError::from)?;
        Ok(pinned_cids::count(&mut conn).await?)
    }
}
