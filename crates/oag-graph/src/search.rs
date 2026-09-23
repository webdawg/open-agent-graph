use oag_core::Node;
use oag_storage::repo::nodes;

use crate::error::GraphError;
use crate::service::GraphService;

/// Sanitize free text into an FTS5 `MATCH` query: strip characters that are
/// syntax in FTS5's query language so arbitrary user input can't produce a
/// syntax error (or, worse, an unintended column-filter/operator query).
fn sanitize_fts_query(raw: &str) -> String {
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
        let mut conn = self
            .pool()
            .acquire()
            .await
            .map_err(oag_storage::StorageError::from)?;
        Ok(nodes::search(&mut conn, &sanitized, limit).await?)
    }
}
