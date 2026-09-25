use std::collections::{BTreeSet, HashMap};

use oag_core::{Assertion, AssertionStatus, EdgeId, EvidenceType, VerifyResult};
use oag_storage::repo::{assertions as assertions_repo, edges};

use crate::error::GraphError;
use crate::service::GraphService;

/// Hosts where one domain serves many unrelated publishers — grouping by the
/// bare host would treat "every GitHub repo on the internet" as one control
/// group, which is exactly the false-corroboration spec section 62 warns
/// about ("Ten supporting sources controlled by one organization are not ten
/// independent confirmations"). Scope by the first path segment (repo owner
/// / npm scope / HF org) instead.
const MULTI_TENANT_HOSTS: &[&str] = &[
    "github.com",
    "www.github.com",
    "gist.github.com",
    "raw.githubusercontent.com",
    "gitlab.com",
    "bitbucket.org",
    "npmjs.com",
    "www.npmjs.com",
    "pypi.org",
    "crates.io",
    "huggingface.co",
    "sourceforge.net",
];

/// Second-level public suffixes this heuristic knows about, so
/// `foo.co.uk`/`bar.co.uk` aren't collapsed into the single registrable
/// domain `co.uk`. NOT a full Public Suffix List (see plan's deferral note)
/// — approximately right for common cases matters far more here than
/// exhaustive coverage of every ccTLD's registry policy.
const SECOND_LEVEL_SUFFIXES: &[&str] = &[
    "co.uk", "org.uk", "gov.uk", "ac.uk", "me.uk", "net.uk", "co.jp", "ne.jp", "or.jp", "co.in",
    "co.nz", "org.nz", "net.nz", "com.au", "net.au", "org.au", "co.za", "com.br", "com.mx",
    "co.kr", "com.cn", "co.il", "co.id", "com.sg", "com.tw", "com.hk", "co.th",
];

fn registrable_domain(host: &str) -> String {
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() <= 2 {
        return host.to_string();
    }
    let last_two = format!("{}.{}", parts[parts.len() - 2], parts[parts.len() - 1]);
    if parts.len() >= 3 && SECOND_LEVEL_SUFFIXES.contains(&last_two.as_str()) {
        format!("{}.{last_two}", parts[parts.len() - 3])
    } else {
        last_two
    }
}

/// Collapse an evidence URI down to a "who controls this" key (spec section
/// 62): a repo/org/publisher owner on a known multi-tenant host, or a plain
/// registrable domain otherwise. `None` for an unparseable URI or a host-less
/// URL — the caller falls back to grouping by the asserting actor instead.
pub(crate) fn control_group_for_uri(uri: &str) -> Option<String> {
    let parsed = url::Url::parse(uri).ok()?;
    let host = parsed.host_str()?.to_ascii_lowercase();
    if let Some(&canonical_host) = MULTI_TENANT_HOSTS.iter().find(|h| **h == host) {
        let owner = parsed
            .path_segments()
            .and_then(|mut segments| segments.next())
            .filter(|s| !s.is_empty());
        return Some(match owner {
            Some(o) => format!("{canonical_host}/{o}"),
            None => canonical_host.to_string(),
        });
    }
    Some(registrable_domain(&host))
}

/// How much independent verification a given evidence type implies — a
/// specification or the source code itself is much harder to fake than a
/// web page. Deliberately kept separate from `source_independence` (spec
/// section 65: don't collapse signals) — this is about quality, not breadth.
fn evidence_type_weight(t: EvidenceType) -> f32 {
    match t {
        EvidenceType::Specification => 1.0,
        EvidenceType::SourceCode | EvidenceType::ApiResponse => 0.85,
        EvidenceType::Repository => 0.8,
        EvidenceType::Dataset => 0.7,
        EvidenceType::Documentation => 0.65,
        EvidenceType::Manual | EvidenceType::UserObservation | EvidenceType::File => 0.45,
        EvidenceType::AgentObservation => 0.35,
        EvidenceType::WebPage => 0.3,
        EvidenceType::Other => 0.15,
    }
}

/// Maps a verification observation's result (spec section 37) onto a
/// [-1.0, 1.0] agreement contribution. `None` (an unparseable stored value)
/// contributes nothing rather than panicking — `verify_assertion` validates
/// on write, but scoring stays defensive against replicated data written by
/// a future/different peer software version.
fn verify_result_weight(result: &str) -> f32 {
    match VerifyResult::parse(result) {
        Some(VerifyResult::Confirmed) => 1.0,
        Some(VerifyResult::Changed) => 0.3,
        Some(VerifyResult::Unknown) | Some(VerifyResult::Unreachable) => 0.0,
        Some(VerifyResult::NotConfirmed) => -0.5,
        Some(VerifyResult::Contradicted) => -1.0,
        None => 0.0,
    }
}

/// Per-signal corroboration breakdown for one edge (spec sections 62, 65) —
/// deliberately not collapsed into one reputation number (section 65: "Do
/// not collapse everything into one global reputation number").
#[derive(Debug, Clone, serde::Serialize)]
pub struct EdgeCorroboration {
    pub edge_id: EdgeId,
    pub total_assertions: usize,
    pub active_assertions: usize,
    pub disputed_assertions: usize,
    pub distinct_actors: usize,
    /// Sorted, deduplicated control-group keys (spec section 62) backing the
    /// active assertions — explainability, not just a count.
    pub source_groups: Vec<String>,
    pub agreement: f32,
    pub evidence_strength: f32,
    pub source_independence: f32,
}

impl GraphService {
    /// Compute corroboration signals for one edge from its currently-active
    /// assertions, their evidence, and any verification observations
    /// against them (spec sections 62, 65, 81).
    pub async fn get_edge_corroboration(&self, edge_id: EdgeId) -> Result<EdgeCorroboration, GraphError> {
        let mut conn = self
            .pool()
            .acquire()
            .await
            .map_err(oag_storage::StorageError::from)?;

        if edges::get_by_id(&mut conn, edge_id).await?.is_none() {
            return Err(GraphError::NotFound(format!("edge {edge_id}")));
        }

        let all_assertions = assertions_repo::list_by_edge(&mut conn, edge_id).await?;
        let active: Vec<&Assertion> = all_assertions
            .iter()
            .filter(|a| a.status == AssertionStatus::Active)
            .collect();
        let disputed_count = all_assertions
            .iter()
            .filter(|a| a.status == AssertionStatus::Disputed)
            .count();

        let distinct_actors: BTreeSet<_> = active.iter().copied().map(|a| a.actor_id).collect();

        // Control-group key -> the strongest evidence weight seen for that
        // group (0.0 for a bare claim with no URI-bearing evidence).
        let mut group_weights: HashMap<String, f32> = HashMap::new();
        let mut observation_scores: Vec<f32> = Vec::new();

        for assertion in active.iter().copied() {
            let evidence = assertions_repo::list_evidence(&mut conn, assertion.id).await?;
            if evidence.is_empty() {
                group_weights
                    .entry(format!("actor:{}", assertion.actor_id.to_hex()))
                    .or_insert(0.0);
            } else {
                for item in &evidence {
                    let group = item
                        .uri
                        .as_deref()
                        .and_then(control_group_for_uri)
                        .unwrap_or_else(|| format!("actor:{}", assertion.actor_id.to_hex()));
                    let weight = evidence_type_weight(item.evidence_type);
                    let entry = group_weights.entry(group).or_insert(0.0);
                    if weight > *entry {
                        *entry = weight;
                    }
                }
            }

            let observations = assertions_repo::list_observations(&mut conn, assertion.id).await?;
            observation_scores.extend(observations.iter().map(|o| verify_result_weight(&o.result)));
        }

        let mut source_groups: Vec<String> = group_weights.keys().cloned().collect();
        source_groups.sort();
        let distinct_source_groups = source_groups.len();

        // One control group, however many assertions cite it, is not
        // independent corroboration (spec section 62's literal example) —
        // two groups is partial, three or more saturates.
        let source_independence =
            ((distinct_source_groups.saturating_sub(1)) as f32 / 2.0).clamp(0.0, 1.0);

        let evidence_strength = if group_weights.is_empty() {
            0.0
        } else {
            group_weights.values().sum::<f32>() / group_weights.len() as f32
        };

        // Retracted/superseded assertions are lifecycle, not disagreement
        // (spec section 36: supersession is an update, not a contradiction)
        // — only active-vs-disputed feeds this ratio.
        let base_agreement = if active.is_empty() {
            0.0
        } else {
            active.len() as f32 / (active.len() + disputed_count) as f32
        };
        let agreement = if observation_scores.is_empty() {
            base_agreement
        } else {
            let mean_obs = observation_scores.iter().sum::<f32>() / observation_scores.len() as f32;
            let obs_agreement = ((mean_obs + 1.0) / 2.0).clamp(0.0, 1.0);
            (base_agreement + obs_agreement) / 2.0
        };

        Ok(EdgeCorroboration {
            edge_id,
            total_assertions: all_assertions.len(),
            active_assertions: active.len(),
            disputed_assertions: disputed_count,
            distinct_actors: distinct_actors.len(),
            source_groups,
            agreement,
            evidence_strength,
            source_independence,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_github_owner_collapses_to_one_group() {
        let a = control_group_for_uri("https://github.com/example-org/repo-a/blob/main/README.md").unwrap();
        let b = control_group_for_uri("https://github.com/example-org/repo-b").unwrap();
        assert_eq!(a, b);
        assert_eq!(a, "github.com/example-org");
    }

    #[test]
    fn different_github_owners_stay_distinct() {
        let a = control_group_for_uri("https://github.com/org-a/repo").unwrap();
        let b = control_group_for_uri("https://github.com/org-b/repo").unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn plain_domains_collapse_by_registrable_domain() {
        let a = control_group_for_uri("https://docs.example.com/page").unwrap();
        let b = control_group_for_uri("https://www.example.com/other").unwrap();
        assert_eq!(a, b);
        assert_eq!(a, "example.com");
    }

    #[test]
    fn second_level_suffix_is_not_collapsed_away() {
        let a = control_group_for_uri("https://foo.co.uk/page").unwrap();
        let b = control_group_for_uri("https://bar.co.uk/page").unwrap();
        assert_ne!(a, b);
        assert_eq!(a, "foo.co.uk");
    }

    #[test]
    fn malformed_uri_returns_none() {
        assert!(control_group_for_uri("not a url").is_none());
        assert!(control_group_for_uri("mailto:someone@example.com").is_none());
    }
}
