use std::net::SocketAddr;
use std::time::Duration;

use futures_util::StreamExt;
use url::Url;

use crate::error::CrawlError;
use crate::robots::RobotsRules;
use crate::ssrf::is_blocked_ip;

const MAX_REDIRECTS: u8 = 5;
const ALLOWED_CONTENT_TYPES: &[&str] = &[
    "text/html",
    "application/xhtml+xml",
    "application/json",
    "application/ld+json",
    "text/plain",
];

#[derive(Debug, Clone)]
pub struct CrawlerConfig {
    pub allow_private_networks: bool,
    pub max_response_bytes: usize,
    pub request_timeout: Duration,
    pub user_agent: String,
}

impl Default for CrawlerConfig {
    fn default() -> Self {
        Self {
            allow_private_networks: false,
            max_response_bytes: 10 * 1024 * 1024,
            request_timeout: Duration::from_secs(30),
            user_agent: "oag-crawler/0.1 (+https://github.com/webdawg/open-agent-graph)".to_string(),
        }
    }
}

#[derive(Debug)]
pub struct FetchedPage {
    pub final_url: Url,
    pub content_type: String,
    pub body: Vec<u8>,
    /// `blake3:<hex>` — spec section 24 prefers BLAKE3 internally.
    pub content_hash: String,
}

/// Resolve `host` ourselves and validate *every* returned address, so the
/// caller can pin the actual connection to exactly these — closing the
/// classic DNS-rebinding gap (validate one address, connect to a different
/// one the attacker's DNS returns a moment later). Shared by the main page
/// fetch, each redirect hop, and robots.txt/llms.txt/ARD/A2A's own
/// same-origin fetches — every network access this crate makes goes
/// through this.
async fn resolve_and_validate(
    host: &str,
    port: u16,
    allow_private_networks: bool,
) -> Result<Vec<SocketAddr>, CrawlError> {
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
        .await
        .map_err(|e| CrawlError::DnsResolutionFailed(host.to_string(), e))?
        .collect();
    if addrs.is_empty() {
        return Err(CrawlError::NoAddresses(host.to_string()));
    }
    if !allow_private_networks {
        for addr in &addrs {
            if is_blocked_ip(addr.ip()) {
                return Err(CrawlError::BlockedAddress(addr.ip()));
            }
        }
    }
    Ok(addrs)
}

fn build_pinned_client(
    host: &str,
    addrs: &[SocketAddr],
    config: &CrawlerConfig,
) -> Result<reqwest::Client, CrawlError> {
    let mut builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(config.request_timeout)
        .user_agent(config.user_agent.clone());
    for addr in addrs {
        // TLS SNI/certificate validation still uses `host` (the URL/Host
        // header is unchanged) — only the actual socket target is pinned.
        builder = builder.resolve(host, *addr);
    }
    builder.build().map_err(|e| CrawlError::ClientBuild(e.to_string()))
}

/// Fetch one URL safely: SSRF-validated and DNS-pinned, with a manual
/// redirect loop (spec section 72 — each hop gets the *same* full
/// validation as the original URL, otherwise a redirect is a trivial SSRF
/// bypass), a streamed response-size cap, and a content-type allowlist.
pub async fn safe_fetch(url: &Url, config: &CrawlerConfig) -> Result<FetchedPage, CrawlError> {
    let mut current = url.clone();

    for hop in 0..=MAX_REDIRECTS {
        let scheme = current.scheme();
        if scheme != "http" && scheme != "https" {
            return Err(CrawlError::UnsupportedScheme(scheme.to_string()));
        }
        let host = current.host_str().ok_or(CrawlError::NoHost)?.to_string();
        let port = current.port_or_known_default().ok_or(CrawlError::NoHost)?;

        let addrs = resolve_and_validate(&host, port, config.allow_private_networks).await?;
        let client = build_pinned_client(&host, &addrs, config)?;

        let response = client.get(current.clone()).send().await?;
        let status = response.status();

        if status.is_redirection() {
            if hop == MAX_REDIRECTS {
                return Err(CrawlError::TooManyRedirects(MAX_REDIRECTS));
            }
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .ok_or(CrawlError::RedirectWithoutLocation)?
                .to_string();
            current = current
                .join(&location)
                .map_err(|_| CrawlError::RedirectWithoutLocation)?;
            continue;
        }

        response.error_for_status_ref()?;

        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_lowercase();
        if !ALLOWED_CONTENT_TYPES.contains(&content_type.as_str()) {
            return Err(CrawlError::UnsupportedContentType(content_type));
        }

        if let Some(len) = response.content_length() {
            if len as usize > config.max_response_bytes {
                return Err(CrawlError::ResponseTooLarge(config.max_response_bytes));
            }
        }

        let final_url = response.url().clone();
        let mut body = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            body.extend_from_slice(&chunk);
            if body.len() > config.max_response_bytes {
                return Err(CrawlError::ResponseTooLarge(config.max_response_bytes));
            }
        }

        let content_hash = oag_crypto::blake3_content_hash(&body);
        return Ok(FetchedPage {
            final_url,
            content_type,
            body,
            content_hash,
        });
    }

    Err(CrawlError::TooManyRedirects(MAX_REDIRECTS))
}

/// Fetch `/robots.txt` at `url`'s origin and evaluate whether `url`'s path
/// is allowed. A missing or unfetchable robots.txt means everything is
/// allowed (the standard's own fallback) — this itself goes through
/// `safe_fetch`, so it's just as SSRF-guarded as any other request.
pub async fn check_robots(url: &Url, config: &CrawlerConfig) -> RobotsRules {
    let Ok(mut robots_url) = url.join("/robots.txt") else {
        return RobotsRules::allow_all();
    };
    robots_url.set_query(None);
    match safe_fetch(&robots_url, config).await {
        Ok(page) => RobotsRules::parse(&String::from_utf8_lossy(&page.body), &config.user_agent),
        Err(_) => RobotsRules::allow_all(),
    }
}
