# MCP Server

**Source**: `crates/oag-mcp/src/{server,transport,params}.rs`.

Mounted at `/mcp` on the same `oag serve` process, over MCP's Streamable HTTP transport. Same
`Authorization: Bearer` auth as REST, resolved from `Extension<http::request::Parts>` (only works
over a real HTTP transport, not an in-memory duplex transport).

## Tools (one per `GraphService` capability, mirroring REST)

| Tool | REST equivalent |
|---|---|
| `graph_search` | `GET /search` |
| `graph_resolve` | `POST /resolve` |
| `graph_get_node` | `GET /nodes/{id}` |
| `graph_get_actor` | `GET /actors/{id}` |
| `graph_get_edges` | `GET /nodes/{id}/edges` |
| `graph_get_edge` | `GET /edges/{id}` |
| `graph_get_corroboration` | `GET /edges/{id}/corroboration` |
| `graph_get_subgraph` | `GET /subgraph` |
| `graph_find_sources` | `GET /nodes/{id}/sources` |
| `graph_get_assertion` | `GET /assertions/{id}` |
| `graph_get_node_assertions` | `GET /nodes/{id}/assertions` |
| `graph_assert` | `POST /assertions` |
| `graph_add_evidence` | `POST /assertions/{id}/evidence` |
| `graph_verify_assertion` | `POST /assertions/{id}/verify` |
| `graph_dispute_assertion` | `POST /assertions/{id}/dispute` |
| `graph_retract_assertion` | `POST /assertions/{id}/retract` |
| `graph_supersede_assertion` | `POST /assertions/{id}/supersede` |
| `graph_get_history` | `GET /history/{object_type}/{id}` |
| `graph_crawl` | `POST /crawl` — also requires `graph:crawl` |

An explicit cross-surface audit during this session confirmed REST and MCP are in exact 1:1 parity
(every route has a tool equivalent except `/status`, correctly — MCP has its own
`initialize` handshake that serves the same purpose). `graph_get_assertion`/
`graph_get_node_assertions` and the REST `/sources` route were each, at different points, found
missing from one surface and added to match.

## Per-tool tracing

Every tool call is wrapped in `self.traced(tool_name, ...)`, its own dedicated tracing span
(`mcp_tool` with `tool`/`peer_id`/`actor_id` fields, `duration_ms`/`result` on completion) —
independent of REST's HTTP-level `TraceLayer`. This is why MCP calls are correctly logged even
though they don't go through the same middleware stack REST's own span-creation uses.

## Rate limiting — a real bug, found and fixed

`/mcp` was mounted via `rest_router.route_service("/mcp", mcp_service)` *after* `rest_router`
already had its own `rate_limit_middleware` layer applied — that layer only ever covered the
routes that existed when it was applied, so `/mcp` silently never got it. Confirmed live: 130
consecutive `/mcp` calls with the same API key all returned 200 with zero 429s, while the identical
key hammering REST correctly got rate-limited after ~120 requests. The request-body-size cap, by
contrast, *did* still apply to `/mcp` (verified independently, not assumed to fail the same way).

This mattered because MCP exposes real write capabilities (`graph_assert`, `graph_crawl` — the
latter makes the peer issue outbound HTTP to a caller-supplied URL), so any actor with any valid
MCP key could hammer the surface at unlimited volume, bypassing a documented defense.

**Fix**: `oag-cli/src/serve.rs`'s `build_app` gives `/mcp` its own small router with its own
`rate_limit_middleware` layer, applied directly rather than relying on `rest_router`'s copy —
sidesteps the layering-order question entirely instead of depending on it. Verified live
post-fix (130 requests now correctly 429 partway through) and covered by a permanent regression
test — `oag-cli`'s first integration test spinning up the real combined REST+MCP app over a real
TCP listener.

## Tests

`crates/oag-mcp/src/tests.rs` (full tool-call coverage via a real Streamable HTTP client, including
`graph_supersede_assertion_flags_the_old_assertion`); `crates/oag-cli/src/serve.rs`'s
`mcp_endpoint_is_rate_limited_same_as_rest`.
