use scraper::{Html, Selector};

use super::{ExtractedAlias, ExtractedPage};

/// `<title>` and `<meta property="og:title">` become `Name`-type aliases
/// for the crawled page's own node. Deliberately narrow (spec's "explicit
/// subset" scoping) — meta description has no matching `AliasType` variant
/// in this milestone's node-enrichment model, so it's left for a future
/// pass rather than force-fit into an alias.
pub fn extract(html: &str, page_url: &str) -> ExtractedPage {
    let document = Html::parse_document(html);
    let mut page = ExtractedPage::default();
    let mut seen = std::collections::HashSet::new();

    let title_selector = Selector::parse("title").unwrap();
    if let Some(title_el) = document.select(&title_selector).next() {
        let title: String = title_el.text().collect::<String>().trim().to_string();
        if !title.is_empty() && seen.insert(title.clone()) {
            page.aliases.push(ExtractedAlias {
                node_identifier: page_url.to_string(),
                alias: title,
                alias_type: oag_core::AliasType::Name,
            });
        }
    }

    let og_title_selector = Selector::parse(r#"meta[property="og:title"]"#).unwrap();
    if let Some(el) = document.select(&og_title_selector).next() {
        if let Some(content) = el.value().attr("content") {
            let content = content.trim().to_string();
            if !content.is_empty() && seen.insert(content.clone()) {
                page.aliases.push(ExtractedAlias {
                    node_identifier: page_url.to_string(),
                    alias: content,
                    alias_type: oag_core::AliasType::Name,
                });
            }
        }
    }

    page
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_title_and_og_title() {
        let html = r#"
            <html><head>
                <title>Example Page</title>
                <meta property="og:title" content="Example OG Title">
            </head><body></body></html>
        "#;
        let page = extract(html, "https://example.com/page");
        assert_eq!(page.aliases.len(), 2);
        assert!(page.aliases.iter().any(|a| a.alias == "Example Page"));
        assert!(page.aliases.iter().any(|a| a.alias == "Example OG Title"));
        assert!(page.aliases.iter().all(|a| a.alias_type == oag_core::AliasType::Name));
    }

    #[test]
    fn deduplicates_identical_title_and_og_title() {
        let html = r#"
            <html><head>
                <title>Same</title>
                <meta property="og:title" content="Same">
            </head></html>
        "#;
        let page = extract(html, "https://example.com/page");
        assert_eq!(page.aliases.len(), 1);
    }

    #[test]
    fn missing_title_produces_no_alias() {
        let html = "<html><head></head><body>no title here</body></html>";
        let page = extract(html, "https://example.com/page");
        assert!(page.aliases.is_empty());
    }
}
