# ENGINEERING_LOG — Project OAG

Append-only engineering journal. Newest entries at the top. Each entry records date,
invocation, repository, files modified, features implemented, tests executed, test results,
performance measurements, commit identifiers, push status, and remaining defects.

---

## 2026-11-09 — imagination Reticulum carrier

**Repository:** `oag-imagination-random-data-compute`
**Files created:**
- `crates/oag-imagination-reticulum/Cargo.toml`, `src/lib.rs`, `tests/loopback.rs`
- workspace `Cargo.toml` (new member + `oag-imagination` path dep)
**Files modified:** `spec/03-transports.md`, `README.md`, workspace `Cargo.toml`.

**Feature implemented.** The last carrier left as a seam. `ReticulumTransport` (client, implements
both `ReticulumCarrier` and `Transport`) and `serve`/`serve_on` (server), framing over a real
Reticulum `Link` with the same 4-byte length-prefix + 1400-byte chunk format the base project's own
Reticulum transport uses.

**Design decisions.** A separate crate so `oag-imagination` keeps no `reticulum` dependency; and
`oag-reticulum` could not host the adapter either, because `oag-imagination` depends on the base
project's crates, making that a circular *repository* dependency. Identity comes from a seed string
(the caller passes the owning `oag` peer's id) rather than a peer identity, preserving the base
project's "never derive from the public `PeerId`" property without an `oag-crypto` dependency.

**Tests executed.** `cargo test --workspace` — 40 passed (oag-imagination) + 3 passed (new
adapter), 0 failed.

**Test results.** New: `a_request_travels_over_a_real_reticulum_link` (real `TcpServer`/`TcpClient`
loopback, real engine answer, reply carries the server node's contribution);
`a_send_to_an_unknown_destination_fails_softly`; `a_bad_destination_address_is_a_clean_error`.

**Performance measurements.** None. No benchmark was run. The suite runs in ~10s, dominated by the
failure-path timeout, which was shortened to 2s via a new `with_announce_wait` builder.

**Commits:** `3dfc759` (transport chunked-path coverage, rust-ai), `9f63af7` (adapter) — pushed.
**Remaining defects.** The adapter has not been exercised over real radio, only loopback TCP as the
Reticulum interface. The identity seed is a caller responsibility: a changed seed silently changes
the node's address, which is documented but not enforced. `serve` hardcodes a 5s announce interval.

---

## 2026-11-09 — mesh commit marker (crash-consistent store set)

**Repository:** `oag-mesh`
**Files modified:**
- `crates/oag-mesh/src/persistence.rs` — added `CommitEntry`/`CommitMarker`/`CommitState`,
  `hash_file`, `write_commit_marker`, `verify_commit_marker`, `STORE_FILES`, and a
  `PersistenceError::TornSet` variant; corrected the module doc (it still said "three in-memory
  stores" when there are seven).
- `crates/oag-mesh/src/transport.rs` — `save_to_dir` hashes each of the seven files and writes the
  marker last; `load_from_dir` verifies it before loading.
- `spec/08-persistence.md`, `OPEN_QUESTIONS.md`.

**Feature implemented.** A saved set of the seven stores is now atomic *to a reader*. Each file
was already individually atomic, but a crash between saves could leave a torn set where every file
parses and nothing downstream could tell they disagreed. The marker closes that: written last, and
verified first, with a `TornSet` refusal when the files contradict it.

**Tests executed.** `cargo test -p oag-mesh` — 175 passed, 0 failed.

**Test results.** oag-mesh 165 → 175. New: 7 persistence tests (legacy/no-marker, consistent,
deleted file, modified file, corrupt marker, wrong version, idempotent re-write) and 3 transport
tests (fresh save verifies; load refuses a torn set; load still accepts a pre-marker directory).

**Performance measurements.** None. BLAKE3 over seven small JSON files on load is not a measured
bottleneck and no benchmark was run.

**Commit:** `7223852` — pushed.
**Remaining defects.** No fsync, so a power loss can still lose a rename. No atomic multi-store
*write* on the pull side. A caller that saves rarely can still lose up to a whole save's worth of
in-memory state. The torn-set path is tested by constructing the torn state directly, not by
inducing a real crash -- a real kill mid-save is not reproducible on demand.

---

## 2026-11-09 — search pagination

**Repository:** `open-agent-graph` (`rust-ai`)
**Files modified:**
- `crates/oag-storage/src/repo/nodes.rs` — `search` takes an `offset`; SQL is now
  `LIMIT ? OFFSET ?`.
- `crates/oag-graph/src/search.rs` — `search` takes an `offset`, clamped at 0.
- `crates/oag-graph/src/resolve.rs` — the internal candidate lookup passes 0.
- `crates/oag-api/src/dto.rs`, `handlers.rs` — REST `offset` query parameter.
- `crates/oag-mcp/src/params.rs`, `server.rs` — MCP `offset` argument.
- `crates/oag-cli/src/main.rs`, `commands.rs` — `--offset` flag.
- `crates/oag-api/src/human.rs`, `templates/search.html` — fixed 50-per-page with
  Previous/Next links.
- Tests: `crates/oag-graph/src/tests.rs`, `crates/oag-api/src/tests.rs`,
  `crates/oag-mcp/src/tests.rs`, `crates/oag-cli/src/commands.rs`.
- Docs: `spec/07-search.md`, `spec/10-rest-api.md`, `spec/11-mcp-server.md`,
  `spec/12-cli.md`, `spec/13-human-interface.md`, `docs/api.md`, `OPEN_QUESTIONS.md`,
  `ARCHITECTURE_DECISIONS.md`.

**Feature implemented.** `offset` on all four search surfaces. Previously a `limit` with no
`offset` meant only the first page was reachable, and the Human Interface hard-coded 50 with no
way forward.

**Tests executed.** `cargo test -p oag-graph` (87 passed), `-p oag-api` (60 passed),
`-p oag-mcp` (19 passed), `-p oag-cli` (19 passed, 2 ignored for needing public DNS).

**Test results.** All passed. oag-graph 86 → 87, oag-api 59 → 60, oag-mcp 18 → 19,
oag-cli 20 → 21.

**Performance measurements.** None. No benchmark was run; the change adds one bound parameter to
an existing indexed FTS5 query and does not alter its plan.

**Commit:** `e1e6110` — pushed.
**Remaining defects.** Ordering is FTS5 `rank`, which is not a stable keyset: a node inserted
between two page fetches can shift later pages. Recorded in `OPEN_QUESTIONS.md`; a
`(rank, node_id)` cursor would fix it. Not built, because no current caller needs strict
consistency and the fix costs an extra `ORDER BY` column that every other caller would pay for.

---

## 2026-11-09 — verification metrics

**Repository:** `open-agent-graph` (`rust-ai`)
**Files modified:**
- `crates/oag-graph/src/service.rs` — added `Arc<AtomicU64>` `verification_jobs` /
  `verification_failures` fields on `GraphService`; added `record_verification(bool)`.
- `crates/oag-graph/src/assert.rs` — `verify_assertion` now wraps the whole call (including the
  `Permission::GraphVerify` check) in one counted outcome.
- `crates/oag-graph/src/metrics.rs` — two new `MetricsSnapshot` fields, populated from the atoms.
- `crates/oag-graph/src/tests.rs` — new `verification_counters_count_jobs_and_failures`.
- `crates/oag-api/src/metrics.rs` — two new Prometheus lines; test fixture and expected text
  updated (14 → 16 metric lines).
- `OPEN_QUESTIONS.md`, `spec/19-observability.md` — documented.

**Feature implemented.** `verification_jobs` / `verification_failures`, counted per
`verify_assertion` call. A rejected attempt (unknown id, missing permission) counts as a failure,
because the count is taken *after* the outcome is known.

**Why not a `COUNT(*)`.** Rejected attempts never become events, so a snapshot query returns a
permanent zero. Recorded in `RESEARCH_JOURNAL.md`.

**Tests executed.** `cargo test -p oag-graph verification_counters` (1 passed);
`cargo test -p oag-api metrics` (4 passed).

**Test results.** All passed. oag-graph total now 86 tests; oag-api 59.

**Commit:** `0a84a78` — pushed.
**Remaining defects.** None known. `blob_store_size` remains unbuilt because no blob store exists.

---

## 2026-11-09 — `oag ipfs pin <file>`

**Repository:** `open-agent-graph` (`rust-ai`)
**Files modified:** `crates/oag-cli/src/commands.rs`, `crates/oag-cli/src/main.rs`,
`spec/12-cli.md`.

**Feature implemented.** Pin a local file into the native IPFS node from the CLI, mirroring
`oag backup --pin`. Fails loudly when no provider is configured rather than silently doing
nothing — a pin command that appears to succeed while pinning nothing is worse than an error.

**Tests executed.** `cargo test -p oag-cli` — 20 passed (2 new: disabled-provider error,
directory/oversized rejection).

**Commit:** `bab5b74` — pushed. **Remaining defects:** none.

---

## 2026-11-09 — declare_alias over REST and MCP

**Repository:** `open-agent-graph` (`rust-ai`)
**Files modified:** `crates/oag-api/src/dto.rs`, `crates/oag-api/src/handlers.rs`,
`crates/oag-api/src/lib.rs`, `crates/oag-api/src/tests.rs`, `crates/oag-mcp/src/params.rs`,
`crates/oag-mcp/src/server.rs`, `crates/oag-mcp/src/tests.rs`, `docs/api.md`,
`OPEN_QUESTIONS.md`.

**Feature implemented.** `POST /api/v1/nodes/{id}/aliases` and the `graph_declare_alias` MCP tool,
both `graph:assert`. Deliberately REST/MCP rather than CLI: it matches the established
dispute/retract/supersede convention, and the crawler already covers the CLI use case.

**Tests executed.** oag-api and oag-mcp targeted tests; all passed. MCP total now 18.

**Commit:** `1b6863e` — pushed. **Remaining defects:** none.

---

## 2026-11-08 — IPFS site routes tested; spec/22 corrected

**Repository:** `open-agent-graph` (`rust-ai`)
**Files modified:** `crates/oag-api/src/ipfs_site.rs`, `crates/oag-api/src/tests.rs`,
`spec/22-future-work.md`.

**Feature implemented.** HTTP test `ipfs_site_routes_are_unauthenticated_and_inject_the_cid`.
Corrected a stale spec note claiming Phase 3 static hosting was unbuilt — it is built.

**Tests executed.** `cargo test -p oag-api ipfs_site` — passed.

**Commit:** `4ac3a57` — pushed. **Remaining defects:** none.

---

## 2026-11-08 — pad traffic reinforcement

**Repository:** `open-agent-graph` (`rust-ai`)
**Files modified:** `spec/23-tensor-pad-and-evolution-layer.md`, imagination engine.

**Feature implemented.** Recorded that traffic strengthens the tensor pad and that pads act as
data pads. See `INVENTION_REGISTER.md` OAG-INV-005.

**Commit:** `4bdef2d` — pushed.

---

## 2026-11-08 — onion rejection paths

**Repository:** `oag-mesh`
**Files modified:** `crates/oag-mesh/src/onion.rs`.

**Feature implemented.** Tests for the onion layer's rejection paths: empty path → `EmptyPath`,
key/path mismatch → `BadLength`, malformed layer (bad hex, wrong nonce length, AEAD tag failure).
Six onion tests.

**Tests executed.** `cargo test -p oag-mesh onion` — passed.

**Commit:** `0a3a61f` — pushed. **Remaining defects:** padding discipline end-to-end is unverified;
the onion chain and anonymous events are not yet composed.

---

## 2026-11-08 — cyclic route search terminates

**Repository:** `oag-mesh`
**Files modified:** `crates/oag-mesh/src/transport.rs`.

**Feature implemented.** `find_route_across_mesh_terminates_on_a_cyclic_peer_graph` (A↔B, B→C,
C→B) with a 10s timeout. Guards against an infinite loop in route search on a cyclic topology.

**Commit:** `23c8310` — pushed.

---

## 2026-11-08 — persistence temp-name uniqueness

**Repository:** `oag-mesh`
**Files modified:** `crates/oag-mesh/src/persistence.rs`.

**Feature implemented.** Each save gets a unique temp name (spec/08), removing a race where
concurrent saves could collide on a fixed temp path.

**Commit:** `be18343` — pushed.

---

## 2026-11-08 — blinded responder dispatch

**Repository:** `oag-mesh`
**Files modified:** `crates/oag-mesh/src/dispatch.rs`, `crates/oag-mesh/src/persistence.rs`.

**Feature implemented.** Filter subscription under a responder-chosen random id plus a polled
inbox; the subscription registry and inbox are persisted (spec/08, spec/14). See
`INVENTION_REGISTER.md` OAG-INV-004.

**Commits:** `0b21f3c`, `7f4d364` — pushed.

---

## 2026-11-08 — three-node chain, stale carrier status

**Repository:** `oag-imagination-random-data-compute`
**Files modified:** `crates/oag-imagination/src/integration.rs`, `README.md`.

**Feature implemented.** `a_request_can_travel_through_a_three_node_chain`. Corrected the README's
stale carrier status.

**Commit:** `ca94a0d` — pushed. Imagination total now 37 tests.

---

## 2026-11-08 — hardware randomness seam resolved

**Repository:** `oag-imagination-random-data-compute`
**Files modified:** `crates/oag-imagination/src/source.rs`.

**Feature implemented.** Resolved the stale `source.rs` TODO: `DeviceRandom` already implements
the seam. Added `a_device_source_reads_exactly_the_device_bytes`.

**Commit:** `0559614` — pushed.

---

## 2026-11-08 — empty-node and address-less-peer paths

**Repository:** `oag-imagination-random-data-compute`
**Files modified:** `crates/oag-imagination/src/node.rs`.

**Feature implemented.** `a_node_with_no_pad_starts_empty_and_peers_without_addresses_are_skipped`.

**Commit:** `1b037db` — pushed.

---

## 2026-11-08 — unreachable next hop degrades

**Repository:** `oag-imagination-random-data-compute`
**Files modified:** `crates/oag-imagination/src/socket.rs`.

**Feature implemented.** `an_unreachable_next_hop_still_gets_a_response_not_a_hang` — ten socket
tests. Consistent with "degrade, don't drop."

**Commit:** `898c927` — pushed.

---

## 2026-11-08 — spec/03 carrier status corrected

**Repository:** `oag-imagination-random-data-compute`
**Files modified:** `spec/03-transports.md`.

**Feature implemented.** Corrected stale text: HTTP, tunnel, and IPFS carriers are built; only the
Reticulum adapter remains a seam.

**Commit:** `5b51569` — pushed.

---

## 2026-11-09 — persistent project records created

**Repository:** `open-agent-graph` (`rust-ai`)
**Files created:** `PROJECT_STATE.md`, `RESEARCH_JOURNAL.md`, `INVENTION_REGISTER.md`,
`ENGINEERING_LOG.md`, `EXPERIMENTS.md`, `ARCHITECTURE_DECISIONS.md`.

**Note.** A first draft of `PROJECT_STATE.md` was created and then removed at the operator's
request during a context-clearing exchange. The records were recreated on the next directive
because PART XV requires them and they must live inside the authorized repositories.

**Remaining defects.** None. These files are living documents and will be appended, not rewritten.
