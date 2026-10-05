# Petname

**Source**: `crates/oag-petname/src/lib.rs`.

Deterministic, human-readable display names derived from a peer's cryptographic identity — for
display purposes only, **never** used as an identifier. `oag identity show` and the Human
Interface use this to make a `PeerId` easier for a human to recognize/recall across sessions
without giving up the underlying hash-derived identity as the real identifier.

## How it works

A peer's own `PeerId` seeds a `ChaCha8Rng`, which draws a fixed number of words from a large
(~280k-word) pronouncing-dictionary word list, deterministically — the same `PeerId` always
produces the same petname, on any peer, without coordination (same shape as `canonicalize_value`'s
"same input always produces the same output" guarantee elsewhere in this project).

## Relationship to future addressing ideas

This crate's word-based *display* naming is the direct conceptual precedent for the
not-yet-built "long-word subdomain addressing" idea — extending the same word-based approach from
display into actual network addressing. See [22-future-work.md](22-future-work.md).

## Tests

`crates/oag-petname/src/lib.rs`'s own unit tests (determinism, distinct peer ids producing
distinct petnames).
