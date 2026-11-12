use std::collections::{BTreeSet, HashMap};

use oag_core::{Actor, Assertion, AssertionStatus, EdgeId, EvidenceType, VerifyResult};
use oag_storage::repo::{actors, assertions as assertions_repo, edges};

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
/// Minimal union-find, used to group evidence items into sources by connected components.
struct UnionFind {
    parent: Vec<usize>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        Self { parent: (0..n).collect() }
    }

    fn find(&mut self, i: usize) -> usize {
        let mut root = i;
        while self.parent[root] != root {
            root = self.parent[root];
        }
        let mut cur = i;
        while self.parent[cur] != cur {
            let next = self.parent[cur];
            self.parent[cur] = root;
            cur = next;
        }
        root
    }

    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.parent[rb] = ra;
        }
    }
}

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
        // A mutual-nonce receipt (spec/31-service-receipts.md) is bound by both
        // parties' randomness plus the transferred content's hash — neither side
        // can fabricate one alone, so it's weighted with the harder-to-fake tier.
        EvidenceType::ServiceReceipt => 0.85,
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

/// How strong an actor's identity claim is (spec section 65's
/// `identity_assurance` signal): a bare local actor (no cryptographic key,
/// no external reference) proves nothing about who's really behind it; a
/// verified public key (see `GraphService::declare_actor`'s
/// `PublicKeyProof`) means the actor has demonstrably controlled the same
/// keypair the whole time, independent of which peer relays its claims.
fn identity_assurance_score(actor: &Actor) -> f32 {
    match (actor.public_key.is_some(), actor.identity_uri.is_some()) {
        (true, true) => 1.0,
        (true, false) => 0.7,
        (false, true) => 0.3,
        (false, false) => 0.0,
    }
}

/// Half-life for the `freshness` signal: how long until a claim's recency
/// contribution decays to half strength. Spec section 65 requires the
/// signal to exist, not a specific decay curve — six months is a reasonable
/// default for a knowledge graph about software/agents (fast-moving enough
/// that a year-old claim shouldn't read as current, slow enough that a
/// two-week-old claim isn't penalized). Exponential decay rather than a
/// hard cutoff, so there's no arbitrary cliff where a claim flips from
/// "fresh" to "stale" overnight.
const FRESHNESS_HALF_LIFE_SECONDS: f64 = 180.0 * 86_400.0;

/// Exponential recency decay from a single timestamp: 1.0 at `age == 0`,
/// 0.5 at one half-life, approaching 0.0 as the claim ages further. Negative
/// age (clock skew, or a caller-supplied `observed_at` in the future) is
/// clamped to 0 rather than yielding a freshness above 1.0.
fn freshness_from_age(now: i64, timestamp: i64) -> f32 {
    let age_seconds = (now - timestamp).max(0) as f64;
    2.0f64.powf(-age_seconds / FRESHNESS_HALF_LIFE_SECONDS) as f32
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
    /// Exponential recency decay (half-life above) from the most recently
    /// observed-or-asserted active assertion. 0.0 when there are no active
    /// assertions — nothing current backs the edge at all.
    pub freshness: f32,
    /// Mean [`identity_assurance_score`] across the edge's distinct actors —
    /// mean, not max, so one well-identified actor can't fully vouch for a
    /// chorus of anonymous ones asserting the same edge (same reasoning as
    /// `evidence_strength` averaging per source group rather than taking
    /// the single best item).
    pub identity_assurance: f32,
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

        // One entry per evidence item, carrying *both* identities it can be grouped by -- the
        // content it carries and the source it came from -- plus the weight it contributes. A
        // bare claim with no evidence has only a source key, at weight 0.0.
        let mut items: Vec<(Option<String>, String, f32)> = Vec::new();
        let mut observation_scores: Vec<f32> = Vec::new();

        for assertion in active.iter().copied() {
            let evidence = assertions_repo::list_evidence(&mut conn, assertion.id).await?;
            if evidence.is_empty() {
                items.push((None, format!("actor:{}", assertion.actor_id.to_hex()), 0.0));
            } else {
                for item in &evidence {
                    // `content_hash` is computed from the fetched body by the crawler, so two
                    // URIs carrying the same hash are the same article -- and one article
                    // syndicated to several domains is one source, the same failure spec
                    // section 62 warns about with a different surface.
                    let content_key = item
                        .content_hash
                        .as_deref()
                        .filter(|hash| !hash.is_empty())
                        .map(|hash| format!("content:{hash}"));
                    let source_key = item
                        .uri
                        .as_deref()
                        .and_then(control_group_for_uri)
                        .unwrap_or_else(|| format!("actor:{}", assertion.actor_id.to_hex()));
                    items.push((content_key, source_key, evidence_type_weight(item.evidence_type)));
                }
            }

            let observations = assertions_repo::list_observations(&mut conn, assertion.id).await?;
            observation_scores.extend(observations.iter().map(|o| verify_result_weight(&o.result)));
        }

        // Grouping is the *union* of the two relations, not either one alone: one organization
        // is one source even when it publishes many distinct articles, and one article is one
        // source even when many organizations carry it. Neither relation implies the other --
        // two distinct domains can carry identical text, and one domain can carry distinct
        // text -- so the grouping is the connected components of the graph that has an edge
        // for each relation.
        let mut uf = UnionFind::new(items.len());
        let mut first_with_key: HashMap<&str, usize> = HashMap::new();
        for (i, (content_key, source_key, _)) in items.iter().enumerate() {
            for key in content_key.iter().chain(std::iter::once(source_key)) {
                match first_with_key.get(key.as_str()) {
                    Some(&first) => uf.union(first, i),
                    None => {
                        first_with_key.insert(key, i);
                    }
                }
            }
        }

        // Per component: the strongest weight, and how many members carry each key. The
        // reported label is the key the members actually share -- the reason they are one
        // source -- falling back to the smallest key when the component is a singleton.
        let mut weights: HashMap<usize, f32> = HashMap::new();
        let mut keys_per_component: HashMap<usize, HashMap<String, usize>> = HashMap::new();
        for (i, (content_key, source_key, weight)) in items.iter().enumerate() {
            let root = uf.find(i);
            weights.insert(root, weights.get(&root).copied().unwrap_or(0.0).max(*weight));
            let keys = keys_per_component.entry(root).or_default();
            if let Some(content_key) = content_key {
                *keys.entry(content_key.clone()).or_insert(0) += 1;
            }
            *keys.entry(source_key.clone()).or_insert(0) += 1;
        }

        let mut source_groups: Vec<String> = keys_per_component
            .values()
            .map(|keys| {
                keys.iter()
                    .filter(|(_, &count)| count > 1)
                    .map(|(key, _)| key)
                    .min()
                    .or_else(|| keys.keys().min())
                    .cloned()
                    .unwrap_or_default()
            })
            .collect();
        source_groups.sort();
        let distinct_source_groups = source_groups.len();

        // One control group, however many assertions cite it, is not
        // independent corroboration (spec section 62's literal example) —
        // two groups is partial, three or more saturates.
        let source_independence =
            ((distinct_source_groups.saturating_sub(1)) as f32 / 2.0).clamp(0.0, 1.0);

        let evidence_strength = if weights.is_empty() {
            0.0
        } else {
            weights.values().sum::<f32>() / weights.len() as f32
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

        let freshness = active
            .iter()
            .map(|a| a.observed_at.unwrap_or(a.asserted_at))
            .max()
            .map(|ts| freshness_from_age(self.now(), ts))
            .unwrap_or(0.0);

        let identity_assurance = if distinct_actors.is_empty() {
            0.0
        } else {
            let mut total = 0.0f32;
            for actor_id in &distinct_actors {
                let score = actors::get_by_id(&mut conn, *actor_id)
                    .await?
                    .map(|actor| identity_assurance_score(&actor))
                    .unwrap_or(0.0);
                total += score;
            }
            total / distinct_actors.len() as f32
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
            freshness,
            identity_assurance,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freshness_is_one_at_zero_age() {
        assert_eq!(freshness_from_age(1_000, 1_000), 1.0);
    }

    #[test]
    fn freshness_is_half_at_one_half_life() {
        let now = 1_000_000_000;
        let half_life = FRESHNESS_HALF_LIFE_SECONDS as i64;
        let got = freshness_from_age(now, now - half_life);
        assert!((got - 0.5).abs() < 1e-4, "got {got}");
    }

    #[test]
    fn freshness_clamps_future_timestamps_to_full_strength() {
        assert_eq!(freshness_from_age(1_000, 5_000), 1.0);
    }

    fn actor_with(public_key: Option<[u8; 32]>, identity_uri: Option<&str>) -> Actor {
        Actor {
            id: oag_core::ActorId::derive(b"identity-assurance-test-actor"),
            actor_type: oag_core::ActorType::Agent,
            name: None,
            public_key,
            identity_uri: identity_uri.map(str::to_string),
            metadata: serde_json::json!({}),
            created_at: 0,
        }
    }

    #[test]
    fn identity_assurance_tiers() {
        assert_eq!(identity_assurance_score(&actor_with(None, None)), 0.0);
        assert_eq!(identity_assurance_score(&actor_with(None, Some("https://example.com/me"))), 0.3);
        assert_eq!(identity_assurance_score(&actor_with(Some([1u8; 32]), None)), 0.7);
        assert_eq!(identity_assurance_score(&actor_with(Some([1u8; 32]), Some("https://example.com/me"))), 1.0);
    }

    #[test]
    fn union_find_joins_transitively_and_is_deterministic() {
        // The grouping question is connectivity, not pairwise equality: A shares a content
        // hash with B, and B shares a domain with C, so all three are one source even though
        // A and C share nothing directly.
        let mut uf = UnionFind::new(4);
        uf.union(0, 1);
        uf.union(1, 2);
        let (a, b, c, d) = (uf.find(0), uf.find(1), uf.find(2), uf.find(3));
        assert_eq!(a, b);
        assert_eq!(b, c, "connectivity must be transitive");
        assert_ne!(a, d, "an item sharing no key stays its own source");

        // Path compression must not change the answer.
        assert_eq!(uf.find(2), uf.find(0));
    }

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
