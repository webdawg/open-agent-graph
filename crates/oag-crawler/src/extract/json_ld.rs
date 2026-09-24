use oag_core::AliasType;
use scraper::{Html, Selector};
use serde_json::Value;

use super::{ExtractedAlias, ExtractedFact, ExtractedPage};

/// A narrow, explicit schema.org subset — not a general JSON-LD/schema.org
/// processor (spec's "don't build a complete universal ontology" stance,
/// already established for predicates, applies here too). Recognizes:
/// `SoftwareApplication`/`SoftwareSourceCode` (-> `instance_of
/// concept:software`, `codeRepository` -> `repository_at`), `Organization`
/// (`url` -> `operated_by`), `name` (-> a `Name` alias), and `sameAs`
/// (-> `related_to` edges). Anything else is silently ignored rather than
/// erroring — an unrecognized `@type` is not a parse failure.
fn process_node(obj: &serde_json::Map<String, Value>, page_url: &str, page: &mut ExtractedPage) {
    let types: Vec<String> = match obj.get("@type") {
        Some(Value::String(s)) => vec![s.to_lowercase()],
        Some(Value::Array(arr)) => arr.iter().filter_map(|v| v.as_str()).map(str::to_lowercase).collect(),
        _ => Vec::new(),
    };

    if let Some(name) = obj.get("name").and_then(Value::as_str) {
        let name = name.trim();
        if !name.is_empty() {
            page.aliases.push(ExtractedAlias {
                node_identifier: page_url.to_string(),
                alias: name.to_string(),
                alias_type: AliasType::Name,
            });
        }
    }

    if types.iter().any(|t| t == "softwareapplication" || t == "softwaresourcecode") {
        page.facts.push(ExtractedFact {
            subject: page_url.to_string(),
            subject_type: Some("software".to_string()),
            predicate: "instance_of".to_string(),
            object: "concept:software".to_string(),
            object_type: Some("concept".to_string()),
        });
        if let Some(repo) = obj.get("codeRepository").and_then(Value::as_str) {
            page.facts.push(ExtractedFact {
                subject: page_url.to_string(),
                subject_type: Some("software".to_string()),
                predicate: "repository_at".to_string(),
                object: repo.to_string(),
                object_type: Some("repository".to_string()),
            });
        }
    }

    if types.iter().any(|t| t == "organization") {
        if let Some(org_url) = obj.get("url").and_then(Value::as_str) {
            page.facts.push(ExtractedFact {
                subject: page_url.to_string(),
                subject_type: None,
                predicate: "operated_by".to_string(),
                object: org_url.to_string(),
                object_type: Some("organization".to_string()),
            });
        }
    }

    match obj.get("sameAs") {
        Some(Value::String(s)) => push_same_as(page, page_url, s),
        Some(Value::Array(arr)) => {
            for v in arr {
                if let Some(s) = v.as_str() {
                    push_same_as(page, page_url, s);
                }
            }
        }
        _ => {}
    }
}

fn push_same_as(page: &mut ExtractedPage, page_url: &str, target: &str) {
    page.facts.push(ExtractedFact {
        subject: page_url.to_string(),
        subject_type: None,
        predicate: "related_to".to_string(),
        object: target.to_string(),
        object_type: None,
    });
}

fn process_document(value: &Value, page_url: &str, page: &mut ExtractedPage) {
    match value {
        Value::Array(items) => {
            for item in items {
                process_document(item, page_url, page);
            }
        }
        Value::Object(obj) => {
            if let Some(Value::Array(graph)) = obj.get("@graph") {
                for item in graph {
                    process_document(item, page_url, page);
                }
            } else {
                process_node(obj, page_url, page);
            }
        }
        _ => {}
    }
}

/// Extract from `<script type="application/ld+json">` blocks embedded in an
/// HTML page. Malformed individual blocks are skipped, not fatal — one bad
/// script tag shouldn't discard facts found in a good one.
pub fn extract_from_html(html: &str, page_url: &str) -> ExtractedPage {
    let document = Html::parse_document(html);
    let selector = Selector::parse(r#"script[type="application/ld+json"]"#).unwrap();
    let mut page = ExtractedPage::default();
    for el in document.select(&selector) {
        let text: String = el.text().collect();
        if let Ok(value) = serde_json::from_str::<Value>(&text) {
            process_document(&value, page_url, &mut page);
        }
    }
    page
}

/// Extract from a document fetched directly as `application/ld+json`.
pub fn extract_from_json(body: &str, page_url: &str) -> ExtractedPage {
    let mut page = ExtractedPage::default();
    if let Ok(value) = serde_json::from_str::<Value>(body) {
        process_document(&value, page_url, &mut page);
    }
    page
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_software_application_with_repository_and_name() {
        let html = r#"
            <html><head>
            <script type="application/ld+json">
            {
                "@context": "https://schema.org",
                "@type": "SoftwareApplication",
                "name": "Example Tool",
                "codeRepository": "https://github.com/example/tool"
            }
            </script>
            </head></html>
        "#;
        let page = extract_from_html(html, "https://example.com/tool");
        assert!(page.aliases.iter().any(|a| a.alias == "Example Tool"));
        assert!(page.facts.iter().any(|f| f.predicate == "instance_of" && f.object == "concept:software"));
        assert!(page
            .facts
            .iter()
            .any(|f| f.predicate == "repository_at" && f.object == "https://github.com/example/tool"));
    }

    #[test]
    fn extracts_organization_operated_by() {
        let json = r#"{"@type": "Organization", "name": "Example Org", "url": "https://example.org"}"#;
        let page = extract_from_json(json, "https://example.com/about");
        assert!(page.facts.iter().any(|f| f.predicate == "operated_by" && f.object == "https://example.org"));
        assert!(page.aliases.iter().any(|a| a.alias == "Example Org"));
    }

    #[test]
    fn extracts_same_as_array() {
        let json = r#"{"@type": "SoftwareApplication", "sameAs": ["https://x.com/a", "https://x.com/b"]}"#;
        let page = extract_from_json(json, "https://example.com/tool");
        assert_eq!(page.facts.iter().filter(|f| f.predicate == "related_to").count(), 2);
    }

    #[test]
    fn handles_at_graph_wrapper() {
        let json = r#"{"@graph": [{"@type": "SoftwareApplication", "name": "A"}, {"@type": "Organization", "name": "B", "url": "https://b.example"}]}"#;
        let page = extract_from_json(json, "https://example.com/x");
        assert!(page.aliases.iter().any(|a| a.alias == "A"));
        assert!(page.aliases.iter().any(|a| a.alias == "B"));
    }

    #[test]
    fn unrecognized_type_is_not_an_error_just_ignored() {
        let json = r#"{"@type": "Recipe", "name": "Pasta"}"#;
        let page = extract_from_json(json, "https://example.com/recipe");
        // name still becomes an alias (that mapping isn't type-gated), but
        // no instance_of/operated_by facts are produced for an unknown type.
        assert!(page.aliases.iter().any(|a| a.alias == "Pasta"));
        assert!(page.facts.is_empty());
    }

    #[test]
    fn malformed_json_in_one_script_block_does_not_lose_facts_from_others() {
        let html = r#"
            <html><head>
            <script type="application/ld+json">{ not valid json </script>
            <script type="application/ld+json">{"@type": "Organization", "name": "Still Works", "url": "https://ok.example"}</script>
            </head></html>
        "#;
        let page = extract_from_html(html, "https://example.com/x");
        assert!(page.aliases.iter().any(|a| a.alias == "Still Works"));
    }
}
