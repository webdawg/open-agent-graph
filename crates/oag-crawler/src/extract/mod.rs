pub mod a2a;
pub mod ard;
pub mod html_meta;
pub mod json_ld;
pub mod llms_txt;

/// `scheme://host[:port]` for `url`, with no path/query/fragment — the
/// "site" a page belongs to, as opposed to the one page itself. Shared by
/// `llms_txt` (whose facts are about the site, not any single page) and the
/// crawler orchestrator (which needs the same origin to find `llms.txt`/
/// ARD/A2A discovery resources).
pub fn origin_of(url: &url::Url) -> String {
    match url.port() {
        Some(port) => format!("{}://{}:{port}", url.scheme(), url.host_str().unwrap_or("")),
        None => format!("{}://{}", url.scheme(), url.host_str().unwrap_or("")),
    }
}

/// One candidate relationship an extractor found. Turned into an
/// `AssertInput` (with `extraction_method: StructuredExtraction`) by the
/// orchestrator — extractors never talk to `GraphService` directly, keeping
/// parsing separate from graph-writing (easy to unit test with fixtures,
/// no network/DB needed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedFact {
    pub subject: String,
    pub subject_type: Option<String>,
    pub predicate: String,
    pub object: String,
    pub object_type: Option<String>,
}

/// A discovered name for some node. `node_identifier` is a raw (not yet
/// canonicalized) identifier string, same convention as
/// `ExtractedFact::subject` — usually the crawled page's own URL, but not
/// always: a single ARD response can describe several distinct resources,
/// each aliased independently (spec section 17).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedAlias {
    pub node_identifier: String,
    pub alias: String,
    pub alias_type: oag_core::AliasType,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExtractedPage {
    pub facts: Vec<ExtractedFact>,
    pub aliases: Vec<ExtractedAlias>,
}

impl ExtractedPage {
    pub fn merge(&mut self, other: ExtractedPage) {
        self.facts.extend(other.facts);
        self.aliases.extend(other.aliases);
    }
}

/// Plain-text rendering of an HTML page's `<body>`, script/style content
/// explicitly excluded (their contents are ordinary text nodes to `scraper`,
/// not stripped by default), whitespace-collapsed and truncated to
/// `max_chars`. Used only as LLM extraction's prompt input (spec section
/// 74) -- the deterministic extractors above parse the raw HTML/DOM
/// directly and never need this.
pub fn body_text(html: &str, max_chars: usize) -> String {
    let document = scraper::Html::parse_document(html);
    let selector = scraper::Selector::parse("body").unwrap();
    let mut parts = Vec::new();
    if let Some(body) = document.select(&selector).next() {
        for node in body.descendants() {
            let scraper::node::Node::Text(text) = node.value() else { continue };
            let in_script_or_style = node.ancestors().any(|ancestor| {
                matches!(ancestor.value(), scraper::node::Node::Element(el) if el.name() == "script" || el.name() == "style")
            });
            if !in_script_or_style {
                parts.push(text.text.as_ref());
            }
        }
    }
    let collapsed = parts.join(" ").split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_text_strips_tags_and_collapses_whitespace() {
        let html = "<html><head><title>ignored</title></head><body>\n  <h1>Hello</h1>\n  <p>World  wide</p>\n</body></html>";
        assert_eq!(body_text(html, 1000), "Hello World wide");
    }

    #[test]
    fn body_text_excludes_script_and_style_content() {
        let html = "<html><body><p>Visible</p><script>evil()</script><style>.x{}</style></body></html>";
        assert_eq!(body_text(html, 1000), "Visible");
    }

    #[test]
    fn body_text_truncates_to_max_chars() {
        let html = "<html><body>abcdefghij</body></html>";
        assert_eq!(body_text(html, 5), "abcde");
    }
}
