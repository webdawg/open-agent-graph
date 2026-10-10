# The Gravity Layer

**Source**: `crates/oag-sync/src/gravity.rs`, `crates/oag-storage/migrations/0010_node_gravity.sql`,
`crates/oag-storage/src/repo/node_gravity.rs`, `crates/oag-sync/src/gossip.rs`.

A small, truly-random, per-node quantity — `gravity_level`, a float in `[0.0, 1.0)` — that very
slightly throttles that node's own processing speed. Universal and always-on for every `oag serve`
instance, unlike the opt-in features elsewhere in this project (`[identity].ephemeral`,
`[reticulum].enabled`): gravity is "different everywhere," not a feature a deployment turns on.

This resolves an open question `spec/22-future-work.md` and `USER_INPUT_RECORD.md` Entry 3
deliberately left unresolved when the idea was first captured — whether "slowing each node down"
meant the tensor pad's own magnitude decaying, or the node's own clock being throttled — in favor
of the second reading, per `USER_INPUT_RECORD.md` Entry 5's explicit instruction. **Gravity never
touches the tensor pad.** It is unrelated to `oag-tensor`/`oag-brain`
(`spec/23-tensor-pad-and-evolution-layer.md`) — a third, independent per-node property, not a
third force acting on the same pad those two already describe.

## Rolling and persisting a node's gravity level

`gravity::roll_gravity_level` draws from `rand::rngs::OsRng` — the OS's own CSPRNG — not the
deterministic, fixed-seed RNG pattern this project otherwise uses for *reproducibility*
(`oag-tensor`/`oag-brain`'s seeded projection weights, `oag-petname`'s deterministic naming).
Gravity wants the opposite property: real, per-node unpredictability. Per `USER_INPUT_RECORD.md`
Entry 5's explicit instruction, this is a stated placeholder — "use a really good randomness
generator for this for now, but hardware later" — and the code is shaped so that swap is cheap:
every consumer only ever sees the resulting `f32`, never which source it came from, so replacing
`OsRng` with a real hardware entropy/measurement source later changes nothing downstream.

`gravity::get_or_generate(pool, peer_id, now)` looks up this peer's `node_gravity` row; if none
exists, it rolls one and persists it with `INSERT OR IGNORE` (never overwrites an existing value —
confirmed by `repo::node_gravity`'s own test). This makes a node's gravity level stable across
restarts for a permanent identity — behaving like the stand-in for a real physical trait it is,
rather than a fresh coin flip every time anyone asks — while an ephemeral identity
(`spec/24-ephemeral-peer-trust.md`) naturally gets a fresh roll every restart too, same as its
`peer_id`.

## The processing-speed effect

Applied inside `gossip::spawn_gossip_loop`, which already processes every known peer address
sequentially, "one at a time" — the exact anchor `USER_INPUT_RECORD.md` Entry 3's original capture
used for this same phrase. The gravity level is looked up once per process lifetime (not re-queried
every tick — it can't change mid-flight) and `gravity::apply_slowdown` is called once per address
processed per gossip tick.

`gravity::delay_for` is the whole mapping, a pure function: `gravity_level.clamp(0, 1) *
MAX_DELAY_MICROS`, where `MAX_DELAY_MICROS = 500` — half a millisecond at the theoretical maximum
(`gravity_level = 1.0`), a small fraction of that for anything less. "Very very slightly," taken
literally: confirmed live against the 50-peer chaos integration test, which converges in
essentially the same wall-clock time with or without gravity wired in.

## Not built

Any effect beyond the gossip loop's own per-tick processing (e.g. throttling event ingestion
directly) — the single, well-scoped anchor above is this phase's full scope. No CLI/config
override to disable gravity per node, since it's meant to be universal rather than opt-in. No
cross-node interaction of any kind — a node's gravity level affects only its own processing, never
anyone else's.

## CLI

`oag gravity show` — prints this peer's gravity level, rolling and persisting one if it doesn't
have one yet (same shape as `oag tensor show`).

## Tests

`repo::node_gravity` (a missing row reads as `None`; a second insert never overwrites the first).
`gravity.rs` (rolled levels land in `[0, 1)` and aren't all identical across 20 real rolls; the
delay mapping is exact and hand-checkable at `0.0`/`0.5`/`1.0` and clamps out-of-range input;
`get_or_generate` is stable across repeated real-database lookups). Live: confirmed a real running
`oag serve` instance and a separate `oag gravity show` CLI invocation against the same data
directory read back the identical persisted value, and the 50-peer chaos test still converges with
gravity wired into its gossip loop.
