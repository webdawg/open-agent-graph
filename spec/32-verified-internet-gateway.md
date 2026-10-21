# Verified Internet Gateway

**Source**: `crates/oag-api/src/gateway.rs`, `crates/oag-api/src/ntp_wire.rs`,
`crates/oag-cli/src/serve.rs` (front-end spawn), `crates/oag-cli/src/config.rs` (`[gateway]`).

A generalized proxy for real public-internet protocols (NTP, HTTP to start — more can be added
later by adding `GatewayService` variants) where a peer **never answers from its own local
state**. Every answer is either read from a cache that only ever holds a *quorum-verified* result,
or produced by asking several independently-configured peers the same question and requiring a
majority to agree before serving anything.

This is a different mechanism from `spec/31-service-receipts.md`'s peer-to-peer content transfer
(which serves *this peer's own* pinned content, receipted but single-sourced) — this feature
never serves this peer's own answer to anything; it only ever serves a cross-checked composite.

## The mechanism

A peer asked to resolve a `GatewayService` (`Ntp`, or `Http { url }`) does, in order:

1. **Cache hit** -- if it holds a still-fresh, previously quorum-verified answer for this exact
   query, serve that. See "Caching" below for why this is also the feature's horizontal-scaling
   mechanism, not just a latency shortcut.
2. **Quorum fan-out** -- if `[gateway].query_peers` is configured, ask every one of them
   concurrently (over a new, signed, peer-to-peer `POST /oag/gateway/v1/query` endpoint -- same
   signature-authenticated posture as `spec/31`'s `/oag/transfer/v1/fetch`, not the REST API's
   bearer-token layer). Requires `[gateway].quorum_min_agree` of them to return the *same* answer
   (NTP: within `tolerance_ms` of each other; HTTP: byte-identical, compared by content hash) or
   fails with `NoQuorum`. Only this step's result is ever written to the cache.
3. **Real origin** -- if not configured with `query_peers`, but configured as a real upstream
   origin for this service (`[gateway.ntp].upstream_servers` / `[gateway.http].allow_upstream`),
   performs one real external fetch/query and returns it as its own single, honest contribution.
   Verification happens one level up, at whichever peer asked *this* one as part of its own
   `query_peers` fan-out.
4. **Relay** -- otherwise, forwards to one of its own `[gateway].relay_peers` (first success wins) --
   the exact same signed, re-sign-per-hop, cycle-and-depth-guarded relay shape
   `spec/31-service-receipts.md` Phase 2 already uses, generalized from "fetch a pinned CID" to
   "resolve this service query." Never forwards a request verbatim; each hop builds and signs a
   fresh request of its own.

If none of these apply, the request fails (`CannotResolve`).

A **real front-end listener** — a UDP NTP server, an HTTP reverse proxy — fronting this for actual
outside, non-OAG clients refuses to even bind/start unless enough `query_peers` are configured to
meet its own `quorum_min_agree` (`gateway::require_quorum_capable`, checked by `oag serve` at
startup). This is a structural guarantee, not a convention: there is no way to run a front-end
listener in an unverified, single-source mode.

## Why this is also a scaling/DDoS-absorption mechanism

Because step 2's result is cached, and a cache hit (step 1) is a perfectly legitimate contribution
when *this* peer is later asked by someone else's fan-out: once *any* peer with `query_peers`
configured produces a quorum-verified answer, every peer that asks it — directly, or transitively
through further relaying — gets that answer back cheaply from cache, itself still just as valid an
input to whichever peer is running *its own* quorum check. The verified result propagates and
multiplies through the network like a cache wave. Which peers end up serving a hot resource is
simply whichever topology an operator configured — there is no automatic discovery/routing here,
the same deliberate scope cut `spec/31-service-receipts.md` Phase 2 already made (see
`OPEN_QUESTIONS.md`).

This only stays honest because **a peer only ever caches the output of its own quorum step**
(step 2) — never a raw single-source answer (steps 3/4). A single lying or wrong cache-serving
peer is caught the same way any other quorum member's bad answer is: it only gets served onward if
it *also* shows up as the majority among however many peers the next asker queries, which requires
collusion among a majority of them -- the same Byzantine-fault-tolerance-via-redundancy property
every quorum system has.

## Caching

- **NTP**: a cached entry is really a verified *offset* in disguise. On every cache read, the
  elapsed wall-clock time since the entry was computed is added back on top of the stored estimate
  — reconstructing the correct current time without re-running the fan-out. TTL is a fixed,
  configured value (`[gateway.ntp].cache_ttl_ms`, default 2000ms) — NTP has no protocol-level
  cache-control concept.
- **HTTP**: TTL is read from the *verified* response's own `Cache-Control` header (`no-store`/
  `no-cache` -> never cached; `max-age=N` -> `N` seconds, capped by
  `[gateway.http].max_cache_ttl_ms`; otherwise `[gateway.http].default_cache_ttl_ms`, default
  5000ms).

## NTP specifics

A minimal, hand-rolled NTPv4 (RFC 5905) packet codec (`ntp_wire.rs`) — no extension fields, no
authentication, no leap-second table, no clock-filter/Marzullo's-algorithm precision refinement.
"Hand-roll the narrow useful subset, not the whole spec," the same posture this project already
takes for its Prometheus exporter and Kubo HTTP client.

- Querying a real upstream server (`[gateway.ntp].upstream_servers`) uses the real, full
  four-timestamp NTP client algorithm (`T1`/`T2`/`T3`/`T4`) -- a real wire exchange, no reason to
  approximate it. The response's sender address is checked against the address actually queried
  (a bare bound UDP socket will happily deliver traffic from anyone).
- Every OAG-to-OAG hop (`query_peer` fanning out, or relaying) carries its time estimate over
  plain signed JSON, not the wire protocol — a simpler RTT/2 correction is applied there instead,
  documented as a deliberate, bounded-error simplification.
- The real front-end UDP server reports `stratum = 2` and `refid = "OAGG"` (never claims to BE a
  stratum-1 reference clock), and echoes the client's own Origin Timestamp exactly as RFC 5905
  requires. Internally, Receive and Transmit Timestamps are both set to the one verified estimate
  (real NTP servers timestamp them separately; this gateway's processing time is negligible enough
  that one shared value is an acceptable simplification, with sub-millisecond precision loss from
  the ms-resolution internal representation this trades for implementation simplicity).
- Live-verified with a real third-party client (a hand-written raw-socket Python NTP client, since
  system `sntp`/`ntpdate` don't support a custom port) against a real running front-end: mode 4,
  stratum 2, refid `OAGG`, Origin Timestamp echoed correctly (within the documented sub-millisecond
  rounding), served time within ~60ms of true wall-clock time end to end through a real quorum of
  3 peers each independently querying the real `pool.ntp.org`.

## HTTP specifics

GET-only for v1 — no custom headers, no POST/PUT. A general-purpose verified HTTP proxy that also
forwards arbitrary request semantics is a much bigger surface (what would "verified" even mean for
a non-idempotent POST?) — deliberately out of scope.

**Convention**: `GET /<target-url>` on the front-end's own listen address — the request path (with
its own query string reattached) *is* the URL to fetch-and-verify. A target URL with its own
un-encoded `?` before its intended query needs percent-encoding to avoid being split early by the
proxy's own request-line parsing; this is a known, documented rough edge of the "URL-as-path"
convention, not a bug.

SSRF-guarded and DNS-pinned via `oag_crawler::fetch::resolve_and_validate`/`build_pinned_client`
(made `pub` for this reuse) -- the identical safety property `oag-crawler`'s own `safe_fetch` has,
minus its narrow content-type allowlist (a general-purpose gateway must proxy arbitrary content).

**A real bug found and fixed during live verification, not a crawler-specific issue**:
`build_pinned_client`'s repeated `reqwest::ClientBuilder::resolve()` calls only keep the *last*
pinned address for a given host — it overwrites rather than accumulating a fallback list. Passing
every DNS-resolved address through (as `oag-crawler`'s own `safe_fetch` already does) means
whichever address happens to be last in DNS-returned order wins, with zero fallback if it's
unreachable. Confirmed live: this environment resolves a real Cloudflare IPv6 address for a
dual-stack site (`example.com`) but has no outbound IPv6 route — "Network is unreachable," no
fallback to the (working) IPv4 address also in the resolved set, since the IPv6 entry happened to
be pinned last. **Fixed in this feature's own `fetch_upstream_http`** by pinning to the first IPv4
address when one exists, before calling `build_pinned_client` — the bias every real-world
dual-stack-broken network needs. Not fixed inside `oag_crawler::fetch` itself, since that's
shared, already-shipped code with its own existing callers/behavior; flagged in
`OPEN_QUESTIONS.md` as a latent issue worth a closer look there too.

## Config (`[gateway]`, `[gateway.ntp]`, `[gateway.http]`)

```toml
[gateway]
query_peers = []           # [{address = "...", peer_id = "..."}, ...] -- same RelayPeer shape as [transfer]
relay_peers = []
hops_remaining = 8         # relay depth budget, same shape as [transfer].relay_peers
fanout_count = 0           # 0 = fan out to every configured query_peers entry
quorum_min_agree = 2

[gateway.ntp]
listen_addr = ""           # unset (default) = real NTP UDP front-end disabled
upstream_servers = []       # e.g. ["pool.ntp.org:123"] -- this peer may be an NTP origin
tolerance_ms = 1000
cache_ttl_ms = 2000

[gateway.http]
listen_addr = ""           # unset (default) = real HTTP front-end disabled
allow_upstream = false      # this peer may be an HTTP origin
allow_private_networks = false   # same SSRF-guard posture as [crawler]
max_response_bytes = 2097152     # 2 MiB
default_cache_ttl_ms = 5000
max_cache_ttl_ms = 60000
```

Every setting defaults to disabled/empty -- a fresh `oag serve` process runs this feature's
peer-to-peer protocol handler (harmless with nothing configured) but starts no real front-end
listener and acts as neither an origin nor a relay for anything, until an operator opts in.

## CLI

`oag gateway query --service ntp|http [--url <url>] [--data-dir] [--config]` -- manually runs this
peer's own configured `[gateway]` resolution (cache/quorum/origin/relay) without needing a real
NTP/HTTP client to trigger it. Useful for testing and for scripted health checks.

## Explicitly out of scope (stated plainly, not silently dropped)

- **Automatic peer/content discovery or routing** -- `query_peers`/`relay_peers` are a
  hand-written, operator-configured list, exactly like `spec/31`'s `relay_peers`. A real
  discovery mechanism (how would a peer even learn *which* upstream peers can answer a given
  query?) is a separate, much bigger, genuinely unbuilt problem.
- **More protocols** (FTP, DNS, whois, ...) -- the engine (`resolve_gateway_query`,
  `fanout_and_verify`, the cache, the relay shape) is protocol-agnostic; adding one is "add a
  `GatewayService` variant plus an origin-resolution function," not a redesign. None are built yet.
- **Provenance/evidence recording** -- unlike `spec/31`'s transfers, a gateway query writes
  nothing to the graph (no assertion, no evidence). This keeps the feature's first version scoped
  to the verification engine and the two real front-ends, not also a new graph-recording shape for
  "I asked, and got a verified answer" events. Revisit if a real audit need shows up.
- **A graph-recorded Byzantine-fault audit of disagreeing peers** -- a peer that answers a fan-out
  with a wrong/stale value is simply outvoted; nothing records *who* disagreed or adjusts their
  trust. This is the same still-unbuilt "witness-based audit" mechanism named in
  `spec/22-future-work.md` and `future_flood_propagation_and_anonymity.md`/
  `future_service_announcements_and_audit.md` (memory) -- this feature's quorum fan-out is a
  structurally similar mechanism (many independent paths, compare answers) but deliberately does
  not yet feed that still-undesigned audit/trust-consequence layer.
- **Non-GET HTTP, custom headers/cookies, HTTPS origin identity for the proxied site** -- the
  front-end only ever returns the verified response body/status; it doesn't forward request
  headers to the upstream or preserve response headers (other than using `Cache-Control` for its
  own TTL decision) back to the client.

## Tests

`crates/oag-api/src/gateway.rs` (12 unit tests: request/response sign-verify round trips and
tamper rejection, NTP quorum clustering including outlier rejection, HTTP quorum requiring matching
content hash, cache TTL from `Cache-Control` including `no-store` and the `max-age` cap, cache
offset reconstruction on read, cache TTL-expiry and zero-TTL no-op); `crates/oag-api/src/ntp_wire.rs`
(4 unit tests: NTP timestamp round-trip, client-request round-trip, server-response rejects being
parsed as a client request, server response correctly echoes the client's Origin Timestamp).

Live-verified with four real `oag serve` processes (three independent leaf peers, each configured
with real NTP/HTTP upstream access; one front peer with all three as `query_peers`,
`quorum_min_agree = 2`, and both real front-end listeners enabled) -- see the NTP and HTTP
"specifics" sections above for the exact live results, including the dual-stack IPv6 bug found and
fixed along the way.
