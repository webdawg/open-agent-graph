use oag_core::AliasType;
use serde::Deserialize;

use super::{ExtractedAlias, ExtractedFact, ExtractedPage};

/// A narrow, explicit subset of ARD's shape (spec section 77): a
/// `resources` array, each with `name`/`type`/`url`/`capabilities`/
/// `operator`. Unlike the other extractors, one ARD document can describe
/// *several* distinct resources — each gets its own `instance_of` fact and
/// its own aliases, keyed by that resource's own `url` (falling back to the
/// site origin only if a resource has none).
#[derive(Debug, Deserialize)]
struct ArdDocument {
    #[serde(default)]
    resources: Vec<ArdResource>,
}

#[derive(Debug, Deserialize)]
struct ArdResource {
    name: Option<String>,
    #[serde(rename = "type")]
    resource_type: Option<String>,
    url: Option<String>,
    #[serde(default)]
    capabilities: Vec<String>,
    operator: Option<ArdOperator>,
}

#[derive(Debug, Deserialize)]
struct ArdOperator {
    name: Option<String>,
    url: Option<String>,
}

pub fn parse(body: &str, site_origin: &str) -> ExtractedPage {
    let mut page = ExtractedPage::default();
    let Ok(doc) = serde_json::from_str::<ArdDocument>(body) else {
        return page;
    };

    for resource in doc.resources {
        let resource_id = resource.url.clone().unwrap_or_else(|| site_origin.to_string());
        let node_type = match resource.resource_type.as_deref() {
            Some(t) if t.eq_ignore_ascii_case("mcp_server") || t.eq_ignore_ascii_case("mcp-server") => "mcp_server",
            _ => "agent",
        };

        page.facts.push(ExtractedFact {
            subject: resource_id.clone(),
            subject_type: Some(node_type.to_string()),
            predicate: "instance_of".to_string(),
            object: format!("concept:{node_type}"),
            object_type: Some("concept".to_string()),
        });

        if let Some(name) = resource.name.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            page.aliases.push(ExtractedAlias {
                node_identifier: resource_id.clone(),
                alias: name.to_string(),
                alias_type: AliasType::Name,
            });
        }

        if let Some(operator) = &resource.operator {
            if let Some(org) = operator.url.clone().or_else(|| operator.name.clone()) {
                page.facts.push(ExtractedFact {
                    subject: resource_id.clone(),
                    subject_type: Some(node_type.to_string()),
                    predicate: "operated_by".to_string(),
                    object: org,
                    object_type: Some("organization".to_string()),
                });
            }
        }

        for capability in &resource.capabilities {
            page.facts.push(ExtractedFact {
                subject: resource_id.clone(),
                subject_type: Some(node_type.to_string()),
                predicate: "provides_capability".to_string(),
                object: capability.clone(),
                object_type: Some("capability".to_string()),
            });
        }
    }

    page
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
    {
        "resources": [
            {
                "name": "Example MCP Server",
                "type": "mcp_server",
                "url": "https://example.com/mcp",
                "capabilities": ["search", "fetch"],
                "operator": { "name": "Example Org", "url": "https://example.org" }
            },
            {
                "name": "Example Agent",
                "type": "agent",
                "url": "https://example.com/agent"
            }
        ]
    }
    "#;

    #[test]
    fn extracts_one_instance_of_fact_per_resource() {
        let page = parse(SAMPLE, "https://example.com");
        let instance_of_count = page.facts.iter().filter(|f| f.predicate == "instance_of").count();
        assert_eq!(instance_of_count, 2);
    }

    #[test]
    fn distinguishes_mcp_server_from_agent_type() {
        let page = parse(SAMPLE, "https://example.com");
        assert!(page
            .facts
            .iter()
            .any(|f| f.subject == "https://example.com/mcp" && f.object == "concept:mcp_server"));
        assert!(page
            .facts
            .iter()
            .any(|f| f.subject == "https://example.com/agent" && f.object == "concept:agent"));
    }

    #[test]
    fn each_resource_gets_its_own_alias() {
        let page = parse(SAMPLE, "https://example.com");
        assert!(page
            .aliases
            .iter()
            .any(|a| a.node_identifier == "https://example.com/mcp" && a.alias == "Example MCP Server"));
        assert!(page
            .aliases
            .iter()
            .any(|a| a.node_identifier == "https://example.com/agent" && a.alias == "Example Agent"));
    }

    #[test]
    fn extracts_capabilities_and_operator() {
        let page = parse(SAMPLE, "https://example.com");
        let caps: Vec<&str> = page
            .facts
            .iter()
            .filter(|f| f.predicate == "provides_capability" && f.subject == "https://example.com/mcp")
            .map(|f| f.object.as_str())
            .collect();
        assert!(caps.contains(&"search"));
        assert!(caps.contains(&"fetch"));
        assert!(page.facts.iter().any(|f| f.predicate == "operated_by" && f.object == "https://example.org"));
    }

    #[test]
    fn resource_without_url_falls_back_to_site_origin() {
        let json = r#"{"resources": [{"name": "No URL Resource", "type": "agent"}]}"#;
        let page = parse(json, "https://example.com");
        assert!(page.facts.iter().any(|f| f.subject == "https://example.com"));
    }

    #[test]
    fn invalid_json_yields_empty_page() {
        let page = parse("{{{not json", "https://example.com");
        assert!(page.facts.is_empty());
    }
}
