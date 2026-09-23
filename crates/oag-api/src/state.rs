use std::sync::Arc;

use oag_graph::GraphService;

use crate::rate_limit::RateLimiter;

#[derive(Clone)]
pub struct AppState {
    pub graph: Arc<GraphService>,
    pub rate_limiter: Arc<RateLimiter>,
}

impl AppState {
    pub fn new(graph: Arc<GraphService>) -> Self {
        Self {
            graph,
            rate_limiter: Arc::new(RateLimiter::default()),
        }
    }
}
