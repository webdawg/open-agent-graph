//! `GET /ipfs-site/{cid}` and `GET /ipfs-site/_bundle.js` (spec/29-ipfs-pinning.md,
//! "static page hosting" -- `USER_INPUT_RECORD.md` Entry 19/this session).
//!
//! `oag serve` never fetches from IPFS itself here -- these two routes only
//! ever hand a browser a CID (plus the operator's optional direct dial
//! target) and a vendored bundle; the browser does 100% of the real
//! content retrieval, purely over bitswap/libp2p, never through an HTTP
//! gateway/proxy (see `crates/oag-api/assets/ipfs-site-bundle.src.js`).
//! Deliberately unauthenticated, unlike `/ui/*`: a hosted site is meant to
//! be publicly viewable like any other website, not gated behind a
//! `graph:read` API key a visitor would have no way to obtain.

use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Response};

use crate::state::AppState;

const BUNDLE_JS: &[u8] = include_bytes!("../assets/ipfs-site-bundle.js");

pub async fn bundle_js() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/javascript; charset=utf-8")],
        BUNDLE_JS,
    )
}

/// A real CID (base58btc `Qm...` or base32/base36 CIDv1) is always plain
/// ASCII alphanumeric -- reject anything else outright rather than trying
/// to safely escape an arbitrary path segment into a `<script>` body.
fn looks_like_a_cid(cid: &str) -> bool {
    !cid.is_empty() && cid.len() <= 100 && cid.chars().all(|c| c.is_ascii_alphanumeric())
}

pub async fn bootstrap_page(Path(cid): Path<String>, State(state): State<AppState>) -> Response {
    if !looks_like_a_cid(&cid) {
        return (StatusCode::BAD_REQUEST, Html("<p>not a valid CID</p>".to_string())).into_response();
    }
    // `serde_json::to_string` escapes quotes/backslashes/control characters
    // correctly for a JS string literal; the extra "</" guard stops a
    // (structurally impossible, given `looks_like_a_cid` above, but cheap
    // to guard anyway) value from prematurely closing this <script> tag.
    let cid_js = serde_json::to_string(&cid).unwrap_or_else(|_| "null".to_string()).replace("</", "<\\/");
    let multiaddr_js = match &state.ipfs_libp2p_ws_multiaddr {
        Some(multiaddr) => serde_json::to_string(multiaddr.as_ref())
            .unwrap_or_else(|_| "null".to_string())
            .replace("</", "<\\/"),
        None => "null".to_string(),
    };
    let html = format!(
        r#"<!doctype html>
<html>
<head>
<meta charset="utf-8">
<title>oag ipfs site</title>
<script>
  window.__OAG_IPFS_CID__ = {cid_js};
  window.__OAG_IPFS_MULTIADDR__ = {multiaddr_js};
</script>
</head>
<body>
<div id="oag-ipfs-root">loading {cid} over ipfs...</div>
<script type="module" src="/ipfs-site/_bundle.js"></script>
</body>
</html>
"#
    );
    Html(html).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_cid_looking_path_segments() {
        assert!(looks_like_a_cid("bafybeih5crpmkgaewasbbss3nbrdjxdhyilswd2otxqtoiqwfdmbej7v3i"));
        assert!(looks_like_a_cid("QmTestCid123"));
        assert!(!looks_like_a_cid(""));
        assert!(!looks_like_a_cid("../../etc/passwd"));
        assert!(!looks_like_a_cid("<script>alert(1)</script>"));
        assert!(!looks_like_a_cid(&"Q".repeat(101)));
    }
}
