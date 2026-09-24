/// A minimal robots.txt evaluator: only `User-agent`/`Disallow` directives,
/// matched for both our own user-agent and `*` (the fallback group), per
/// the common-sense subset of the de-facto standard. No `Allow`-override
/// precedence, `Crawl-delay`, or `Sitemap` parsing — those don't affect
/// whether a fetch is permitted, which is all spec section 46's pipeline
/// step 2 needs.
pub struct RobotsRules {
    disallow: Vec<String>,
}

impl RobotsRules {
    /// `body` is the raw text of a fetched `/robots.txt` (or `None` if the
    /// fetch failed/404'd — no file present means everything is allowed,
    /// per the standard's own convention).
    pub fn parse(body: &str, user_agent: &str) -> Self {
        let mut groups: Vec<(Vec<String>, Vec<String>)> = Vec::new(); // (agents, disallows)
        let mut current_agents: Vec<String> = Vec::new();
        let mut current_disallows: Vec<String> = Vec::new();
        let mut in_group = false;

        for raw_line in body.lines() {
            let line = raw_line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let Some((key, value)) = line.split_once(':') else { continue };
            let key = key.trim().to_lowercase();
            let value = value.trim().to_string();

            match key.as_str() {
                "user-agent" => {
                    if in_group && !current_disallows.is_empty() {
                        // A new User-agent line after we've already seen
                        // Disallow lines starts a new group.
                        groups.push((std::mem::take(&mut current_agents), std::mem::take(&mut current_disallows)));
                        in_group = false;
                    }
                    current_agents.push(value.to_lowercase());
                }
                "disallow" => {
                    in_group = true;
                    if !value.is_empty() {
                        current_disallows.push(value);
                    }
                }
                _ => {}
            }
        }
        if !current_agents.is_empty() {
            groups.push((current_agents, current_disallows));
        }

        // Prefer a group naming our exact user-agent; fall back to `*`.
        let ua = user_agent.to_lowercase();
        let disallow = groups
            .iter()
            .find(|(agents, _)| agents.iter().any(|a| a == &ua))
            .or_else(|| groups.iter().find(|(agents, _)| agents.iter().any(|a| a == "*")))
            .map(|(_, disallows)| disallows.clone())
            .unwrap_or_default();

        Self { disallow }
    }

    pub fn allow_all() -> Self {
        Self { disallow: Vec::new() }
    }

    /// `path` is the URL path (plus query, if any) being requested.
    pub fn is_allowed(&self, path: &str) -> bool {
        !self.disallow.iter().any(|rule| path.starts_with(rule.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_file_allows_everything() {
        let rules = RobotsRules::allow_all();
        assert!(rules.is_allowed("/anything"));
    }

    #[test]
    fn wildcard_group_disallow_is_respected() {
        let body = "User-agent: *\nDisallow: /private\nDisallow: /admin\n";
        let rules = RobotsRules::parse(body, "oag-crawler");
        assert!(!rules.is_allowed("/private/data"));
        assert!(!rules.is_allowed("/admin"));
        assert!(rules.is_allowed("/public"));
    }

    #[test]
    fn specific_user_agent_group_takes_priority_over_wildcard() {
        let body = "User-agent: *\nDisallow: /\n\nUser-agent: oag-crawler\nDisallow: /only-this\n";
        let rules = RobotsRules::parse(body, "oag-crawler");
        assert!(rules.is_allowed("/anything-else"), "our specific group should apply, not the wildcard's blanket disallow");
        assert!(!rules.is_allowed("/only-this"));
    }

    #[test]
    fn empty_disallow_value_means_allow_everything() {
        let body = "User-agent: *\nDisallow:\n";
        let rules = RobotsRules::parse(body, "oag-crawler");
        assert!(rules.is_allowed("/anything"));
    }

    #[test]
    fn comments_and_blank_lines_are_ignored() {
        let body = "# a comment\n\nUser-agent: *\n# another comment\nDisallow: /blocked\n";
        let rules = RobotsRules::parse(body, "oag-crawler");
        assert!(!rules.is_allowed("/blocked"));
        assert!(rules.is_allowed("/ok"));
    }
}
