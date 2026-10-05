# Crawler

**Source**: `crates/oag-crawler/src/{crawler,fetch,ssrf,robots,llm_extract,extract/*}.rs`.

Turns a crawled page into the same signed events any other caller would produce — no separate
bookkeeping path.

## Crawl sequence (`CrawlerService::crawl`)

1. `robots.txt` check.
2. `safe_fetch` (SSRF-guarded, see below).
3. **Baseline assertion**: `instance_of` the page's own URL → `concept:website` or
   `concept:document` (near-certain confidence, 0.95 — "we directly observed this URL serving this
   content," unlike everything extracted from it). This guarantees the page's own node exists
   before any alias or extracted fact is attached to it.
4. Content-type-driven structured extraction (see below).
5. LLM extraction fallback over the page's own text, if configured (disabled by default).
6. Best-effort `llms.txt`, `/.well-known/ard.json`, `/.well-known/agent-card.json` discovery.

Every call counts toward the `crawls_total` metric; failures also count toward `crawls_failed`.

## SSRF defenses (`ssrf.rs`)

Blocks, by default: loopback, RFC 1918 private ranges, link-local (covers the
`169.254.169.254` cloud-metadata endpoint for free, since that address is itself link-local),
unspecified, broadcast, documentation, and multicast ranges, on both IPv4 and IPv6 (including
IPv4-mapped IPv6, a common bypass vector). Checked against the *resolved* IP, not the hostname
string, so DNS rebinding to a blocked address is caught. `--allow-private-networks` opts in
explicitly, off by default.

## Fetch limits (`fetch.rs`)

Redirect cap, response-size cap (streamed, never buffered whole before the cap is checked),
request timeout. `ALLOWED_CONTENT_TYPES` is a safe allowlist (`text/html`, `application/xhtml+xml`,
`application/json`, `application/ld+json`, `text/plain`, `text/markdown`), checked against the
`Content-Type` header with any `; charset=...` parameter stripped and lowercased first.
`text/markdown` was missing from this list until added — the exact content-type real markdown
hosting typically serves, and without it the crawler couldn't ingest any markdown-serving site at
all, including the one used to validate this fix live.

## Structured extraction (content-type-driven)

| Content-Type | Extractor | Finds |
|---|---|---|
| `text/html`, `application/xhtml+xml` | `html_meta` + `json_ld` | `<title>`/`og:title` → Name alias; embedded JSON-LD |
| `application/ld+json` | `json_ld` | JSON-LD directly |
| `text/markdown` | `markdown` | First top-level `# ` heading → Name alias |

### Markdown title extraction — found missing, added

Plain markdown has no `<title>` tag, so crawled markdown nodes had no name/alias at all until
`extract/markdown.rs` was added, mirroring `html_meta.rs`'s exact shape: the first `# ` heading
becomes a `Name`-type alias. Deliberately only the *first* H1 — later ones are section headings,
not the document's own title. Verified live: re-crawling a real markdown page went from 1 declared
alias (llms.txt-derived) to 2 (plus the new title alias).

## LLM extraction (`llm_extract.rs`)

Disabled by default; never talks to anything unless `[crawler].llm_provider = "openai_compatible"`
is explicitly configured. Candidates are recorded with `extraction_method: LlmExtraction`, a
per-candidate confidence, and a dedicated extractor actor named after the model — the same
provenance machinery `assert()` already provides for every other extraction method.

## CLI / REST / MCP surface

| Surface | Form |
|---|---|
| CLI | `oag crawl <url> [--allow-private-networks]` |
| REST | `POST /crawl`, requires `graph:crawl` |
| MCP | `graph_crawl`, requires `graph:crawl` |

No per-request `allow_private_networks` override on REST/MCP — only the operator's own
`config.toml` can widen the SSRF policy. See [09-authentication-and-authorization.md](09-authentication-and-authorization.md)
for why `graph:crawl` is a separate permission from `graph:assert`.

## Tests

`crates/oag-crawler/src/tests.rs` (SSRF blocking/opt-in, response-size/redirect/timeout caps,
`markdown_content_type_is_allowed`, `crawling_markdown_declares_h1_as_name_alias`,
`full_crawl_populates_graph_via_all_extractors`); `crates/oag-crawler/src/ssrf.rs`'s own
IP-range unit tests; `crates/oag-crawler/src/robots.rs`'s parser tests.
