use oag_core::Node;
use oag_storage::repo::nodes;
pub use oag_storage::repo::nodes::SearchCursor;

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

/// One page of a keyset-paginated search.
#[derive(Debug, Clone)]
pub struct SearchPage {
    pub results: Vec<Node>,
    /// Opaque cursor for the next page, or `None` when this page reached the end of the
    /// result set. Pass it back as-is; it is not meaningful to interpret.
    pub next_cursor: Option<String>,
}

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
    pub async fn search(&self, query: &str, limit: i64, offset: i64) -> Result<Vec<Node>, GraphError> {
        let sanitized = sanitize_fts_query(query);
        if sanitized.is_empty() {
            return Ok(Vec::new());
        }
        let limit = limit.clamp(0, MAX_SEARCH_LIMIT);
        // Negative offset is clamped rather than rejected: the pagination
        // surfaces treat "unset" as 0, and a caller passing a negative value
        // almost certainly meant the start, not an error mid-response. Kept
        // at this one service layer so REST, MCP, CLI, and the Human
        // Interface all behave identically.
        let offset = offset.max(0);
        let mut conn = self
            .pool()
            .acquire()
            .await
            .map_err(oag_storage::StorageError::from)?;
        let started = std::time::Instant::now();
        let results = nodes::search(&mut conn, &sanitized, limit, offset).await?;
        self.search_latency.record(started.elapsed().as_secs_f64() * 1_000.0);
        Ok(results)
    }

    /// One page of results on either pagination contract.
    ///
    /// `cursor` wins when both are supplied: `offset` counts rows from the start and `cursor`
    /// names the last row seen, so applying both would describe a page neither promises. Both
    /// REST and MCP call this rather than each reimplementing the choice, so the two surfaces
    /// cannot drift apart.
    pub async fn search_page(
        &self,
        query: &str,
        limit: i64,
        offset: i64,
        cursor: Option<&str>,
    ) -> Result<SearchPage, GraphError> {
        match cursor {
            Some(cursor) => self.search_after(query, limit, Some(cursor)).await,
            None => {
                let results = self.search(query, limit, offset).await?;
                // Offer a cursor for the next page even on the offset path, so a caller can
                // switch to the stronger contract mid-walk instead of starting over. A full
                // page means there may be more; a short page means there is not.
                let next_cursor = if results.len() as i64 == limit.max(0) {
                    results.last().map(|node| SearchCursor { node_id: node.id }.encode())
                } else {
                    None
                };
                Ok(SearchPage { results, next_cursor })
            }
        }
    }

    /// Keyset-paginated search: the `limit` matches that follow `cursor`.
    ///
    /// This is the path a caller should use to walk a result set that may change underneath
    /// it. [`Self::search`]'s `OFFSET` counts rows from the start, so a document arriving
    /// that outranks the current page shifts every later offset and the walk repeats or skips
    /// a row -- `search_cursor_walk_covers_every_match_once_across_an_insertion` demonstrates
    /// it. A cursor names the last row the caller saw, so the boundary moves with the data.
    ///
    /// `cursor` is the [`SearchPage::next_cursor`] from the previous call, or `None` for the
    /// first page. A cursor from a different query is not meaningful: FTS5's `rank` is
    /// computed per query, so the row it names may not even match. Passing one is not an
    /// error, but the page it produces is not guaranteed to be the one the caller expected.
    pub async fn search_after(
        &self,
        query: &str,
        limit: i64,
        cursor: Option<&str>,
    ) -> Result<SearchPage, GraphError> {
        let sanitized = sanitize_fts_query(query);
        if sanitized.is_empty() {
            return Ok(SearchPage { results: Vec::new(), next_cursor: None });
        }
        let limit = limit.clamp(0, MAX_SEARCH_LIMIT);
        // Fetch one extra row to learn whether another page exists without a second query.
        // Without it a caller cannot distinguish "last page" from "page that happens to be
        // full", and would have to ask once more to find out.
        let fetch = limit.saturating_add(1);
        // A malformed cursor is a caller bug, not a server fault: start over rather than
        // erroring, so a truncated or hand-edited token degrades to a fresh search instead
        // of a failure the caller cannot correct mid-walk.
        let cursor = cursor.and_then(SearchCursor::decode);
        let mut conn = self
            .pool()
            .acquire()
            .await
            .map_err(oag_storage::StorageError::from)?;
        let started = std::time::Instant::now();
        let rows = nodes::search_after(&mut conn, &sanitized, fetch, cursor).await?;
        self.search_latency.record(started.elapsed().as_secs_f64() * 1_000.0);
        let has_more = rows.len() as i64 > limit;
        let rows: Vec<_> = rows.into_iter().take(limit.max(0) as usize).collect();
        let next_cursor = if has_more {
            rows.last().map(|(node, _)| SearchCursor { node_id: node.id }.encode())
        } else {
            None
        };
        Ok(SearchPage {
            results: rows.into_iter().map(|(node, _)| node).collect(),
            next_cursor,
        })
    }
}
