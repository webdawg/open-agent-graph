# Human Interface

**Source**: `crates/oag-api/src/human.rs`, `crates/oag-api/templates/{node,assertion,search}.html`.

A minimal human-browsable HTML view, secondary to the APIs, making provenance visually
inspectable.

## Routes

| Route | Shows |
|---|---|
| `GET /ui/search?key=&q=` | A plain GET form over `GraphService::search`, results linking to node pages |
| `GET /ui/nodes/{id}?key=` | Name, type, canonical identifier, aliases, relationships, assertions (with evidence/disputes/retraction/last observation), history |
| `GET /ui/assertions/{id}?key=` | Subject/predicate/object, actor, origin peer, event id, signature status, evidence, verification, disputes, retraction, supersession |

## Auth

Reuses the exact same `graph:read` API keys as REST, accepted via a `?key=` query parameter — a
plain browser link can't set an `Authorization` header. This keeps the read-security posture
identical to REST; nothing becomes newly public on any deployment. Known, accepted tradeoff: the
key ends up in browser history and any server access logs that record query strings — but this
project's own request-logging span logs `route` as the path template, never the full query
string, so it does *not* leak the key through this project's own logging (a reverse proxy in front
of a real deployment might still capture it, which is why this remains a documented tradeoff, not
a closed question).

## Rendering and XSS

`askama` (compile-time HTML templates), not hand-rolled string building. Several displayed fields
(crawled page titles, evidence excerpts, actor names) are attacker/crawler-controlled strings —
hand-rolled escaping would be a real stored-XSS risk class, not a style preference. Every
interpolated value goes through askama's default (escaped) interpolation; nothing is marked
`|safe` anywhere. askama's escaper uses numeric character references (`&#60;script&#62;`), not
named entities (`&lt;script&gt;`) — confirmed by test against a deliberately hostile payload.

## Retraction detail — found and fixed

Both templates originally showed full dispute detail (reason, actor, timestamp) per assertion but
only the bare `status: "retracted"` flag for retractions. Fixed at the same time as REST/MCP's
equivalent gap (see [05-assertions-lifecycle.md](05-assertions-lifecycle.md)) — both templates now
render a "Retraction" section identical in shape to "Disputes."

## Deliberately out of scope

No pagination on relationship/history/assertion lists (same "correct at today's scale, revisit if
it becomes a real problem" reasoning applied elsewhere); no CSS beyond minimal readability; no
dedicated narrower "viewer" permission distinct from `graph:read`.

## Tests

`crates/oag-api/src/tests.rs` — `human_interface_escapes_hostile_content`,
`human_interface_pages_require_the_same_api_key_as_rest`, `human_search_page_finds_nodes_and_links_to_them`.
