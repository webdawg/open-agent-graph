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
    "text/markdown",
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
/// `pub` (not just crate-internal) so other crates needing the exact same
/// DNS-rebinding-safe SSRF guard -- resolve once, validate every returned
/// address, let the caller pin its connection to exactly those -- can reuse
/// it instead of re-implementing it at a weaker standard. `oag-api`'s
/// gateway feature is the first other caller: it needs the identical guard
/// for arbitrary-content-type HTTP fetches, which this crate's own
/// `safe_fetch` can't be reused for directly (it enforces a narrow
/// crawler-specific `ALLOWED_CONTENT_TYPES` allowlist that a general-purpose
/// proxy must not).
pub async fn resolve_and_validate(
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

/// `pub` for the same reason as `resolve_and_validate` above.
pub fn build_pinned_client(
    host: &str,
    addrs: &[SocketAddr],
    config: &CrawlerConfig,
) -> Result<reqwest::Client, CrawlError> {
    let mut builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(config.request_timeout)
        .user_agent(config.user_agent.clone());
    // reqwest's `resolve` maps a host to exactly ONE address and *overwrites* on every call, so
    // looping over the set silently keeps only the LAST entry. That is a real failure mode, not a
    // theoretical one: a dual-stack host resolved to [IPv4, IPv6] with no outbound IPv6 route
    // failed outright with zero fallback to the working IPv4 address, purely because IPv6
    // happened to be resolved last. Pin exactly one address, chosen by the caller --
    // `prefer_ipv4` below is the usual choice. Pinning nothing would let the system resolver
    // decide, which is exactly the DNS-reordering lottery this pinning exists to remove.
    if let Some(addr) = addrs.first() {
        // TLS SNI/certificate validation still uses `host` (the URL/Host
        // header is unchanged) — only the actual socket target is pinned.
        builder = builder.resolve(host, *addr);
    }
    builder.build().map_err(|e| CrawlError::ClientBuild(e.to_string()))
}

/// Reduces a resolved address set to a single IPv4 address when one is present, and otherwise
/// leaves it untouched.
///
/// This is the bias every real-world dual-stack-broken network needs: when a host has both an A
/// and an AAAA record and this machine has no usable IPv6 route, the IPv6 attempt fails and there
/// is no fallback -- `build_pinned_client` pins one address, so "whichever came last" is the only
/// choice on offer. Preferring IPv4 makes the outcome deterministic and matches what a browser or
/// `curl` does on such a network. An IPv6-only host has no A record, so it is unaffected.
pub fn prefer_ipv4(addrs: Vec<SocketAddr>) -> Vec<SocketAddr> {
    match addrs.iter().position(|a| a.is_ipv4()) {
        Some(pos) => vec![addrs[pos]],
        None => addrs,
    }
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
        // Pin a deterministic address rather than whichever the resolver happened to return last:
        // see `prefer_ipv4` for why the unpinned order is a real failure mode.
        let addrs = prefer_ipv4(addrs);
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::get;
    use axum::Router;

    fn v4(port: u16) -> SocketAddr {
        format!("127.0.0.1:{port}").parse().unwrap()
    }

    fn v6(port: u16) -> SocketAddr {
        format!("[::1]:{port}").parse().unwrap()
    }

    #[test]
    fn prefer_ipv4_reduces_a_dual_stack_set_to_its_ipv4_address() {
        // The order the resolver happened to return must not decide the outcome.
        let result = prefer_ipv4(vec![v6(80), v4(80), v4(8080)]);
        assert_eq!(result, vec![v4(80)]);
    }

    #[test]
    fn prefer_ipv4_leaves_an_ipv6_only_set_untouched() {
        // An IPv6-only host has no A record, so it must still be reachable -- the bias is a
        // preference for IPv4 when one exists, never a requirement for it.
        let result = prefer_ipv4(vec![v6(80), v6(8080)]);
        assert_eq!(result, vec![v6(80), v6(8080)]);
    }

    #[test]
    fn prefer_ipv4_passes_an_empty_set_through() {
        assert!(prefer_ipv4(vec![]).is_empty());
    }

    /// `build_pinned_client` pins exactly one address: reqwest's `resolve` maps a host to a single
    /// SocketAddr and *overwrites* on every call, so looping over the set silently kept only the
    /// last entry. With the set `[IPv4-live, IPv6-dead]` the fetch must succeed -- under the old
    /// last-wins behaviour it would have pinned the dead IPv6 entry and failed with no fallback.
    #[tokio::test]
    async fn build_pinned_client_pins_the_first_address_in_the_set() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            axum::serve(listener, Router::new().route("/", get(|| async { "hello" })))
                .await
                .unwrap();
        });

        let client = build_pinned_client(
            "localhost",
            &[v4(port), v6(port)],
            &CrawlerConfig::default(),
        )
        .unwrap();
        let response = client
            .get(format!("http://localhost:{port}/"))
            .send()
            .await
            .expect("the first address (live IPv4) must be the one pinned");
        assert_eq!(response.status(), 200);
    }

    /// The complementary case, kept because it is the failure the fix removes: with the unusable
    /// address *first*, pinning it fails and there is no fallback. This is why `safe_fetch` runs
    /// the set through `prefer_ipv4` before pinning rather than relying on resolver order.
    #[tokio::test]
    async fn pinning_an_unusable_first_address_fails_without_fallback() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            axum::serve(listener, Router::new().route("/", get(|| async { "hello" })))
                .await
                .unwrap();
        });

        let client = build_pinned_client(
            "localhost",
            &[v6(port), v4(port)],
            &CrawlerConfig { request_timeout: Duration::from_secs(2), ..Default::default() },
        )
        .unwrap();
        let result = client.get(format!("http://localhost:{port}/")).send().await;
        assert!(
            result.is_err(),
            "pinning the unusable first address must fail -- there is no second attempt"
        );
    }

    /// The path `safe_fetch` actually takes: an unpinned resolver order decides nothing, because
    /// the set is reduced to one address first.
    #[tokio::test]
    async fn the_safe_fetch_path_reduces_a_dual_stack_set_before_pinning() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            axum::serve(listener, Router::new().route("/", get(|| async { "hello" })))
                .await
                .unwrap();
        });

        // Worst-case resolver order: the address that cannot work comes last.
        let resolved = vec![v4(port), v6(port)];
        let pinned = prefer_ipv4(resolved);
        assert_eq!(pinned, vec![v4(port)]);

        let client = build_pinned_client("localhost", &pinned, &CrawlerConfig::default()).unwrap();
        let response = client
            .get(format!("http://localhost:{port}/"))
            .send()
            .await
            .expect("the reduced set must fetch successfully");
        assert_eq!(response.status(), 200);
    }
}
