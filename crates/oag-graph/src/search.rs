use oag_core::Node;
use oag_storage::repo::nodes;

use crate::error::GraphError;
use crate::service::GraphService;

/// Spec section 61 ("disk exhaustion" / unbounded-response defenses): SQLite
/// treats a negative `LIMIT` as "no limit at all" (confirmed live: `limit=-1`
/// over REST returned every matching row, not zero) -- a caller could
/// otherwise force this peer to serialize its entire matching node set in
/// one response regardless of what they asked for, with no size cap on the
/// way out the door. Clamped at the one service layer every surface
/// (REST/MCP/CLI) already calls through, rather than at each surface
/// separately.
const MAX_SEARCH_LIMIT: i64 = 1000;

/// Sanitize free text into an FTS5 `MATCH` query: strip characters that are
/// syntax in FTS5's query language so arbitrary user input can't produce a
/// syntax error (or, worse, an unintended column-filter/operator query).
pub(crate) fn sanitize_fts_query(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| if c.is_alphanumeric() || c.is_whitespace() { c } else { ' ' })
        .collect();
    cleaned
        .split_whitespace()
        .map(|word| format!("\"{word}\""))
        .collect::<Vec<_>>()
        .join(" ")
}

impl GraphService {
    /// Relevance-only search over node name/description (spec sections 63,
    /// 65 — the other ranking signals need corroboration data this
    /// milestone doesn't produce; see plan's "search for this milestone"
    /// note).
    pub async fn search(&self, query: &str, limit: i64) -> Result<Vec<Node>, GraphError> {
        let sanitized = sanitize_fts_query(query);
        if sanitized.is_empty() {
            return Ok(Vec::new());
        }
        let limit = limit.clamp(0, MAX_SEARCH_LIMIT);
        let mut conn = self
            .pool()
            .acquire()
            .await
            .map_err(oag_storage::StorageError::from)?;
        Ok(nodes::search(&mut conn, &sanitized, limit).await?)
    }
}
