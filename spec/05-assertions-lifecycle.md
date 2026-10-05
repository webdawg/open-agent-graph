# Assertions Lifecycle

**Source**: `crates/oag-graph/src/assert.rs`.

## Operations

| Operation | Permission | Ownership check | Notes |
|---|---|---|---|
| `assert` | `graph:assert` | none | Creates the assertion; attaches any evidence passed inline as separate `ADD_EVIDENCE` events |
| `add_evidence` | `graph:assert` | none | Independent of the assertion it supports — new evidence can strengthen an old claim without re-asserting |
| `verify_assertion` | `graph:verify` | none | Records a verification observation (`confirmed`/`contradicted`/`unreachable`) |
| `dispute_assertion` | `graph:assert` | none | "Disputing is itself a claim about a claim" — anyone with assert permission can dispute anyone's claim, by design |
| `retract_assertion` | `graph:retract-own` | **enforced** | Caller must be the assertion's original asserting actor, or hold `admin` |
| `supersede_assertion` | `graph:assert` | none, deliberately | See below |

## Why `supersede` has no ownership check (and why that's deliberate, not a gap)

`retract_assertion` explicitly checks `assertion.actor_id == auth.actor_id` (or `admin`) before
proceeding — "own" is enforced, not just checked for presence. `supersede_assertion` does not make
the equivalent check, and this was investigated specifically (not assumed): the reasoning recorded
in `OPEN_QUESTIONS.md`'s "Supersession surface" section is that superseding is semantically closer
to "asserting a new fact that obsoletes an old one" than to retraction's own-claim-only semantics —
e.g. actor B asserts "the sky is blue," actor A (unrelated) later asserts a more precise claim and
marks B's as superseded by it. This models real evolving understanding and doesn't require
permission from the original author, the same way disputing doesn't.

## What *is* checked for supersede, verify, dispute, and add_evidence: existence

None of these four check that the referenced assertion exists in `GraphService` itself — they rely
entirely on the projector (`project_supersede_assertion`, `project_verify_assertion`,
`project_dispute_assertion`, `project_add_evidence`), which validates existence and rejects the
*whole event* atomically (`EventsError::NotFound`) if the target doesn't exist. This was confirmed
with dedicated tests for all four cases — previously only `retract_assertion`'s had direct test
coverage; the fact that the others relied on the same correct-by-construction mechanism was true
but untested.

For `supersede_assertion` specifically, both the old *and* new assertion ids are validated to
exist, and the test also confirms the real assertion's status comes back `Active` — not partially
superseded — after both of two different failure scenarios (fake old id, fake new id).

## Retraction detail visibility

`get_assertion` (REST, MCP, and both Human Interface templates) composes an assertion's evidence/
observations/disputes/retractions together. This was asymmetric until fixed: disputes showed full
detail (reason, disputing actor, timestamp) while a retraction only ever showed the bare `status:
"retracted"` flag. The root cause went one layer deeper than missing wiring —
`oag_storage::repo::assertions::Retraction` wasn't even `Serialize`, and `list_retractions`
returned the raw, unconverted `RetractionRow` instead of the typed struct its sibling
`list_disputes` already converted to. Fixed at the storage layer (`row_to_retraction`, mirroring
`row_to_dispute`) and wired into every surface that already showed dispute detail.

## Validation

`AssertInput`/`EvidenceInput` are validated before any write: subject/predicate/object/type length
caps, evidence-per-assertion count cap (`MAX_EVIDENCE_PER_ASSERTION = 20`), and per-field evidence
length caps. See [04-data-model.md](04-data-model.md)'s "Input validation" section for the shared
mechanism.

## Tests

`crates/oag-graph/src/tests.rs` (full lifecycle coverage — assert/evidence/verify/dispute/retract/
supersede, `retract_of_missing_assertion_is_not_found`); `crates/oag-events/src/tests.rs`
(`supersede_assertion_with_an_unknown_id_on_either_side_is_rejected_atomically`,
`verify_and_dispute_assertion_for_a_nonexistent_assertion_are_both_rejected`,
`add_evidence_for_a_nonexistent_assertion_is_not_found`); `crates/oag-api/src/tests.rs`
(`full_rest_vertical_slice`, which checks retraction detail end-to-end).
