use url::Url;

#[derive(Debug, thiserror::Error)]
pub enum CanonicalizeError {
    #[error("could not parse URL: {0}")]
    InvalidUrl(#[from] url::ParseError),
}

/// Tracking parameters stripped during canonicalization (spec section 9).
/// Deliberately narrow — the spec warns against aggressively collapsing URLs
/// when semantic equivalence is uncertain.
const TRACKING_PARAMS: &[&str] = &[
    "utm_source",
    "utm_medium",
    "utm_campaign",
    "utm_term",
    "utm_content",
    "gclid",
    "fbclid",
];

/// Normalize a URL per spec section 9: lowercase scheme/host, strip default
/// ports, strip empty paths down to `/`, resolve dot-segments (handled by the
/// `url` crate's parser), and drop known tracking parameters. The caller is
/// responsible for keeping both `original_url` and this canonical form.
pub fn canonicalize_url(raw: &str) -> Result<String, CanonicalizeError> {
    let mut url = Url::parse(raw)?;

    // Scheme/host case and default-port stripping, dot-segment resolution are
    // all handled by `Url::parse` itself. We additionally strip tracking
    // params and empty fragments.
    if url.path().is_empty() {
        url.set_path("/");
    }

    let filtered_query: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| !TRACKING_PARAMS.contains(&k.as_ref()))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();

    if filtered_query.is_empty() {
        url.set_query(None);
    } else {
        let query_string = filtered_query
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join("&");
        url.set_query(Some(&query_string));
    }

    Ok(url.to_string())
}

/// Build the canonical identifier string a [`crate::ids::NodeId`] is derived
/// from, for identifier kinds that have a stable canonical form (spec section
/// 15): `url:`, `domain:`, `urn:`, `package:`, `doi:`, `isbn:`, `github:`,
/// `concept:`, `agent:`, `peer:`.
pub fn canonical_identifier(kind: &str, value: &str) -> String {
    format!("{}:{}", kind.trim().to_lowercase(), value.trim())
}

/// Build a canonical identifier for an entity with no stable canonical URI
/// (spec section 15) — it gets a freshly generated UUIDv7 instead.
pub fn generate_uuid_identifier() -> String {
    format!("uuid:{}", uuid::Uuid::now_v7())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_tracking_params() {
        let got = canonicalize_url("https://Example.com/Path?utm_source=x&keep=1").unwrap();
        assert_eq!(got, "https://example.com/Path?keep=1");
    }

    #[test]
    fn empty_path_becomes_slash() {
        let got = canonicalize_url("https://example.com").unwrap();
        assert_eq!(got, "https://example.com/");
    }

    #[test]
    fn strips_default_port() {
        let got = canonicalize_url("https://example.com:443/foo").unwrap();
        assert_eq!(got, "https://example.com/foo");
    }

    #[test]
    fn deterministic() {
        let a = canonicalize_url("https://example.com/a?utm_campaign=x&z=1&a=2").unwrap();
        let b = canonicalize_url("https://example.com/a?utm_campaign=y&z=1&a=2").unwrap();
        assert_eq!(a, b);
    }
}
