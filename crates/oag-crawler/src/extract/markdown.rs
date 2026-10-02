use super::{ExtractedAlias, ExtractedPage};

/// The first top-level (`# `) heading becomes a `Name`-type alias for the
/// crawled page's own node — the markdown equivalent of `html_meta`'s
/// `<title>` extraction, since plain text/markdown has no `<title>` tag of
/// its own to fall back on. Deliberately only the *first* `# ` heading:
/// later ones are section headings within the document, not the
/// document's own title.
pub fn extract(markdown: &str, page_url: &str) -> ExtractedPage {
    let mut page = ExtractedPage::default();

    let title = markdown.lines().find_map(|line| {
        let rest = line.trim_start().strip_prefix("# ")?;
        let rest = rest.trim();
        if rest.is_empty() {
            None
        } else {
            Some(rest.to_string())
        }
    });

    if let Some(title) = title {
        page.aliases.push(ExtractedAlias {
            node_identifier: page_url.to_string(),
            alias: title,
            alias_type: oag_core::AliasType::Name,
        });
    }

    page
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_first_h1_as_name_alias() {
        let markdown = "# The Title\n\nSome body text.\n\n## A subheading\n\nMore text.";
        let page = extract(markdown, "https://example.com/doc.md");
        assert_eq!(page.aliases.len(), 1);
        assert_eq!(page.aliases[0].alias, "The Title");
        assert_eq!(page.aliases[0].alias_type, oag_core::AliasType::Name);
        assert_eq!(page.aliases[0].node_identifier, "https://example.com/doc.md");
    }

    #[test]
    fn ignores_subsequent_h1s() {
        let markdown = "# First\n\n# Second";
        let page = extract(markdown, "https://example.com/doc.md");
        assert_eq!(page.aliases.len(), 1);
        assert_eq!(page.aliases[0].alias, "First");
    }

    #[test]
    fn no_h1_produces_no_alias() {
        let markdown = "## Only a subheading\n\nbody";
        let page = extract(markdown, "https://example.com/doc.md");
        assert!(page.aliases.is_empty());
    }

    #[test]
    fn empty_h1_produces_no_alias() {
        let markdown = "#   \n\nbody";
        let page = extract(markdown, "https://example.com/doc.md");
        assert!(page.aliases.is_empty());
    }

    #[test]
    fn leading_whitespace_before_hash_is_tolerated() {
        let markdown = "   # Indented Title\n\nbody";
        let page = extract(markdown, "https://example.com/doc.md");
        assert_eq!(page.aliases[0].alias, "Indented Title");
    }
}
