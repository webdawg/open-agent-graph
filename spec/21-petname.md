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

## Relationship to addressing

`address_label` (same file, same wordlist, same `PeerId`-seeded `ChaCha8Rng` approach as
`peer_name`) is the actual network-*addressing* sibling of this crate's word-based *display*
naming — a separate, much shorter 4-word function, since `peer_name`'s 128 words are nowhere
close to fitting a single DNS label. See [30-long-word-addressing.md](30-long-word-addressing.md).

## Tests

`crates/oag-petname/src/lib.rs`'s own unit tests: `peer_name` determinism and distinctness;
`address_label` determinism/distinctness plus DNS-label validity (non-empty, ≤63 bytes, exactly 4
hyphen-joined lowercase-alphabetic words, no leading/trailing hyphen) across many distinct
`PeerId`s; `address_wordlist`'s candidate count staying large despite the length cap.
