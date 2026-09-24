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
