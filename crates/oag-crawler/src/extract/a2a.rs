use oag_core::AliasType;
use serde::Deserialize;

use super::{ExtractedAlias, ExtractedFact, ExtractedPage};

/// A narrow, explicit subset of the A2A Agent Card shape (spec section 78)
/// — `name`, `provider.{organization,url}`, `skills[].{name,id}`. Unknown
/// fields are ignored by `serde` default behavior; a card that doesn't
/// parse at all yields an empty `ExtractedPage` rather than an error, since
/// "not a valid Agent Card" just means there's nothing to extract here.
#[derive(Debug, Deserialize)]
struct AgentCard {
    name: Option<String>,
    provider: Option<AgentProvider>,
    #[serde(default)]
    skills: Vec<AgentSkill>,
}

#[derive(Debug, Deserialize)]
struct AgentProvider {
    organization: Option<String>,
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AgentSkill {
    name: Option<String>,
    id: Option<String>,
}

pub fn parse(body: &str, agent_url: &str) -> ExtractedPage {
    let mut page = ExtractedPage::default();
    let Ok(card) = serde_json::from_str::<AgentCard>(body) else {
        return page;
    };

    page.facts.push(ExtractedFact {
        subject: agent_url.to_string(),
        subject_type: Some("agent".to_string()),
        predicate: "instance_of".to_string(),
        object: "concept:a2a-agent".to_string(),
        object_type: Some("concept".to_string()),
    });

    if let Some(name) = card.name.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        page.aliases.push(ExtractedAlias {
            node_identifier: agent_url.to_string(),
            alias: name.to_string(),
            alias_type: AliasType::Name,
        });
    }

    if let Some(provider) = &card.provider {
        // Prefer a URL (a real identifier) over a bare organization name.
        if let Some(org) = provider.url.clone().or_else(|| provider.organization.clone()) {
            page.facts.push(ExtractedFact {
                subject: agent_url.to_string(),
                subject_type: Some("agent".to_string()),
                predicate: "operated_by".to_string(),
                object: org,
                object_type: Some("organization".to_string()),
            });
        }
    }

    for skill in &card.skills {
        if let Some(capability) = skill.name.clone().or_else(|| skill.id.clone()) {
            page.facts.push(ExtractedFact {
                subject: agent_url.to_string(),
                subject_type: Some("agent".to_string()),
                predicate: "provides_capability".to_string(),
                object: capability,
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
        "name": "Example Agent",
        "provider": { "organization": "Example Org", "url": "https://example.org" },
        "skills": [
            { "id": "search", "name": "Web Search" },
            { "id": "translate" }
        ]
    }
    "#;

    #[test]
    fn extracts_baseline_instance_of() {
        let page = parse(SAMPLE, "https://agent.example.com");
        assert!(page.facts.iter().any(|f| f.predicate == "instance_of" && f.object == "concept:a2a-agent"));
    }

    #[test]
    fn extracts_name_as_alias() {
        let page = parse(SAMPLE, "https://agent.example.com");
        assert!(page.aliases.iter().any(|a| a.alias == "Example Agent"));
    }

    #[test]
    fn prefers_provider_url_over_organization_name() {
        let page = parse(SAMPLE, "https://agent.example.com");
        assert!(page.facts.iter().any(|f| f.predicate == "operated_by" && f.object == "https://example.org"));
    }

    #[test]
    fn extracts_skills_as_capabilities_falling_back_to_id() {
        let page = parse(SAMPLE, "https://agent.example.com");
        let caps: Vec<&str> = page
            .facts
            .iter()
            .filter(|f| f.predicate == "provides_capability")
            .map(|f| f.object.as_str())
            .collect();
        assert!(caps.contains(&"Web Search"));
        assert!(caps.contains(&"translate"));
    }

    #[test]
    fn invalid_json_yields_empty_page_not_error() {
        let page = parse("not json at all", "https://agent.example.com");
        assert!(page.facts.is_empty());
        assert!(page.aliases.is_empty());
    }
}
