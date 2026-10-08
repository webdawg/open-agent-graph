# Tensor Pad and the Evolution Layer

**Source**: `crates/oag-tensor/src/lib.rs`, `crates/oag-storage/src/repo/tensor_pads.rs`,
`crates/oag-sync/src/service.rs` (`observe_sync_outcome`), `oag-brain/` (outside the main
workspace — see below).

Each peer ("ant node," per the colony metaphor in [22-future-work.md](22-future-work.md)) carries
a small persistent memory, the **tensor pad**: a fixed-width `Vec<f32>` (`TENSOR_PAD_DIM = 32`),
stored one row per peer in the `tensor_pads` table, keyed by that peer's own `peer_id`. Two
separate things act on it, kept deliberately apart:

## The ant memory node (`oag-tensor`)

Pure Rust, zero external dependencies, a normal workspace member. `TensorPad::update(signal,
learning_rate)` nudges every element toward `signal` via an exponential moving average (`v =
(1-lr)*v + lr*s`) — not backprop/training, just a persistent, nudgeable representation.

The only writer in v1: after every `SyncService::sync_with_peer` round, `oag-sync` builds a
4-element signal from that round's real `SyncSummary` (`ln(applied+1)`, `ln(already_known+1)`,
`ln(forks+1)`, `ln(errors.len()+1)`), zero-padded to 32, and updates this peer's own pad with
`learning_rate = 0.1`. This is best-effort and silent on failure — a tensor-pad write must never
fail (or even be visible as an error from) the sync it's observing, same reasoning as
[14-replication-sync.md](14-replication-sync.md)'s `discover_peers`.

`oag tensor show --data-dir <dir>` prints the current pad as a JSON array. No REST/MCP surface yet
— introspection-only, like `oag edge corroboration`.

## The evolution layer (`oag-brain`)

Changes ant brains and nothing else. Its one job: real scaled dot-product self-attention
("Attention Is All You Need") over the pads in play, used only to *program* (overwrite) each
participating node's own pad. Given `N=2` pads (this node's own, and an incoming peer's) stacked
into an `N x D` matrix `X`: `Q = X·Wq`, `K = X·Wk`, `V = X·Wv` (fixed, deterministically-seeded
`D x D` projection matrices — nothing is *trained*, no training objective was specified, but the
computation itself is the real mechanism, not a placeholder standing in for one), `scores =
softmax(Q·Kᵀ/√D)`, `output = scores·V`. Row 0 reprograms this node's own pad; row 1 is returned so
the caller can symmetrically program its own.

**Lives entirely outside the main cargo workspace** — `oag-brain/Cargo.toml` has its own empty
`[workspace]` table, not a member of the root workspace. This is the one deliberate structural
exception to this project's "every optional feature is a normal workspace member gated by a
runtime `enabled` flag" pattern (see [15-reticulum-transport.md](15-reticulum-transport.md)):
`tensorflow-sys` needs `libtensorflow`, a large C library, at *compile* time, not just runtime — a
runtime flag can't protect `cargo build --workspace` from a dependency that fails to even compile
without a system library present. Keeping `oag-brain` out of the workspace graph entirely
guarantees the main workspace's build is unaffected by whether that's installed anywhere.
`tensorflow-sys`'s own build script auto-downloads a working CPU-only `libtensorflow` for x86-64
Linux; no manual system install is required.

Depends on `oag-tensor`/`oag-storage`/`oag-crypto` via path dependencies — reads/writes only the
`tensor_pads` table, sharing the same `--data-dir` (and therefore the same `oag.sqlite` and
`identity.key`) as a paired `oag serve` process, but is never spawned by it and has no access to
node/edge/assertion storage or the event/replication path. A minimal standalone HTTP surface
(`POST /brain/exchange`) plus a one-shot `oag-brain --peer <url>` CLI mode for manual testing.

## Why the split

Two different maturity levels living side by side: the memory itself needed nothing but the
standard library and fits every existing guarantee this project makes about building a single,
dependency-light binary. The evolution layer needed a large, pre-1.0-feeling native ML dependency
— rather than let that risk leak into the one thing every other feature in this project depends on
building cleanly, it was kept structurally separate from day one.

## Test trace collection

`crates/oag-sync`'s two heaviest integration tests (`two_peer_replication` in `src/tests.rs`,
`fifty_peers_converge_after_chaos` in `tests/distributed_convergence.rs`) each end by snapshotting
every test peer's `MetricsSnapshot` plus tensor pad into `traces/<test_name>-<unix_ts>.json` at the
repo root (gitignored). Best-effort and silent on failure, like `observe_sync_outcome` above — a
trace write must never fail the test it's observing. This is purely data collection for later
evolutionary analysis across runs; no analysis tooling reads these yet, and the helper is
deliberately duplicated across the two call sites rather than factored into shared test
infrastructure for just two uses.

## Tests

`crates/oag-tensor/src/lib.rs` (EMA math, padding/truncation). `crates/oag-storage/src/tests.rs`
(`tensor_pad_*` — encode/decode round-trip, overwrite-on-conflict). `crates/oag-sync`'s existing
`two_peer_replication` and `fifty_peers_converge_after_chaos` integration tests exercise
`observe_sync_outcome` as a side effect of real sync. `oag-brain/src/brain.rs` (`attend` tests,
including two hand-checkable invariants independent of the actual weight values: all-zero input
produces all-zero output, and identical input rows produce identical output rows — these confirm
the graph really computes the stated formula rather than merely producing plausible numbers).
