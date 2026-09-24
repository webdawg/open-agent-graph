use oag_core::AliasType;
use url::Url;

use super::{origin_of, ExtractedAlias, ExtractedFact, ExtractedPage};

/// Parses the de-facto `llms.txt` format (spec section 79): an `# Title`
/// line, then `- [Link Title](url)` bullets (optionally under `##`
/// section headings, which this parser doesn't need to distinguish —
/// every discovered link is treated the same way regardless of section).
/// Every link becomes a `links_to` fact from the site's origin — spec's own
/// caution applies: "inclusion does not automatically grant independent
/// authority," which is exactly why this uses a neutral `links_to`
/// predicate rather than something endorsement-flavored.
///
/// `base_url` is the URL `llms.txt` was fetched from — used both to derive
/// the site origin (the facts' subject) and to resolve relative link hrefs
/// (`/docs/start`, common in real `llms.txt` files) against, since a bare
/// relative path would otherwise be miscategorized as an opaque concept
/// identifier rather than the URL it actually is.
pub fn parse(body: &str, base_url: &Url) -> ExtractedPage {
    let site_origin = origin_of(base_url);
    let mut page = ExtractedPage::default();
    let mut title: Option<String> = None;

    for raw_line in body.lines() {
        let line = raw_line.trim();
        if title.is_none() {
            if let Some(rest) = line.strip_prefix("# ") {
                let rest = rest.trim();
                if !rest.is_empty() {
                    title = Some(rest.to_string());
                }
                continue;
            }
        }
        let Some(rest) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) else {
            continue;
        };
        if let Some((_link_text, url)) = parse_markdown_link(rest) {
            let resolved = base_url.join(&url).map(|u| u.to_string()).unwrap_or(url);
            page.facts.push(ExtractedFact {
                subject: site_origin.clone(),
                subject_type: None,
                predicate: "links_to".to_string(),
                object: resolved,
                object_type: None,
            });
        }
    }

    if let Some(title) = title {
        page.aliases.push(ExtractedAlias {
            node_identifier: site_origin,
            alias: title,
            alias_type: AliasType::Name,
        });
    }

    page
}

/// Parses `[text](url)` from the start of `s` (after any leading bullet
/// marker has already been stripped). Returns `None` for anything else —
/// a bullet line that isn't a markdown link is simply not a discoverable
/// resource, not a parse error.
fn parse_markdown_link(s: &str) -> Option<(String, String)> {
    let open_bracket = s.find('[')?;
    let close_bracket = open_bracket + s[open_bracket..].find(']')?;
    let text = s[open_bracket + 1..close_bracket].to_string();
    let rest = &s[close_bracket + 1..];
    let open_paren = rest.find('(')?;
    let close_paren = open_paren + rest[open_paren..].find(')')?;
    let url = rest[open_paren + 1..close_paren].to_string();
    if url.is_empty() {
        return None;
    }
    Some((text, url))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
# Example Project

> A short description of the project.

## Docs

- [Getting Started](https://example.com/docs/start): how to begin
- [API Reference](https://example.com/docs/api)

## Optional

- [Changelog](https://example.com/changelog)
"#;

    fn base() -> Url {
        Url::parse("https://example.com/llms.txt").unwrap()
    }

    #[test]
    fn extracts_title_as_alias() {
        let page = parse(SAMPLE, &base());
        assert!(page.aliases.iter().any(|a| a.alias == "Example Project" && a.node_identifier == "https://example.com"));
    }

    #[test]
    fn extracts_all_links_regardless_of_section() {
        let page = parse(SAMPLE, &base());
        let urls: Vec<&str> = page.facts.iter().map(|f| f.object.as_str()).collect();
        assert!(urls.contains(&"https://example.com/docs/start"));
        assert!(urls.contains(&"https://example.com/docs/api"));
        assert!(urls.contains(&"https://example.com/changelog"));
        assert_eq!(page.facts.len(), 3);
        assert!(page.facts.iter().all(|f| f.predicate == "links_to"));
        assert!(page.facts.iter().all(|f| f.subject == "https://example.com"));
    }

    #[test]
    fn resolves_relative_links_against_the_site_origin() {
        let body = "# T\n\n- [Relative](/docs/start)\n";
        let page = parse(body, &base());
        assert_eq!(page.facts[0].object, "https://example.com/docs/start");
    }

    #[test]
    fn empty_body_produces_nothing() {
        let page = parse("", &base());
        assert!(page.facts.is_empty());
        assert!(page.aliases.is_empty());
    }

    #[test]
    fn non_link_bullets_are_ignored() {
        let body = "# T\n\n- just a plain bullet, no link\n- [Real](https://example.com/x)\n";
        let page = parse(body, &base());
        assert_eq!(page.facts.len(), 1);
    }
}
