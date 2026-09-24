#[derive(Debug, thiserror::Error)]
pub enum CrawlError {
    #[error("unsupported URL scheme '{0}', only http/https are crawled")]
    UnsupportedScheme(String),
    #[error("URL has no host")]
    NoHost,
    #[error("DNS resolution failed for '{0}': {1}")]
    DnsResolutionFailed(String, std::io::Error),
    #[error("DNS resolution for '{0}' returned no addresses")]
    NoAddresses(String),
    #[error("address {0} is blocked by SSRF policy (private/loopback/link-local/reserved)")]
    BlockedAddress(std::net::IpAddr),
    #[error("blocked by robots.txt")]
    RobotsDisallowed,
    #[error("too many redirects (limit {0})")]
    TooManyRedirects(u8),
    #[error("redirect had no Location header")]
    RedirectWithoutLocation,
    #[error("response exceeded the {0}-byte size cap")]
    ResponseTooLarge(usize),
    #[error("unsupported content-type '{0}'")]
    UnsupportedContentType(String),
    #[error("http request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("http client build failed: {0}")]
    ClientBuild(String),
    #[error(transparent)]
    Graph(#[from] oag_graph::GraphError),
}
