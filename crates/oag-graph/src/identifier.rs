/// Turn a caller-supplied subject/object/search value into the canonical
/// identifier string a [`oag_core::NodeId`] is derived from (spec sections
/// 14-15). Shared between `resolve` and `assert` so the same input always
/// maps to the same node both ways.
///
/// - Looks like a URL -> `url:<canonicalized>`.
/// - Already has an identifier-scheme prefix (`concept:`, `urn:`,
///   `package:`, ...) with no spaces -> used as-is.
/// - Otherwise -> slugified and treated as a `concept:` identifier, e.g.
///   `"Model Context Protocol"` -> `concept:model-context-protocol`.
pub fn canonicalize_value(value: &str) -> (String, Option<&'static str>) {
    let trimmed = value.trim();

    if let Some((scheme, _)) = trimmed.split_once(':') {
        // Only http(s) get real URL canonicalization — the `url` crate
        // happily parses `concept:model-context-protocol` as an opaque-path
        // URL with scheme "concept", which would double-prefix it.
        if scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https") {
            if let Ok(canonical_url) = oag_core::canonicalize_url(trimmed) {
                return (
                    oag_core::canonical_identifier("url", &canonical_url),
                    Some("url"),
                );
            }
        } else if !scheme.is_empty()
            && !trimmed.contains(' ')
            && scheme.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            // Already a canonical-style identifier: concept:, urn:, package:,
            // doi:, isbn:, github:, agent:, peer:, domain:, ...
            return (trimmed.to_lowercase(), None);
        }
    }

    let slug = trimmed
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-");
    (oag_core::canonical_identifier("concept", &slug), Some("concept"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_gets_url_prefix() {
        let (id, _) = canonicalize_value("https://Example.com/Path?utm_source=x");
        assert_eq!(id, "url:https://example.com/Path");
    }

    #[test]
    fn existing_scheme_passthrough() {
        let (id, kind) = canonicalize_value("concept:model-context-protocol");
        assert_eq!(id, "concept:model-context-protocol");
        assert_eq!(kind, None);
    }

    #[test]
    fn bare_name_becomes_concept_slug() {
        let (id, kind) = canonicalize_value("Model Context Protocol");
        assert_eq!(id, "concept:model-context-protocol");
        assert_eq!(kind, Some("concept"));
    }

    #[test]
    fn deterministic_across_calls() {
        assert_eq!(canonicalize_value("Foo Bar").0, canonicalize_value("foo   bar").0);
    }
}
