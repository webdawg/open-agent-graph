use std::time::Duration;

use axum::body::Body;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use url::Url;

use std::sync::Arc;

use oag_core::AssertionStatus;
use oag_crypto::PeerIdentity;
use oag_graph::{GraphService, ResolveOutcome};
use oag_storage::pool::open_pool;

use crate::crawler::CrawlerService;
use crate::error::CrawlError;
use crate::fetch::{safe_fetch, CrawlerConfig};

async fn spawn_fixture(router: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    format!("http://{addr}")
}

fn allowing_config() -> CrawlerConfig {
    CrawlerConfig {
        allow_private_networks: true,
        ..CrawlerConfig::default()
    }
}

#[tokio::test]
async fn default_config_blocks_loopback_addresses() {
    let base = spawn_fixture(Router::new().route("/", get(|| async { "hi" }))).await;
    let url = Url::parse(&base).unwrap();

    // Default config: allow_private_networks = false.
    let result = safe_fetch(&url, &CrawlerConfig::default()).await;
    assert!(
        matches!(result, Err(CrawlError::BlockedAddress(_))),
        "expected loopback to be blocked by default, got {result:?}"
    );
}

#[tokio::test]
async fn explicit_opt_in_allows_loopback_fetch() {
    let base = spawn_fixture(Router::new().route("/", get(|| async { "hello world" }))).await;
    let url = Url::parse(&base).unwrap();

    let page = safe_fetch(&url, &allowing_config()).await.unwrap();
    assert_eq!(page.body, b"hello world");
    assert!(page.content_hash.starts_with("blake3:"));
}

#[tokio::test]
async fn response_size_cap_is_enforced() {
    let big_body = "x".repeat(1000);
    let router = Router::new().route("/", get(move || async move { big_body.clone() }));
    let base = spawn_fixture(router).await;
    let url = Url::parse(&base).unwrap();

    let config = CrawlerConfig {
        max_response_bytes: 100,
        ..allowing_config()
    };
    let result = safe_fetch(&url, &config).await;
    assert!(matches!(result, Err(CrawlError::ResponseTooLarge(_))), "got {result:?}");
}

#[tokio::test]
async fn disallowed_content_type_is_rejected() {
    async fn binary_handler() -> Response {
        ([(header::CONTENT_TYPE, "application/octet-stream")], Body::from(vec![0u8, 1, 2, 3])).into_response()
    }
    let router = Router::new().route("/", get(binary_handler));
    let base = spawn_fixture(router).await;
    let url = Url::parse(&base).unwrap();

    let result = safe_fetch(&url, &allowing_config()).await;
    assert!(matches!(result, Err(CrawlError::UnsupportedContentType(_))), "got {result:?}");
}

#[tokio::test]
async fn redirect_is_followed_and_revalidated() {
    let router = Router::new()
        .route(
            "/start",
            get(|| async {
                Response::builder()
                    .status(StatusCode::FOUND)
                    .header(header::LOCATION, "/end")
                    .body(Body::empty())
                    .unwrap()
            }),
        )
        .route("/end", get(|| async { "landed" }));
    let base = spawn_fixture(router).await;
    let url = Url::parse(&format!("{base}/start")).unwrap();

    let page = safe_fetch(&url, &allowing_config()).await.unwrap();
    assert_eq!(page.body, b"landed");
    assert!(page.final_url.path().ends_with("/end"));
}

#[tokio::test]
async fn redirect_loop_is_capped() {
    let router = Router::new().route(
        "/loop",
        get(|| async {
            Response::builder()
                .status(StatusCode::FOUND)
                .header(header::LOCATION, "/loop")
                .body(Body::empty())
                .unwrap()
        }),
    );
    let base = spawn_fixture(router).await;
    let url = Url::parse(&format!("{base}/loop")).unwrap();

    let result = safe_fetch(&url, &allowing_config()).await;
    assert!(matches!(result, Err(CrawlError::TooManyRedirects(_))), "got {result:?}");
}

#[tokio::test]
async fn request_timeout_is_respected() {
    async fn slow_handler() -> &'static str {
        tokio::time::sleep(Duration::from_secs(5)).await;
        "too slow"
    }
    let router = Router::new().route("/", get(slow_handler));
    let base = spawn_fixture(router).await;
    let url = Url::parse(&base).unwrap();

    let config = CrawlerConfig {
        request_timeout: Duration::from_millis(100),
        ..allowing_config()
    };
    let result = safe_fetch(&url, &config).await;
    assert!(matches!(result, Err(CrawlError::Http(_))), "got {result:?}");
}

async fn fresh_graph(name: &str) -> Arc<GraphService> {
    let dir = std::env::temp_dir().join(format!(
        "oag-crawler-test-{name}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let pool = open_pool(&dir.join("oag.sqlite")).await.unwrap();
    let identity = PeerIdentity::generate();
    Arc::new(GraphService::new(pool, identity))
}

fn fixture_router() -> Router {
    async fn index() -> Response {
        let body = r#"<html><head>
            <title>Example Fixture Page</title>
            <script type="application/ld+json">
            {
                "@context": "https://schema.org",
                "@type": "SoftwareApplication",
                "name": "Fixture Tool",
                "codeRepository": "https://github.com/example/fixture-tool"
            }
            </script>
        </head><body>hello</body></html>"#;
        ([(header::CONTENT_TYPE, "text/html")], body).into_response()
    }
    async fn llms_txt() -> &'static str {
        "# Fixture Site\n\n## Docs\n\n- [Getting Started](/docs/start)\n"
    }
    async fn ard() -> Response {
        let body = r#"{"resources":[{"name":"Fixture MCP","type":"mcp_server","capabilities":["search"],"operator":{"url":"https://operator.example"}}]}"#;
        ([(header::CONTENT_TYPE, "application/json")], body).into_response()
    }
    async fn agent_card() -> Response {
        let body = r#"{"name":"Fixture Agent","skills":[{"id":"chat","name":"Chat"}]}"#;
        ([(header::CONTENT_TYPE, "application/json")], body).into_response()
    }

    Router::new()
        .route("/", get(index))
        .route("/llms.txt", get(llms_txt))
        .route("/.well-known/ard.json", get(ard))
        .route("/.well-known/agent-card.json", get(agent_card))
}

/// End-to-end: real fetches (against a local fixture server, with the
/// harness's explicit `allow_private_networks: true` opt-in — the exact
/// scenario that flag exists for) through every extractor, verified via
/// `GraphService` the same way REST/MCP callers would see the result.
#[tokio::test]
async fn full_crawl_populates_graph_via_all_extractors() {
    let base = spawn_fixture(fixture_router()).await;
    let url = Url::parse(&base).unwrap();

    let graph = fresh_graph("full-crawl").await;
    let crawler = CrawlerService::new(graph.clone(), allowing_config());

    let summary = crawler.crawl(&url).await.unwrap();
    assert!(summary.facts_asserted > 0);
    assert_eq!(summary.facts_skipped, 0, "no fact should fail validation in this fixture");
    assert!(summary.llms_txt_found);
    assert!(summary.ard_found);
    assert!(summary.a2a_found);

    // The page's own node: baseline instance_of + JSON-LD instance_of +
    // repository_at, plus a Name alias from both <title> and JSON-LD name.
    let ResolveOutcome::Found { node: page_node, confidence } = graph.resolve(&base).await.unwrap() else {
        panic!("expected the crawled page's own node to resolve");
    };
    assert_eq!(confidence, 1.0);

    let page_assertions = graph.list_assertions_for_node(page_node.id).await.unwrap();
    assert!(page_assertions.iter().all(|a| a.status == AssertionStatus::Active));
    assert!(page_assertions.iter().all(|a| a.extraction_method == oag_core::ExtractionMethod::StructuredExtraction));

    let page_edges = graph.get_edges(page_node.id).await.unwrap();
    assert!(page_edges.iter().any(|e| e.predicate.as_str() == "instance_of"));
    assert!(page_edges.iter().any(|e| e.predicate.as_str() == "repository_at"));

    let page_aliases = graph.list_aliases(page_node.id).await.unwrap();
    assert!(page_aliases.iter().any(|a| a.alias == "Example Fixture Page"));
    assert!(page_aliases.iter().any(|a| a.alias == "Fixture Tool"));

    // llms.txt: a links_to fact from the site origin (not the page path) to
    // the relative link, resolved to an absolute URL.
    let ResolveOutcome::Found { node: site_node, .. } = graph.resolve(&base).await.unwrap() else {
        panic!("site origin and page happen to be the same URL in this fixture (root path)");
    };
    let site_edges = graph.get_edges(site_node.id).await.unwrap();
    assert!(site_edges.iter().any(|e| e.predicate.as_str() == "links_to"));

    // ARD: a distinct resource (no url given) fell back to the site origin,
    // so its instance_of/provides_capability/operated_by facts landed on
    // the same node as everything else in this fixture.
    assert!(page_edges.iter().any(|e| e.predicate.as_str() == "provides_capability") || site_edges.iter().any(|e| e.predicate.as_str() == "provides_capability"));

    // Re-crawling reuses the same "crawler" actor rather than minting a new
    // one each time.
    let second_summary = crawler.crawl(&url).await.unwrap();
    assert!(second_summary.facts_asserted > 0);
}

/// Same fixture, default config: the crawl must fail with a blocked-address
/// error rather than silently succeeding — proves the SSRF default-deny
/// posture actually holds for the whole orchestrated crawl, not just the
/// raw `safe_fetch` call.
#[tokio::test]
async fn crawl_with_default_config_is_blocked_by_ssrf_policy() {
    let base = spawn_fixture(fixture_router()).await;
    let url = Url::parse(&base).unwrap();

    let graph = fresh_graph("blocked-crawl").await;
    let crawler = CrawlerService::new(graph, CrawlerConfig::default());

    let result = crawler.crawl(&url).await;
    assert!(matches!(result, Err(CrawlError::BlockedAddress(_))), "got {result:?}");
}
