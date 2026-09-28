use std::sync::Arc;

use oag_core::{ActorType, ExtractionMethod, NodeId, Permission};
use oag_graph::{AssertInput, AuthContext, EvidenceInput, GraphService, ResolveOutcome};
use url::Url;

use crate::error::CrawlError;
use crate::extract::{self, origin_of, ExtractedPage};
use crate::fetch::{check_robots, safe_fetch, CrawlerConfig};
use crate::llm_extract::LlmExtractor;

const CRAWLER_ACTOR_NAME: &str = "crawler";

/// Page text past this length is truncated before being sent to an LLM
/// extractor (spec section 74) -- bounds prompt size/cost; deterministic
/// extraction (spec section 73) already ran over the full page and doesn't
/// use this limit.
const LLM_EXTRACTION_MAX_CHARS: usize = 8_000;

#[derive(Debug, Default)]
pub struct CrawlSummary {
    pub page_url: String,
    pub facts_asserted: usize,
    pub facts_skipped: usize,
    pub aliases_declared: usize,
    pub llms_txt_found: bool,
    pub ard_found: bool,
    pub a2a_found: bool,
    pub llm_candidates_asserted: usize,
}

fn now_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Ties the fetch pipeline (SSRF-guarded, robots-checked) and the
/// structured extractors together, turning what they find into the same
/// signed events REST/MCP produce — no parallel write path (spec section
/// 90).
pub struct CrawlerService {
    graph: Arc<GraphService>,
    config: CrawlerConfig,
    llm_extractor: Arc<dyn LlmExtractor>,
}

impl CrawlerService {
    pub fn new(graph: Arc<GraphService>, config: CrawlerConfig, llm_extractor: Arc<dyn LlmExtractor>) -> Self {
        Self { graph, config, llm_extractor }
    }

    /// Reuses a single, stable "crawler" actor across separate `oag crawl`
    /// invocations (spec section 26 — one peer, one crawler actor), rather
    /// than `declare_actor`'s normal per-call id derivation minting a fresh
    /// identity every time. No API key is issued or needed: the CLI
    /// operates with the same direct local trust `oag key create`/`oag
    /// peer *` already do — this `AuthContext` is constructed in-process,
    /// never passed over the network.
    async fn crawler_auth(&self) -> Result<AuthContext, CrawlError> {
        let actor_id = match self
            .graph
            .find_actor_by_name(CRAWLER_ACTOR_NAME, ActorType::Crawler)
            .await?
        {
            Some(actor) => actor.id,
            None => {
                self.graph
                    .declare_actor(ActorType::Crawler, Some(CRAWLER_ACTOR_NAME.to_string()), None, None)
                    .await?
            }
        };
        Ok(AuthContext {
            actor_id,
            permissions: vec![Permission::GraphAssert],
        })
    }

    /// Reuses a single actor per distinct model, named after that model
    /// (spec section 74's "extractor identity" and "model information" are
    /// then both just this actor's declared name -- discoverable via `oag
    /// actor get` like any other actor, not a separate bookkeeping field).
    /// Only ever called when there's at least one candidate to assert, so a
    /// disabled extractor (whose `model_name()` is never reached because
    /// `extract` always returns empty) never causes an actor to be
    /// declared.
    async fn llm_extractor_auth(&self) -> Result<AuthContext, CrawlError> {
        let actor_name = format!("llm-extractor:{}", self.llm_extractor.model_name());
        let actor_id = match self.graph.find_actor_by_name(&actor_name, ActorType::Model).await? {
            Some(actor) => actor.id,
            None => {
                self.graph
                    .declare_actor(ActorType::Model, Some(actor_name), None, None)
                    .await?
            }
        };
        Ok(AuthContext {
            actor_id,
            permissions: vec![Permission::GraphAssert],
        })
    }

    /// Crawl one URL: robots.txt check, safe fetch, baseline
    /// `instance_of` assertion (guarantees the page's own node exists),
    /// then whatever JSON-LD/HTML-meta/llms.txt/ARD/A2A facts and aliases
    /// were found — each attached to the evidence it actually came from.
    pub async fn crawl(&self, url: &Url) -> Result<CrawlSummary, CrawlError> {
        let auth = self.crawler_auth().await?;
        let mut summary = CrawlSummary {
            page_url: url.to_string(),
            ..Default::default()
        };

        let robots = check_robots(url, &self.config).await;
        if !robots.is_allowed(url.path()) {
            return Err(CrawlError::RobotsDisallowed);
        }

        let page = safe_fetch(url, &self.config).await?;
        let final_url = page.final_url.to_string();
        let is_html = page.content_type == "text/html" || page.content_type == "application/xhtml+xml";

        let page_evidence = EvidenceInput {
            evidence_type: Some("web_page".to_string()),
            uri: Some(final_url.clone()),
            content_hash: Some(page.content_hash.clone()),
            retrieved_at: Some(now_ts()),
            ..Default::default()
        };

        // Baseline assertion (see crate docs / plan): guarantees the page's
        // own node exists before any alias or extracted relationship is
        // attached to it. Near-certain — we directly observed this URL
        // serving this content — so a high confidence, unlike everything
        // extracted from it.
        self.graph
            .assert(
                &auth,
                AssertInput {
                    subject: final_url.clone(),
                    subject_type: Some(if is_html { "website".to_string() } else { "document".to_string() }),
                    predicate: "instance_of".to_string(),
                    object: if is_html { "concept:website".to_string() } else { "concept:document".to_string() },
                    object_type: Some("concept".to_string()),
                    evidence: vec![page_evidence.clone()],
                    actor_confidence: Some(0.95),
                    observed_at: None,
                    extraction_method: Some(ExtractionMethod::StructuredExtraction),
                },
            )
            .await?;
        summary.facts_asserted += 1;

        let body_str = String::from_utf8_lossy(&page.body).to_string();
        let mut page_extracted = ExtractedPage::default();
        match page.content_type.as_str() {
            "text/html" | "application/xhtml+xml" => {
                page_extracted.merge(extract::html_meta::extract(&body_str, &final_url));
                page_extracted.merge(extract::json_ld::extract_from_html(&body_str, &final_url));
            }
            "application/ld+json" => {
                page_extracted.merge(extract::json_ld::extract_from_json(&body_str, &final_url));
            }
            _ => {}
        }
        self.assert_extracted(&auth, &page_extracted, page_evidence.clone(), 0.7, &mut summary).await;

        // Spec section 73's priority order: LLM extraction (section 74) is
        // the fallback pass, run after every deterministic extractor above,
        // over the page's own text. A disabled extractor returns no
        // candidates, so this is a no-op (no actor declared, no requests
        // made) when LLM extraction isn't configured.
        if is_html {
            let text = extract::body_text(&body_str, LLM_EXTRACTION_MAX_CHARS);
            if !text.trim().is_empty() {
                if let Ok(candidates) = self.llm_extractor.extract(&text, &final_url).await {
                    if !candidates.is_empty() {
                        let llm_auth = self.llm_extractor_auth().await?;
                        for candidate in candidates {
                            let result = self
                                .graph
                                .assert(
                                    &llm_auth,
                                    AssertInput {
                                        subject: candidate.subject,
                                        subject_type: None,
                                        predicate: candidate.predicate,
                                        object: candidate.object,
                                        object_type: None,
                                        evidence: vec![EvidenceInput {
                                            evidence_type: Some("agent_observation".to_string()),
                                            ..page_evidence.clone()
                                        }],
                                        actor_confidence: Some(candidate.confidence),
                                        observed_at: None,
                                        extraction_method: Some(ExtractionMethod::LlmExtraction),
                                    },
                                )
                                .await;
                            match result {
                                Ok(_) => summary.llm_candidates_asserted += 1,
                                Err(_) => summary.facts_skipped += 1,
                            }
                        }
                    }
                }
            }
        }

        let site_origin = origin_of(url);

        if let Ok(llms_url) = url.join("/llms.txt") {
            if let Ok(llms_page) = safe_fetch(&llms_url, &self.config).await {
                let extracted = extract::llms_txt::parse(&String::from_utf8_lossy(&llms_page.body), &llms_url);
                let evidence = EvidenceInput {
                    evidence_type: Some("documentation".to_string()),
                    uri: Some(llms_url.to_string()),
                    content_hash: Some(llms_page.content_hash.clone()),
                    retrieved_at: Some(now_ts()),
                    ..Default::default()
                };
                // spec section 79: inclusion in llms.txt does not itself
                // grant authority — kept at a moderate, not high, confidence.
                self.assert_extracted(&auth, &extracted, evidence, 0.5, &mut summary).await;
                summary.llms_txt_found = true;
            }
        }

        if let Ok(ard_url) = url.join("/.well-known/ard.json") {
            if let Ok(ard_page) = safe_fetch(&ard_url, &self.config).await {
                let extracted = extract::ard::parse(&String::from_utf8_lossy(&ard_page.body), &site_origin);
                let evidence = EvidenceInput {
                    evidence_type: Some("api_response".to_string()),
                    uri: Some(ard_url.to_string()),
                    content_hash: Some(ard_page.content_hash.clone()),
                    retrieved_at: Some(now_ts()),
                    ..Default::default()
                };
                self.assert_extracted(&auth, &extracted, evidence, 0.7, &mut summary).await;
                summary.ard_found = true;
            }
        }

        if let Ok(a2a_url) = url.join("/.well-known/agent-card.json") {
            if let Ok(a2a_page) = safe_fetch(&a2a_url, &self.config).await {
                let extracted = extract::a2a::parse(&String::from_utf8_lossy(&a2a_page.body), &final_url);
                let evidence = EvidenceInput {
                    evidence_type: Some("api_response".to_string()),
                    uri: Some(a2a_url.to_string()),
                    content_hash: Some(a2a_page.content_hash.clone()),
                    retrieved_at: Some(now_ts()),
                    ..Default::default()
                };
                self.assert_extracted(&auth, &extracted, evidence, 0.7, &mut summary).await;
                summary.a2a_found = true;
            }
        }

        Ok(summary)
    }

    async fn assert_extracted(
        &self,
        auth: &AuthContext,
        page: &ExtractedPage,
        evidence: EvidenceInput,
        confidence: f32,
        summary: &mut CrawlSummary,
    ) {
        for fact in &page.facts {
            let result = self
                .graph
                .assert(
                    auth,
                    AssertInput {
                        subject: fact.subject.clone(),
                        subject_type: fact.subject_type.clone(),
                        predicate: fact.predicate.clone(),
                        object: fact.object.clone(),
                        object_type: fact.object_type.clone(),
                        evidence: vec![evidence.clone()],
                        actor_confidence: Some(confidence),
                        observed_at: None,
                        extraction_method: Some(ExtractionMethod::StructuredExtraction),
                    },
                )
                .await;
            match result {
                Ok(_) => summary.facts_asserted += 1,
                Err(_) => summary.facts_skipped += 1,
            }
        }

        for alias in &page.aliases {
            // The alias's node must already exist — guaranteed for every
            // alias our own extractors produce (each aliases a node that
            // one of this same source's own facts asserts something
            // about), but a fact that itself got skipped (e.g. failed
            // validation) could leave an alias with nowhere to attach; that
            // alias is then just skipped too, not a hard failure.
            if let Ok(node_id) = self.resolve_node_id(&alias.node_identifier).await {
                if self
                    .graph
                    .declare_alias(auth, node_id, alias.alias.clone(), alias.alias_type)
                    .await
                    .is_ok()
                {
                    summary.aliases_declared += 1;
                }
            }
        }
    }

    async fn resolve_node_id(&self, identifier: &str) -> Result<NodeId, CrawlError> {
        match self.graph.resolve(identifier).await? {
            ResolveOutcome::Found { node, .. } => Ok(node.id),
            _ => Err(CrawlError::Graph(oag_graph::GraphError::NotFound(format!(
                "node for {identifier}"
            )))),
        }
    }
}
