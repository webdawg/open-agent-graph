# Self-Hosted Roadmap Governance

**Source**: `crates/oag-graph/src/roadmap.rs`, `crates/oag-storage/src/repo/edges.rs`'s
`list_by_predicate_and_object`, `GET /api/v1/roadmap` (`oag-api/src/handlers.rs`), `GET
/ui/roadmap` (`oag-api/src/human.rs`, `templates/roadmap.html`), the MCP tool
`graph_list_roadmap_proposals`.

"What should this project build next" is deliberately **not** a new consensus mechanism
(`USER_INPUT_RECORD.md` Entry 2, `spec/22-future-work.md`) — a roadmap proposal is just an
ordinary assertion backed by ordinary evidence, ranked by the same corroboration signals every
other claim already gets. This builds the two genuinely new things the future-work text named: a
predicate/concept-namespace convention, and a Human Interface view for browsing/ranking.

## The convention — zero new vocabulary

A roadmap proposal is: `(proposal, instance_of, concept:oag-roadmap-proposal)`. `instance_of` is
already-documented (`docs/predicates.md`) as the baseline-typing predicate — reusing it instead of
inventing a dedicated predicate keeps this feature at zero new vocabulary, matching "no new
consensus mechanism to build" as literally as possible. `proposal` is whatever identifier the
proposer chooses — a `concept:<slug>` for an idea with no canonical URL, or a real `url:` to an
issue or document.

**Submitting a proposal is the ordinary `POST /api/v1/assertions`** — no new write path exists or
is needed. Evidence attached to that assertion (links, rationale) is what backs the proposal,
exactly like every other claim. Corroboration accrues the same way it does for anything else:
multiple independent actors asserting the *same* `(proposal, instance_of, hub)` edge is what
raises `source_independence`/`distinct_actors`.

## The hub node needs no bootstrap

`concept:oag-roadmap-proposal` is not pre-seeded by any migration or CLI command — it springs into
existence the first time anything is asserted against it, via the completely ordinary
`resolve_or_create_node` path every assertion already goes through
(`oag-events/src/projector.rs`). This is the exact same mechanism that already created
`concept:software-architecture-reference` (`docs/architecture.md`'s "Software architecture
reference space" section) — a second, independent instance of the same convention, not new
machinery. `GraphService::list_roadmap_proposals` returns an empty list, not an error, when the
hub has never been asserted against.

## The one genuinely new query

`EdgeCorroboration`/`get_node_authority` already existed and are reused unchanged — no new ranking
math. What didn't exist was a way to *list* every edge with a given predicate+object (both
existing query functions, `list_by_subject`/`list_by_object`, filter on only one side). The new
`oag_storage::repo::edges::list_by_predicate_and_object` adds exactly that, in the same style as
its siblings.

`GraphService::list_roadmap_proposals`: resolve the hub by canonical identifier, list its
`instance_of` edges, fetch each proposal's node and `get_edge_corroboration`, sort by
`(active_assertions, source_independence, evidence_strength)` descending — a deterministic
multi-key sort, **not** a collapsed single score, matching `EdgeCorroboration`'s own existing
design philosophy (its doc comment already says its signals are "never collapsed into a single
score"). Deliberately scoped to roadmap proposals specifically rather than generalized into a
"list instances of any hub" capability — the future-work text didn't ask for the latter, and
nothing else in this codebase needs it yet.

## Surfaces

`GET /api/v1/roadmap` (REST, `Permission::GraphRead`, same auth every other read endpoint uses),
`GET /ui/roadmap?key=...` (Human Interface, same `?key=` query-param auth `search_page`/
`node_page`/`assertion_page` already use — a ranked list linking to each proposal's node page,
corroboration signals shown as separate labeled numbers, never collapsed), and the MCP tool
`graph_list_roadmap_proposals` (no arguments), matching `docs/architecture.md`'s own stated MCP
design intent of one tool per `GraphService` capability.

## Explicitly deferred

- Any new predicate or schema change — the convention is 100% reuse of `instance_of` plus an
  ordinary `concept:` node, by design.
- A generic "list instances of any hub node" capability — scoped tightly to roadmap proposals;
  trivially generalizable later if a second hub-browsing need ever arises (see
  `USER_INPUT_RECORD.md` Entry 8 for this judgment call recorded explicitly).
- Any UI/API for *submitting* a proposal beyond the existing `POST /api/v1/assertions` — no new
  write path was added.

## Tests

`oag-storage`'s `list_by_predicate_and_object_filters_both_fields` (matches by predicate+object
together, excludes a decoy with only one field matching). `oag-graph`'s `roadmap::tests`: an empty
graph returns an empty list rather than an error; two proposals from different actor/source
combinations are both returned and ranked in the correct order; the hub node provably doesn't
exist before the first proposal and does after, with no special bootstrap. Live: a real `oag
serve` instance, two proposals posted via real REST calls from two different actors with evidence
from two different domains, `GET /api/v1/roadmap` and `GET /ui/roadmap` both confirmed to rank the
more-corroborated proposal first.
