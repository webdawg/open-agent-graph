use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchParams {
    pub query: String,
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ResolveParams {
    pub value: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NodeIdParams {
    pub node_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EdgeIdParams {
    pub edge_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SubgraphParams {
    pub node_id: String,
    pub depth: Option<u32>,
    pub max_nodes: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EvidenceParam {
    #[serde(rename = "type")]
    pub evidence_type: Option<String>,
    pub uri: Option<String>,
    pub title: Option<String>,
    pub excerpt: Option<String>,
    pub content_hash: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AssertParams {
    pub subject: String,
    pub subject_type: Option<String>,
    pub predicate: String,
    pub object: String,
    pub object_type: Option<String>,
    #[serde(default)]
    pub evidence: Vec<EvidenceParam>,
    pub confidence: Option<f32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AddEvidenceParams {
    pub assertion_id: String,
    #[serde(flatten)]
    pub evidence: EvidenceParam,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct VerifyParams {
    pub assertion_id: String,
    pub result: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DisputeParams {
    pub assertion_id: String,
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RetractParams {
    pub assertion_id: String,
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct HistoryParams {
    pub object_type: String,
    pub id: String,
}
