# ENGINEERING_LOG — Project OAG

Append-only engineering journal. Newest entries at the top. Each entry records date,
invocation, repository, files modified, features implemented, tests executed, test results,
performance measurements, commit identifiers, push status, and remaining defects.

---

## 2026-11-21 — the IPFS mirror works against a real node

**Repository:** `oag-listend`, commit `aad2d9b` on `master`; records in `open-agent-graph`.

**The defect, found by using a real node.** `SPEC.md` said the mirror was "tested only against a
stub, never a real Kubo node". So Kubo was built from source (`go build ./cmd/ipfs` from the module
the proxy serves) and the daemon was run against it. The write half failed: `2 mirrored, 0 failed`
only ever happened against the stub. Kubo answers `POST /api/v0/add` with
`Transfer-Encoding: chunked`, which the client refused by design.

**The fix.** Both halves now decode chunked replies: `split_reply` separates head and body and
decodes when the head asks for it; `decode_chunked` walks `<hex>[;ext]\r\n<data>\r\n` to a zero-size
chunk, refusing malformed framing and capping the decoded size. The unused `Chunked` variant is gone.

**Read half, also validated against the real node.** `GET /ipfs/{cid}` for a CID the daemon had never seen
(added straight to the node, so it is not in `mirror_cids`) returned 200 with the byte-identical object and the
right content type and `X-Content-Hash`; no `X-IPFS-CID`, because the daemon has not recorded one for it. That is
the intended asymmetry: the header carries a CID the daemon knows about.

**Validation against Kubo 0.43.1 (real node, loopback):**
- `POST /api/v0/add` returns a CID and the node **pins** it (recursive pin list includes both).
- `POST /api/v0/cat` returns exactly the bytes; identical to the stored file and to what listend
  serves over `GET /ipfs/{cid}` (200, 844 bytes, `X-IPFS-CID`, ETag = content hash).
- A non-audio object added to the node is refused by `GET /ipfs/{cid}` (400), so the sniff rule
  applies to bytes fetched from the node too.
- A restart reports `offered 0 of 2` and the node's object count is unchanged (10 → 10), so the
  persisted CID mapping makes the startup pass idempotent across restarts with a real node.

**Also settled with evidence.** This Kubo accepts `hash=blake3` and returns a CIDv1 raw-codec CID
whose digest is exactly the file's BLAKE3 (`bafkr4i…` → `fd1b77a8…`). That does **not** unify
fetch-by-hash: for files larger than one block the CID is the DAG root, not the file's BLAKE3, so
fetch-by-hash remains limited to single-block items. Recorded in SPEC rather than attempted.

**Tests executed.** New unit tests: a reply split across two chunks, a chunk size over the bound,
and a body that stops early. `./scripts/check.sh` **146 tests**; `--release` **146**;
`--features reticulum` **156**. All six gates each.

**Push status:** `oag-listend` pushed to `master` (`5cbec03..aad2d9b`); this entry's commit pending.

---

## 2026-11-20 — CI now installs and starts the shipped unit

**Repository:** `oag-listend`, commit `d76d961` on `master`; records in `open-agent-graph`.

**The gap.** Every job only *linted* the unit (`systemd-analyze verify`). The four sandbox
directives (`ProtectSystem=strict`, `ProtectHome`, `PrivateTmp`, `NoNewPrivileges`) cannot be
exercised without root, so the shipped unit file itself had never been started by CI.

**What changed.** A fourth job, `check-service`: builds the release binary, installs it to
`/usr/local/bin`, installs `scripts/oag-listend.service` **verbatim**, creates the `oag` user,
`systemctl enable --now`, then fetches the musical ping and the index over HTTP, and stops the
unit. The unit file is unchanged.

**Tests executed.** The serve check was exercised locally against a running daemon: 44,844-byte
ping and a 200 index. **The `sudo`/`systemctl` steps are verified by the runner on the first run,
not locally** — this machine has no passwordless sudo, which is exactly why the job exists.

**Further validation before pushing.** Every `run:` block in the workflow passes `bash -n`, and the
service job's serve block was then executed **verbatim** against a daemon on the unit's port: exit 0,
44,844-byte ping. The `sudo install`/`systemctl` lines remain unrun locally.

**Failure diagnosability.** A bare 60 s timeout leaves a red run with no clue, so the serve step
now prints `systemctl status` and the last 40 journal lines before exiting 1. `oag-listend`
commit `5cbec03`; the block passes `bash -n`.

**Docs tied to it.** The README's service section now says the exact unit is installed and started by CI
(`check-service`) and repeats the `PrivateTmp` trap. `oag-listend` commit `244b358`.

**Push status:** `oag-listend` pushed to `master` (`10d1411..d76d961`); this entry's commit pending.

---

## 2026-11-20 — correction: the unit's sandbox IS verified (earlier note was wrong)

**Repository:** `oag-listend` untouched; records in `open-agent-graph`.

**The earlier note was wrong.** The previous entry said the four sandbox directives
(`ProtectSystem=strict`, `ProtectHome`, `PrivateTmp`, `NoNewPrivileges`) "remain lint-verified
only". They do not — they are now **run-verified**, and the reason they looked broken was my own
test staging.

**Root cause, found by bisecting properly.** Staging the binary under `/var/tmp` failed with
`Unable to locate executable ... No such file or directory`. `PrivateTmp=true` is the cause:
systemd's `PrivateTmp` covers **`/tmp` and `/var/tmp`**, so the staged binary was masked. Staging
under `/dev/shm` (not masked) and running the **full daemon** with all four directives
→ **active/running**, serving audio (`Hello`, ping `verified=true`, publish/fetch verified).

**Also corrected: an earlier bisect was invalid.** A probe helper reported "active" for units that
were already active, because `systemctl start` on an active unit is a no-op — so several
"directive X is fine" results were stale. Re-run with an explicit stop/reset before each start.

**What is now verified about the shipped unit.** `ExecStart` flag set, `RuntimeDirectory`,
`StateDirectory`, `ReadWritePaths`, all four sandbox directives, `Restart=on-failure`, and a clean
`systemctl stop` (inactive, socket removed, `oag-listend shutting down` logged).

**Housekeeping.** All probe units, their staged binaries and directories removed; the shipped
`scripts/oag-listend.service` is untouched (`git status` clean).

**Tests executed.** None new; operational verification only. `./scripts/check.sh` last run exit 0,
six gates, 143 tests (153 with `--features reticulum`).

---

## 2026-11-20 — the shipped unit actually runs, not just lints

**Repository:** `oag-listend` untouched; records in `open-agent-graph`, commit pending.

**Why.** The directive's first criterion is "the daemon turns on and listens", and until now the
systemd unit had only been checked with `systemd-analyze verify` — a syntax/lint pass, not a run.

**How it was run without root.** A user-level systemd manager is available on this machine, so the
shipped unit was derived into a user unit: the same `ExecStart` flag set, `RuntimeDirectory`,
`StateDirectory`, `Restart=on-failure`, with only the paths, `User=`/`Group=` and `WantedBy=`
adjusted for an unprivileged user.

**Observed.**
- `systemctl --user start` → **active**; the daemon served (`Hello`, ping `verified=true`,
  publish/fetch verified) while running as a service.
- systemd created **both** directories: `RuntimeDirectory` → `/run/user/1000/oag-listend-test`
  and `StateDirectory` → `~/.local/state/oag-listend-test`, and the content-addressed store was
  written under the latter.
- `systemctl --user stop` → **inactive**, the Unix socket **removed**, and
  `oag-listend shutting down` in the journal — the clean stop the unit is designed for.

**What could not be verified, stated plainly.** The sandbox directives (`ProtectSystem=strict`,
`ProtectHome`, `PrivateTmp`, `NoNewPrivileges`) are designed for a system install with the binary
in `/usr/local/bin`; staging the binary outside `/home` still failed the namespace setup, so those
four lines remain lint-verified only. Everything else about the unit is now run-verified.

**Housekeeping.** The test unit, its staged binary, and both directories were removed afterwards;
the shipped `scripts/oag-listend.service` is untouched (`git status` clean).

**Tests executed.** None new; this is an operational verification. `./scripts/check.sh` last run
exit 0, six gates, 143 tests (153 with `--features reticulum`).

**Follow-up.** The trap found while verifying is now in `SPEC.md` ("Running it"): the unit is
recorded as run-verified, and `PrivateTmp` masking `/tmp` and `/var/tmp` is documented so the
next operator who repoints `ExecStart` does not chase a "missing" binary. `oag-listend`
commit `10d1411`.

**Closing verification on the SPEC-updated HEAD (`10d1411`).** `check.sh` 143, `--release` 143,
`--features reticulum` 153 — all six gates each, working tree clean.

**Push status:** `open-agent-graph` records pending; `oag-listend` pushed.

---

## 2026-11-20 — record honesty: Reticulum status, and the mesh pull question closed

**Repository:** `oag-listend` untouched; records in `open-agent-graph`, commit `d069076` on `main`.

**Why.** The directive gates `oag-mesh` on "not this mission until listend serves locally" -- a
precondition now met -- and the workspace's own next-priorities list had both a stale status and an
item explicitly asking for a re-examination. Both were cheap to settle and neither needed new code.

**Item 1 was stale.** It still said Reticulum was "hardware-bound ... honestly unbuilt". It is built:
opt-in behind `--features reticulum`, speaking over Reticulum's own `TcpServer` interface. What
remains unproven is interop with another implementation, alongside the real-Kubo validation of the
IPFS halves. Corrected.

**Item 4 is now closed as examined.** The claim was "no atomic multi-store write during a pull".
Read the code: `pull_changed_stores` merges each changed store **in memory only** and writes
nothing, and `save_to_dir` -- which writes all seven stores plus a commit marker atomically -- has
exactly one production caller (`node.rs`, the shutdown/save path). So the atomic machinery is not on
the pull path at all. The residual gap is that a pull's merged state is durable only when the caller
later saves, and the crate's own docs say the caller decides when to save. That is a scheduling
question, not a correctness hole; reopening it needs an operator's durability requirement.

**Two more items read, and one corrected.**

- *Imagination gather.* The note implied a defect: that fanning out over the caller's `nodes`
  rather than the manifest's `pad_ids` is wrong. It is not. Each request is a pad-agnostic
  `PadRequest { pad_id: "" }` and `datapad::gather(manifest, &pads)` filters the responses, so a
  node returns what it holds and the manifest decides what is used. Asking extra nodes is the
  design. The only open question left is whether the bound (32) should be configurable, which
  needs an operator's requirement. `PROJECT_STATE` corrected so the note does not read as a
  defect.
- *rust-ai HTTP timeouts.* Confirmed accurate: `EMBEDDINGS_*` and `KUBO_*` are public
  compile-time `const`s. Still needs an operator's requirement, so not acted on.
- *Mesh fallback classification.* Confirmed accurate: `pull_and_merge` skips the full-pull
  fallback exactly when the heads error is `is_connect() || is_timeout()`, and the comment
  records the reasoning plus a measurement (a black-holed peer costs six timeouts). The
  recorded blind spot -- a slow-but-alive peer whose heads time out -- is real but is a
  judgement, and resolving it needs a measurement of how often a timed-out heads endpoint
  still serves stores. Not acted on.

**Not acted on, deliberately.** The remaining items each need something this machine cannot
supply: a real corpus (collisions), a measurement of timed-out-but-alive peers (mesh fallback),
and a design pass (`spec/06`, `spec/07`). `spec/29` explicitly defers the static-page items.

**Tests executed.** None; records only. `./scripts/check.sh` last run exit 0, six gates, 143 tests
(153 with `--features reticulum`).

**Push status:** `open-agent-graph` pushed to `main`; `oag-listend` unchanged and clean.

---

## 2026-11-20 — pin the skipped-mirror policy

**Repository:** `oag-listend`, commit `8a5ebef` on `master`; records in `open-agent-graph`.

**Why.** The previous entry recorded a fix with an untested literal, because the interleaving that
reaches the branch cannot be built deterministically. That left the *policy* unpinned, which is the
part worth pinning.

**What changed.** The policy is extracted as `mirror_skipped()` and asserted: a mirror that never ran
because the item was evicted is a `MirrorOutcome::Failed` with the reason "evicted before it could be
mirrored". `accept_and_mirror` now calls it.

**Tests executed.** New `a_skipped_mirror_is_a_recorded_failure`. `./scripts/check.sh` exit 0, six
gates, **143 tests** (66 unit + 77 end-to-end); `--features reticulum` exit 0, **153 tests**.
A clean `git clone` then passed all three gates (143 / 143 / 153) with 0 daemons left running.

**Push status:** `oag-listend` pushed to `master` (`e70e862..8a5ebef`); this entry's commit pending.

---

## 2026-11-20 — record a mirror skipped by eviction

**Repository:** `oag-listend`, commit `e70e862` on `master`; records in `open-agent-graph`.

**The gap.** `accept_and_mirror` returned `Ok(hash)` without recording anything when the item had
been evicted between the accept and the mirror -- a concurrent publish pushed it out, so `serve`
failed. `SPEC.md` says every mirror outcome is recorded and the shutdown report lists each one, so
an operator had no way to tell that this mirror never happened.

**The fix.** It records `MirrorOutcome::Failed` with the reason "evicted before it could be
mirrored".

**Tests executed.** None new, and stated plainly: the branch needs a specific interleaving (an
eviction landing between two lock acquisitions inside one call), and constructing that
deterministically would require injecting a failure into the middle of `accept_and_mirror`. So
this is a documented-contract fix, **not** a tested guard. `./scripts/check.sh` exit 0, six gates,
**142 tests**.

**Push status:** `oag-listend` pushed to `master` (`0d44577..e70e862`); this entry's commit pending.

---

## 2026-11-20 — a Reticulum stop now drains in-flight exchanges

**Repository:** `oag-listend`, commits `0986767` and `85d6856` on `master`; records in
`open-agent-graph`.

**The gap.** `SPEC.md` promises that in-flight exchanges are given `SHUTDOWN_GRACE` rather than cut
off mid-transfer. Every carrier counts its sessions for that drain -- except Reticulum, whose
workers were invisible to it, so a stop could cut a publish or a reply off.

**The fix.** `ReticulumCarrier` now exposes `active_handle()`/`active_sessions()` like the others;
its workers increment and decrement the counter, and `main` includes it in the drain loop.

**Tests executed.** New `an_in_flight_exchange_is_counted_and_drains`: with a hung mirror, a
publish's worker stays in flight; the counter must reach 1, and after `shutdown()` it must drain to
0. `./scripts/check.sh` **142 tests**; `./scripts/check.sh --features reticulum` **152 tests**.

**Push status:** `oag-listend` pushed to `master` (`e6fd62d..85d6856`); this entry's commit pending.

---

## 2026-11-20 — one Reticulum link no longer stalls the others

**Repository:** `oag-listend`, commit `e77b785` on `master`; records in `open-agent-graph`.

**The gap.** `handle()` was awaited inline in the carrier's event loop, which every link shares.
Handling a publish can wait on an IPFS mirror, so one publish to a hung Kubo node stalled *every*
link for the add timeout -- the exact property the other carriers already test and the Reticulum
carrier did not have.

**The fix.** Each link gets a worker task fed by a bounded queue (`MAX_QUEUED_FRAMES`, 64). A slow
accept delays only its own link, and a link's frames keep their order. A full queue drops that
link's buffer rather than holding up the loop.

**Tests executed.** New `a_hung_mirror_on_one_link_does_not_stall_another`: with a stub that
accepts the mirror connection and never answers, one link publishes (its reply waits out the add
timeout) while a second link fetches an item that is already held. The second must answer within
5 s. **Checked to fail against the pre-refactor inline handling, where that fetch took 10.31 s.**
`./scripts/check.sh` **142 tests**; `./scripts/check.sh --features reticulum` **151 tests**.

**Push status:** `oag-listend` pushed to `master` (`60771ac..e77b785`); this entry's commit pending.

---

## 2026-11-20 — bound a Reticulum packet send

**Repository:** `oag-listend`, commit `c96948f` on `master`; records in `open-agent-graph`.

**The gap.** `reply()` awaited `send_packet` with no bound, inside the single event loop that serves
every link. A stalled link would therefore stall all of them -- the same defect the other carriers'
per-syscall timeouts exist to prevent, and the Reticulum carrier was the one place without it.

**The fix.** Each send is bounded by `IO_TIMEOUT` (10 s); when the bound elapses the rest of that
frame is abandoned rather than queued behind the stalled link.

**Tests executed.** The bound is a small helper so it can be pinned: `bounded_send` returns `false`
when the bound elapses and `true` for a send that completes. **The probe without the bound hangs**
(killed at 300 s rather than failing), which is exactly the defect. The test passes its own 10 ms
timeout so it does not spend the real one -- the first version did, adding 10 s to every run.
`./scripts/check.sh` **142 tests**; `./scripts/check.sh --features reticulum` **150 tests**.

**Push status:** `oag-listend` pushed to `master` (`12a6a60..c96948f`); this entry's commit pending.

---

## 2026-11-20 — cap Reticulum links; correct the test counts

**Repository:** `oag-listend`, commits `5940d4b` and `12a6a60` on `master`; records in
`open-agent-graph`.

**The gap.** The Reticulum carrier held one reassembly buffer per link and links were unbounded, so
a peer could make the daemon hold an unbounded number of buffers. Every other carrier caps sessions
at 64.

**The fix.** The same cap, and the buffer policy moved into a `LinkBuffers` type so it can be unit
tested directly: the cap, an untracked link's data being ignored, several frames in one payload, a
partial frame staying buffered, and a length above the frame cap being refused outright and
dropping the link. SPEC's bounds paragraph and the coverage list updated.

**A correction worth recording.** Counting the suites exactly showed the documented counts were
wrong: the library has **65** unit tests, not 64, so the default suite is **65 unit + 77 end-to-end
= 142**, and with `--features reticulum` **70 unit + 79 end-to-end = 149**. `SPEC.md`, `README.md`
and `PROJECT_STATE.md` had been carried forward from an earlier count; `PROJECT_STATE`'s "current
bar" was stale at `64 unit + 70 end-to-end`. All corrected.

**Tests executed.** `./scripts/check.sh` exit 0, six gates, **142 tests**;
`./scripts/check.sh --features reticulum` exit 0, six gates, **149 tests**.

**Push status:** `oag-listend` pushed to `master` (`92a5854..12a6a60`); this entry's commit pending.

---

## 2026-11-20 — Reticulum: answer every frame in a packet

**Repository:** `oag-listend`, commit `92a5854` on `master`; records in `open-agent-graph`.

**The defect, found by reading the carrier against the protocol.** A Reticulum packet can carry
several frames, exactly as a client may pipeline them on a stream carrier (the native protocol's
own pipelining test covers that). The carrier cleared its reassembly buffer after one frame, so a
second frame packed into the same packet was silently dropped.

**The fix.** Drain only the bytes consumed and loop until no complete frame remains; a declared
length above `MAX_FRAME_BYTES` is now refused outright rather than treated as "not all here yet".

**Tests executed.** New `reticulum_answers_pipelined_frames_in_one_packet`: two frames packed into
one packet must both be answered in order and both stored. **Checked to fail when the buffer is
cleared after the first frame.** Fixtures are `generate_n(seed, Tone, 10)` -- small enough to fit
one packet, and different seeds give different bytes (identical silence fixtures would hash the
same and prove nothing). `./scripts/check.sh` **142 tests**; `./scripts/check.sh --features
reticulum` **144 tests**; both all six gates.

**Push status:** `oag-listend` pushed to `master` (`cfcf68c..92a5854`); this entry's commit pending.

---

## 2026-11-20 — the last carrier: Reticulum, feature-gated

**Repository:** `oag-listend`, commit `cfcf68c` on `master`; records in `open-agent-graph`.

**Why.** The directive says to extend to "whatever the imagination `Transport` seam already has",
and Reticulum was the only carrier left. An earlier note recorded it as hardware-bound; that was
wrong. `oag-imagination-reticulum` serves over Reticulum's own `TcpServer` interface, so no radio
is involved and the whole path is testable on one machine.

**Measured before committing.** Adding `reticulum = "=0.1.0"` brings **about 160 transitive
packages** (gRPC, protobuf codegen, a crypto stack) against the three dependencies the rest of the
daemon uses, and needs `protoc` at build time. That is disproportionate for an unconditional
dependency, so the carrier is **opt-in** behind a cargo feature: the default build and the two
existing CI jobs do not compile it, and a third job installs `protobuf-compiler` and runs the whole
bar with it.

**What was built.** `--reticulum ADDR` (and `--reticulum-seed SEED`, default `oag-listend`, stable
because it derives the destination's address) serves the same frame protocol over the interface.
An incoming frame goes through the same `crate::server::handle`, so `AudioStore::accept` is the
inbound rule here too — no per-carrier audio rules. Bounds: one reassembly buffer per link capped
at a frame plus one packet, the frame cap enforced by the codec, and replies chunked to a packet's
capacity.

**Tests executed.** New `tests/reticulum_carrier.rs` (cfg-gated): a minimal client dials the
interface, waits for the announce, activates one link, then publishes, fetches, verifies, answers a
ping with audio, and is refused non-audio.
* `./scripts/check.sh` — exit 0, six gates, **142 tests** (unchanged; reticulum absent).
* `./scripts/check.sh --features reticulum` — exit 0, six gates, **143 tests**.
* By hand: the daemon binds `127.0.0.1:18942`, prints its destination address, and exits 0 on
  SIGTERM; `--help` documents both flags.

**Honest limitation, recorded in SPEC.** Loopback proves the audio surface, not interop with
another Reticulum implementation.

**Clean-clone verification.** A fresh `git clone` passes all three gates: `check.sh` **142
tests**, `check.sh --release` **142 tests**, `check.sh --features reticulum` **144 tests**
(143 before the pipelining fix added its own test).

**Push status:** `oag-listend` pushed to `master` (`e84d0e8..cfcf68c`); this entry's commit pending.

---

## 2026-11-20 — replicate CI for real, and fix what it caught

**Repository:** `oag-listend`, commit `0650bc0` on `master`; records in `open-agent-graph`.

**Why.** After fixing the rustup profile, the workflow had still never been executed from this
machine. Network was available, so the CI job was replicated rather than assumed: install rustup
with `--profile minimal`, add `rustfmt` and `clippy`, clone the repository, run `./scripts/check.sh`
and `--release` with that toolchain.

**What it caught.** The first simulated run failed four tests: `push_pull()` located the built
example from `std::env::current_exe()`, assuming the test binary sits two levels under the target
directory. Here `current_exe()` reports a relocated path
(`target/debug/build/oag-listend/<hash>/out/<test>`), so the derived path does not exist. The same
suite passed elsewhere, which is why it had not surfaced.

**The fix.** `push_pull()` now derives the target directory from `CARGO_MANIFEST_DIR` (fixed at
compile time, always the workspace) honouring `CARGO_TARGET_DIR`, trying `debug` then `release`.
The nested build remains as a fallback for a clean tree.

**Tests executed.** A fresh clone under the exact CI toolchain: `./scripts/check.sh` exit 0, all six
gates, **142 tests**; `./scripts/check.sh --release` exit 0, all six gates, **142 tests**. Locally the
gate then passed **3 consecutive runs** after one contention-related flake during the build.
Scratch toolchain and clone removed afterwards.

**Push status:** `oag-listend` pushed to `master` (`8ab9a0c..0650bc0`); this entry's commit pending.

---

## 2026-11-20 — CI could never have passed: the minimal profile has no rustfmt or clippy

**Repository:** `oag-listend`, commit `8ab9a0c` on `master`; records in `open-agent-graph`.

**The defect.** `.github/workflows/check.yml` installed Rust with `--profile minimal`. That profile
is `rustc`, `rust-std` and `cargo` only, so the gate's first two steps -- `cargo fmt --check` and
`cargo clippy` -- would fail with "not installed for the toolchain", and **CI has been red on every
push** since the workflow was added.

**Verified, not assumed.** rustup is not installed on this machine (Rust comes from the distro), so
the profile was installed into a scratch `RUSTUP_HOME`/`CARGO_HOME`:

- the toolchain's `components` file lists only `cargo`, `rust-std`, `rustc`;
- `cargo fmt --version` → `'cargo-fmt' is not installed for the toolchain 'stable-...'`;
- `cargo clippy --version` → `'cargo-clippy' is not installed ...`;
- `rustup component add rustfmt clippy` → both then report versions.

**The fix.** Both jobs now run `rustup component add rustfmt clippy` after the install. Depending
on a profile's contents is exactly what fails silently, so the components the gate needs are named.

**Tests executed.** The gate itself is unchanged and still green locally: **64 unit + 78 end-to-end =
142 tests**. The scratch toolchain was removed afterwards.

**Push status:** `oag-listend` pushed to `master` (`bab8bb9..8ab9a0c`); this entry's commit pending.

---

## 2026-11-20 — test the client's runtime-failure exit code

**Repository:** `oag-listend`, commits `106dec3` and `bab8bb9` on `master`; records in
`open-agent-graph`.

**The gap.** `push_pull`'s usage errors (exit 2) were tested; its runtime failures were not. An
operator's script depends on that code, and a hang would stall it. Probed by hand first: a closed
port gives `push_pull: Connection refused` and exit 1; a missing Unix socket gives exit 1 in 47 ms;
a closed UDP port gives `udp read timed out` at exactly the 10 s bound.

**What changed.** New `the_client_fails_cleanly_against_a_dead_daemon`: runs the client against
`127.0.0.1:1` under a real 15 s timeout and asserts exit 1 with a reason on stderr. Written as a
`#[tokio::test]` with `tokio::time::timeout` on purpose -- a plain `Command::output()` cannot bound
a hang, and an elapsed-time assertion afterwards is too late.

**Tests executed.** **Checked to fail when the client exits 2 instead of 1** (the probe had to delete
the built example first, since `push_pull()` reuses a stale binary), and to pass with it.
`./scripts/check.sh` green across all six gates: **64 unit + 78 end-to-end = 142 tests**. A clean `git clone` then passed **both** profiles, 142 tests each, with 0 daemons left
running afterwards.

**Push status:** `oag-listend` pushed to `master` (`28e50a2..bab8bb9`); this entry's commit pending.

---

## 2026-11-20 — failing tests no longer leave daemons running

**Repository:** `oag-listend`, commit `28e50a2` on `master`; records in `open-agent-graph`.

**The gap, noticed while cleaning up.** A sweep of stray processes showed eight `oag-listend`
daemons still running. A **passing** suite leaks none — `cargo test --test binary` leaves 0 behind
— so the leaks came from *failing* probe runs. `tokio::process::Child` does not kill on drop, so a
test that panics leaves its daemon bound to its ports and holding its store directory.

**What changed.** A `Daemon` guard in `tests/binary.rs` (`Deref`/`DerefMut` to `Child`, kill on
drop) wraps all seven spawn sites. A test that wanted a clean stop has already sent it and awaited
the exit, so the guard's SIGKILL only fires where nothing else reaped the child.

**Tests executed.** Injected a failure into a daemon-spawning test: **before, the child survived;
after, 0 daemons are left behind.** `./scripts/check.sh` green across all six gates:
**64 unit + 77 end-to-end = 141 tests**, and `--release` likewise afterwards, with 0 daemons
left on the machine.

**Push status:** `oag-listend` pushed to `master` (`6c63b9b..28e50a2`); this entry's commit pending.

---

## 2026-11-19 — the clippy gate now denies warnings

**Repository:** `oag-listend`, commit `6c63b9b` on `master`; records in `open-agent-graph`.

**The gap.** `scripts/check.sh` ran `cargo clippy --all-targets` with no `-D warnings`. A new lint was
printed and the gate still passed — weaker than the rustdoc gate, which already used
`RUSTDOCFLAGS="-D warnings"`. The earlier "clippy 0 warnings" results were true of the default
lint set, but nothing enforced them.

**What changed.** `cargo clippy -p oag-listend --all-targets -- -D warnings`. Adding it **immediately
surfaced a real lint the gate had been passing**: a needless borrow in `tests/persistence.rs`
(`dir.join(&"cd".repeat(32))`). Fixed. The header comment, which described four gates when the
script runs six, is corrected too.

**Tests executed.** `./scripts/check.sh` green across all six gates: **64 unit + 77 end-to-end =
141 tests**. The gate's new strictness was confirmed by the failure it produced before the fix.
Afterwards a clean `git clone` passed **both** `./scripts/check.sh` and `--release`, all six
gates, 141 tests each.

**Push status:** `oag-listend` pushed to `master` (`db790de..6c63b9b`); this entry's commit pending.

---

## 2026-11-19 — boundary probes: the remaining numeric caps, by hand

**Repository:** `oag-listend`; records in `open-agent-graph`. No code change.

**Method.** The two defects found earlier in the session (the soft head bound, and before it the
CID injection) both came from *probing a boundary* rather than reading. So the remaining numeric
caps were probed against the real binary:

- **HTTP `Content-Length` cap.** Declared `MAX_BODY_BYTES` → accepted (the daemon waits for the
  body); `MAX_BODY_BYTES + 1` and `+1MB` → `413`. Exact.
- **Frame cap before allocation.** `read_frame` checks `len > max_frame` before `vec![0u8; len]`, so
  a lying length cannot make the daemon allocate on a client's word. Exact.
- **HTTP listing limit.** Uses the same `normalize_limit(limit, MAX_LIST_LIMIT)` as the native
  carrier, so `limit > cap` clamps rather than erroring.
- **`OPTIONS` with a body.** A preflight carrying `Content-Length: 200` and 200 bytes still gets
  its full `204`, because `read_request` consumes the declared body before routing -- no unread data
  is left for the close to turn into a reset.

- **Malformed request lines**, all answered without a panic: HTTP/1.0 with no `Host` → 200;
  no version → 200; method only and an empty request line → 400; an absolute-form target
  → 404 (this is not a proxy); `POST` with an empty body → 400; `DELETE` → 404.

**Tests executed.** None new; the suite is 141 green as of this entry.

**Push status:** all five clean and pushed.

---

## 2026-11-19 — the HTTP head bound is now exact

**Repository:** `oag-listend`, commits `1133256` and `db790de` on `master`; records in
`open-agent-graph`.

**Found by probing the boundary by hand.** `MAX_HEAD_BYTES` is documented as "the largest HTTP
request head this carrier will read", but the growth check runs at the top of the read loop, so the
cap was soft by up to one read chunk: a head of 16384 was served (200), **16385 was also served
(200)**, and 20000 was refused (400).

**The fix.** The head itself — everything up to and including the blank line — is now measured and
refused when it exceeds the bound. A body arriving in the same read does not count against it, so a
large publish is unaffected; the per-read growth check remains as the guard against unbounded
accumulation.

**Tests executed.** New `the_request_head_bound_is_exact` pins both sides: a head of exactly
`MAX_HEAD_BYTES` is served, one byte more is 400. **Checked to fail when the new check is disabled**,
and to pass with it. `./scripts/check.sh` green across all six gates:
**64 unit + 77 end-to-end = 141 tests**.

**Verification after the fix.** Clean `git clone` → `./scripts/check.sh` and
`./scripts/check.sh --release`: **both exit 0, all six gates, 141 tests**.

**An incident, recorded plainly.** The first attempt cloned into `/tmp`, which is a 7.8 GB
tmpfs; three clones filled it and the run died with `Disk quota exceeded`. Nothing in the
repositories was affected (they live on the 203 GB root filesystem), and the clones were
removed. The verification was redone with the clone on the large disk.

**Push status:** `oag-listend` pushed to `master` (`604124f..db790de`); this entry's commit pending.

---

## 2026-11-19 — README accuracy, and a clean-clone verification of the whole session

**Repository:** `oag-listend`, commit `604124f` on `master`; records in `open-agent-graph`.

**What changed.** README described `scripts/check.sh` as "fmt, clippy, rustdoc, tests"; it runs six
gates (fmt, clippy, rustdoc, examples, tests, systemd unit). Corrected. Re-read the whole README
against the code: the "four generated items" claim is accurate (`wav::sounds(seed, 3)` plus the
canonical ping), as are the `--dir`, `--max-bytes`, `--ipfs-api`, `--check`, signal and
non-loopback descriptions.

**Verification.** Clean `git clone` of `oag-listend`, then `./scripts/check.sh` and
`./scripts/check.sh --release`: **both exit 0, all six gates, 140 tests each**, and the clone's
working tree is clean afterwards (nothing untracked). This is the first release-profile run since
the source changes earlier in the session, so the debug/release overflow divergence is covered too.

**Push status:** `oag-listend` pushed to `master` (`30ec208..604124f`); this entry's commit pending.

---

## 2026-11-19 — assert the HTTP index, and catch SPEC up with the suite

**Repository:** `oag-listend`, commit `30ec208` on `master`; records in `open-agent-graph`.

**The gap.** `GET /` is the operator's "what is this", and README points an operator at it. The test
asserted only that the body contained the daemon's name, so a change to the reported protocol,
item count, byte accounting, or the audio-only statement would have passed unnoticed. Separately,
`SPEC.md`'s coverage list had fallen behind the suite: the servable-bound round trip, the sidecar
bounds, load-time eviction, the load-path skips, the mirror-log cap, hang-up resilience, IPv6 on the
stream/HTTP carriers and the unwritable-stdout behaviour were all unpinned in the spec.

**What changed.** The index test now asserts the protocol version, `items: 0`, `bytes: 0 / <bound>`
and the audio-only line. SPEC's coverage list gains four bullets naming the behaviours above.

**Tests executed.** **Checked to fail when the index wording changes** (`only audio is accepted`
→ `only audio is taken`), and to pass with it. `./scripts/check.sh` green across all six gates:
**64 unit + 76 end-to-end = 140 tests**.

**Push status:** `oag-listend` pushed to `master` (`3cfe821..30ec208`); this entry's commit pending.

---

## 2026-11-19 — test the stream and HTTP carriers over IPv6

**Repository:** `oag-listend`, commits `d734948` and `3cfe821` on `master`; records in
`open-agent-graph`.

**Found by experiment.** The directive says the service is reached "any way that anyone connects".
Bound all four carriers on `[::1]` by hand: native, HTTP and UDP all served audio and verified, so
IPv6 works. Only UDP had a test -- a hardcoded IPv4 address in the native or HTTP bind path would
have broken both silently.

**What changed.** New `the_stream_and_http_carriers_serve_over_ipv6_loopback` in
`tests/back_and_forth.rs`. It binds both carriers on `[::1]:0`, then connects to the **literal**
`[::1]:<port>` rather than the address the carrier reported, so it proves reachability at IPv6 and
not merely at whatever was bound. It does a full publish/fetch/verify round trip and a
`GET /audio/{hash}` whose body must be the audio bytes. It skips cleanly where the host has no
IPv6 loopback.

**Tests executed.** **Checked to fail when `ListendServer::bind_shared` binds `127.0.0.1:0`
regardless of the requested family**, and to pass with the requested family.
`./scripts/check.sh` green across all six gates: **64 unit + 76 end-to-end = 140 tests**.

**Push status:** `oag-listend` pushed to `master` (`91f8124..3cfe821`); this entry's commit pending.

---

## 2026-11-19 — test hang-up resilience

**Repository:** `oag-listend`, commits `69b1fc5` and `91f8124` on `master`; records in
`open-agent-graph`.

**Found by experiment, not by reading.** A client that disappears mid-reply is ordinary, not
hostile. Ran the real binary and had **300 native** and **300 HTTP** clients send a request and close
without reading: afterwards both carriers still answered (`GET /ping` 200, 44844 bytes; `GET /` 200),
the process was alive, it exited 0 on SIGTERM, and no panic was logged.

**What changed.** New `clients_hanging_up_mid_reply_leave_the_daemon_serving` in
`tests/robustness.rs`: 200 hang-ups on each of the native and HTTP carriers, then a normal `Hello`
and `GET /` must still succeed. The ping is used because it is the largest reply the daemon sends
unprompted, so it is the most likely to be mid-write when the peer vanishes.

**Tests executed.** **Checked to fail when a session error stops the accept loop** (sessions served
inline with `break` on error), and to pass with the spawn-per-session behaviour.
`./scripts/check.sh` green across all six gates: **64 unit + 75 end-to-end = 139 tests**.

**Push status:** `oag-listend` pushed to `master` (`8e2c50e..91f8124`); this entry's commit pending.

---

## 2026-11-19 — test every skip reason on the store load path

**Repository:** `oag-listend`, commits `760793f` and `8e2c50e` on `master`; records in
`open-agent-graph`.

**The gap.** Only the corrupted-file skip was covered. A store directory is operator-controlled, so a
restart must skip what it cannot honestly serve and report how many: a name that is not a content
hash, bytes that do not hash to their name, a non-audio payload under a hash-shaped name, a
non-regular file named like a hash, and a file larger than the bound.

**What changed.** New `a_restart_skips_what_it_cannot_honestly_serve` in `tests/persistence.rs`: one
directory with six entries, one of them honest, asserting `loaded == 1` and `skipped == 5`. The
oversized entry is **valid audio whose name is its own hash** -- that is what makes the test detect
the size guard, since a merely misnamed file would be skipped by the hash check regardless.

**Tests executed.** **Checked to fail when the load size guard is removed** from `load_dir`, and to
pass with it. `./scripts/check.sh` green across all six gates:
**64 unit + 74 end-to-end = 138 tests**.

**Push status:** `oag-listend` pushed to `master` (`b4f11ef..8e2c50e`); this entry's commit pending.

---

## 2026-11-19 — test the mirror log's cap

**Repository:** `oag-listend`, commits `0d456fe` and `b4f11ef` on `master`; records in
`open-agent-graph`.

**The gap.** `MIRROR_LOG_LIMIT` (64) keeps the most recent mirror outcomes and drops the oldest, so
a long run cannot grow the log without limit. It is a memory bound with no test.

**What changed.** New `the_mirror_log_is_capped_and_keeps_the_newest` in `tests/ipfs_mirror.rs`: it
records 70 failures and asserts the log holds 64, `mirror_summary()` counts the retained 64, the
oldest (`70 - 64`) was dropped, and the newest is last.

**Tests executed.** **Checked to fail when the cap is disabled** (`if false && ...`), and to pass
with it. `./scripts/check.sh` green across all six gates:
**64 unit + 73 end-to-end = 137 tests**.

**Push status:** `oag-listend` pushed to `master` (`3a9558a..b4f11ef`); this entry's commit pending.

---

## 2026-11-19 — test eviction during a store load

**Repository:** `oag-listend`, commits `1985272` and `3a9558a` on `master`; records in
`open-agent-graph`.

**The gap.** Runtime eviction is covered by `tests/eviction.rs`; the load path is not. A restart
whose directory exceeds `--max-bytes` must honour the bound and unlink what it drops. Without that,
an operator who lowers the bound would find the daemon holding more than it advertises, with the
surplus files never leaving the disk.

**What changed.** New `a_restart_over_the_bound_evicts_and_unlinks`. The directory is read in
filesystem order, so *which* item is dropped is not deterministic; the test asserts the invariants
an operator depends on instead: `bytes_held() <= bound`, `report.loaded == store.len()`,
`report.evicted + report.loaded == 3`, at least one eviction, and the directory naming exactly the
same set as the index (nothing served that is not on disk, no file left for a dropped item).

**Tests executed.** **Checked to fail when the load-time unlink loop is removed from `load_dir`**,
and to pass with it. `./scripts/check.sh` green across all six gates:
**64 unit + 72 end-to-end = 136 tests**.

**Push status:** `oag-listend` pushed to `master` (`34ce2bc..3a9558a`); this entry's commit pending.

---

## 2026-11-19 — test the sidecar bounds, not just the constants

**Repository:** `oag-listend`, commits `bc9979e` and `34ce2bc` on `master`; records in
`open-agent-graph`.

**The gap.** The store directory is operator-controlled, so a sidecar is bounded before it is read:
a `.json` over `MAX_SIDECAR_BYTES` (4096) and a `.cid` over `MAX_CID_SIDECAR_BYTES` (128) are
ignored rather than pulled into memory, and a `.cid` is also run through `validate_cid`. Neither
guard was covered by a test.

**What changed.** New `oversized_sidecars_are_ignored_and_the_audio_still_loads` in
`tests/persistence.rs`. Two items: one with a small honest `.json` sidecar (asserted to be read, so
the test proves the guard is what ignores the large one rather than a path that never reads
sidecars), and one with both sidecars over their bounds (asserted ignored) while its audio still
loads, serves, and verifies.

**Tests executed.** **Checked to fail when the `.json` size guard is removed** from `read_sidecar`,
and to pass with it. `./scripts/check.sh` green across all six gates:
**64 unit + 71 end-to-end = 135 tests**.

**Push status:** `oag-listend` pushed to `master` (`057ae78..34ce2bc`); this entry's commit pending.

---

## 2026-11-19 — test the servable bound, not just the constant

**Repository:** `oag-listend`, commit `af99f91` on `master`; records in `open-agent-graph`.

**The gap.** `MAX_SERVABLE_BYTES` exists so that no accepted item is unfetchable: a fetch reply also
carries the hash, a length prefix and an optional CID, so an item at the frame cap would make a reply
larger than the cap and the client's own decoder would reject it. Only the constant was covered by a
unit test; the property it guarantees was never exercised.

**What changed.** New `an_item_at_the_frame_bound_is_served_and_one_byte_more_is_refused` in
`tests/back_and_forth.rs`. It builds a well-formed 16-bit PCM WAV of exactly `MAX_SERVABLE_BYTES`
(zero payload, honest header), publishes it, fetches it back whole, verifies the hash against the
bytes, then publishes one byte more and asserts a refusal and that the store still holds one item.

**Tests executed.** **Checked to fail when the 256-byte headroom is removed from the constant**
(`MAX_FRAME_BYTES - 256` → `MAX_FRAME_BYTES`), and to pass with it — so the test pins the exact
off-by-one the constant prevents. `./scripts/check.sh` green across all six gates:
**64 unit + 70 end-to-end = 134 tests**.

**Push status:** `oag-listend` pushed to `master` (`ff76849..af99f91`); this entry's commit pending.

---

## 2026-11-19 — a full log sink no longer stops the audio

**Repository:** `oag-listend`, commit `ff76849` on `master`; records in `open-agent-graph`.

**The defect, reproduced.** Every line the daemon printed went through `println!`, which panics when
the write fails. `oag-listend --no-generate --listen 127.0.0.1:17900 > /dev/full` exited **101** with
`failed printing to stdout: No space left on device (os error 28)`. Under systemd stdout *is* the
journal, so an unavailable journal, a full disk, or a pipe whose reader exited would panic the
process and stop serving audio.

**The fix.** Output goes through `say!` / `complain!`, which ignore a failed write. Verified by hand
that a daemon with stdout on `/dev/full` still answers `Hello`, still publishes and fetches with
`verified=true`, and still exits 0 on SIGTERM.

**Tests executed.** New `an_unwritable_stdout_does_not_stop_the_audio` in `tests/binary.rs`: it opens
`/dev/full`, spawns the daemon with that as stdout, waits on the listener (the announcement cannot be
read by design), then asserts Hello, publish and fetch verify, and a clean exit 0. **Checked to fail
against the pre-fix `main.rs` and pass after.** `./scripts/check.sh` green across all six gates:
**64 unit + 69 end-to-end = 133 tests**.

**Push status:** `oag-listend` pushed to `master` (`579b01c..ff76849`); this entry's commit pending.

---

## 2026-11-19 — refuse a CID the mirror hands back that cannot be one

**Repository:** `oag-listend`, commit `579b01c` on `master`; records in `open-agent-graph`.

**The defect.** The write half of the IPFS mirror read `Hash` out of Kubo's `/api/v0/add` reply,
stored it, persisted it to `<hash>.cid`, and later wrote it into `X-IPFS-CID` and `ETag` on every
fetch of that audio — without ever running it through `validate_cid`, the plausibility check the read
half already applies. The mirror is cleartext HTTP by design, so that reply comes from the network:
a malicious or compromised node, or an on-path attacker, answering with
`{"Hash":"evil\r\nX-Injected: yes"}` would put a second header line into responses the daemon sends.

**The fix.** `IpfsPublisher::add` now runs the returned CID through `validate_cid`. A refusal becomes
`MirrorOutcome::Failed`, which is exactly what a failed mirror means: the audio was already written
durably before the mirror was attempted, so it is still stored and served. No header can carry an
unvalidated value, on either the native or the HTTP surface.

**Tests executed.** Two new tests: a unit test (`a_cid_that_is_not_plausible_is_refused_not_stored`)
and an end-to-end one (`an_implausible_cid_from_the_node_is_refused_not_served`) that mirrors to a
stub returning header text and asserts the audio still verifies, the failure is recorded, and no
`X-Injected` or `X-IPFS-CID` header appears. **Both were checked to fail with the guard disabled.**
`./scripts/check.sh` green across all six gates: **64 unit + 68 end-to-end = 132 tests**.

**Push status:** `oag-listend` pushed to `master` (`b11dba2..579b01c`); this entry's commit pending.

---

## 2026-11-19 — refuse ambiguous HTTP body framing

**Repository:** `oag-listend`, commit `b11dba2` on `master`; records in `open-agent-graph`.

**Why.** Auditing the HTTP reader for protocol correctness turned up two ways a client — or a reverse
proxy in front — could make the body's extent ambiguous.

**What changed.**

- *Duplicated `Content-Length`.* The reader accepted more than one and let the last win. RFC 7230
  3.3.3 requires rejecting it, and a proxy that chooses a different value is the classic
  request-smuggling setup. Now refused with `400 multiple Content-Length headers`.
- *Non-chunked transfer encoding.* Only `Transfer-Encoding: chunked` was refused, so
  `Transfer-Encoding: gzip` fell through and its body was read as if `Content-Length` framed it.
  Any transfer encoding is now refused, with a message that still says to send `Content-Length`.

Both refusals happen **before** the body is read, so `unread_body` is set and the body is drained
after a half-close rather than turning the refusal into a TCP reset that discards the response.

**Tests executed.** Two new end-to-end tests (`two_content_length_headers_are_refused`,
`a_non_chunked_transfer_encoding_is_refused`). **Each was checked to fail when its guard is
disabled** — disabling both also fails the existing chunked test, which shares the transfer-encoding
guard. `./scripts/check.sh` green across all six gates: **64 unit + 66 end-to-end = 130 tests**.
`SPEC.md` and `README.md` updated, including the test count.

**Push status:** `oag-listend` pushed to `master` (`6380b2f..b11dba2`); this entry's commit pending.

---

## 2026-11-19 — operator failure modes, exercised

**Repository:** `oag-listend`; records in `open-agent-graph`. No code change.

**Why.** An operator hits these before any test does, and a panic or a hang here is a defect even
when every test is green.

**Observed, on the real binary:**

- Port already in use: exit 1, `oag-listend: cannot bind 127.0.0.1:17500: Address already in use
  (os error 98)`. No panic, no retry loop.
- `--unix` at an existing regular file: exit 1, `"/tmp/regfile.sock" exists and is not a socket` —
  the file is not deleted.
- `--unix` at a socket another live daemon serves: exit 1, `"/tmp/live.sock" is already being served`.
- Socket left by a `SIGKILL`ed daemon: a restart reclaims it and binds.
- Clean SIGTERM: the socket file is removed.

**A false alarm, recorded as such.** An earlier run of the stale-socket case used `timeout 30` around
the daemon, and `kill -9 $!` killed `timeout`, which forwarded a stop to the daemon — so the socket
vanished and the log showed a clean shutdown. Re-tested with the daemon as the direct child: SIGKILL
leaves the stale socket, as it must.

**Further failure modes exercised, all clean (no panic, no hang):** `--max-bytes 0` (starts,
reports the zero bound, refuses audio); a read-only `--dir` (generated sounds are rejected with
the persist reason, the daemon still serves); `--dir` at a regular file (exit 1, `File exists`);
an invalid `--listen` (exit 1, `invalid socket address`); `--seed abc` (usage error, exit 2); a
150-character `--unix` path (`path must be shorter than SUN_LEN`, exit 1); `--unix` in a
missing directory (`No such file or directory`, exit 1).

**Tests executed.** None new in this sweep; the suite is 133 green as of this entry.

**Push status:** all five clean and pushed.

---

## 2026-11-19 — acceptance sentence run by hand, observed

**Repository:** `oag-listend`; records in `open-agent-graph`. No code change.

**The run.** `oag-listend --listen 127.0.0.1:17100 --http 127.0.0.1:17101 --udp 127.0.0.1:17120
--unix /tmp/acc.sock --dir /tmp/acc-store --seed 7`, then the operator client against it. Observed:

- Startup generated 4 items; `Hello { protocol_version: 1, item_count: 4 }`.
- TCP, UDP and Unix each: `published tone ... fetched tone ... same-hash ... verified=true`, same for
  noise; `non-audio: Rejected { reason: "input is not audio; only audio is accepted" }`.
- HTTP: `GET /audio` lists hashes with `X-Total-Count`; `GET /audio/{hash}` returns 200,
  `Content-Type: audio/wav`, byte-identical to the stored file, `Cache-Control: immutable`; `Range`
  returns 206 matching the prefix; `HEAD` 200; non-audio `POST` 400; unknown hash 404.
- SIGTERM: daemon exit 0.
- The pruner fired during the run (`removed 9 items`), confirming the newly wired config is live.

**Tests executed.** `./scripts/check.sh` exit 0, 128 tests; the above is the operator-visible proof.
Closing live run on `8a5ebef`: native, UDP and Unix each published and fetched with
`verified=true`, non-audio was refused, `GET /ping` returned 200 with 44,844 bytes and
`GET /` 200, SIGTERM exited 0, and a `--check` restart loaded 6 items.

Re-run on the final HEAD after all of the above: all four carriers published and fetched with
`verified=true`, non-audio was refused, `GET /ping` returned 200 with 44,844 bytes, SIGTERM
exited 0, and a `--check` restart loaded 6 items.

The suite was then repeated **8 consecutive times** (5 idle, 3 under three busy loops): 0 failures,
so no test depends on timing luck. After the later additions (140 tests, including hang-up and
per-syscall-timeout cases) it was run **3 more times under three busy loops**: 0 failures.

**Push status:** all five clean and pushed.

---

## 2026-11-19 — the imagination carrier seam is fully accounted for

**Repository:** `oag-listend`; records in `open-agent-graph`. No code change.

**Question closed.** Read `oag-imagination-random-data-compute/spec/03-transports.md` directly instead
of trusting the summary. The seam's carriers are: `TcpTransport`, `UdpTransport`, `HttpTransport`
(JSON POST), `IpfsTransport`/`PublishingIpfsTransport`, `ProxiedTransport` (CONNECT tunnel), and
`ReticulumTransport`. Every one is covered in `oag-listend` — TCP, UDP, HTTP, IPFS, and the Unix
socket as carriers; the CONNECT tunnel end to end; Reticulum unbuilt and hardware-bound. There is no
seventh carrier and no unimplemented seam item, so "extend carriers one at a time" has no remaining
implementable step.

**Tests executed.** None new; `./scripts/check.sh` last run exit 0, 128 tests.

**Push status:** all five clean and pushed.

---

## 2026-11-19 — re-verified the settled invariants, not the notes

**Repository:** `oag-listend`; records in `open-agent-graph`. No code change this pass.

**What was re-checked, because the last gap was found exactly this way:**

- Build-generation pruner now wired in **all five** repositories: `.cargo/config.toml` sha256
  `7973fe4d70d3b1fd` in each, including `oag-listend` where it was missing.
- `scripts/rustc-wrapper.sh`, `scripts/prune-target.py`, `scripts/test_prune_target.py` sha256-identical
  across all five; no `__pycache__` tracked anywhere.
- Licence: `AGPL-3.0-or-later` in every `Cargo.toml` and the AGPL v3 text in every `LICENSE`.
- `#![forbid(unsafe_code)]` in **21 of 21** `.rs` files.
- `README.md` documents every flag `main.rs` actually accepts; `SPEC.md` status line says "serves".
  Re-read both in full afterwards; the only remaining inaccuracy was `check.sh` described without
  its examples gate, now corrected (commit `e84d0e8`, documentation only).
  Final integrity check: all five repositories have 0 tracked build artefacts and 0
  untracked non-`target/` files; `oag-listend` tracks 37 files.
- Repository identity by remote: all five `origin` URLs match the directive, and no sixth
  `oag*` checkout exists.
- Environment-dependence sweep of the tests: after the `push_pull()` fix, the only
  environment references left are `CARGO_BIN_EXE_*`, `CARGO_PKG_VERSION`,
  `CARGO_MANIFEST_DIR`, `std::env::temp_dir()` and a handled `CARGO_TARGET_DIR` -- no
  other `current_exe()` assumption.
- Final workspace state: all five repositories clean, 0 unpushed, correct branches
  (`open-agent-graph`/`oag-mesh` on `main`; the other three on `master`), 0 stray daemon
  processes, and `.cargo/config.toml`, `rustc-wrapper.sh`, `prune-target.py` and
  `test_prune_target.py` byte-identical across all five (5 occurrences of each hash).
- Gate documentation caught up: `scripts/README.md`'s `check.sh` row and the README build
  section both predated the examples gate and `--features` (commit `0d44577`, documentation
  only).
- Panic-safety sweep of the library: every slice and index reachable from network bytes is
  guarded by a length check or a `find_*` result, and every `unwrap`/`expect` outside the
  test modules sits on a value just validated. No hostile-input panic found.
- Dependency set: three direct dependencies (`blake3`, `thiserror`, `tokio`) and 42 packages in
  total, all mainstream and maintained; `cargo-audit` is not installed here, so no advisory scan
  was run — recorded as an absence, not a pass.
- Frame codec audited: bounds-checked `Reader`, string lengths validated against remaining
  bytes, hash validated as 64 hex and lowercased. The encoders' `as u16` / `as u32` casts are
  latent truncation, but every daemon-side value is bounded by construction (16 MiB frame cap,
  10 000 list limit, 100 000 load cap), so it is unreachable; making 40 call sites fallible for it
  would be churn, not hardening.
- Generator and UDP carrier audited: the WAV writer's RIFF/data sizes are checked
  against the actual bytes, the sample count is capped, and the canonical ping is
  asserted to fit a datagram; the datagram carrier refuses any reply that exceeds one
  datagram rather than truncating it. No defect.

**Tests executed.** `./scripts/check.sh` exit 0, **128 tests** (64 unit + 64 end-to-end).

**Push status:** no commit needed; all five clean and pushed.

---

## 2026-11-19 — wire the build-generation pruner in oag-listend

**Repository:** `oag-listend`, commit `6380b2f` on `master`; records in `open-agent-graph`.

**The gap.** Re-checking the settled tooling invariant, rather than trusting the earlier record, showed
that `oag-listend/.cargo/` existed but was **empty**: the build-generation pruner that the other four
repositories run through `scripts/rustc-wrapper.sh` was silently inactive in this repository — the one
whose `target/` is most likely to grow.

**What changed.** `.cargo/config.toml` copied verbatim from `open-agent-graph`; the file is now
sha256-identical (`7973fe4d70d3b1fd`) across all five repositories. Verified that `cargo build` now
runs through the wrapper (it creates `target/.rustc_info.json`) and that `scripts/test_prune_target.py`
still passes 16 tests.

**Tests executed.** `./scripts/check.sh` green across all five gates: **64 unit + 64 end-to-end**.

**Regression check.** A clean `git clone` of `oag-listend` builds through the wrapper
(creates `target/.rustc_info.json`) and `./scripts/check.sh` passes all six gates, **128 tests**.

**Push status:** `oag-listend` pushed to `master`; this entry's commit pending.

---

## 2026-11-12 — oag-listend is founded: the global daemon that serves audio

**Repository:** `oag-listend` (new, https://github.com/webdawg/oag-listend), plus `open-agent-graph`
(the directive and records).

**Founding.** The operator's statements, verbatim in `oag-listend/USER_INPUT_RECORD.md`: "this is
the global daemon that serves all audio information to all networks ... a real audio serving
service that serves audio to any present and future technology - any way that anyone connects to
this service - it just serves audio", and "the service also listens to all incoming but only
accepting audio as an input and nothing else".

**What was built.** The audio surface, AGPL-3.0-or-later: `AudioItem` (bytes + BLAKE3 identity),
`AudioStore` (bounded, oldest-first eviction, `serve` by hash), `AudioFormat::sniff` (WAV, AIFF,
FLAC, Ogg, MP3 by ID3 or frame sync, M4A by audio-bearing ISO BMFF brand), and
`AudioStore::accept` -- the inbound rule, enforced at the surface so no carrier can forget it:
bytes are admitted only if they carry a recognised audio signature, checked on the bytes and never
on a declared type. `SPEC.md` carries a truthful "founded, not yet implemented" status line. The
shared build-prune tooling is installed and byte-identical.

**Tests executed.** `cargo test -p oag-listend`: 5 passed, 0 failed. The inbound-rule test proves
six audio signatures are accepted and six non-audio inputs (HTML, JSON, PNG, PDF, empty, plain
text) are rejected *regardless of their declared type*, and that an MP4 video brand is rejected as
ISO-BMFF-but-not-audio.

**Directive.** `DIRECTIVE.md` now names five repositories, adds "THE SERVING PURPOSE" (every core
task directive serves the audio-serving purpose; prefer the contribution that brings a carrier
closer to its first byte or hardens the inbound rule), and threads the fifth repo through every
count, branch list, and record-location rule.

**Commit identifiers:** `oag-listend` founding commit (see that repository's log); `open-agent-graph`
this entry's commit.
**Push status:** both pushed.
**Remaining defects / next.** No carrier is wired. The first slice is one carrier (TCP, borrowed
from the imagination layer's `Transport` seam), one item, served end to end with the receiver
verifying the hash. The format sniff is a closed list that grows only when a real carrier needs a
format.

---

## 2026-11-14 — bounded shutdown grace; register stop signals eagerly

**Repository:** `oag-listend`, commits `16f953f` and `19c7c4f` on `master`; records in
`open-agent-graph`.

**Two related defects, both in shutdown.**

1. *In-flight exchanges were cut off.* SIGTERM/SIGINT stopped the accept loops and exited at
   once, so a client mid-download lost the tail of the item. Now accepting stops immediately, then
   the daemon waits up to `SHUTDOWN_GRACE` (2 s) for sessions to drain -- honestly counted per
   stream carrier via an atomic counter -- and a client that never reads cannot hold a stop open.
2. *An early Ctrl+C could kill the daemon outright.* Reproduced only under CPU load: the signal
   arrived before `main` reached the select that registered the handler, so the default terminate
   action fired (exit by signal, not 0). The window ran from exec to that point. Both stop
   signals are now registered at the very top of `main`, using `unix::signal` for SIGINT as well as
   SIGTERM because `ctrl_c()` only registers on first poll.

**Test hardening found while writing the test.** The shutdown tests only waited for stdout; under
load the kill could still land before the child reached `main`. They now confirm a successful Hello
first, which proves the handlers exist (registration precedes binding). The in-flight scenario also
moved to the native carrier: it had allocated its HTTP port up front, which raced another process.

**Verification.** New `an_in_flight_download_survives_a_stop`: a client requests an 8 MiB item over a
2 KiB receive buffer, does not read while SIGTERM arrives, then drains and must receive every byte,
with the daemon exiting 0 promptly. Under 3 busy loops, 10 consecutive runs: **0 failures**
(previously 3 of 8 failed on the signal race alone).

**Tests executed.** `cargo test -p oag-listend`: **64 unit + 64 end-to-end, 0 failed**;
`./scripts/check.sh` green across all five gates.

**Commit identifiers:** `oag-listend` `16f953f`, `19c7c4f`; this entry's commit pending.

---

## 2026-11-14 — accept multibase characters in CID validation

**Repository:** `oag-listend`, commit `8ab17c3` on `master`; records in `open-agent-graph`.

**The defect.** A defect I introduced earlier in this run: `validate_cid` accepted only ASCII
alphanumerics, so base64url CIDs containing `-` or `_` were refused as implausible identifiers before
any request was made. Real CIDs in that encoding would have been unusable.

**What changed.** The validator now accepts those two characters -- the multibase alphabet extends
beyond alphanumerics -- while still rejecting path separators, whitespace and over-length strings.
Test pins both directions: `bafyabc-_` is accepted; `..%2f..`, spaces and over-length are refused.

**Tests executed.** `cargo test -p oag-listend`: **64 unit + 61 end-to-end, 0 failed**;
`./scripts/check.sh` green.

**Commit identifiers:** `oag-listend` `8ab17c3`; this entry's commit pending.

---

## 2026-11-14 — serial execution check

**Repository:** `oag-listend`, no code change.

**Why.** A suite can pass under parallel execution while hiding order dependence or leaked state
between tests. Run serially to rule that out.

**Result.** `cargo test -p oag-listend -- --test-threads=1`: **125 passed, 0 failed** (64 unit +
61 end-to-end), no failures, so nothing depends on parallel scheduling.

**Commit identifiers:** none (no change needed).

---

## 2026-11-14 — fix the `ftyp` largesize parse to match ISO 14496-12

**Repository:** `oag-listend`, commit `ebf9211` on `master`; records in `open-agent-graph`.

**The defect.** Found by reading the sniff code rather than its tests. When a box declares
`size == 1`, the 64-bit `largesize` sits **immediately after the type field** (bytes 8..16,
ISO 14496-12), which pushes the major brand to bytes 16..20 and the compatible brands to 24.
The parser read `largesize` from bytes 16..24 and checked the major brand at 8..12 -- and the
unit test constructed its fixture to match that mistake, so the test passed while the parse was
wrong.

**What changed.** Rewritten for all three header shapes: normal size, `size == 0` (to end of file),
and `size == 1` (largesize at 8..16). The test now covers each shape with the audio brand as major
and as compatible, plus an `avc1`/`mp42` video box in the largesize form staying refused.

**Verified both ways.** Restoring the old offsets makes the new test fail; the fix passes.

**Tests executed.** `cargo test -p oag-listend`: **64 unit + 61 end-to-end, 0 failed**;
`./scripts/check.sh` green across all five gates.

**Commit identifiers:** `oag-listend` `ebf9211`; this entry's commit pending.

---

## 2026-11-14 — test that SIGINT is a clean stop

**Repository:** `oag-listend`, commit `fe837a7` on `master`; records in `open-agent-graph`.

**The gap.** SPEC and README both promise "handles SIGINT and SIGTERM", but only SIGTERM was
covered automatically. SIGINT (Ctrl+C) is the signal an operator actually hits first.

**What changed.** New binary-level test sends SIGINT to the daemon and asserts exit 0 with the Unix
socket removed.

**Verified both ways.** Replacing the `ctrl_c()` arm of the shutdown select with a pending future
makes the test fail; restoring it passes. (As before, the probe required rebuilding the binary
explicitly -- `cargo test` does not rebuild bins either.)

**Tests executed.** `cargo test -p oag-listend`: **64 unit + 61 end-to-end, 0 failed**;
`./scripts/check.sh` green across all five gates.

**Commit identifiers:** `oag-listend` `fe837a7`; this entry's commit pending.

---

## 2026-11-14 — run the bar against a clean clone

**Repository:** `oag-listend`, no code change.

**Why.** The new CI workflow checks out committed content only; a local run reuses whatever is in
`target/` and can hide dependence on untracked state. A clone is the closest local simulation of what
CI actually does.

**Method.** `git clone /home/webdawg/DATA/CODE/oag-listend /tmp/oag-fresh` -- resulting tree contains
only committed files (Cargo.toml, Cargo.lock, crates/, scripts/, .github/, docs) and no `target/`.

**Result.** `./scripts/check.sh` in the clone: **exit 0**, all five gates -- fmt, clippy `--all-targets`,
rustdoc `-D warnings`, examples built, tests (**64 unit + 60 end-to-end**), systemd unit verified.

**Commit identifiers:** none (no change needed).

**Note.** Release-mode gates were verified separately earlier; CI runs the debug suite, which is what
this clone exercised.

---

## 2026-11-14 — enforce the bar in CI

**Repository:** `oag-listend`, commits `a9688d6` and `9c8882e` on `master`; records in
`open-agent-graph`.

**The gap.** `./scripts/check.sh` held the whole bar, but nothing required anyone to run it: a push
could land without fmt, clippy, rustdoc-with-warnings-denied, the examples build, the suite, or the
systemd unit check ever executing. Verification was a matter of discipline rather than enforcement.

**What changed.** New `.github/workflows/check.yml`: on push to `master`, pull request, and manual
dispatch, install Rust minimal via rustup and run `./scripts/check.sh`. It deliberately invokes the
script instead of restating its steps, so what an operator runs and what CI enforces cannot drift.
README's Build section notes the workflow. No third-party actions beyond `actions/checkout@v4`;
rustup is fetched over TLS with certificate validation.

**Verification performed here.** Workflow YAML parsed and its job/step structure inspected;
`sh -n scripts/check.sh` re-verified; no behaviour change (documentation plus one workflow file).
`./scripts/check.sh` remains green locally: 64 unit + 60 end-to-end.

**Commit identifiers:** `oag-listend` `a9688d6`, `9c8882e`; this entry's commit pending.

---

## 2026-11-14 — binary-level tests in release

**Repository:** `oag-listend`, no code change.

**Why.** The shipped-client tests (all transports, IPv6 UDP, both binaries' CLIs, `--check`, SIGTERM)
landed after the last release-mode run, so they had only been exercised in debug.

**Result.** `cargo test -p oag-listend --release --test binary`: **9 passed, 0 failed**, with
`--examples` built in release first.

**Commit identifiers:** none (no change needed).

---

## 2026-11-14 — full bar from `cargo clean`

**Repository:** `oag-listend`, no code change.

**Why.** Several defects this run were only found by rebuilding explicitly rather than trusting an
existing `target/`. This is the strongest form of that check: delete all build output and run the
whole advertised bar.

**Observed.** `cargo clean` removed 8,282 files (1.7 GiB). `./scripts/check.sh` then ran end to end:
fmt, clippy `--all-targets`, rustdoc with warnings denied, examples built, the suite (**64 unit +
60 end-to-end**), and `systemd-analyze verify` on the unit -- **exit 0, `all gates passed`**, with
the working tree clean afterwards.

**Commit identifiers:** none (no change needed).

**Standing note for whoever picks this up.** Run `./scripts/check.sh` after `cargo clean` before
claiming the bar is green; incremental runs reuse artefacts and have twice masked real breakage in
this repository's history.

---

## 2026-11-14 — shipped-client tests must work from a clean tree

**Repository:** `oag-listend`, commit `8c09523` on `master`; records in `open-agent-graph`.

**The defect.** Deleting `target/debug/examples/push_pull` and running
`cargo test -p oag-listend --test binary` failed three tests: **`cargo test` does not build examples**,
so the shipped-client tests could not run. Consequently `./scripts/check.sh` -- advertised as "the
whole bar in one command" -- failed for anyone after `cargo clean` or on a fresh clone.

**What changed.**
- `scripts/check.sh` now runs `cargo build -p oag-listend --examples` before the suite, with a comment
  stating why.
- The `push_pull()` helper builds the example itself if it is missing, so plain
  `cargo test -p oag-listend` (the directive's canonical command) is self-sufficient too. A nested
  cargo invocation here is safe because tests run after the outer build has released its lock.

**Verified.** Removed the whole `target/debug/examples` directory, then ran `./scripts/check.sh` end to
end: exit 0 across fmt, clippy, rustdoc, examples, tests, and the systemd unit. Also re-verified
`cargo test -p oag-listend --test binary` on the same clean state: 9 passed.

**Tests executed.** `cargo test -p oag-listend`: **64 unit + 60 end-to-end, 0 failed**.

**Commit identifiers:** `oag-listend` `8c09523`; this entry's commit pending.

---

## 2026-11-14 — cover every client transport; fail loudly when the daemon dies early

**Repository:** `oag-listend`, commit `ebf4715` on `master`; records in `open-agent-graph`.

**Coverage gap.** The shipped client had automated coverage only for `--help`. New test drives the
real binary over native TCP, UDP and Unix against one live daemon, asserting `verified=true`,
`same-hash`, and a typed `Rejected` on each.

**Defect found by writing it -- in the harness, not the daemon.** `announce()` broke out of its read
loop on EOF and returned whatever it had, so a daemon that died before announcing looked like success;
the test then spent five seconds polling for a socket. It now panics with the partial output.

**Verified both ways.** Removing the runtime-directory creation (so the Unix bind fails) now fails
immediately: "the daemon closed its output before announcing; got oag-listend udp on 127.0.0.1:…".
Restoring it passes.

**Tests executed.** `cargo test -p oag-listend`: **64 unit + 60 end-to-end, 0 failed**;
`./scripts/check.sh` green across all five gates.

**Commit identifiers:** `oag-listend` `ebf4715`; this entry's commit pending.

---

## 2026-11-14 — test that `push_pull --udp` reaches an IPv6 daemon

**Repository:** `oag-listend`, commit `7c2e6e3` on `master`; records in `open-agent-graph`.

**Why.** The IPv6 client fix was verified only by hand; a behaviour change in a shipped artefact
deserves an automated guard.

**What changed.** New binary-level test: start the daemon with `--udp [::1]:<port>`, run the shipped
client against it, assert `verified=true` and a typed `Rejected`. It skips cleanly where the host has
no IPv6 loopback rather than failing.

**A trap worth recording.** My first discrimination probe appeared to pass with the bug in place. Cause:
the test executes `target/debug/examples/push_pull`, which `cargo test --test binary` does **not**
rebuild, so it ran a stale example. Rebuilding the example first made the probe fail as expected.
Same class of mistake as the earlier release-binary staleness -- verify artefacts by content, and
rebuild explicitly before trusting a probe.

**Tests executed.** `cargo test -p oag-listend`: **64 unit + 59 end-to-end, 0 failed**;
`./scripts/check.sh` green. Probe verified both ways after rebuilding.

**Commit identifiers:** `oag-listend` `7c2e6e3`; this entry's commit pending.

---

## 2026-11-14 — let `push_pull --udp` reach an IPv6 daemon

**Repository:** `oag-listend`, commit `b17dc28` on `master`; records in `open-agent-graph`.

**The defect.** While checking address-family coverage I found the shipped UDP client could not
reach an IPv6 daemon at all: it bound its local socket to `127.0.0.1` unconditionally, so sending to
`[::1]:port` failed with `Address family not supported by protocol (os error 97)` -- an error that
points away from the actual cause. An operator on an IPv6-only network would have had no working
client.

**What changed.** The target is parsed first; an IPv6 peer gets a `[::]:0` bind, everything else keeps
the previous IPv4 behaviour.

**Verified with release binaries on both families:**

```
v6: fetched noise: 2888acf2ca6200eb same-hash 1164 bytes verified=true
v4: identical
```

**Tests executed.** `cargo test -p oag-listend`: 64 unit + 58 end-to-end, 0 failed;
`./scripts/check.sh` green.

**Commit identifiers:** `oag-listend` `b17dc28`; this entry's commit pending.

---

## 2026-11-14 — verify IPv6 end to end

**Repository:** `oag-listend`, no code change.

**Why.** The founding statement says "any present and future technology", and an IPv6-only network is a
real deployment case -- but nothing had ever bound or connected over IPv6.

**Observed.** With `--listen [::1]:7951 --http [::1]:7952`: both carriers report their bracketed
address; native publish/fetch returns `same-hash ... verified=true`; HTML is typed-rejected; HTTP
`GET /` returns 200; SIGTERM exits 0.

**Note.** The IPFS client sends `Host: [::1]:port`, which is the correct HTTP/1.1 form for an IPv6
authority -- worth recording because a bare `::1:port` would be malformed, and this path has not been
exercised against a real node.

**Commit identifiers:** none (no change needed). `./scripts/check.sh` green: 64 unit + 58 end-to-end.

---

## 2026-11-14 — full five-surface acceptance on current HEAD

**Repository:** `oag-listend`, no code change.

**Why.** Several behaviour changes landed after the previous full pass (persisted CID index, off-lock
sidecar write), so the mission's own acceptance sentence was re-run end to end on release binaries
against a stub Kubo node.

**Observed.**
- Startup: four generated items, `held 48816`.
- Native TCP, UDP and Unix each: publish/fetch `same-hash ... verified=true`, HTML typed-rejected.
- HTTP: `GET /audio/<hash>` 200, 8,044 bytes byte-identical; `Range: 0-100` -> 206.
- IPFS: `GET /ipfs/<cid>` 200, 8,044 bytes **byte-identical** to what was posted.
- Shutdown lists one CID per distinct item.

**Bonus confirmation.** All three carriers publish the *same* tone and noise, and all three report
the same CID (`bafyrelay6`) -- proving the persisted-idempotence fix works across carriers within a
run, not just across restarts. Only four distinct items reached the node, not eight.

**Commit identifiers:** none (no code change). `./scripts/check.sh` green: 64 unit + 58 end-to-end.

---

## 2026-11-14 — a store directory full of stray files

**Repository:** `oag-listend`, no code change.

**Why.** Operators put things in directories. The loader promises only verified entries are held and
the rest are reported, but that had never been exercised against a realistically cluttered store.

**Observed.** `--check` over a directory containing misnamed audio, a README, an empty file, a
hash-named non-audio file, an orphan `.cid`, a directory named like a hash, and a subdirectory:
`loaded 0 skipped 4 evicted 0`, exit 0. Nothing trusted, nothing fatal, and every stray accounted
for -- files with extensions are skipped as sidecars, non-audio names fail the hash check, and
non-regular entries are refused without being read.

**Commit identifiers:** none (no change needed). `./scripts/check.sh` green: 64 unit + 58 end-to-end.

---

## 2026-11-14 — verify the documented defaults with no arguments

**Repository:** `oag-listend`, no code change.

**Why.** SPEC and README state the default bind (`0.0.0.0:7100`), the default store bound (1 GiB), and
that the seed comes from the clock and process id. All three had only been read, never run.

**Observed.**
```
oag-listend http on 127.0.0.1:7932
oag-listend listening on 0.0.0.0:7100
  note: 0.0.0.0:7100 is not a loopback address -- this serves audio to that interface
  seed 1801033544959692469  max-bytes 1073741824  held 0
```
`GET /` reported `protocol 1`, `items: 0`, `bytes: 0 / 1073741824` -- matching SPEC's description of
that route exactly. SIGTERM exited 0.

(Note: with no `--http`, port 7100 speaks the frame protocol only, so the usage line is not available
over HTTP in that configuration.)

**Commit identifiers:** none (no change needed). `./scripts/check.sh` green: 64 unit + 58 end-to-end.

---

## 2026-11-14 — run the README command block verbatim

**Repository:** `oag-listend`, no code change.

**Why.** The README's examples had been checked piecemeal; this runs them all in order, from the
README's own command line, against a live daemon.

**Result -- all nine behave as their comments promise.** POST returns a 64-char hash; `GET
/audio/<hash>` -> 200; `GET /` -> `oag-listend`; `GET /audio` lists sorted hashes;
`GET /audio?offset=2&limit=2` reports `X-Total-Count: 5`; `HEAD` -> 200 with headers only; `Range:
0-1023` -> 206; `If-None-Match` -> 304; `/ping` -> 44,844 bytes.

**Commit identifiers:** none (no change needed). `./scripts/check.sh` green: 64 unit + 58 end-to-end.

---

## 2026-11-14 — carrier-count language check

**Repositories:** `oag-listend` and `open-agent-graph`, no code change.

**Why.** "Four carriers" vs "five ways to reach the audio" could be drift or a deliberate distinction; checked rather than assumed.

**Result -- consistent, nothing changed.** TCP, HTTP, UDP and the Unix socket are the four *carriers*;
IPFS is framed throughout as a mirror on top of them (SPEC's status line: "Four carriers are live … and
`--ipfs-api` additionally mirrors accepted audio to IPFS"; README: "Five ways to reach the audio"). The
only other hits are dated historical entries in PROJECT_STATE ("47 unit + 21 end-to-end"), which are
records of their time, not current claims.

**Commit identifiers:** none. `./scripts/check.sh` green across all five gates.

---

## 2026-11-14 — verify the documented CLI surface against the parsers

**Repository:** `oag-listend`, no code change.

**Why.** A flag documented but not accepted (or accepted but never documented) is an operator-facing
lie, and the docs were written piecemeal. Checked mechanically rather than by reading.

**Result.** All thirteen flags the two binaries accept -- `--addr`, `--check`, `--dir`, `--help`,
`--http`, `--ipfs-api`, `--listen`, `--max-bytes`, `--no-generate`, `--seed`, `--udp`, `--unix`,
`--version` -- appear somewhere in README or SPEC, and no flag appears in the docs that either binary
does not accept. Both directions clean.

**Commit identifiers:** none (no change needed). `./scripts/check.sh` green: 64 unit + 58 end-to-end.

---

## 2026-11-14 — documentation reconciled against the code

**Repositories:** `oag-listend` and `open-agent-graph`; no code change.

**Method.** Three independent mechanical checks rather than recollection:

1. Section-by-section read of README and SPEC against the router, store, and session loops.
2. A sweep of every test-count claim across all markdown.
3. An identifier-existence check: every backticked name in README/SPEC must appear in the crate.

**Drift found and fixed across this reconciliation:** PROJECT_STATE omitted the fifth repository; SPEC's
status paragraph was off by 8 unit and 25 end-to-end tests and predated the IPFS surface, fuzzing,
release verification, the `--unix` client, and `check.sh`; SPEC's HTTP section omitted
`GET /ipfs/{cid}`; SPEC's native section omitted `--unix`; the persistence layout omitted
`<hash>.cid` and its validation; and the IPFS coverage claim named only `/api/v0/add`.

**Result now.** Identifier check clean -- the only names absent from the crate are `ProxiedTransport`
and `Transport`, which live in oag-imagination and are cited deliberately. Count claims agree at
64 unit + 58 end-to-end. All five repos clean and pushed; `./scripts/check.sh` green.

**Commit identifiers:** none this entry (previous entries carry `97d99ee`, `0cb16ba`, `dc5c00c`,
`311d268`).

---

## 2026-11-14 — finish reconciling SPEC with the code

**Repository:** `oag-listend`, commits `97d99ee` and `0cb16ba` on `master`; records in
`open-agent-graph`.

**Method.** Continued the directive's own step 4 -- read SPEC section by section against the router,
the store, and the session loops -- rather than trusting memory of what was built.

**Drift found and corrected.**
- The native carrier section omitted `push_pull --unix` from its examples, and still said publishes go
  through `AudioStore::accept` when every carrier now goes through `accept_and_mirror`.
- The persistence section described the content-addressed directory without `<hash>.cid`, did not say
  a `.cid` sidecar must be a plausible identifier within 128 bytes, and did not note that sidecars do
  not count toward the entry cap or are removed on eviction.
- The IPFS client's test coverage was described as "a stub that mimics Kubo's add endpoint"; both
  halves are stub-tested.

**Tests executed.** No behaviour change; `./scripts/check.sh` green (64 unit + 58 end-to-end).

**Commit identifiers:** `oag-listend` `97d99ee`, `0cb16ba`; this entry's commit pending.

---

## 2026-11-14 — complete SPEC's HTTP route list

**Repository:** `oag-listend`, commit `dc5c00c` on `master`; records in `open-agent-graph`.

**The gap.** Reading the HTTP carrier section against the router, as the directive's step 4 suggests:
it listed every route **except `GET /ipfs/{cid}`** -- the read half of the IPFS surface -- and still
said `POST /audio` goes through `AudioStore::accept` when it now goes through `accept_and_mirror`,
like every other carrier.

**What changed.** One sentence adding the CID route (local-first for held audio, otherwise fetched from
Kubo, under the same sniff rule, with `X-Content-Hash` and immutable caching), and a correction to
name the real entry point.

**Tests executed.** No behaviour change; `./scripts/check.sh` green (64 unit + 58 end-to-end).

**Commit identifiers:** `oag-listend` `dc5c00c`; this entry's commit pending.

---

## 2026-11-14 — bring SPEC's status paragraph up to date

**Repository:** `oag-listend`, commit `311d268` on `master`; records in `open-agent-graph`.

**The defect.** Following the directive's own start-up steps, I read SPEC's status line for the first
time in a while: it claimed **56 unit + 33 end-to-end** (actual: **64 + 58**) and its coverage list
predated the entire IPFS surface, the hostile-input fuzzing, the release verification, the
`--unix` client, and `scripts/check.sh`. A reader would have learned almost nothing true about what is
actually proven.

**What changed.** Rewritten against what the suite covers today: the back-and-forth, eviction and its
CID sidecar, the frame cap and lying lengths, listing pagination, all six signatures both ways,
per-format HTTP types, every refusal path, shared store, persistence, concurrency including the hung
mirror, the two-way IPFS mirror with its idempotence guarantees, the musical ping on every carrier,
the robustness fuzzing, and both shipped binaries' CLIs. Also states that `./scripts/check.sh` holds
the whole bar.

**Tests executed.** `cargo test -p oag-listend`: 64 unit + 58 end-to-end, 0 failed;
`./scripts/check.sh` green.

**Commit identifiers:** `oag-listend` `311d268`; this entry's commit pending.

---

## 2026-11-14 — write the CID sidecar off the store lock

**Repository:** `oag-listend`, commit `afde29d` on `master`; records in `open-agent-graph`.

**The defect.** Reading back the persisted-CID code I had just written found a blocking
`std::fs::write` executed while the store mutex was held -- precisely the pattern this crate removes
everywhere else (`persist` uses `tokio::fs` for exactly this reason). A slow disk would have stalled
every other client behind one 180-byte sidecar write.

**What changed.** The directory is read under a short lock, the sidecar written asynchronously with
no lock held via a `persist_cid` helper, then the lock retaken for the in-memory record. The startup
mirror pass uses the same helper.

**Tests executed.** `cargo test -p oag-listend`: **64 unit + 58 end-to-end, 0 failed**;
`./scripts/check.sh` green.

**Commit identifiers:** `oag-listend` `afde29d`; this entry's commit pending.

---

## 2026-11-14 — test that eviction removes the recorded CID sidecar

**Repository:** `oag-listend`, commit `b315a10` on `master`; records in `open-agent-graph`.

**The gap.** The persisted-CID work claimed eviction takes the `.cid` with the audio, but nothing
tested it. A sidecar left behind would be actively wrong: a later restart would load a CID for audio
that is no longer held and skip re-adding it, so an evicted item could silently vanish from IPFS.

**What changed.** New `eviction_removes_the_recorded_cid`: a store bounded to one sound, publish two,
assert the evicted audio *and* its `.cid` are gone while the survivor keeps its own.

**Discriminating.** Leaving the sidecar behind (removing the unlink) makes the test fail; restoring
it passes.

**Tests executed.** `cargo test -p oag-listend`: **64 unit + 58 end-to-end, 0 failed**;
`./scripts/check.sh` green.

**Commit identifiers:** `oag-listend` `b315a10`; this entry's commit pending.

---

## 2026-11-14 — persist the CID index so restarts stop re-adding

**Repository:** `oag-listend`, commit `b7715d6` on `master`; records in `open-agent-graph`.

**How it was found.** Reading my own claim against reality: the README said "a repeated publish, and
every restart, adds what is already recorded exactly once", but idempotence lived only in memory.
Two runs against a counting stub showed the same two content hashes under four different CIDs -- run
2 re-uploaded everything.

**What changed.**
- The CID recorded for a content hash is written beside the audio as `<hash>.cid`, validated on read
  (small, plausible identifier), and reloaded when the directory loads, populating both index maps.
- Eviction removes the sidecar along with the audio and media-type sidecars.
- `mirror_existing` now returns the number of **offers made**, not items considered, so the
  operator's `offered N of M stored item(s)` line cannot overstate work done. SPEC states the
  persistence and that a lost sidecar costs one redundant add, never audio.

**Tests executed.** `cargo test -p oag-listend`: **64 unit + 57 end-to-end, 0 failed**;
`./scripts/check.sh` green. New `a_restart_does_not_readd_what_the_store_recorded` proves it two ways
-- no outcome logged and no request reaching the node -- because a redundant add could fail to connect
and still be an attempt. Verified discriminating by ignoring the persisted CID: fails without it,
passes with it.

**Commit identifiers:** `oag-listend` `b7715d6`; this entry's commit pending.

---

## 2026-11-14 — inspection only; no further increment available

**Repositories:** all five, no code change.

**Checked.** oag-listend HEAD `840a946`; all five working trees clean and pushed on their correct
branches; `./scripts/check.sh --release` green (fmt, clippy, rustdoc `-D warnings`, 120 tests,
systemd unit verified).

**Deliberately not done.** No new feature or refactor was started, because the remaining candidates
all need something outside this repository:

- Reticulum -- physical interface plus `oag-storage`.
- Real-Kubo testing of both IPFS halves -- a running node.
- Native-protocol read by CID -- a sixth frame operation, excluded by step 2 of the directive.

Starting any of them anyway would be untested carrier code, which this run has consistently refused.

**Commit identifiers:** none.

---

## 2026-11-14 — verify the non-loopback warning and serving on a real interface

**Repository:** `oag-listend`, no code change.

**Why.** SPEC promises "when a carrier binds a non-loopback address the daemon says so" and the systemd unit binds `0.0.0.0`; both had only ever been read, never run.

**Observed.** Binding `0.0.0.0`, the note is printed once per non-loopback carrier (native, HTTP, UDP). Serving over the host's actual address (`10.0.181.6`) rather than loopback: HTTP `GET /` -> 200, `POST /audio` -> 201, native publish/fetch `verified=true`, HTML refused, SIGTERM exit 0.

**Commit identifiers:** none (no code change).

---

## 2026-11-14 — verify refused startup generation through the binary

**Repository:** `oag-listend`, no code change.

**Why.** SPEC promises "a generated sound the store refuses is printed as a rejection rather than
dropped", which was covered only by a unit test of `publish_generated`. This exercises it through the
shipped binary.

**Observed with `--max-bytes 100`** (every generated sound exceeds the bound):

```
generated sound rejected: audio is 1324 bytes, larger than the 100 byte store bound
generated sound rejected: audio is 1484 bytes, ...
generated sound rejected: audio is 844 bytes, ...
generated sound rejected: audio is 44844 bytes, ...
oag-listend listening on 127.0.0.1:7895   held 0
```

All four refusals reported with their sizes; the daemon still starts and serves (unknown hash -> 404);
new audio is refused 413 with nothing stored (`items: 0`); SIGTERM exits 0.

**Commit identifiers:** none (no code change).

---

## 2026-11-14 — sweep for leftovers and unwind paths in the crate

**Repository:** `oag-listend`, no code change.

**Checked.**
- No unfinished markers anywhere: no `TODO`, `FIXME`, `XXX`, `HACK`, `unimplemented!`, or leftover
  probe text in `crates/`, `scripts/`, or the markdown.
- Zero `unwrap()` / `expect()` in library code outside `#[cfg(test)]`.
- Every `panic!` is inside a test module, so no request path can unwind into a panic -- the
  availability property the robustness fuzz was built to protect.

**Result:** clean on all three; nothing to change.

**Commit identifiers:** none (no code change). `./scripts/check.sh --release` green: 120 tests.

---

## 2026-11-14 — full gate run on current HEAD (closing verification)

**Repositories:** all five, no code change.

**Result.** `./scripts/check.sh --release` exit 0: fmt, clippy `--all-targets`, rustdoc with warnings
denied, **120 tests in release**, and `systemd-analyze verify` on the shipped unit.

All five checkouts clean and pushed on their correct branches (`main`, `main`, `master`, `master`,
`master`).

**What is still not serving.** Nothing in oag-listend. The three items that remain need resources
this machine does not have or a protocol decision from the operator:

- Reticulum -- needs a physical interface and `oag-storage`.
- Real-Kubo testing of both IPFS halves -- needs a running node; everything so far is tested against
  stubs and says so in SPEC.
- A native-protocol read by CID -- would be a sixth frame operation, which SPEC deliberately excludes.

**Commit identifiers:** none (no code change).

---

## 2026-11-14 — document the non-pruning files in `scripts/`

**Repository:** `oag-listend`, commit `840a946` on `master`; records in `open-agent-graph`.

**The gap.** `scripts/README.md` is titled around pruning and documented only the three pruner
scripts, while `check.sh` and `oag-listend.service` sit in the same directory undocumented -- an
operator listing the directory sees files nothing explains.

**What changed.** A short "Other files in this directory" section covering both, and stating why they
are *not* copied to the other four repositories (there is no audio daemon there to check), so the
"copy a change to the other four" rule is not misread as covering them.

**Tests executed.** Documentation only; `./scripts/check.sh` remains green.

**Commit identifiers:** `oag-listend` `840a946`; this entry's commit pending.

---

## 2026-11-14 — hunt for flakiness in the timing-sensitive suites

**Repository:** `oag-listend`, no code change.

**Why.** The concurrency, robustness, binary and IPFS tests are timing-sensitive by design -- session
cap waits, a stalled reader against an 8 MiB item, a hung mirror, a header flood -- and each had only
ever been observed once per change. A guard that passes once and fails intermittently is not a
guard.

**Method.** Eight consecutive runs of `--test concurrency --test robustness --test binary
--test ipfs_mirror` (25 tests each).

**Result.** 8/8 clean, 200 executions, zero failures, stable pass count every run.

**Commit identifiers:** none (no change needed).

---

## 2026-11-14 — refresh PROJECT_STATE's bar and gates

**Repository:** `open-agent-graph`, commit `97c0e13` on `main`; records in the same file.

**The drift.** The measured table said 115 tests (now 120), and the capabilities section did not
mention `scripts/check.sh` or the panic-safety fuzzing added since.

**What changed.** Table updated to 120; capabilities now state that every gate is one command
(`scripts/check.sh [--release]` running fmt, clippy, rustdoc `-D warnings`, the suite, and
`systemd-analyze verify`) and that hostile input is fuzzed across the codec and all four carriers with
liveness asserted after. Next-priorities bar updated to 64 unit + 56 end-to-end.

**Tests executed.** No code change; `./scripts/check.sh` remains green.

**Commit identifiers:** `open-agent-graph` `97c0e13`; this entry's commit pending.

---

## 2026-11-14 — document mirror idempotence in the README

**Repository:** `oag-listend`, commit `802eae2` on `master`; records in `open-agent-graph`.

**The gap.** The idempotence fix was documented in SPEC but not in the README, and one sentence about
the read route said the fetched object "applies the same audio rule" -- as though the IPFS node
applied it rather than this daemon.

**What changed.** README now states that mirroring is idempotent (a repeated publish and every restart
add what is already recorded exactly once, rather than rewriting the library) and that *what comes
back* from a CID is held to the same audio rule.

**Tests executed.** Documentation only; `./scripts/check.sh` remains green (64 unit + 56 end-to-end).

**Commit identifiers:** `oag-listend` `802eae2`; this entry's commit pending.

---

## 2026-11-14 — verify idempotent mirroring on the release binary, and release gates

**Repository:** `oag-listend`, no code change.

**Why.** The idempotence fix was unit-tested; this confirms it through the operator path on the
artifact that ships, and re-runs the whole gate set in release, which had not happened since the
fuzzing and mirror work landed.

**Observed.**
- Three identical HTTP publishes against a daemon with `--ipfs-api` produced exactly one mirror:
  ```
  oag-listend shutting down
    ipfs mirror: 1 mirrored, 0 failed
      5e0e16cd1ccf54c9 -> bafyrelay1
  ```
  Before the fix each publish would have added again.
- `./scripts/check.sh --release`: exit 0 across fmt, clippy, rustdoc `-D warnings`, the full suite in
  release, and the systemd unit.

**Commit identifiers:** none (no code change).

---

## 2026-11-14 — do not re-add audio that is already on IPFS

**Repository:** `oag-listend`, commit `e357b82` on `master`; records in `open-agent-graph`.

**How it was found.** Reading the acceptance output rather than just its pass/fail: the same bytes
came back under three different CIDs, one per carrier. `accept` returns early for a duplicate content
hash -- but `accept_and_mirror` mirrored *unconditionally* afterwards, so every repeated publish, and
every restart via `mirror_existing` (same path), rewrote the whole library to IPFS.

**What changed.** Both mirror paths now skip a content hash that already has a recorded CID. Mirroring
is idempotent; a restart no longer re-uploads what it already holds. SPEC states this.

**The test nearly passed vacuously twice**, which is the more useful lesson:
1. Its stub accepted exactly one connection, so a redundant add was answered by nothing and never
   counted. Raising the accept count exposed a `Connection refused` instead.
2. The robust evidence is the **mirror log**: a redundant attempt is logged even if the network call
   fails. The assertion is now `mirror_log().len() == 1`.

Verified discriminating: removing the guard makes the test fail; restoring it passes.

**Tests executed.** `cargo test -p oag-listend`: **64 unit + 56 end-to-end, 0 failed**;
`./scripts/check.sh` green across fmt, clippy, rustdoc, tests, and the systemd unit.

**Commit identifiers:** `oag-listend` `e357b82`; this entry's commit pending.

---

## 2026-11-14 — confirm the tooling invariant after adding a script

**Repositories:** all five; records in `open-agent-graph`.

**Why.** The directive states that `prune-target.py`, `rustc-wrapper.sh`, and
`test_prune_target.py` are byte-identical across all five repositories, and that a change to one is
copied to the other four. This invocation added `scripts/check.sh` to oag-listend, so the invariant
was re-measured rather than assumed.

**Result -- holds.** One hash per script across all five checkouts:
`prune-target.py` `e9235a0c9fb268af`, `rustc-wrapper.sh` `db7326a7f865c7aa`,
`test_prune_target.py` `e456fa76baced4d3`.

**Scope note.** `check.sh` is deliberately *not* in that set: it runs gates for the audio daemon, which
only exists in oag-listend. Copying it elsewhere would fail.

**Commit identifiers:** none (no change needed).

---

## 2026-11-14 — include the systemd unit in `scripts/check.sh`

**Repository:** `oag-listend`, commit `2480f5f` on `master`; records in `open-agent-graph`.

**The gap.** The script said "every quality gate", but it omitted the one gate that is not a cargo
command: `systemd-analyze verify` on the shipped unit.

**What changed.** It now verifies the unit against a temporary copy whose `ExecStart` names a binary
this checkout has actually built (release preferred, then debug), skipping with a message when the
tool or the binary is absent rather than failing spuriously.

**Two findings while wiring it.**
- `systemd-analyze` requires the file be named `*.service`; a bare `mktemp` name fails with
  `Failed to prepare filename ...: Invalid argument`.
- The binary may not have been built at all, which would look identical to a broken unit.

**Verified.** `./scripts/check.sh` exits 0 with all five gates: fmt, clippy, rustdoc, tests, systemd
unit.

**Commit identifiers:** `oag-listend` `2480f5f`; this entry's commit pending.

---

## 2026-11-14 — `scripts/check.sh`: the whole bar in one command

**Repository:** `oag-listend`, commit `d44d880` on `master`; records in `open-agent-graph`.

**The gap.** The repository is now held to four gates -- `cargo fmt --check`, `cargo clippy
--all-targets`, `RUSTDOCFLAGS="-D warnings" cargo doc`, and the suite in debug *and* release -- but
they existed only as knowledge in prior entries. A future invocation had to remember all of them.

**What changed.** New executable `scripts/check.sh`: runs the gates in order, stops at the first
failure, prints `all gates passed`. `--release` adds the release suite; any other argument exits 2
with usage. README's Build section documents both forms.

**Bugs caught while writing it.** `set -u` plus an unset `RELEASE` aborted the script at the test
step after three gates had already run; initialised explicitly.

**Verified.** `./scripts/check.sh` exit 0, `./scripts/check.sh --release` exit 0 (release suite
included), `./scripts/check.sh --bogus` exit 2 with usage.

**Commit identifiers:** `oag-listend` `d44d880`; this entry's commit pending.

---

## 2026-11-14 — test an oversized body without `Expect`

**Repository:** `oag-listend`, commit `9233b84` on `master`; records in `open-agent-graph`.

**The gap.** The oversized-body refusal had coverage only for the `Expect: 100-continue` variant. A
plain `POST` declaring more than `MAX_BODY_BYTES` -- the more common case, since most clients do not
send `Expect` at all -- was exercised only by reading the code.

**What changed.** New test: send such a head, write a few bytes, half-close so the post-refusal drain
ends rather than waiting out its timeout, and assert **413** with the store still empty. It also
confirms the status chosen when the `too_large` flag was introduced: the client is never told to
commit an upload this carrier will reject.

**Tests executed.** `cargo test -p oag-listend`: **64 unit + 55 end-to-end, 0 failed**; clippy and
rustfmt clean.

**Commit identifiers:** `oag-listend` `9233b84`; this entry's commit pending.

---

## 2026-11-14 — fuzz the lying-length case

**Repository:** `oag-listend`, commit `b0021e0` on `master`; records in `open-agent-graph`.

**The gap.** The directive's sharpest bound is "never an unbounded read", and the new robustness
guard never fed the daemon a size it *declares* but does not send -- the exact shape a hostile or
broken peer uses to make a reader wait for bytes that never arrive.

**What changed.** The guard now sends short bodies under native declarations of 1, 100, 100,000 and
`MAX_FRAME_BYTES`, then half-closes, and asserts the session ends at the close within two seconds.
HTTP gets the same shape via an unsatisfied `Content-Length`, asserting it answers or closes rather
than staying open.

**Why the assertion is trustworthy.** Making `read_exact_bounded` ignore EOF -- i.e. pretending the
close never happened -- makes this test **fail at 2.1 s**; restoring the real code makes it pass.
Without that probe the loop would have asserted nothing at all.

**Tests executed.** `cargo test -p oag-listend`: **64 unit + 54 end-to-end, 0 failed**; clippy and
rustfmt clean.

**Commit identifiers:** `oag-listend` `b0021e0`; this entry's commit pending.

---

## 2026-11-14 — extend the panic-safety fuzz to UDP and the Unix socket

**Repository:** `oag-listend`, commit `aef6f73` on `master`; records in `open-agent-graph`.

**The gap.** The new guard covered the native and HTTP receive paths. UDP decodes datagrams through
its own function, and the Unix carrier accepts through its own path, so a panic specific to either
would have been missed.

**What changed.** Both carriers now take 100 validly-framed garbage bodies before the daemon must
still answer `Hello` and accept a publish. The test binds all four carriers around one store.

**Tests executed.** `cargo test -p oag-listend`: **64 unit + 54 end-to-end, 0 failed**; clippy and
rustfmt clean.

**Commit identifiers:** `oag-listend` `aef6f73`; this entry's commit pending.

---

## 2026-11-14 — fuzz the codec and carriers for panic-safety

**Repository:** `oag-listend`, commit `ba7d58f` on `master`; records in `open-agent-graph`.

**Why.** Every guard so far checks a *property*; none checked that hostile input cannot take the
daemon down. A panic in a request path is an availability bug, and the parsers were only ever fed
well-formed or deliberately-malformed-but-designed inputs.

**What changed.** New `tests/robustness.rs`, fully deterministic (fixed-seed LCG, so any failure
reproduces):
- `random_bodies_never_panic_the_codec`: 5,000 random bodies through `decode_frame`,
  `decode_request`, `decode_response`, and `AudioFormat::sniff`, plus declared lengths at the cap
  boundary (`MAX_FRAME_BYTES`), just past it, and `u32::MAX`.
- `garbage_on_the_wire_leaves_the_daemon_serving`: 100 validly-framed random bodies to the native
  carrier and 100 terminated random heads to HTTP -- sized so nothing blocks -- then asserts the
  daemon still answers `Hello` and accepts a publish with a verifying hash.

**Result.** Both green on the first run: no panics, no unexpected error variants, and the daemon
served normally immediately after 200 hostile connections.

**Tests executed.** `cargo test -p oag-listend`: **64 unit + 54 end-to-end, 0 failed**; clippy and
rustfmt clean.

**Commit identifiers:** `oag-listend` `ba7d58f`; this entry's commit pending.

---

## 2026-11-14 — put `#![forbid(unsafe_code)]` on the two files that lacked it

**Repository:** `oag-listend`, commit `6d87c5b` on `master`; records in `open-agent-graph`.

**The gap.** The directive requires `#![forbid(unsafe_code)]` unless justified. Every library module
had it; **`main.rs` and `examples/push_pull.rs` did not**, so the constraint was satisfied only by
convention rather than stated everywhere.

**What changed.** Both files now carry the attribute. There is no `unsafe` anywhere in the crate, so
this adds a guarantee rather than changing behaviour.

**Tests executed.** `cargo test -p oag-listend`: **64 unit + 52 end-to-end, 0 failed**; clippy and
rustfmt clean.

**Commit identifiers:** `oag-listend` `6d87c5b`; this entry's commit pending.

---

## 2026-11-14 — verify licence compliance across all five repositories

**Repositories:** all five, by remote; records in `open-agent-graph`.

**Why.** The directive states the licence requirement explicitly ("GNU Affero General Public License,
version 3 or any later version ... Do not relicense"), so it should be checked rather than assumed.

**Result -- compliant, nothing changed.**
- All five `LICENSE` files carry the GNU AGPL **Version 3** text.
- All manifests declare `license = "AGPL-3.0-or-later"` (`license.workspace = true` in the crates).
- No conflicting headers anywhere in `crates/` or `scripts/`: no MIT, BSD, Apache-2.0, GPL-2, or SPDX
  identifiers in `oag-listend`.
- `README.md` and `SPEC.md` state the licence in the directive's own terms.

**Commit identifiers:** none (no change needed). `cargo test -p oag-listend` remains 64 unit +
52 end-to-end, 0 failed.

---

## 2026-11-14 — verify the systemd unit and document the IPFS option in it

**Repository:** `oag-listend`, commit `f673912` on `master`; records in `open-agent-graph`.

**Why.** The systemd unit is a shipped artefact that had only ever been read, never parsed, and it
never mentioned `--ipfs-api` -- so an operator reading the unit could not discover the mirror exists.

**What changed.**
- Added a commented line to `ExecStart` showing how to enable `--ipfs-api 127.0.0.1:5001` and what
  its failure behaviour is, pointing at SPEC's IPFS section.
- Ran `systemd-analyze verify`: **clean**, with no warnings on `User`/`Group`, `StateDirectory`,
  `RuntimeDirectory`, `ReadWritePaths`, or `ProtectSystem`. Its only complaint before was that
  `/usr/local/bin/oag-listend` does not exist yet, which is the documented install step; verifying a
  copy whose `ExecStart` points at the built binary removes that artefact of not being installed.

**Tests executed.** No code change; `cargo test -p oag-listend` remains 64 unit + 52 end-to-end,
0 failed.

**Commit identifiers:** `oag-listend` `f673912`; this entry's commit pending.

---

## 2026-11-14 — test the operator client's own CLI

**Repository:** `oag-listend`, commit `4368dbf` on `master`; records in `open-agent-graph`.

**The gap.** `push_pull` is a shipped artefact and its `--help` had already drifted once (the usage
line omitted `--version`/`--help`), but unlike the daemon binary it had **no automated coverage** --
only hand verification.

**What changed.** A test locates the built example relative to the test binary and asserts every option
is documented (`--addr`, `--udp`, `--unix`, `--seed`, `--version`, `--help`), that `--version`
succeeds, and that an unknown flag exits 2 rather than panicking. The same guard the daemon binary has.

**Tests executed.** `cargo test -p oag-listend`: **64 unit + 52 end-to-end, 0 failed**; clippy, rustdoc
and rustfmt clean.

**Commit identifiers:** `oag-listend` `4368dbf`; this entry's commit pending.

---

## 2026-11-14 — give `push_pull` a `--unix` option

**Repository:** `oag-listend`, commit `7bb4df7` on `master`; records in `open-agent-graph`.

**The gap.** The Unix carrier was the one surface with no operator client. TCP and UDP had `push_pull`,
HTTP any browser, but the socket could only be exercised by a hand-written client or a test -- an
operator had no shipped way to try it.

**What changed.** `push_pull --unix PATH` speaks the identical frame protocol over a filesystem socket,
with the same per-read timeouts as the network carriers. Its usage line and README updated.

**Verified with release binaries against a live daemon:**

```
fetched tone:  66790ab6a57e13ef same-hash 1004 bytes verified=true
fetched noise: e4fbf604f67408bd same-hash 1164 bytes verified=true
daemon exit=0, socket removed on shutdown
```

**Tests executed.** `cargo test -p oag-listend`: 64 unit + 51 end-to-end, 0 failed; clippy, rustdoc and
rustfmt clean.

**Commit identifiers:** `oag-listend` `7bb4df7`; this entry's commit pending.

---

## 2026-11-14 — have `push_pull` report each item's IPFS CID

**Repository:** `oag-listend`, commit `d15ece8` on `master`; records in `open-agent-graph`.

**The gap.** The shipped client is the "smallest honest demonstration" of audio back and forth, but
it discarded the CID the daemon now returns. An operator running against a mirroring daemon could
only learn CIDs from the daemon's shutdown report.

**What changed.** Each fetched item prints `ipfs=<cid>` when one is known; the example's own `--help`
says so. Verified with release binaries against a stub Kubo:

```
fetched tone:  f3cc52cb94cfb6dc same-hash 1484 bytes verified=true ipfs=bafyrelay1
fetched noise: 70ae79d80760f361 same-hash  844 bytes verified=true ipfs=bafyrelay2
```

**Tests executed.** `cargo test -p oag-listend`: 64 unit + 51 end-to-end, 0 failed; clippy and rustdoc
clean.

**Commit identifiers:** `oag-listend` `d15ece8`; this entry's commit pending.

---

## 2026-11-14 — rustdoc clean under denied warnings

**Repository:** `oag-listend`, commit `a16f3b4` on `master`; records in `open-agent-graph`.

**Why.** Third quality gate never run on this crate, after rustfmt and clippy. Documentation is the
part most likely to drift silently while code changes around it.

**Finding and fix.** One redundant explicit intra-doc link -- `[`accept_and_mirror`](crate::accept_and_mirror)`
-- where the label already resolves. Removed the target. `RUSTDOCFLAGS="-D warnings" cargo doc
-p oag-listend --no-deps` now finishes clean.

**Gates now green.** `cargo fmt --check`, `cargo clippy -p oag-listend --all-targets`,
`RUSTDOCFLAGS="-D warnings" cargo doc`, and `cargo test -p oag-listend` (64 unit + 51 end-to-end,
0 failed) -- in debug and, earlier, release.

**Commit identifiers:** `oag-listend` `a16f3b4`; this entry's commit pending.

---

## 2026-11-14 — clear every clippy lint in the crate

**Repository:** `oag-listend`, commit `c1a3da5` on `master`; records in `open-agent-graph`.

**Why.** The crate had never been through clippy. The first run found ten real lints; the count is
now **zero** across `--all-targets`.

**What changed.**
- `unix.rs` matched `Some(metadata)` against `metadata().ok()`; now a plain `Ok` pattern.
- `http.rs`'s route list had overindented continuation lines.
- `server.rs` passed `&vec![9u8; 100]` where a slice suffices.
- A test compared `.len() > 0` instead of `!is_empty()`.
- Two concurrency tasks re-bound `addr` inside an already-`move` closure.
- UDP's frame-bound test asserted two constants directly, which clippy flags and which proves
  nothing at runtime. Rewritten as the derived equality
  `MAX_FRAME_BYTES + 4 == MAX_DATAGRAM_BYTES` -- and my first attempt wrote it backwards, which the
  test caught immediately.

**Tests executed.** `cargo test -p oag-listend`: **64 unit + 51 end-to-end, 0 failed**;
`cargo clippy -p oag-listend --all-targets`: **0 warnings**.

**Commit identifiers:** `oag-listend` `c1a3da5`; this entry's commit pending.

---

## 2026-11-14 — record why the HTTP CONNECT tunnel needs no carrier here

**Repository:** `oag-listend`, commit `af50f45` on `master`; records in `open-agent-graph`.

**The question.** The directive names "whatever the imagination `Transport` seam already has".
That seam lists TCP, UDP, HTTP, an HTTP CONNECT tunnel, IPFS, and Reticulum. Four are implemented and
Reticulum is hardware-bound; the CONNECT tunnel had never been analysed.

**Finding.** `ProxiedTransport` is a *client* wrapper -- it connects to a proxy, asks for a tunnel,
then runs another carrier's exchange through it. oag-listend is a server: a client tunnelling to its
TCP listener needs no code on our side, because TCP is TCP. So there is nothing to build, only
something to stop re-investigating.

**What changed.** SPEC's Next now accounts for the whole seam: four implemented, Reticulum
hardware-bound, the CONNECT tunnel explained as a non-item, and the two real-Kubo verification items
folded into one.

**Tests executed.** No code change; `cargo test -p oag-listend`: 64 unit + 51 end-to-end, 0 failed.

**Commit identifiers:** `oag-listend` `af50f45`; this entry's commit pending.

---

## 2026-11-14 — run the whole suite in release mode

**Repository:** `oag-listend`, no code change.

**Why.** Several guards are timing-sensitive by design -- the session cap (700 ms of expected
silence), the stalled reader (a 1 KiB receive buffer against an 8 MiB item), the hung mirror, and the
header flood -- and until now they had only ever been run in debug. Operators run release, so any
optimisation-sensitive fragility would surface there first.

**Result.** `cargo test --release -p oag-listend`: **115 passed, 0 failed**, across all twelve test
targets, with the timing-sensitive tests comfortably inside their bounds.

**Commit identifiers:** none (no code change).

---

## 2026-11-14 — record the two-way IPFS surface in PROJECT_STATE

**Repository:** `open-agent-graph`, commit `284070a` on `main`; records in the same file.

**The defect.** The cross-project record predated the by-CID read route: its capabilities still
described IPFS as write-only, and its measured bar was 109 tests.

**What changed.** The oag-listend bullets now describe both halves -- best-effort lock-free writing,
and local-first reading with the sniff rule applied to whatever comes back -- plus verification and
caching headers, the chunked-reply refusal, the cleartext-downgrade refusal, and the standing caveat
that none of it is tested against a real Kubo node. Bar updated to 115.

**Tests executed.** No code change; `cargo test -p oag-listend`: 64 unit + 51 end-to-end, 0 failed.

**Commit identifiers:** `open-agent-graph` `284070a`; this entry's commit pending.

---

## 2026-11-14 — serve a held CID from the store instead of asking IPFS

**Repository:** `oag-listend`, commit `9fd6c01` on `master`; records in `open-agent-graph`.

**The gap.** The by-CID route always went out to Kubo, so an item we already held became unreachable
when the node was down. That is the wrong failure direction: local audio should not depend on a
remote service being up.

**What changed.** A reverse index (`cid -> content hash`) sits beside the existing forward one and is
dropped on eviction. `GET /ipfs/{cid}` serves a CID we hold straight from the store -- verifiable
here, no network -- and only sends an unfamiliar CID to `/api/v0/cat`. Both paths then face the same
sniff rule.

**Bug caught while writing the test.** The reverse index was declared and exposed but **never
populated**, so the local path silently never matched and the request fell through to the node. The
test's stub answers `add` but not `cat`, which is exactly what exposed it.

**Tests executed.** `cargo test -p oag-listend`: **64 unit + 51 end-to-end, 0 failed**. New test
publishes (mirroring to a stub whose cat 404s) and asserts the CID still serves 200 `audio/wav`.

**Commit identifiers:** `oag-listend` `9fd6c01`; this entry's commit pending.

---

## 2026-11-14 — verify the two-way IPFS surface with the release binaries

**Repository:** `oag-listend`, no code change.

**Why.** The by-CID read path had only been exercised through tests on the debug build. The shipped
artifact is what an operator runs, and this is the first feature whose value depends on two halves
working together against a node.

**Setup.** `cargo build --release -p oag-listend` (explicitly the bin, not `--examples`), plus a Python
stub implementing both `/api/v0/add` and `/api/v0/cat` and remembering what it was given.

**Observed.**
```
published hash: 5e0e16cd1ccf54c9...
CID from X-IPFS-CID: bafyrelay1
GET /ipfs/$CID -> 200 audio/wav 8044B
byte-identical to what was posted
  ETag: "bafyrelay1"
  Cache-Control: public, max-age=31536000, immutable
  X-Content-Hash: 5e0e16cd1ccf54c9...d78d85a
exit after SIGTERM: 0
```
The write half stored the audio, the publish answered with the CID, and the read half served the same
bytes back identified by that CID with a BLAKE3 the receiver can check.

**Harness note.** A first stub attempt over-read past the header terminator and lost the start of the
multipart body; fixed in the harness, not the daemon. Worth recording because a faulty stub looks
exactly like a broken carrier.

**Commit identifiers:** none (no code change).

---

## 2026-11-14 — make CID-served audio cacheable and verifiable

**Repository:** `oag-listend`, commit `2fb53b6` on `master`; records in `open-agent-graph`.

**The gap.** The by-CID read route served bytes with no caching header and, unlike every other route,
no way for the receiver to prove what it had got. A client holding a CID could not verify the audio it
received beyond trusting the daemon.

**What changed.** The route now answers with `Cache-Control: public, max-age=31536000, immutable`
(content at a CID cannot change), an `ETag` of the CID, and `X-Content-Hash` carrying the BLAKE3 of the
bytes actually sent -- the same verification guarantee the rest of the carrier gives.

**Tests executed.** `cargo test -p oag-listend`: **64 unit + 50 end-to-end, 0 failed**; the by-CID test
now asserts all three headers and checks `X-Content-Hash` against an independently computed BLAKE3.

**Commit identifiers:** `oag-listend` `2fb53b6`; this entry's commit pending.

---

## 2026-11-14 — refuse chunked IPFS replies instead of misreading them as audio

**Repository:** `oag-listend`, commit `5d6ef42` on `master`; records in `open-agent-graph`.

**The defect.** Auditing the by-CID read path before trusting it: Kubo answers larger `/api/v0/cat`
responses with `Transfer-Encoding: chunked`, and this client read the reply from the first blank line
onward as if it were the body -- so chunk framing would have been served (or sniffed) as audio,
silently corrupting it. A first patch attempt landed only in the add path, which the new test caught.

**What changed.** Both halves detect a chunked reply head and return a typed refusal. New unit test
feeds a real chunked response to `cat` and asserts the refusal rather than the framing bytes.

**Tests executed.** `cargo test -p oag-listend`: **64 unit + 50 end-to-end, 0 failed**; 0 warnings.

**Commit identifiers:** `oag-listend` `5d6ef42`; this entry's commit pending.

---

## 2026-11-14 — serve audio by CID over HTTP

**Repository:** `oag-listend`, commit `b452fc5` on `master`; records in `open-agent-graph`.

**The gap.** SPEC's last functional item. The mirror could *publish* and a client could learn a CID,
but nothing could read one back: a client holding only a CID still needed this daemon (or its own
IPFS node) to get bytes that were not in the local store.

**What changed.**
- `IpfsPublisher::cat(cid)` -- the read half, calling `/api/v0/cat` with the same per-read/write
  bounds as `add` and a response cap of `MAX_SERVABLE_BYTES` plus the reply-head allowance.
- `GET /ipfs/{cid}` on the HTTP carrier. It applies exactly the audio rule every other inbound path
  applies: non-audio is refused with **nothing echoed and nothing stored**, an implausible identifier
  is refused before any request is sent (so a crafted path cannot become query injection), an
  unreachable node answers `502`, and with no mirror configured the route does not exist (`404`).
- Documented honestly: the crate carries no multihash implementation, so it identifies what comes
  back by the CID the caller asked for rather than verifying the bytes hash to it.
- No new frame operation -- this is a carrier-level route, so the protocol stays at five operations.

**Tests executed.** `cargo test -p oag-listend`: **63 unit + 50 end-to-end, 0 failed**. New: a
served object carries the right content type and `ETag: "<cid>"`; a non-audio object is refused
without being echoed; the no-mirror/unreachable/bad-identifier cases give 404/502/502; and a bad CID
is refused with no network call at all.

**Commit identifiers:** `oag-listend` `b452fc5`; this entry's commit pending.

---

## 2026-11-14 — bring PROJECT_STATE's oag-listend record up to date

**Repository:** `open-agent-graph`, commit `6b3047c` on `main`; records in the same file.

**The defect.** The cross-project record had fallen behind the mission again: its capabilities
section still said **four carriers** and that **IPFS was deliberately not built** -- both false since
the mirror landed -- and its measured bar read 94 tests. The next-priorities entry opened by calling
both IPFS and Reticulum unbuilt.

**What changed.** Capabilities rewritten for five ways to reach the audio; the mirror described with
its real properties (best-effort, accept under the lock then send without it, stored library mirrored
on startup, CID on the wire and as `X-IPFS-CID`) and its real limitation (**untested against a real
Kubo node**, `https://` refused rather than downgraded). `--check` now described as not contacting
IPFS. Measured bar updated to 109 tests; next priorities renamed to what is actually open --
Reticulum, and testing IPFS against a real node.

**Tests executed.** No code change; `cargo test -p oag-listend`: 62 unit + 47 end-to-end, 0 failed.

**Commit identifiers:** `open-agent-graph` `6b3047c`; this entry's commit pending.

---

## 2026-11-14 — drop an unused IPFS accessor; flag-interaction matrix checked

**Repository:** `oag-listend`, commit `3a66325` on `master`; records in `open-agent-graph`.

**Cleanup.** Audited every public item added with the mirror for actual use. `is_mirroring()` had
zero callers; removed so the surface matches what the daemon and tests call. Everything else
(`mirror_log`, `mirror_summary`, `content_id`, `record_mirror_outcome`, `set_ipfs`,
`accept_and_mirror`, `STARTUP_MIRROR_BUDGET`, `MirrorOutcome`) is used.

**Flag interactions checked by hand**, since the `--check --ipfs-api` defect was exactly that kind:

| Combination | Behaviour |
|---|---|
| `--check --unix /nonexistent/...` | exits 0, touches nothing |
| `--check --no-generate` | store report only |
| `--check --max-bytes 0` | reports bound 0 / item bound 0, exit 0 |
| `--max-bytes 0` serving | POST -> 413 |
| `--ipfs-api --check` | fixed earlier this session; no node contacted |

**Tests executed.** `cargo test -p oag-listend`: 62 unit + 47 end-to-end, 0 failed; 0 warnings.

**Commit identifiers:** `oag-listend` `3a66325`; this entry's commit pending.

---

## 2026-11-14 — keep `--check` a store verification, not a mirror run

**Repository:** `oag-listend`, commit `bace3a3` on `master`; records in `open-agent-graph`.

**The defect.** Found by checking flag interactions rather than single features. The IPFS setup was
placed before the `--check` early return, so `--check --ipfs-api ADDR` would configure a publisher
and run `mirror_existing` over the whole library -- up to the 30 s budget -- print two extra lines,
and only then print the store report. That contradicts `--check`'s documented contract ("open the
store, print the load report, and exit") and would surprise any script expecting a quick answer.

**What changed.** The check now returns before any IPFS configuration; the mirror is attached only on
the serving path. New test: with an unreachable `--ipfs-api`, `--check` still succeeds fast, prints
`store ok`, and does not announce a mirror.

**Tests executed.** `cargo test -p oag-listend`: **62 unit + 47 end-to-end, 0 failed**; 0 warnings.

**Commit identifiers:** `oag-listend` `bace3a3`; this entry's commit pending.

---

## 2026-11-14 — verify the IPFS path with the release binary; a stale-artifact trap

**Repository:** `oag-listend`, no code change.

**The trap.** The first attempt to verify the shipped artifact failed with `unknown argument
--ipfs-api` -- and correctly so: `target/release/oag-listend` was timestamped 14:09, hours old, while
`target/debug` was current. Cause found by experiment: `cargo build --release -p oag-listend
--examples` builds the *example* targets and their library dependency, **not** the binary, so it
never refreshed the daemon. `cargo build --release -p oag-listend` does (992,352 -> 1,035,544 bytes,
flag present). A bare `touch` also does not trigger a rebuild, while a real content change does --
so this is normal cargo behaviour, not a wrapper fault. Lesson: when verifying a shipped artifact,
build that target explicitly and check the result's content rather than trusting that a build ran.

**Verified with the fresh release binary** (`push_pull` also release-built), against a Python stub
mimicking `/api/v0/add`:
- Startup prints `offering accepted audio to IPFS at 127.0.0.1:7902 (best-effort)`; store
  `loaded 0 skipped 0 evicted 0`; four generated items.
- TCP publish/fetch verified byte-for-byte; HTML labelled `audio/wav` -> typed `Rejected`.
- Shutdown after SIGTERM (exit 0):
  ```
  ipfs mirror: 6 mirrored, 0 failed
    3c16fc2d5862d8ce -> bafyverify1
    ...
    3d065856e46d39e7 -> bafyverify6
  ```
  Six = the four startup sounds plus the tone and noise published during the run.

**Commit identifiers:** none (no code change).

---

## 2026-11-14 — refuse a TLS Kubo endpoint instead of sending audio in clear

**Repository:** `oag-listend`, commit `13ce5bb` on `master`; records in `open-agent-graph`.

**The defect.** Auditing the IPFS client found that `split_api` stripped an `https://` prefix and
then spoke plain HTTP to whatever port followed. An operator pasting a TLS endpoint would see a
configuration that appeared to work while audio went out unencrypted.

**What changed.** `https://` is now refused with a stated reason rather than silently downgraded.
`http://host:port` and a bare `host:port` still resolve, which the new unit test pins alongside the
refusal. README and SPEC record the behaviour.

**Tests executed.** `cargo test -p oag-listend`: **62 unit + 46 end-to-end, 0 failed**; 0 warnings.

**Commit identifiers:** `oag-listend` `13ce5bb`; this entry's commit pending.

---

## 2026-11-14 — keep a hung IPFS node from stalling the daemon

**Repository:** `oag-listend`, commits `2125e51` and `2a72087` on `master`; records in
`open-agent-graph`.

**The defect.** Reviewing the mirror with the same rigour as the rest of the crate found a defect
introduced one turn earlier: `accept` awaited the IPFS add **while holding the store lock**. A Kubo
node that hung would therefore stall every carrier's publish *and* fetch for up to `ADD_TIMEOUT`
(10 s) -- violating the concurrency property SPEC claims.

**What changed.**
- Every carrier now goes through one function, `accept_and_mirror`: `accept` under the lock (rule,
  durable write, insert), then the bytes are read under a short lock and sent **with no lock held**.
  `AudioStore::accept` stays the pure inbound rule, so a mirror cannot become part of what
  "accepted" means. Startup generation uses the same path.
- Fixed two further gaps found while rewiring: `http.rs` still called `accept` directly, so **HTTP
  publishes bypassed the mirror entirely**; and the CID map was filled only by the startup pass, so
  an item published normally had a CID that no client could discover.

**Tests executed.** `cargo test -p oag-listend`: **61 unit + 46 end-to-end, 0 failed**. New
`a_hung_mirror_does_not_stall_other_clients` publishes into a stub that accepts and never replies,
then requires another client's `Hello` within three seconds. Verified it discriminates: holding the
store lock across the add makes it fail at 3.3 s; the real code passes.

**Commit identifiers:** `oag-listend` `2125e51`, `2a72087`; this entry's commit pending.

---

## 2026-11-14 — carry the CID on fetch, so clients can reach IPFS

**Repository:** `oag-listend`, commit `de5aa23` on `master`; records in `open-agent-graph`.

**The gap.** The mirror recorded CIDs and printed them at shutdown, but a client that *fetched*
audio could not learn where else it lives. SPEC's own follow-up list named this.

**What changed.**
- `Response::Audio` gains an optional trailing `cid`. It is appended after the bytes, so a decoder
  that stops after the declared length ignores it, and a reply from a daemon that predates the field
  still decodes -- no protocol version change, because nothing that worked stops working.
- HTTP exposes `X-IPFS-CID` on the item route.
- `AudioStore` remembers CID by content hash (`content_id`) and drops the entry on eviction.
- `handle` on TCP/Unix and UDP look the CID up under a short lock, keeping the byte copy outside it.

**Tests executed.** `cargo test -p oag-listend`: **61 unit + 45 end-to-end, 0 failed**. New: a
with-CID reply round-trips; a hand-built reply without the field decodes to `cid: None` (proving
backward compatibility rather than asserting it); and end to end a mirrored item fetches with its
CID over both TCP and HTTP.

**Commit identifiers:** `oag-listend` `de5aa23`; this entry's commit pending.

---

## 2026-11-14 — mirror the stored library on startup

**Repository:** `oag-listend`, commit `2dd386e` on `master`; records in `open-agent-graph`.

**The gap.** Found by the follow-up list added with the IPFS mirror. A restart with `--ipfs-api`
mirrored only audio published *after* it started; everything already in `--dir` would never reach
IPFS, despite the flag being described as offering every accepted item.

**What changed.**
- `AudioStore::mirror_existing(budget)` offers every held item through the same best-effort path the
  per-publish mirror uses.
- Bounded **as a whole** by `STARTUP_MIRROR_BUDGET` (30 s), so a large store cannot hold startup open
  behind a slow mirror; each item still keeps its own per-call bound, and an item that does not finish
  is simply left for its next change.
- `main.rs` calls it after attaching the publisher and prints how many of the held items were offered.
- The per-publish mirror was factored into one `offer_to_ipfs` helper so both paths are identical.

**Tests executed.** `cargo test -p oag-listend`: **60 unit + 44 end-to-end, 0 failed**. New: a
two-item store is fully offered (and the stub receives both); a stub that accepts but never answers
stops at a 200 ms budget rather than waiting out each call's 10 s timeout.

**Commit identifiers:** `oag-listend` `2dd386e`; this entry's commit pending.

---

## 2026-11-14 — add an IPFS mirror behind `--ipfs-api`

**Repository:** `oag-listend`, commit `4846a01` on `master`; records in `open-agent-graph`.

**What it is.** Audio the daemon accepts can now also be offered to a Kubo node, so it reaches a
network whose clients never have to connect here. This is the named next carrier work.

**Design decisions, and why.**
- A *mirror*, not a carrier and not a new operation: the frame protocol permits only publish /
  fetch-by-hash / list / hello (+ ping), so IPFS cannot add a way to ask for something, and the
  BLAKE3 content hash remains the identity. The CID is an additional address for the same bytes.
- It lives in `AudioStore::accept` -- the one surface all four carriers call -- because mirroring per
  carrier would fork the behaviour.
- **Best-effort.** The durable write happens first; a mirror failure is recorded, never propagated,
  because losing a copy must not lose the audio. The last 64 outcomes are retained and printed at
  shutdown with their CIDs (`ipfs mirror: N mirrored, M failed`, then one line each).
- No new dependency: the client hand-rolls `POST /api/v0/add` as multipart form data (the crate
  already hand-rolls its own HTTP server and carries no framework), with a 10 s bound per read and
  write and a 64 KiB cap on the reply.

**Honest limitation.** Tested end to end against a stub mimicking Kubo's add endpoint, through the
real binary and flag. **Not tested against a real Kubo node**, and it does not decode chunked replies.
Both stated in SPEC and README rather than left to be discovered.

**Tests executed.** `cargo test -p oag-listend`: **60 unit + 42 end-to-end, 0 failed**. New: four
client unit tests (CID extraction, non-2xx, missing Hash, unreachable API); three store-level tests
(mirrored outcome carries the CID and the posted body carries the audio; a 500 records a failure and
still serves the audio; an unreachable API is reported, not fatal); one binary test that starts the
daemon with `--ipfs-api`, publishes, and asserts the stub was called exactly once and the shutdown
report reads `1 mirrored, 0 failed`. README and SPEC updated; README flag coverage is complete.

**Commit identifiers:** `oag-listend` `4846a01`; this entry's commit pending.

---

## 2026-11-14 — make the module docs match what the code does

**Repository:** `oag-listend`, commit `d9801c7` on `master`; records in `open-agent-graph`.

**The gap.** Extended the flag-vs-README check to each module's own header claims and compared them
with the code:

- `http.rs` listed three routes, but `route()` handles five. `GET /ping` -- the headline "even pings
  are music" directive -- and `GET /audio` (the paginated listing) were both missing, as was
  `OPTIONS`.
- `protocol.rs` said the operations were "exactly: identify, publish audio, fetch audio, list
  audio", which contradicts `OP_PING`, the `Ping` request/response variants, and SPEC's stated five.

**What changed.** Both headers rewritten to enumerate what the code actually accepts. The other
module headers (`lib.rs`, `server.rs`, `udp.rs`, `unix.rs`) were checked and make no exhaustive claim
that omits anything.

**Tests executed.** No behaviour change; `cargo test -p oag-listend`: 56 unit + 38 end-to-end,
0 failed; 0 warnings.

**Commit identifiers:** `oag-listend` `d9801c7`; this entry's commit pending.

---

## 2026-11-14 — document `--seed` and `--no-generate`

**Repository:** `oag-listend`, commit `93486d1` on `master`; records in `open-agent-graph`.

**The gap.** Checked every flag the binary accepts against the README: all ten others appear, but
`--seed` occurred only inside an example command line with no explanation, and `--no-generate` not at
all. An operator reading the README could not learn that startup generation can be disabled or
re-seeded.

**What changed.** One paragraph after the startup-set description: `--seed U64` chooses the generated
sounds (default from the clock and process id, matching `default_seed()`), and `--no-generate` skips
them for an operator who wants the daemon to serve only what it is given.

**Tests executed.** No code change; `cargo test -p oag-listend`: 56 unit + 38 end-to-end, 0 failed.
(The binary test already asserts `--help` lists all eleven flags.)

**Commit identifiers:** `oag-listend` `93486d1`; this entry's commit pending.

---

## 2026-11-14 — verify the release artifact, not just the debug build

**Repository:** `oag-listend`, no code change.

**Why.** The systemd unit ships `cargo build --release`, and the release binary had last been checked
before the `--help` fix, the SIGTERM test, and the later work. The artifact an operator actually runs
is the one worth verifying.

**Observed** (`target/release/oag-listend`, 992,352 bytes; `target/release/examples/push_pull`), with
all four carriers on one store:
- TCP: tone and noise published then fetched `same-hash ... verified=true`; HTML labelled
  `audio/wav` -> typed `Rejected`.
- UDP: same, `verified=true`.
- HTTP: POST 201; GET 200 `audio/wav` **8,044 bytes byte-identical** to the posted file; `Range:
  0-99` -> 206.
- SIGTERM: clean stop, **exit 0**.

**Commit identifiers:** none (no code change). `cargo test -p oag-listend` remains 56 unit +
38 end-to-end, 0 failed.

---

## 2026-11-14 — record integrity and repository hygiene

**Repository:** `open-agent-graph`; no code change.

**Checked.**
- `ENGINEERING_LOG.md` is reverse-chronological with **no duplicate headings** (an earlier pass had
  left two near-identical entries and a stray separator; both are gone).
- `oag-listend`'s tracked file set is exactly the intended sources, tests, scripts, and docs:
  **zero** `target/` entries, working tree clean, nothing unpushed.
- All five checkouts clean and pushed on their correct branches (`main`, `main`, `master`,
  `master`, `master`).
- `cargo test -p oag-listend`: 56 unit + 38 end-to-end, 0 failed.

**Not changed.** No defect found, so nothing was modified beyond this entry. `PROJECT_STATE.md`
already reflects the current bar (56 + 38) and the closed operator surface; its next priorities are
current, with the IPFS/Reticulum carriers the only open items and the IPFS assessment now recorded in
SPEC.

**Commit identifiers:** none (no code change).

---

## 2026-11-14 — record what the IPFS carrier would actually take

**Repository:** `oag-listend`, commit `0e74ccf` on `master`; records in `open-agent-graph`.

**Why.** SPEC named IPFS and Reticulum as deliberately unbuilt but gave no reason, so each invocation
re-investigated them from zero. That investigation is now recorded where the next reader will find it.

**What it concludes.** Reticulum needs a physical interface and `oag-storage` -- nothing can be written
or run here. IPFS has a client half already in the imagination layer (`HttpIpfsPublisher` posting
multipart to Kubo's `/api/v0/add`), but building it here would mean adding an HTTP *client* to a crate
that deliberately hand-rolls its own server and carries no framework, plus hand-built multipart. Two
constraints make it more than a new file: the frame protocol permits only publish / fetch-by-hash /
list / hello (+ ping), so IPFS must be a *mirror* of accepted audio rather than a new operation; and a
network call on the accept path couples the store's durability guarantee to an external service,
which is a design change to settled code.

**Decision.** Still not built. A fake-endpoint test would prove the client logic but cannot prove
real-IPFS behaviour, so the honest status remains "not built". The entry records how to build it
behind `--ipfs-api ADDR`, best-effort, labelled untested against real IPFS, if the operator wants it.

**Tests executed.** No code change; `cargo test -p oag-listend` remains 56 unit + 38 end-to-end,
0 failed.

**Commit identifiers:** `oag-listend` `0e74ccf`; this entry's commit pending.

---

## 2026-11-14 — clean-state acceptance run (all four carriers)

**Repository:** `oag-listend`, no code change; records in `open-agent-graph`.

**Why.** Every documented bound now has a test, so this invocation ran the directive's acceptance
sentence once from an empty store instead of adding more coverage.

**Command.** `oag-listend --listen 127.0.0.1:7801 --http 127.0.0.1:7802 --udp 127.0.0.1:7803 --unix
<dir>/audio.sock --dir <dir>/store --seed 20261114`, then `push_pull` over TCP and UDP, then curl
over HTTP, then a restart, then SIGTERM.

**Observed.**
- Startup: store `loaded 0 skipped 0 evicted 0`; four generated items (silence, tone, noise, the
  canonical ping); `held 48816`.
- TCP and UDP both: ping 44,844 bytes `verified=true`; published tone 1,164 B and noise 1,324 B each
  came back `same-hash ... verified=true`; HTML labelled `audio/wav` returned a typed
  `Rejected { reason: "input is not audio; only audio is accepted" }`.
- HTTP: POST 201 with a 64-char hash; GET 200 `audio/wav` 8,044 bytes **byte-identical** to the
  posted file; `Range: 0-1023` -> 206; `HEAD` -> 200 with headers and no body; unknown hash -> 404;
  `<html>` labelled `audio/wav` -> 400; `/ping` -> 44,844 bytes.
- Restart: `loaded 7 skipped 0 evicted 0`, and the startup set was **not** regenerated (the store
  already held audio). Held 59,348.
- SIGTERM: clean stop, exit 0, socket removed (the first `wait` reported 127 only because that
  daemon was not a child of the shell running `wait`; the second, waited-on-as-a-child, exited 0).

**Commit identifiers:** none (no code change).

---

## 2026-11-14 — record the fifth repository in PROJECT_STATE

**Repository:** `open-agent-graph`, commit `9579399` on `main`; records in the same file.

**The defect.** `PROJECT_STATE.md` is the authoritative cross-project record the directive says to
keep current, and it did not know the mission's own repository existed: the header said "across its
**three** repositories", the table listed four, `oag-listend` was absent from both the repository
table and the measured table, the closing line said "all three working trees", and the date was five
days stale. A reader could not learn from it that the perpetual-directive work was happening at all.

**What changed.** Header corrected to five repositories with today's date; `oag-listend` added to the
repository table and to the measured table with figures taken from the checkout (94 tests, 103
commits); `oag-collisions` added with its factual commit count and an honest dash for a test count
this file has not measured; the closing line now says five working trees; and a capabilities section
for the serving surface added under Completed capabilities. The next-priorities entry's stale
"52 unit + 31 end-to-end" bar was refreshed to the real 56 + 38, and its list of what is now measured
was brought up to date.

**Tests executed.** No code change; `cargo test -p oag-listend` remains 56 unit + 38 end-to-end,
0 failed.

**Commit identifiers:** `open-agent-graph` `9579399`; this entry's commit pending.

---

## 2026-11-14 — prove the UDP listing page fits a datagram

**Repository:** `oag-listend`, commit `cf9d3c7` on `master`; records in `open-agent-graph`.

**The gap.** SPEC: "The page is bounded ... a smaller cap on UDP -- because an unbounded listing would
eventually exceed the frame cap". Nothing measured it, and with a handful of items no cap is
observable at all.

**What changed.** New `the_udp_listing_page_is_capped_at_what_a_datagram_can_carry`. It fills the
store directly with **1,100 distinct sounds** -- past the ~990 hashes a 65,507-byte datagram can carry
-- then asks for everything (`limit: u32::MAX`) and asserts exactly `MAX_DATAGRAM_LIST_LIMIT` come
back, that `total` still reports all 1,100, and that the first page is the head of the sorted walk.
Noise rather than tones, because a tone's pitch repeats every 620 seeds and would silently publish
duplicates.

**Why the test is trustworthy.** Raising `MAX_DATAGRAM_LIST_LIMIT` to 2000 makes the response exceed a
datagram, so the carrier correctly refuses it and the test fails; restoring 900 makes it pass. A
first draft used only 950 items, where even a wrong cap still fit a datagram -- that version proved
nothing about the property, so the count was raised past the boundary.

**Tests executed.** `cargo test -p oag-listend`: 56 unit + 38 end-to-end, 0 failed; 0 warnings.
README corrected from "plus 37" to "plus 38".

**Commit identifiers:** `oag-listend` `cf9d3c7`; this entry's commit pending.

---

## 2026-11-14 — prove the HTTP header cap refuses a flood

**Repository:** `oag-listend`, commit `9dbce2c` on `master`; records in `open-agent-graph`.

**The gap.** `MAX_HEAD_BYTES` exists so a header flood cannot make the daemon allocate, but no test
measured it. It was the last documented bound without coverage.

**What changed.** New `an_oversized_header_block_is_refused`: send a valid request line followed by
padding headers past the cap with **no blank line ending the head** -- which would wait forever
without a cap -- then assert `400 Bad Request` and an empty store. The client half-closes after
sending, so the post-refusal drain sees an end rather than waiting out its timeout.

**Why the test is trustworthy.** Raising `MAX_HEAD_BYTES` to 16 MiB makes the server keep reading
until its IO timeout and answer nothing, so the test fails (12.4 s); restoring the real value makes it
pass.

**Tests executed.** `cargo test -p oag-listend`: 56 unit + 37 end-to-end, 0 failed; 0 warnings.
README corrected from "plus 36" to "plus 37".

**Commit identifiers:** `oag-listend` `9dbce2c`; this entry's commit pending.

---

## 2026-11-14 — prove the session cap is enforced

**Repository:** `oag-listend`, commit `10ddda3` on `master`; records in `open-agent-graph`.

**The gap.** SPEC documents "at most 64 sessions served at once per carrier". Nothing measured it,
so a future change that dropped or bypassed the semaphore would have gone unnoticed.

**What changed.** New `a_client_past_the_session_cap_waits_for_a_free_slot`: fill every slot with a
session that has *proven* it is live (each sends Hello and reads the answer, which can only happen
after the permit is acquired), then show a further client's connection is completed by the kernel but
goes unanswered for 700 ms, and is served promptly once one session is released.

**Why the test is trustworthy.** It was checked both ways. Raising `MAX_CONNECTIONS` is *not* a valid
probe -- the test holds exactly that many slots by construction -- so the guard was validated against
an effectively unbounded cap (`usize::MAX / 4`), which makes it fail; restoring the real code makes it
pass.

**Tests executed.** `cargo test -p oag-listend`: 56 unit + 36 end-to-end, 0 failed; 0 warnings.
README corrected from "plus 35" to "plus 36".

**Commit identifiers:** `oag-listend` `10ddda3`; this entry's commit pending.

---

## 2026-11-14 — prove a stalled reader does not stall other clients

**Repository:** `oag-listend`, commit `d0d67b7` on `master`; records in `open-agent-graph`.

**The gap.** SPEC's concurrency section claims the store lock is released before byte copies, so one
slow client cannot stall the others. Nothing measured it; every test either stopped the daemon or used
small items that fit in kernel buffers.

**What changed.** New `a_client_stalled_mid_fetch_does_not_stall_another`: publish an 8 MiB item,
have a client with a **1 KiB receive buffer** ask for it and then stop reading, and assert a second
client still gets its answer within three seconds. The size is deliberate -- large enough that no
combination of socket buffers absorbs it, so the stall is real rather than absorbed by the kernel.
Also corrected a stale doc comment on the persistence test: the path writes through `tokio::fs`, not
a blocking `std::fs::write`.

**Why the test is trustworthy.** A regression test that passes on the bug is worthless, so the guard
was verified both ways: temporarily holding the store lock across the network write makes it fail
with "a second client must be served while another is stalled: Elapsed"; restoring the real code
makes it pass. (Holding the lock only across the clone does not trip it -- that is milliseconds --
which is exactly why the write, not the clone, is what the test pins.)

**Tests executed.** `cargo test -p oag-listend`: 56 unit + 35 end-to-end, 0 failed; 0 warnings.
README corrected from "plus 34" to "plus 35".

**Commit identifiers:** `oag-listend` `d0d67b7`; this entry's commit pending.

---

## 2026-11-14 — test that SIGTERM is a clean stop

**Repository:** `oag-listend`, commit `37ef3fa` on `master`; records in `open-agent-graph`.

**The gap.** SPEC and README both claim the daemon "handles SIGINT and SIGTERM ... exits 0, the Unix
socket is removed", but every automated test stopped it with `Child::kill()` -- SIGKILL. The signal
`systemctl stop` actually sends was verified only by hand.

**What changed.** New `sigterm_is_a_clean_stop`: start the daemon with `--unix`, wait for its own
"listening on" announcement, send SIGTERM, then assert it exits **0**, prints that it is shutting
down, and removes the socket file so the next start does not trip over a stale path. The stdout
announcement wait is factored into an `announce` helper shared with the serving test.

**Tests executed.** `cargo test -p oag-listend`: 56 unit + 34 end-to-end, 0 failed; 0 warnings.
README corrected from "plus 33" to "plus 34".

**Commit identifiers:** `oag-listend` `37ef3fa`; this entry's commit pending.

---

## 2026-11-14 — bring SPEC's status paragraph in line with what is tested

**Repository:** `oag-listend`, commit `e3ca5f3` on `master`; records in `open-agent-graph`.

**The defect.** The status paragraph enumerated the end-to-end coverage but had fallen behind the
suite: it did not mention eviction, the per-frame cap, native listing pagination, per-format
recognition or per-format content types, the operator binary, or `--check`, and it stated no test
count. A reader could not tell what was actually proven.

**What changed.** Rewritten to state the current suite -- 56 unit + 33 end-to-end, all green -- and
enumerate what it covers: the native back-and-forth (publish, fetch, verify, reject non-audio,
not-found); eviction under the byte bound; the per-frame cap refusing an oversized length before any
body is read; native listing pagination; each of the six named audio signatures in both directions
over the wire; HTTP serving every format under its own content type; HTTP refusal, streaming,
ranges, HEAD, validators and CORS; the shared store across carriers; UDP; the Unix socket;
persistence across a restart; concurrency; the operator binary and `--check`; and the musical ping on
every carrier.

**Tests executed.** No code change; `cargo test -p oag-listend`: 56 unit + 33 end-to-end, 0 failed.

**Commit identifiers:** `oag-listend` `e3ca5f3`; this entry's commit pending.

---

## 2026-11-14 — fix the daemon's own --help text

**Repository:** `oag-listend`, commit `230ee5b` on `master`; records in `open-agent-graph`.

**The defect.** The operator's `--help` was malformed and incomplete. The usage line listed only
`[--listen] [--http] [--seed] [--max-bytes] [--no-generate]`, omitting `--udp`, `--unix`, `--dir`,
`--check`, and `--help`. And the flag descriptions were wrong: `--http`'s line ran straight into
`--udp` (no newline between them), while `--udp`/`--unix`/`--dir`/`--check` carried eleven spaces of
stray indentation because those lines used literal newlines in the source where their neighbours used
`\<newline>` continuations.

**What changed.** `usage()` rewritten as a single multi-line literal, so the spacing an operator reads
is exactly the spacing written; the usage line now lists every accepted flag. The binary test is
strengthened from "contains `--listen`" to asserting that all eleven flags appear **and** that each
flag's description sits on its own line, which is precisely the invariant that broke.

**Tests executed.** `cargo test -p oag-listend`: 56 unit + 33 end-to-end, 0 failed; 0 warnings.

**Commit identifiers:** `oag-listend` `230ee5b`; this entry's commit pending.

---

## 2026-11-14 — verify the generated WAV containers are playable, not just sniffable

**Repository:** `oag-listend`, commit `178de8e` on `master`; records in `open-agent-graph`.

**The gap.** Directive step 5 requires "valid WAV". The header test checked tags, channels, and bit
depth, but never the size fields. A file whose RIFF or `data` size disagrees with its actual bytes is
accepted by a magic-bytes sniff, served happily by the daemon, and rejected by every real player --
so "valid" was asserted more weakly than it should have been.

**What changed.** New unit tests: `the_container_sizes_agree_with_the_bytes` walks all three kinds and
asserts the RIFF size covers the file, the fmt chunk is 16 bytes of PCM, the byte rate and block align
match 16-bit mono at 8 kHz, the data size covers exactly the payload, and the sample count matches the
seed's duration; `the_canonical_ping_container_sizes_agree` does the same for the ping, which every
carrier serves.

**Tests executed.** `cargo test -p oag-listend`: 56 unit + 33 end-to-end, 0 failed; 0 warnings.
README corrected from "54 unit plus 33" to "56 unit plus 33".

**Commit identifiers:** `oag-listend` `178de8e`; this entry's commit pending.

---

## 2026-11-14 — serve every recognised format under its own HTTP content type

**Repository:** `oag-listend`, commit `fc9ba98` on `master`; records in `open-agent-graph`.

**The gap.** "HTTP serves the bytes with an audio content type" was proven only for WAV. A FLAC or an
MP3 whose served header was wrong would silently fail to play in a browser, and nothing would catch
it.

**What changed.** New `every_recognised_format_is_served_with_its_own_content_type`: each of the six
recognised formats is posted labelled `application/x-www-form-urlencoded` -- so only the bytes can
decide -- then fetched back and asserted to carry its own `audio/*` type (`audio/wav`, `audio/aiff`,
`audio/flac`, `audio/ogg`, `audio/mpeg`, `audio/mp4`) with a byte-identical body.

**Tests executed.** `cargo test -p oag-listend`: 54 unit + 33 end-to-end, 0 failed; 0 warnings.
README corrected from "plus 32" to "plus 33".

**Commit identifiers:** `oag-listend` `fc9ba98`; this entry's commit pending.

---

## 2026-11-14 — test every named audio signature, on both sides of the sniff

**Repository:** `oag-listend`, commit `e89eef4` on `master`; records in `open-agent-graph`.

**The gap.** Directive step 3 names WAV, AIFF, FLAC, Ogg, MP3, and M4A, but only M4A and Ogg had
direct recognition tests. WAV, AIFF, FLAC, and MP3 were never asserted, so a regression in
`AudioFormat::sniff` would have refused valid audio (or admitted near-misses) with nothing to catch
it.

**What changed.**
- `every_named_signature_is_recognised_from_its_bytes`: RIFF/WAVE, FORM/AIFF, FORM/AIFC, the FLAC
  marker, an ID3v2 tag, a raw MPEG-1 Layer III frame header, and Vorbis/Opus/FLAC-in-Ogg from the
  beginning-of-stream page.
- `near_miss_signatures_are_not_audio`: RIFF/AVI, FORM/8SVX, an MPEG header with the reserved version
  field set, one with a zero bitrate field, Theora inside Ogg, plain text, and empty input.
- End to end: publish AIFF, FLAC, MP3/ID3, Ogg/Opus, and M4A over TCP and fetch each back
  byte-identical under its own BLAKE3 -- the sniff is only useful if the wire agrees with it.

**Tests executed.** `cargo test -p oag-listend`: 54 unit + 32 end-to-end, 0 failed; 0 warnings.
README corrected from "52 unit plus 31" to "54 unit plus 32".

**Commit identifiers:** `oag-listend` `e89eef4`; this entry's commit pending.

---

## 2026-11-14 — correct push_pull's own --help text

**Repository:** `oag-listend`, commit `f59919b` on `master`; records in `open-agent-graph`.

**The defect.** The operator client's `--help` disagreed with itself: the usage line omitted
`--version` and `--help` even though both are accepted, and the closing line said only "Sends a ping"
when the example also publishes a tone and a noise, fetches both back and verifies their BLAKE3, and
proves the inbound rule by sending non-audio.

**What changed.** Both lines now match what the example actually does. Verified by hand against a live
daemon over both carriers: TCP and UDP each report `verified=true` for the ping (44,844 bytes), the
tone, and the noise, and a typed `Rejected` for HTML.

**Tests executed.** No code change beyond the help text; `cargo test -p oag-listend` remains
52 unit + 31 end-to-end, 0 failed.

**Commit identifiers:** `oag-listend` `f59919b`; this entry's commit pending.

---

## 2026-11-14 — fix a SPEC self-contradiction about the eviction unlink

**Repository:** `oag-listend`, commit `444c59c` on `master`; records in `open-agent-graph`.

**The defect.** SPEC.md said two opposite things about the eviction unlink: that it "stays
synchronous, being metadata-only", and later that it is "taken off the lock and done asynchronously
after the publish returns". The code does the latter -- `drain_evicted` uses `tokio::fs::remove_file`,
off the lock, awaited inside `accept`. The first sentence was stale from before the off-lock change.

**What changed.** Corrected the stale sentence to match the code and reflowed the paragraph. While
auditing the document against the implementation, verified these claims are accurate:
`DEFAULT_MAX_BYTES` 1 GiB, `MAX_DATAGRAM_BYTES` 65,507, `SESSION_TIMEOUT`/`REQUEST_TIMEOUT` 30 min,
`MAX_LOAD_ENTRIES` 100,000, `MAX_SIDECAR_BYTES` 4 KiB, `MAX_CONNECTIONS` 64 per carrier,
`normalize_limit` treating `limit: 0` as the default page, SIGINT/SIGTERM clean stop, and the twinkle
at 44,844 bytes (fetched over HTTP).

**Tests executed.** No code change; `cargo test -p oag-listend` remains 52 unit + 31 end-to-end,
0 failed.

**Commit identifiers:** `oag-listend` `444c59c`; this entry's commit pending.

---

## 2026-11-14 — answer 413 for too-large audio, as documented

**Repository:** `oag-listend`, commit `f8da5cb` on `master`; records in `open-agent-graph`.

**The defect.** `http.rs`'s module doc promised "POST /audio -- publish a body (413 if too large)",
but the carrier never emitted 413: an oversized declared body was refused as a generic bad request
(400), and so was audio that passed sniff but exceeded the store's `--max-bytes`. The docs and the
behaviour disagreed, and 400 was the wrong status for a well-formed body that is merely too big.

**What changed.** `HttpRequest` carries a `too_large` flag set only at the content-length check; the
BAD path now answers `413 Payload Too Large`. The POST arm maps `ListendError::TooLarge` to 413 as
well, so a client reacts to one status whether the body exceeds the frame cap or the store bound.
README and SPEC corrected from 400 to 413.

**Tests executed.** `cargo test -p oag-listend`: 52 unit + 31 end-to-end, 0 failed. The existing
oversized test now asserts 413 (and asserts it is neither 100 nor 400); a new test posts audio above
the store's own bound and asserts 413 with the store still empty.

**Commit identifiers:** `oag-listend` `f8da5cb`; this entry's commit pending.

---

## 2026-11-14 — test `--check` against a real store

**Repository:** `oag-listend`, commit `7ec9a52` on `master`; records in `open-agent-graph`.

**The gap.** `--check` is documented for operators as "open the store, print the load report, and
exit", but no automated test exercised it, so nothing proved it loads or verifies a directory.

**What changed.** Added to `tests/binary.rs`: write one genuine item (audio plus its media-type
sidecar) under its own BLAKE3 name, run `--check --dir <dir>`, and assert `loaded 1`, `skipped 0`,
`evicted 0`, and that the store's size and bound are reported. Then corrupt the file out of band and
assert the reload reports `loaded 0`, `skipped 1` -- a file whose bytes no longer match its name is
never loaded or served.

**Tests executed.** `cargo test -p oag-listend`: 52 unit + 30 end-to-end, 0 failed. README corrected
from "plus 29" to "plus 30".

**Commit identifiers:** `oag-listend` `7ec9a52`; this entry's commit pending.

---

## 2026-11-14 — a binary-level test for the operator entry point

**Repository:** `oag-listend`, commit `e4bc5dd` on `master`; records in `open-agent-graph`.

**The gap.** Directive step 7 is the operator's binary, and `main.rs` was the one path no automated
test reached: `--version`, `--help`, usage exit codes, and actually starting the daemon were verified
by hand only.

**What changed.** New `tests/binary.rs`. It spawns the real binary via `CARGO_BIN_EXE_oag-listend`,
waits for its own `listening on` line instead of sleeping on a fixed delay, exchanges audio over TCP
(Hello, then publish + BLAKE3 verify), and stops it. A second test asserts `--version` prints the
package version and protocol version, `--help` documents `--listen`, and a bad `--seed`, a bad
`--max-bytes`, or an unknown flag exits **2** with a message and never panics.

**Finding along the way.** Omitting `--listen` is *not* a usage error: the documented default is
`0.0.0.0:7100`, so that case starts the daemon (README lines 107-108). The test was corrected rather
than the default changed, since the default is deliberate and documented.

**Tests executed.** `cargo test -p oag-listend`: 52 unit + 29 end-to-end, 0 failed; 0 warnings.
README corrected from "plus 27" to "plus 29".

**Commit identifiers:** `oag-listend` `e4bc5dd`; this entry's commit pending.

---

## 2026-11-14 — an end-to-end native listing-pagination test

**Repository:** `oag-listend`, commit `284bd64` on `master`; records in `open-agent-graph`.

**The gap.** The native `list known hashes` operation was exercised end to end only for a single item;
its paging was covered solely by a unit test inside `server.rs`.

**What changed.** New test in `tests/back_and_forth.rs`: publish three tones, sort the hashes the way
the store does, then walk the listing -- the whole store, one page of one, a page whose limit runs past
the end (must stop there), and an offset beyond the store (must answer empty with the clamped offset,
not wrap or repeat).

**Tests executed.** `cargo test -p oag-listend`: 52 unit + 27 end-to-end, 0 failed. README corrected
from "plus 26" to "plus 27".

**Commit identifiers:** `oag-listend` `284bd64`; this entry's commit pending.

---

## 2026-11-14 — an end-to-end native frame-cap test

**Repository:** `oag-listend`, commit `ac63ce1` on `master`; records in `open-agent-graph`.

**The gap.** The single-frame cap was covered only by a unit test of `decode_frame`. Nothing showed
that the running daemon refuses an oversized length prefix without reading or allocating a body.

**What changed.** New test in `tests/back_and_forth.rs`: connect, send a four-byte prefix declaring
`MAX_FRAME_BYTES + 1` with no body, assert the session closes promptly rather than waiting for a
body, then reconnect and assert `Hello` still answers -- the refusal ends one frame, not the service.

**Tests executed.** `cargo test -p oag-listend`: 52 unit + 26 end-to-end, 0 failed. README corrected
from "plus 25" to "plus 26".

**Commit identifiers:** `oag-listend` `ac63ce1`; this entry's commit pending.

---

## 2026-11-14 — an end-to-end eviction test

**Repository:** `oag-listend`, commit `cc0a6ac` on `master`; records in `open-agent-graph`.

**The gap.** Eviction was covered only by unit tests inside `lib.rs`. The directive names bounds and
eviction as hardening work, and a carrier that cannot observe the bound proves nothing about the
service.

**What changed.** New `tests/eviction.rs`: a store bounded to exactly the larger tone's size; publish
the larger tone, then the smaller. Over the wire: the older hash is `NotFound`, the newer still
returns its bytes and verifies, and the evicted item's file is unlinked while the survivor's remains.
The test picks two differently sized generated tones at runtime rather than hard-coding sizes.

**Tests executed.** `cargo test -p oag-listend`: 52 unit + 25 end-to-end, 0 failed; 0 warnings.
README corrected from "plus 24" to "plus 25".

**Commit identifiers:** `oag-listend` `cc0a6ac`; this entry's commit pending.

---

## 2026-11-14 — correct the README's unit test count

**Repository:** `oag-listend`, commit `1e67844` on `master`; records in `open-agent-graph`.

**What changed.** The README said "51 unit tests"; the load-cap test took it to 52. Fixed so the
document matches what `cargo test -p oag-listend` actually reports.

**Tests executed.** `cargo test -p oag-listend`: 52 unit + 24 end-to-end, 0 failed.

**Commit identifiers:** `oag-listend` `1e67844`; this entry's commit pending.

---

## 2026-11-14 — a tuned release profile

**Repository:** `oag-listend`, commit `2dbe521` on `master`; records in `open-agent-graph`.

**What changed.** Added `[profile.release]` with `lto = true`, `codegen-units = 1`, `strip = true`.
The systemd unit builds with `cargo build --release`, which until now used the default
profile. The shipped binary is ~1 MB (992,352 bytes).

**Tests executed.** `cargo test -p oag-listend`: 52 unit + 24 end-to-end, 0 failed. Verified
by hand with the release binary: the root reports `oag-listend`, and a range request returns 206.

**Commit identifiers:** `oag-listend` `2dbe521`; this entry's commit pending.

---

## 2026-11-14 — drop dead deps, verify the operator CLI and the tooling invariant

**Repository:** `oag-listend`, commit on `master`; records in `open-agent-graph`.

**Dead dependencies.** `serde` and `serde_json` were declared in the workspace and crate manifests but
used in no file. Removed from both; build and all tests unchanged (52 unit + 24 end-to-end, 0
failed after the subsequent load-cap fix).

**Operator CLI verified by hand.** `--version` -> `oag-listend 0.1.0 (frame protocol 1)`; `--help` ->
the usage; `--check --dir <store> --max-bytes N` -> the load report and the store's item count, bytes
held, and effective item bound. Missing `--listen` exits 2; an invalid `--seed` exits 2.

**Tooling invariant.** The three scripts (`prune-target.py`, `rustc-wrapper.sh`,
`test_prune_target.py`) are byte-identical across all five repositories, one distinct hash per script.
`test_prune_target.py` passes 16 tests in oag-listend.

**Commit identifiers:** `oag-listend` commit pending (this entry's commit follows the load-cap fix).

## 2026-11-14 — count only audio entries toward the load cap

**Repository:** `oag-listend`, commit `4f6ef04` on `master`; records in `open-agent-graph`.

**The defect.** `load_dir` applied `max_entries` to every directory entry, including the `<hash>.json`
sidecars, which are not audio. A directory with many sidecars could therefore truncate the audio load
even when the audio itself fit under the cap.

**What changed.** The cap check now runs after the sidecar skip, so it counts audio entries only.

**Tests executed.** `cargo test -p oag-listend`: 52 unit + 24 end-to-end, 0 failed. New
`sidecars_do_not_count_toward_the_load_cap` writes three audio entries plus their sidecars and asserts
all three load under a cap of three.

**Commit identifiers:** `oag-listend` `4f6ef04`; this entry's commit pending.

---

## 2026-11-14 — oag-listend's .gitignore matches the other four

Running `scripts/test_prune_target.py` left `scripts/__pycache__` untracked in
oag-listend; the other four repos already ignore Python test bytecode. Added the
same rules (`__pycache__/`, `*.pyc`) to oag-listend's `.gitignore` and removed
the stray directory. Commit `8179849` on `master`.

**Tests executed.** Unchanged: 51 unit + 24 end-to-end, 0 failed.
---

## 2026-11-14 — take the eviction unlink off the lock

**Repository:** `oag-listend`, commit `f2ed4ae` on `master`; records in `open-agent-graph`.

**The limitation.** `AudioStore::forget` unlinked the evicted file synchronously while holding the
store mutex. An unlink is metadata-only, but on a slow disk it would stall every other client's
executor thread for its duration. This was the top recorded limitation.

**What changed.** `AudioStore` gained an `evicted: Vec<PathBuf>`. `forget` records the audio and
sidecar paths instead of unlinking. `accept` drains the list with `tokio::fs::remove_file` after
the publish returns, so the unlink happens off the lock and the executor is never blocked. A load
drains inline, since startup has no lock to protect. A failed unlink is best-effort, exactly as the
synchronous version was; the item is gone from the index either way.

**Tests executed.** `cargo test -p oag-listend`: 51 unit + 24 end-to-end, 0 failed. The eviction
test still asserts the file is gone after `accept`.

**Commit identifiers:** `oag-listend` `f2ed4ae`, `490a768` (gives `push_pull` a
`--version`, matching the daemon's `--version`/`--help` pair); this entry's commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** The remaining imagination carriers (IPFS, Reticulum), which stay
deliberately unbuilt.
---

## 2026-11-14 — resolve an uppercased content hash

**Repository:** `oag-listend`, commits `8ad3d19` and the README count on `master`; records in
`open-agent-graph`.

**The inconsistency.** Hash validation accepted uppercase hex (`is_ascii_hexdigit`), but
`AudioStore::serve` is case-sensitive, so a client that uppercased a hash was told the hash was
valid and then that it was missing.

**What changed.** `Reader::hash` and the HTTP `/audio/{hash}` route normalise to lowercase before
lookup. Tests pin both: a native `Fetch` with an uppercased hash resolves, and an HTTP GET with an
uppercased hash returns 200 with the same bytes.

**Tests executed.** `cargo test -p oag-listend`: 51 unit + 24 end-to-end, 0 failed.

**Commit identifiers:** `oag-listend` `8ad3d19` plus the README count; this entry's commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** The eviction unlink is recorded as synchronous. Then the remaining
imagination carriers.
---

## 2026-11-14 — a length past the remaining bytes is a bad string

**Repository:** `oag-listend`, commit `759b7c5` on `master`; records in `open-agent-graph`.

**The inconsistency.** `Reader::string`/`opt_string` pre-checked a length-prefixed string's length
against `self.buf.len()` (the whole message) rather than the bytes remaining. A length exactly one
past the end therefore fell through to `take` and reported `Truncated`, while a longer one
reported `BadString` -- the same condition named two different errors.

**What changed.** Both methods compare against a new `remaining()`; a test pins the distinction
(`[OP_PUBLISH, 0x00, 0x03, b'a', b'b']` is `BadString`).

**Tests executed.** `cargo test -p oag-listend`: 49 unit + 24 end-to-end, 0 failed.

**Commit identifiers:** `oag-listend` `759b7c5`, `7518347` (the response decoder's audio and
listing branches had the same weak whole-buffer pre-check; both now use `remaining()`); this
entry's commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** The eviction unlink is recorded as synchronous. Then the remaining
imagination carriers.
---

## 2026-11-14 — full-surface re-read: no further defect

**Repository:** records only (`open-agent-graph`); no code change this entry.

**What was done.** A line-by-line re-read of every module after the rustfmt pass: `lib.rs`
(`AudioItem`, `AudioFormat::sniff` incl. the `ftyp` brand list and the Ogg codec header,
`AudioStore` accept/publish/serve/hashes/persist/forget, the load report and its entry cap),
`protocol.rs` (frame codec, reader/writer, limits), `wav.rs` (generator, cap, twinkle),
`server.rs` (session loop, bounded reads/writes, ping, startup generation), `http.rs`
(request parsing, every route, ranges, caching, drain), `udp.rs`, `unix.rs`, and `main.rs`.
No new defect found; the previous entries' fixes are all in place.

**Tests executed.** `cargo test -p oag-listend`: 49 unit + 24 end-to-end, 0 failed;
`cargo fmt --check` clean; 0 compiler warnings.

**Commit identifiers:** none (records only).
**Push status:** this entry's commit pending.
**Remaining defects / next.** The eviction unlink is recorded as synchronous. Then the
remaining imagination carriers, which stay deliberately unbuilt.
---

## 2026-11-14 — a HEAD revalidation reports the entity length

**Repository:** `oag-listend`, commit `27e9548` on `master`; records in `open-agent-graph`.

**The defect.** A `304 Not Modified` has no body, but the branch set `Content-Length: 0`
unconditionally. A `HEAD` revalidation describes the entity, so it should report the item's length.

**What changed.** The 304 branch sets `Content-Length` to the item's length when `head_only`.

**Tests executed.** `cargo test -p oag-listend`: 49 unit + 24 end-to-end, 0 failed. The caching test
now asserts a HEAD revalidation is 304 with `content-length: <len>` and an empty body.

**Commit identifiers:** `oag-listend` `27e9548`; this entry's commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** The eviction unlink is recorded as synchronous. Then the remaining
imagination carriers.
---

## 2026-11-14 — push_pull has a --help

**Repository:** `oag-listend`, commit `c561264` on `master`; records in `open-agent-graph`.

**What changed.** `examples/push_pull.rs` gained `--help`/`-h`, printing its usage and exiting 0,
matching the daemon binary. (Its argument errors already exit 2 from the previous increment.)

**Tests executed.** `cargo test -p oag-listend`: 49 unit + 24 end-to-end, 0 failed.

**Commit identifiers:** `oag-listend` `c561264`; this entry's commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** The eviction unlink is recorded as synchronous. Then the remaining
imagination carriers.
---

## 2026-11-14 — CLI argument errors are usage errors, not panics

**Repository:** `oag-listend`, commit `82f4cba` (daemon) and the `push_pull` follow-up on `master`;
records in `open-agent-graph`.

**The defect.** A missing or invalid flag value (`--listen`, `--seed`, `--addr`, ...) used to
`expect`/`panic`, exiting 101 with a backtrace hint instead of a usage error.

**What changed.** `main.rs` gained `value`/`parse_value` helpers that print the usage and exit 2;
`push_pull` gained `flag_value` and the same treatment for its `--addr`/`--seed`. Verified by hand:
missing `--listen` -> exit 2, `--seed abc` -> exit 2, `--help`/`--version` -> exit 0.

**Tests executed.** `cargo test -p oag-listend`: 49 unit + 24 end-to-end, 0 failed (unchanged).

**Commit identifiers:** `oag-listend` `82f4cba` plus the `push_pull` follow-up; this entry's commit
pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** The eviction unlink is recorded as synchronous. Then the remaining
imagination carriers.
---

## 2026-11-14 — the crate is rustfmt-clean

**Repository:** `oag-listend`, commit `367f35e` on `master`; records in `open-agent-graph`.

**What changed.** Ran `cargo fmt` across the crate and examples. The code had accumulated
hand-wrapped lines over many increments; `cargo fmt --check` is now clean.

**Tests executed.** `cargo test -p oag-listend`: 49 unit + 24 end-to-end, 0 failed (unchanged).
Verified by hand after formatting: native publish/fetch `verified=true`, non-audio refused,
`GET /ping` returned 44,844 bytes.

**Commit identifiers:** `oag-listend` `367f35e`; this entry's commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** The eviction unlink is recorded as synchronous. Then the remaining
imagination carriers.
---

## 2026-11-14 — a HEAD range refusal carries no body

**Repository:** `oag-listend`, commit `b93b8fa` on `master`; records in `open-agent-graph`.

**The defect.** The 416 branch on both range routes (`GET /audio/{hash}` and `GET /ping`) built a
body and did not clear it for `HEAD`, so `HEAD` with an unsatisfiable range would have sent a body
-- violating HEAD's contract.

**What changed.** Both 416 branches clear the body and advertise its length when `head_only`.

**Tests executed.** `cargo test -p oag-listend`: 49 unit + 24 end-to-end, 0 failed. The range test
now asserts a HEAD 416 has an empty body.

**Commit identifiers:** `oag-listend` `b93b8fa`; this entry's commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** The eviction unlink is recorded as synchronous. Then the remaining
imagination carriers.
---

## 2026-11-14 — refuse a chunked upload clearly, without losing the response

**Repository:** `oag-listend`, commit `27e7d07` on `master`; records in `open-agent-graph`.

**The defect.** A `POST` with `Transfer-Encoding: chunked` was read as a zero-length body and
rejected as "input is not audio" -- confusing, since the client did send audio. Worse, closing the
socket with the body unread made TCP send a reset, which could discard the 400 the client needed.

**What changed.** `read_request` records the transfer encoding and whether the body was unread. A
chunked `POST /audio` is refused with `400` and "chunked transfer encoding is not supported; send
Content-Length". After writing a response to a request with an unread body, the carrier half-closes
and drains up to `MAX_BODY_BYTES` before closing, so the response arrives intact instead of being
reset away.

**Tests executed.** `cargo test -p oag-listend`: 49 unit + 24 end-to-end, 0 failed (was 49 + 23).
New `a_chunked_upload_is_refused_with_a_clear_message`.

**Verified by hand.** A raw chunked POST returned `HTTP/1.1 400 Bad Request` with the message, whole.

**Commit identifiers:** `oag-listend` `27e7d07`, `0241ecd` (drains the body on the oversized-length
refusal), `a70b2fd` (drains on the head-cap and bad-request-line refusals too); this entry's commit
pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** The eviction unlink is recorded as synchronous. Then the remaining
imagination carriers.
---

## 2026-11-14 — GET /ping supports Range

**Repository:** `oag-listend`, commit `2a13ecc` on `master`; records in `open-agent-graph`.

**What changed.** The musical liveness sound is served audio, so `GET /ping` now honours
`Range: bytes=...`: 206 with `Content-Range`, 416 when unsatisfiable, and `Accept-Ranges: bytes` on
the full response -- matching `GET /audio/{hash}`. `HEAD /ping` still returns the headers with no
body.

**Tests executed.** `cargo test -p oag-listend`: 49 unit + 23 end-to-end, 0 failed. The ping test
now also asserts `bytes=0-9` returns 206 with the first ten bytes and the right `Content-Range`.

**Commit identifiers:** `oag-listend` `2a13ecc`; this entry's commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** The eviction unlink is recorded as synchronous. Then the remaining
imagination carriers.
---

## 2026-11-14 — bound and report the directory entries a load considers

**Repository:** `oag-listend`, commit `bb82865` on `master`; records in `open-agent-graph`.

**The gap.** `with_directory` iterated every entry in the directory. A store directory is an
operator-controlled path, so a mistake -- or a symlink to somewhere huge -- would make startup
process an unbounded number of entries.

**What changed.** `MAX_LOAD_ENTRIES = 100_000`. `with_directory` delegates to a private
`Self::load_dir(dir, max_bytes, max_entries)` so the cap is testable with a small value; the load
stops at the cap and sets `LoadReport::truncated` rather than silently ignoring the rest. The SPEC
and README document the new report field.

**Tests executed.** `cargo test -p oag-listend`: 49 unit + 23 end-to-end, 0 failed (was 48 + 23).
New `a_load_is_capped_and_reports_the_truncation` seeds three items, loads with a cap of two, and
asserts `truncated` and that only two were considered.

**Commit identifiers:** `oag-listend` `bb82865`, `78c6058` (prints the truncation flag, which the
report computed but the startup line did not show); this entry's commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** The eviction unlink is recorded as synchronous. Then the remaining
imagination carriers.
---

## 2026-11-14 — refuse an oversized body before the 100 Continue

**Repository:** `oag-listend`, commit `fda67a3` on `master`; records in `open-agent-graph`.

**The defect.** A `POST` carrying `Expect: 100-continue` and a `Content-Length` above the body cap
was answered with `100 Continue` and only then with `400`. The server told the client to commit an
upload it was about to reject, wasting the client's bandwidth and its own.

**What changed.** The `content_length > MAX_BODY_BYTES` check now runs before the interim response.
The interim write also uses `write_all_bounded`, like every other write.

**Tests executed.** `cargo test -p oag-listend`: 48 unit + 23 end-to-end, 0 failed (was 48 + 22).
New `an_oversized_expect_continue_gets_no_interim` sends the headers and asserts the first response
is `HTTP/1.1 400`, not `100`.

**Commit identifiers:** `oag-listend` `fda67a3`; this entry's commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** The eviction unlink is recorded as synchronous. Then the remaining
imagination carriers.
---

## 2026-11-14 — the startup set is one of each kind plus the ping

**Repository:** `oag-listend`, commit `37f72ef` on `master`; records in `open-agent-graph`.

**The oddity.** `wav::sounds` cycles three kinds, so `STARTUP_SOUNDS = 4` produced silence twice
(silence, tone, noise, silence) while the README and SPEC said "silence, tone, noise".

**What changed.** `STARTUP_SOUNDS = 3`, giving exactly silence, tone, noise, plus the canonical ping
appended by `publish_generated` -- four items. The two `publish_generated` tests were updated for the
new count.

**Tests executed.** `cargo test -p oag-listend`: 48 unit + 22 end-to-end, 0 failed.

**Verified by hand.** The daemon printed four generated items.

**Commit identifiers:** `oag-listend` `37f72ef`; this entry's commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** The eviction unlink is recorded as synchronous. Then the remaining
imagination carriers.
---

## 2026-11-14 — cap the generated sample count

**Repository:** `oag-listend`, commits `20e6051` and `8bd56e8` on `master`; records in
`open-agent-graph`.

**The gap.** `wav::generate_n` is public and took an unbounded sample count, so a caller passing a
huge value could allocate without limit.

**What changed.** `MAX_GENERATED_SAMPLES`, and `generate_n` clamps to it. The first commit set it to
16,000 frames (32 KB); that is below one UDP datagram's payload, which broke
`udp::tests::a_tone_fits_and_a_long_recording_does_not` -- its whole point is a recording too large
for a datagram. The cap is now 40,000 frames (80 KB): still bounded, and deliberately above the
datagram payload so the too-large path stays testable. That first commit was made despite a failing
test because the shell chain did not stop on the test exit code; the corrective commit fixes it and
the suite is green.

**Tests executed.** `cargo test -p oag-listend`: 48 unit + 22 end-to-end, 0 failed. New
`a_chosen_sample_count_is_capped` asserts a `usize::MAX` request yields exactly the cap. (The README
and this log first said 47; the new test makes it 48, corrected in `0906c66` and `4a135f1`.)

**Commit identifiers:** `oag-listend` `20e6051`, `8bd56e8`; this entry's commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** The eviction unlink is recorded as synchronous. Then the remaining
imagination carriers.
---

## 2026-11-14 — refuse to delete a non-socket at the Unix socket path

**Repository:** `oag-listend`, commit `b1c7793` on `master`; records in `open-agent-graph`.

**The hazard.** `UnixCarrier::bind_shared` removed whatever was at the path before binding. A
stale socket must be removed, but a regular file or directory at the path is not a stale socket --
deleting it would destroy something the operator put there.

**What changed.** The path's metadata is checked first: only `file_type().is_socket()` entries are
eligible for removal. Anything else is refused with `InvalidInput` and left untouched. Also added
`Cache-Control: public, max-age=31536000, immutable` to `GET /ping`, matching served audio.

**Tests executed.** `cargo test -p oag-listend`: 47 unit + 22 end-to-end, 0 failed (was 47 + 21).
New `a_non_socket_path_is_refused_not_deleted` writes a file at the path, asserts the bind fails
with `InvalidInput`, and asserts the file is intact.

**Verified by hand.** `oag-listend --unix /tmp/oag-notasocket` printed
`cannot bind unix ... exists and is not a socket` and the file still read `precious`.

**Commit identifiers:** `oag-listend` `b1c7793`; this entry's commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** The eviction unlink is recorded as synchronous. Then the remaining
imagination carriers.
---

## 2026-11-14 — the ping fits every carrier, including UDP

**Repository:** `oag-listend`, commits `0e8cc46` and the SPEC follow-up on `master`; records in
`open-agent-graph`.

**The defect.** The first rendition was the full two-phrase melody, 89,644 bytes -- larger than one
UDP datagram. The datagram carrier therefore could not answer a ping with the music; `handle`
replaced the response with a rejection, which is exactly the "empty pong" the directive forbids.

**What changed.** `wav::twinkle()` now generates the opening phrase only (C C G G A A G), 44,844
bytes, which the directive actually specifies ("start with twinkle twinkle little star"). The wav
test asserts the ping fits a frame *and* a UDP datagram. The UDP and Unix carrier tests now assert
the musical ping, so all four carriers are covered.

**Tests executed.** `cargo test -p oag-listend`: 47 unit + 21 end-to-end, 0 failed.

**Verified by hand.** Native `Ping`, `GET /ping`, and a UDP `Ping` all returned the same 44,844-byte
WAV; the UDP reply was `op 0x11`, 44,844 bytes, `RIFF`/`WAVE`. (A first UDP hand-check timed out
because the client read the datagram with a 4-byte buffer, which truncates it; the daemon was
correct.)

**Commit identifiers:** `oag-listend` `0e8cc46` plus the SPEC follow-up; this entry's commit
pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** The eviction unlink is recorded as synchronous. Then the remaining
imagination carriers.
---

## 2026-11-14 — the musical ping: "even pings are music"

**Repository:** `oag-listend`, commit `84712a1` on `master`; records in `open-agent-graph`.

**The directive.** `USER_INPUT_RECORD.md` Entry 3 (2026-11-14), verbatim: "even pings are music -
start with twinkle twinkle little star". `SPEC.md` records it as an individual directive: a
liveness answer must be audio, not a silent control plane beside the audio service.

**What changed.** `wav::twinkle()` generates the opening phrase (C C G G A A G | F F E E D D C) as
16-bit PCM WAV, deterministic and ramped per note, so every daemon answers with the same bytes and
the same BLAKE3. The native protocol gained `Request::Ping` (op 0x05); `handle` answers it with
`Response::Audio { content_hash, bytes }`. HTTP gained `GET /ping`, served as `audio/wav` with an
`ETag` and a filename. `Hello` is unchanged, so compatibility is preserved. The canonical ping is
appended to the startup set, so it is durable and fetchable by hash as well as answerable with a
ping. The two `publish_generated` tests were updated for the fifth startup item.

**Tests executed.** `cargo test -p oag-listend`: 47 unit + 21 end-to-end, 0 failed (was 46 + 19).
New `the_canonical_ping_is_a_deterministic_wav` (same bytes twice, WAV, exact sample count, fits a
frame), `a_native_ping_is_music` (non-empty, WAV, hash-verifiable), and
`http_ping_serves_the_canonical_music` (200 audio/wav, ETag names the bytes, listed by hash).

**Verified by hand.** The native `Ping` and `GET /ping` both returned the same 89,644-byte WAV with
the same hash (`ebf415e3...`); the startup set printed five generated items.

**Commit identifiers:** `oag-listend` `84712a1`; this entry's commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** The eviction unlink is recorded as synchronous. Then the remaining
imagination carriers.
---

## 2026-11-12 — serve a Content-Disposition with a real filename

**Repository:** `oag-listend`, commit `7902c83` on `master`; records in `open-agent-graph`.

**What changed.** `AudioFormat::file_extension()` maps each format to an extension, and the HTTP
carrier adds `Content-Disposition: inline; filename="<hash>.<ext>"` to a served item. A browser or
player that saves the audio now gets a usable name instead of a bare hash.

**Tests executed.** `cargo test -p oag-listend`: 46 unit + 19 end-to-end, 0 failed. The caching
test asserts the header; the media-type test asserts the extension has no dot.

**Verified by hand with curl.** `Content-Disposition: inline; filename="<hash>.wav"`.

**Commit identifiers:** `oag-listend` `7902c83`; this entry's commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** The eviction unlink is recorded as synchronous. Then the remaining
imagination carriers.
---

## 2026-11-12 — the self-heal attempts a restore on an error too

**Repository:** `oag-listend`, commit `d0f593f` on `master`; records in `open-agent-graph`.

**The gap.** The out-of-band self-heal treated a `try_exists` error as "present", so a missing store
directory was not recreated and the republished item stayed without a durable copy.

**What changed.** An error is treated as absent, so the restore is attempted; `persist` recreates a
missing directory with `create_dir_all` and fails loudly if it cannot. The self-heal test now also
removes the whole directory and asserts it is recreated.

**Tests executed.** `cargo test -p oag-listend`: 46 unit + 19 end-to-end, 0 failed.

**Commit identifiers:** `oag-listend` `d0f593f`; this entry's commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** The eviction unlink is recorded as synchronous. Then the remaining
imagination carriers.
---

## 2026-11-12 — skip non-regular files when loading the store

**Repository:** `oag-listend`, commit `ea895cf` on `master`; records in `open-agent-graph`.

**The hazard.** `with_directory` read every entry whose name looked like a content hash. A FIFO or
device node named like a hash would make `std::fs::read` block forever during startup; a directory
would error. The startup path had no bound against a special file.

**What changed.** The entry's metadata (which follows symlinks) is taken first; only
`metadata.is_file()` entries are read. A symlink to a regular file still loads; a FIFO, device, or
directory is counted in `skipped`.

**Tests executed.** `cargo test -p oag-listend`: 46 unit + 19 end-to-end, 0 failed (was 45 + 19).
New `a_non_regular_file_named_like_a_hash_is_skipped_not_read`.

**Verified by hand.** A directory named like a hash is skipped. A FIFO named like a hash previously
made `oag-listend --check` hang (killed at 10 s); after the fix it prints
`loaded 0 skipped 1` and exits 0.

**Commit identifiers:** `oag-listend` `ea895cf`; this entry's commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** The eviction unlink is recorded as synchronous. Then the remaining
imagination carriers.
---

## 2026-11-12 — restore a persisted file removed out of band

**Repository:** `oag-listend`, commit `e14f4ea` on `master`; records in `open-agent-graph`.

**The gap.** The directory is the durable copy and memory the cache, but a file deleted out of band
(operator action, or a disk that dropped it) left the in-memory item serving audio that the next
start would not have. Nothing detected the divergence.

**What changed.** `accept`, on finding the hash already present in a persistent store, checks
whether the file still exists and re-persists the item if it does not. The check is a `exists()` on
the duplicate-publish path only, so the read path pays nothing.

**Tests executed.** `cargo test -p oag-listend`: 45 unit + 19 end-to-end, 0 failed (was 44 + 19).
New `a_republish_restores_a_file_deleted_out_of_band` deletes the file, republishes the same bytes,
and asserts the file is restored and a restart still serves the item.

**Commit identifiers:** `oag-listend` `e14f4ea`; this entry's commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** The eviction unlink is recorded as synchronous. Then the remaining
imagination carriers.
---

## 2026-11-12 — the bare-run examples use a creatable socket path

**Repository:** `oag-listend`, commit `13877c3` on `master`; records in `open-agent-graph`.

**The defect.** The README and SPEC "turn it on" examples passed
`--unix /run/oag-listend/audio.sock`. A non-root `cargo run` cannot create `/run/oag-listend`;
only systemd's `RuntimeDirectory=` does. The documented command therefore failed for anyone not
using the unit.

**What changed.** Both examples use `/tmp/oag-listend.sock`, whose parent exists. The systemd unit
keeps `/run/oag-listend/audio.sock` and the SPEC notes that the unit creates it.

**Tests executed.** Unchanged (documentation). Verified by hand: the example command binds the Unix
socket with no preceding `mkdir`.

**Commit identifiers:** `oag-listend` `13877c3`; this entry's commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** The eviction unlink is recorded as synchronous. Then the remaining
imagination carriers.
---

## 2026-11-12 — bound the persistence sidecar read

**Repository:** `oag-listend`, commit `8d79d16` on `master`; records in `open-agent-graph`.

**The gap.** `read_sidecar` read the entire `<hash>.json` file with no size bound. A media type is a
few dozen bytes; a hand-placed multi-megabyte sidecar would be read into memory on every load.

**What changed.** `MAX_SIDECAR_BYTES = 4096`; a sidecar larger than that is ignored (the item still
loads, with no media type) rather than read.

**Tests executed.** `cargo test -p oag-listend`: 44 unit + 19 end-to-end, 0 failed (was 43 + 19).
New `an_oversized_sidecar_is_ignored` writes a 100 KB sidecar and asserts the item loads with
`media_type == None`.

**Commit identifiers:** `oag-listend` `8d79d16`; this entry's commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** The eviction unlink is recorded as synchronous. Then the remaining
imagination carriers.
---

## 2026-11-12 — bound the whole HTTP connection, not just the read

**Repository:** `oag-listend`, commit `f2f2275` on `master`; records in `open-agent-graph`.

**The gap.** `REQUEST_TIMEOUT` wrapped only `read_request`. A client that read the response slowly
could hold one of the 64 session slots for the whole transfer, which is the same slow-client hold
the total was added to prevent on the read side.

**What changed.** `serve_connection` wraps `serve_connection_inner` (read, route, response) in
`REQUEST_TIMEOUT`. The per-syscall read/write bounds are unchanged.

**Tests executed.** `cargo test -p oag-listend`: 43 unit + 19 end-to-end, 0 failed (unchanged).

**Commit identifiers:** `oag-listend` `f2f2275`; this entry's commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** The eviction unlink is recorded as synchronous. Then the remaining
imagination carriers.
---

## 2026-11-12 — refuse to steal a live Unix socket

**Repository:** `oag-listend`, commit `d91ffe9` on `master`; records in `open-agent-graph`.

**The defect.** `UnixCarrier::bind_shared` removed any existing socket file before binding. A
stale socket from a killed daemon must be removed, but a socket that is still being served belongs
to a live daemon: removing it and binding a new one split clients silently between two daemons.

**What changed.** Before removing the file, `bind_shared` tries `UnixStream::connect` on it. If the
connect succeeds, another daemon is live and the bind fails with `AddrInUse` and a clear message.
If it fails (stale socket), the file is removed and the bind proceeds.

**Tests executed.** `cargo test -p oag-listend`: 43 unit + 19 end-to-end, 0 failed (was 43 + 18).
New `a_second_daemon_refuses_to_steal_a_live_socket` binds one carrier, asserts a second bind on
the same path errors, then shuts down cleanly.

**Verified by hand.** With a daemon listening on `/tmp/oag-steal.sock`, a second daemon exited 1
with `cannot bind unix /tmp/oag-steal.sock: "..." is already being served`. A socket left by
`kill -9` was still removed and reused.

**Commit identifiers:** `oag-listend` `d91ffe9`; this entry's commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** The eviction unlink is recorded as synchronous. Then the remaining
imagination carriers.
---

## 2026-11-12 — bound each read and write syscall, not the whole transfer

**Repository:** `oag-listend`, commit `dcc8065` on `master`; records in `open-agent-graph`.

**The defect.** The 10 s read/write timeout was applied around the whole `write_all` / `read_exact`.
A legitimate transfer of the largest item the store accepts (up to ~16 MiB) to a slow client takes
far longer than 10 s, so it was cut off mid-transfer -- the client received a truncated file or a
dropped connection.

**What changed.** `server::read_exact_bounded` and `server::write_all_bounded` loop over the
transfer and apply the timeout to each individual `read`/`write` syscall. A stalled client is still
dropped after the timeout; a slow client that keeps making progress is not. The TCP and Unix
carriers (via `serve_session`), the HTTP carrier's response and interim writes, all use them.

**Tests executed.** `cargo test -p oag-listend`: 43 unit + 18 end-to-end, 0 failed (was 41 + 18).
New `a_slow_but_progressing_read_is_not_cut_off` and `a_slow_but_progressing_write_is_not_cut_off`
move 100 bytes at 10 ms each (~1 s, well past the 50 ms bound) over a `duplex` stream and succeed;
a total bound of 50 ms would have failed.

**Commit identifiers:** `oag-listend` `dcc8065`; this entry's commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** The eviction unlink is recorded as synchronous. Then the remaining
imagination carriers.
---

## 2026-11-12 — reject Ogg video by reading the codec header

**Repository:** `oag-listend`, commit `0d1691f` on `master`; records in `open-agent-graph`.

**The defect.** `AudioFormat::sniff` accepted any `OggS` container. Ogg is a container, not a codec:
the same marker wraps Vorbis, Opus, and FLAC audio, and also Theora video. This is the same class of
false positive as the MP4 major-brand check fixed earlier.

**What changed.** `sniff_ogg_codec` parses the beginning-of-stream page (header type `0x02`,
`page_segments` at offset 26) and reads the codec identification header at the start of its payload.
Vorbis (`vorbis`), Opus (`OpusHead`), FLAC (`FLAC`), Speex, and PCM pass; anything else --
including Theora (`theora`) -- is rejected. The `AudioFormat` docs and the inbound-rule spec
record the rule.

**Tests executed.** `cargo test -p oag-listend`: 41 unit + 18 end-to-end, 0 failed (unchanged count).
The inbound-rule test now builds a real Ogg BOS page and covers Vorbis and Opus accepted and Theora
rejected.

**Verified by hand.** Correctly-built Ogg pages posted over HTTP: `vorbis.ogg` -> 201,
`opus.ogg` -> 201, `theora.ogg` -> 400, an unknown codec -> 400.

**Commit identifiers:** `oag-listend` `0d1691f`; this entry's commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** The eviction unlink is recorded as synchronous. Then the remaining
imagination carriers.
---

## 2026-11-12 — a total bound on a session and an HTTP request

**Repository:** `oag-listend`, commit `f8c19e7` on `master`; records in `open-agent-graph`.

**The gap.** Every network timeout was per read. A client that sent a byte every few seconds could
therefore hold a session open indefinitely, and 64 such clients would exhaust the session cap and
deny service to everyone else. A per-read timeout bounds one read, not the request.

**What changed.** `server::SESSION_TIMEOUT` and `http::REQUEST_TIMEOUT`, both 30 minutes, wrap the
whole session loop and the whole `read_request` (headers and body) respectively. The value is
generous because a legitimate client may be uploading the largest item the store accepts over a
slow link; it is still finite. The Unix carrier inherits the session bound; UDP is a single
datagram and needs no total.

**Tests executed.** `cargo test -p oag-listend`: 41 unit + 18 end-to-end, 0 failed (unchanged).
The bound is not hand-tested at 30 minutes; the per-read timeout it composes with is covered by the
existing tests.

**Commit identifiers:** `oag-listend` `f8c19e7`; this entry's commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** The eviction unlink is recorded as synchronous. Then the remaining
imagination carriers.
---

## 2026-11-12 — the HTTP listing releases the store lock before building its body

**Repository:** `oag-listend`, commit `b3d6534` on `master`; records in `open-agent-graph`.

**What changed.** `GET /audio` copied the page under the lock and then built the response body
while still holding it -- the same head-of-line pattern fixed for `Fetch`, in the one place it
remained. The page is now copied under the lock and the body built after release. Also documented
on `AudioStore::publish` that the caller owns `servable_bound` (the production paths enforce it),
and made `--check` take the lock once instead of twice.

**Tests executed.** `cargo test -p oag-listend`: 41 unit + 18 end-to-end, 0 failed (unchanged).

**Commit identifiers:** `oag-listend` `b3d6534`; this entry's commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** The eviction unlink is recorded as synchronous. Then the remaining
imagination carriers.
---

## 2026-11-12 — `--check`, `--version`, and the service user

**Repository:** `oag-listend`, commit `f2fef88` on `master`; records in `open-agent-graph`.

**What changed.** `--check` opens the store and prints the item count, bytes held, the store bound,
and the effective item bound (`servable_bound`) without binding a socket, so an operator can verify
a store before starting the service. `--version` prints `oag-listend <version> (frame protocol
<n>)`. The systemd unit runs as `oag`; the install instructions never created that user, so a fresh
install would fail to start -- the README and the unit comment now create it.

**Tests executed.** `cargo test -p oag-listend`: 41 unit + 18 end-to-end, 0 failed (unchanged; the
flags are argument handling and an ops path, not protocol).

**Verified by hand.** `--version` printed `oag-listend 0.1.0 (frame protocol 1)`. `--check --dir
/tmp/oag-check --max-bytes 1000000` printed `store ok: 4 items, 4976 bytes (bound 1000000, item
bound 1000000)` and exited 0.

**Commit identifiers:** `oag-listend` `f2fef88`; this entry's commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** The eviction unlink is recorded as synchronous. Then the remaining
imagination carriers.
---

## 2026-11-12 — a fetch no longer holds the store mutex during the byte copy

**Repository:** `oag-listend`, commit `1e8140e` on `master`; records in `open-agent-graph`.

**The defect.** `Request::Fetch` cloned `item.bytes` while the store mutex guard was alive, so a
large fetch stalled every other store operation for the duration of a multi-megabyte copy.

**What changed.** `AudioStore` holds `Arc<AudioItem>`; `serve` returns a cloned `Arc` (a hash lookup
plus an atomic increment under the lock). The TCP, HTTP, and UDP carriers clone the handle, release
the guard, and only then copy the bytes or slice the range. `SPEC.md` documents the mutex scope.

**Tests executed.** `cargo test -p oag-listend`: 41 unit + 18 end-to-end, 0 failed (unchanged; the
store test compares through the Arc). Verified by hand: `push_pull` fetched both sounds
`verified=true` and `GET /` reported `items: 6`, `bytes: 6184 / 1073741824`.

**Commit identifiers:** `oag-listend` `1e8140e` plus the README count correction; this entry's
commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** The eviction unlink is recorded as synchronous. Then the remaining
imagination carriers and a documented one-command run.
---

## 2026-11-12 — the persistence write is asynchronous

**Repository:** `oag-listend`, commits `1185102` and the SPEC follow-up on `master`; records in
`open-agent-graph`.

**The limitation.** `AudioStore::accept` did a blocking `std::fs::write` while holding the async
store mutex. On local disk that is milliseconds, but on a slow share it would stall every carrier's
executor thread, not just the publish that caused it.

**What changed.** `accept` and `persist` are `async` and use `tokio::fs::create_dir_all` /
`tokio::fs::write`. The write still completes before the item is answerable, so a write failure is
still a `Persist` refusal -- the guarantee is unchanged, only the executor is no longer blocked.
Callers in the TCP, HTTP, and UDP carriers and in `publish_generated` await it. The eviction unlink
stays synchronous: it is metadata-only, and detaching it would race the tests that assert the file
is gone immediately after eviction.

**Tests executed.** `cargo test -p oag-listend`: 41 unit + 18 end-to-end, 0 failed (unchanged
count). Six store tests and the persistence listing test moved to `#[tokio::test]`; the concurrent
persistent-publish test (8 tasks x 10, then a restart, all 80 served) still passes.

**Commit identifiers:** `oag-listend` `1185102` plus the SPEC follow-up; this entry's commit
pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** The eviction unlink is recorded as synchronous. Then the remaining
imagination carriers and a documented one-command run.
---

## 2026-11-12 — the store under concurrent publishers

**Repository:** `oag-listend`, commit `33f58c8` on `master`; records in `open-agent-graph`.

**What changed.** Added `tests/concurrency.rs`. The store is a single mutex and the persistent
`accept` path does a blocking `std::fs::write` while holding it; neither had a concurrent test.
Two bounded tests: 16 tasks publishing and fetching 20 sounds each against one in-memory store
(320 distinct hashes, every one verified, all 320 listed), and 8 tasks publishing 10 sounds each to
a persistent store then a restart (all 80 survive on disk and are served).

**Tests executed.** `cargo test -p oag-listend`: 41 unit + 18 end-to-end, 0 failed (was 41 + 16).
The two new tests pass in 0.12 s.

**Commit identifiers:** `oag-listend` `33f58c8`; this entry's commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** The blocking write under the store mutex is recorded as a limitation
(local disk makes it milliseconds; a slow share would stall every carrier). Then the remaining
imagination carriers and a documented one-command run.
---

## 2026-11-12 — cap the item bound by what one frame can carry

**Repository:** `oag-listend`, commit `5c13817` on `master`; records in `open-agent-graph`.

**The defect.** The store bound (`--max-bytes`, default 1 GiB) and the protocol frame bound
(`MAX_FRAME_BYTES`, 16 MiB) were independent. No carrier can move an item near 1 GiB -- a `Fetch`
response at the frame cap would itself exceed the frame cap and be rejected by the client's own
decoder -- yet the directory load used `max_bytes` alone, so a directly-placed large file could
load and then be unfetchable.

**What changed.** `protocol::MAX_SERVABLE_BYTES = MAX_FRAME_BYTES - 256` (a margin for the length
prefix and the response envelope). `servable_bound(max_bytes)` caps the store bound by it; both
`AudioStore::accept` and the `with_directory` load use it, and `TooLarge` reports the effective
bound.

**Tests executed.** `cargo test -p oag-listend`: 41 unit + 16 end-to-end, 0 failed (was 40 + 16).
New `the_item_bound_is_the_store_bound_capped_by_a_frame` covers a small bound unchanged, a huge
bound capped, and the cap sitting below the frame cap. The 16 MiB boundary is tested through the
helper rather than with a 16 MiB fixture.

**Commit identifiers:** `oag-listend` `5c13817`; this entry's commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** Continued hardening; a carrier that needs no external infrastructure.
---

## 2026-11-12 — reject MP4 video by reading the whole ftyp brand list

**Repository:** `oag-listend`, commits `5a3f0c9` and `dc70647` on `master`; records in
`open-agent-graph`.

**The defect.** `AudioFormat::sniff` accepted any ISO BMFF file whose major brand was `isom` or
`mp42`. Both are generic brands that MP4 *video* uses, so a video file was accepted as audio -- the
inbound rule's exact failure case. The major brand alone cannot decide this: ffmpeg-written M4A
often has `isom` as the major brand and `M4A ` among the compatible brands.

**What changed.** The sniff reads the `ftyp` box's major and compatible brands and accepts only an
audio brand (`M4A `, `M4B `, `M4P `). A box size of 0 (to end of file) and a 64-bit largesize are
handled. Also added `every_recognised_format_maps_to_an_audio_media_type`, which an earlier edit had
targeted in the wrong file and therefore never landed.

**Tests executed.** `cargo test -p oag-listend`: 40 unit + 16 end-to-end, 0 failed (was 38 + 16).
New `every_recognised_format_maps_to_an_audio_media_type` and
`an_ftyp_box_that_runs_to_the_end_of_the_file_is_still_sniffed`; the inbound-rule test now covers
an `isom`/`avc1` video rejected and an `isom`+`M4A ` audio accepted.

**Verified by hand.** Correctly-built `ftyp` boxes posted over HTTP: `isom`/`avc1` -> 400,
`isom` with `M4A ` compatible -> 201, `M4A ` major -> 201. (Two earlier hand attempts built the
box size field wrong; the daemon was right each time.)

**Commit identifiers:** `oag-listend` `5a3f0c9`, `dc70647`; this entry's commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** Continued hardening; a carrier that needs no external infrastructure.
---

## 2026-11-12 — store byte usage and If-None-Match validator lists

**Repository:** `oag-listend`, commit `8e045cd` on `master`; records in `open-agent-graph`.

**What changed.** Two small correctness items. `GET /` and the startup line now report the store's
bytes held against its bound (`AudioStore::max_bytes` added), so an operator can see the bound
working rather than discovering it when audio stops being accepted. And `If-None-Match` was compared
as a single string, so a shared cache sending a comma-separated validator list never matched; the
header is now split and any matching validator (or `*`) revalidates.

**Tests executed.** `cargo test -p oag-listend`: 38 unit + 16 end-to-end, 0 failed (was 38 + 15).
Extended `http_caches_immutable_audio_with_an_etag` with a `"stale", "<hash>"` list; new
`http_root_reports_store_usage` asserts `items:` and `bytes: N / max` before and after a publish.

**Verified by hand.** Startup printed `held 4976`; `GET /` printed `items: 4` and
`bytes: 4976 / 1073741824`; a list validator returned `304`.

**Commit identifiers:** `oag-listend` `8e045cd`; this entry's commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** Continued hardening; a carrier that needs no external infrastructure.
---

## 2026-11-12 — a zero listing limit means the default, and a public bind says so

**Repository:** `oag-listend`, commit `12bad84` on `master`; records in `open-agent-graph`.

**What changed.** Two operator-safety details. `List { limit: 0 }` was clamped to 1, so a client
asking for the default page got one item; `protocol::normalize_limit` now resolves zero to
`DEFAULT_LIST_LIMIT`, capped by the carrier's maximum. And the default bind is `0.0.0.0` -- every
interface -- so a carrier binding a non-loopback address now prints a note saying so, rather than
leaving the operator to discover it later. A loopback bind prints nothing.

**Tests executed.** `cargo test -p oag-listend`: 38 unit + 15 end-to-end, 0 failed (was 37 + 15).
New `a_zero_limit_means_the_default_not_one_item` covers zero, an explicit limit, an over-cap
limit, and a carrier whose cap is below the default.

**Verified by hand.** A loopback bind printed no note; `0.0.0.0` printed
"note: 0.0.0.0:7450 is not a loopback address -- this serves audio to that interface"; and
`GET /audio?limit=0` returned the default page (4 lines).

**Commit identifiers:** `oag-listend` `12bad84`; this entry's commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** Continued hardening; a carrier that needs no external infrastructure.
---

## 2026-11-12 — cache immutable audio with an ETag

**Repository:** `oag-listend`, commit `e6d0434` on `master`; records in `open-agent-graph`.

**What changed.** A served audio item carries `ETag: "<content hash>"` and
`Cache-Control: public, max-age=31536000, immutable`. A request with a matching `If-None-Match`
(including `*`) is answered 304 with the ETag and no body; a non-matching validator is served
normally. `HttpRequest` parses `If-None-Match`; `HttpResponse` gained an `extra_headers` list so
future metadata headers do not each need a field. `GET /` and the listing do not get an ETag: they
change as the store changes.

**Tests executed.** `cargo test -p oag-listend`: 37 unit + 15 end-to-end, 0 failed (was 37 + 14).
New `http_caches_immutable_audio_with_an_etag`: 200 carries both headers, 304 on a matching and on
`*` validators with an empty body, 200 on a stale validator, and ETag on HEAD.

**Verified by hand with curl.** `GET` returned `ETag: "<hash>"` and
`Cache-Control: public, max-age=31536000, immutable`; revalidation with that ETag returned
`304 Not Modified`; a stale validator returned 200.

**Commit identifiers:** `oag-listend` `e6d0434`; this entry's commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** Continued hardening; a carrier that needs no external infrastructure.
---

## 2026-11-12 — the HTTP listing is paginated and query-string aware

**Repository:** `oag-listend`, commits `3eaf9ba` and the SPEC rewrite on `master`; records in
`open-agent-graph`.

**The defect.** `route` matched on the whole request path, so a query string was part of it:
`GET /audio?offset=0` fell through to 404. Any client that added a query got an error.

**What changed.** `route` splits the path from the query before matching. `GET /audio` is
paginated with `?offset=&limit=` (clamped to `MAX_LIST_LIMIT`) and reports `X-Total-Count` and
`X-Offset`, matching the native paginated listing. An unparsable `limit` falls back to the default;
an `offset` past the end is clamped. The HTTP carrier spec paragraph was also rewritten, since it
still described an earlier surface.

**Tests executed.** `cargo test -p oag-listend`: 37 unit + 14 end-to-end, 0 failed (was 37 + 13).
New `http_query_strings_and_paginated_listing`: both pages, an offset past the end, an unparsable
limit, a query on an audio path serving, and an unknown route with a query still 404.

**Verified by hand with curl.** `?offset=0&limit=2` → 200 with `X-Total-Count: 4`, `X-Offset: 0`,
2 lines; `?offset=2&limit=2` → `X-Offset: 2`; `/audio/<hash>?x=1` → 200; `/nope?x=1` → 404.

**Commit identifiers:** `oag-listend` `3eaf9ba` plus the SPEC follow-up; this entry's commit
pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** Continued hardening; a carrier that needs no external infrastructure.
---

## 2026-11-12 — paginate the listing so it always fits the frame cap

**Repository:** `oag-listend`, commit `4e505d0` on `master`; records in `open-agent-graph`.

**The defect.** `Request::List` took no parameters and the response carried every hash in one
frame. A store with more than about a quarter of a million items would make the daemon send a frame
its own client has to reject as `TooLong` -- the daemon violating its own protocol bound.

**What changed.** `Request::List { offset, limit }`; `Response::Listing { content_hashes, total,
offset }`. Stream carriers clamp the page to `protocol::MAX_LIST_LIMIT` (10,000); UDP clamps to
`MAX_DATAGRAM_LIST_LIMIT` (900, about 59 KiB of body) so the page always fits a datagram. A client
can walk the store with the reported `total`.

**Tests executed.** `cargo test -p oag-listend`: 37 unit + 13 end-to-end, 0 failed (was 36 + 13).
New `a_listing_is_a_bounded_page_with_a_total` covers pages at offset 0, 4, and 5, an over-cap limit
being clamped, and the sorted order. The protocol round-trip tests cover the new fields.

**Verified by hand.** A raw native client walked the four startup sounds in two pages of two:
`offset=0 total=4 page=2`, `offset=2 total=4 page=2`, walked 4, unique 4, sorted.

**Commit identifiers:** `oag-listend` `4e505d0`; this entry's commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** Continued hardening; a carrier that needs no external infrastructure.
---

## 2026-11-12 — the load report counts what is held, not what was read

**Repository:** `oag-listend`, commit `9fe0948` on `master`; records in `open-agent-graph`.

**The defect.** `LoadReport::loaded` was incremented for every entry accepted during a load. A
load that exceeds `--max-bytes` evicts older entries as later ones are inserted, so `loaded`
overstated what the daemon actually holds -- the report said 4 while the store served 2.

**What changed.** `LoadReport` gained `evicted`. After the load loop, `loaded` is set to
`store.len()` (what is held) and `evicted` to the number accepted but pushed out. `skipped` is
unchanged (not read, or rejected). `main.rs` prints all three.

**Tests executed.** `cargo test -p oag-listend`: 36 unit + 13 end-to-end, 0 failed (was 35 + 13).
New `the_load_report_counts_only_what_is_held` writes three WAVs straight to the directory (so the
store has not already deleted the evicted file), opens with a bound that fits two, and asserts
`loaded == 2`, `evicted == 1`, and that one hash is no longer served.

**Verified by hand.** Generated four sounds with `--dir`, then reloaded with `--max-bytes 3000`:
`store /tmp/oag-evict-hand: loaded 2 skipped 0 evicted 2`.

**Commit identifiers:** `oag-listend` `9fe0948`; this entry's commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** Continued hardening; a carrier that needs no external infrastructure.
---

## 2026-11-12 — startup generation seeds an empty store, and reports refusals

**Repository:** `oag-listend`, commits `e0d5004` and the SPEC follow-up on `master`; records in
`open-agent-graph`.

**The defect.** `publish_generated` ran on every start. With `--dir` and the default clock seed,
each restart generated four *new* sounds (different hashes), so the generated set grew without
bound and, once `--max-bytes` was reached, evicted the operator's own audio. The directive's
rationale for the generated set is "so a client that only fetches still receives sound" -- a seed
for an empty store, not a recurring addition.

**What changed.** `ListendServer::publish_generated` returns a `GeneratedReport { accepted, rejected,
skipped_non_empty }` and returns early when the store is not empty. A generated sound the store
refuses is reported instead of being dropped. `main.rs` prints accepted hashes, rejection reasons,
and the skip.

**Tests executed.** `cargo test -p oag-listend`: 35 unit + 13 end-to-end, 0 failed (was 33 + 13).
New `generation_seeds_only_an_empty_store` (a second call adds nothing and sets the skip flag) and
`a_generated_sound_the_store_refuses_is_reported` (all four refusals reported).

**Verified by hand.** Three restarts with `--dir`: run 1 generated 4, runs 2 and 3 printed "store
already holds audio" and the HTTP listing stayed at 4 items. With `--max-bytes 5`, all four
generated sounds were reported as `audio is 1484 bytes, larger than the 5 byte store bound`.

**Commit identifiers:** `oag-listend` `e0d5004` plus the SPEC follow-up; this entry's commit
pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** Continued hardening; a carrier that needs no external infrastructure.
---

## 2026-11-12 — oag-listend's hello says what the daemon is

**Repository:** `oag-listend`, commit `f1c65f0` on `master`; records in `open-agent-graph`.

**What changed.** `Response::Hello` carried only `item_count`, so a client could not tell what it
had reached or whether it spoke the same protocol. It now carries `name` (`oag-listend`),
`protocol_version` (1), and `item_count`. The decode keeps an unrecognised name rather than
rejecting it, so a future daemon is recognisable instead of a protocol error. `GET /` reports the
protocol version as well. `protocol::DAEMON_NAME` and `protocol::PROTOCOL_VERSION` are the single
source for both.

**Tests executed.** `cargo test -p oag-listend`: 33 unit + 13 end-to-end, 0 failed (was 32 + 13).
New `hello_carries_the_daemon_identity_and_a_future_name_still_decodes` round-trips the real hello
and a hypothetical `oag-listend-2` at version 2.

**Verified by hand.** `push_pull` printed `Hello { name: "oag-listend", protocol_version: 1,
item_count: 4 }` over TCP and `... item_count: 5 }` over UDP.

**Commit identifiers:** `oag-listend` `f1c65f0`; this entry's commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** Continued hardening; a carrier that needs no external infrastructure.
---

## 2026-11-12 — oag-listend allows a cross-origin browser client

**Repository:** `oag-listend`, commit `a2188fe` on `master`; records in `open-agent-graph`.

**What changed.** The HTTP carrier emits `Access-Control-Allow-Origin: *` on every response and
answers `OPTIONS` with `Access-Control-Allow-Methods: GET, HEAD, POST, OPTIONS`,
`Access-Control-Allow-Headers: content-type, range`, and `Access-Control-Max-Age: 3600`. A page on
another origin could embed the audio but could not `fetch()` it: the browser blocks the request
without the header, and the preflight previously got a 404. The service holds no credentials, so
`*` is the honest allowance.

**Tests executed.** `cargo test -p oag-listend`: 32 unit + 13 end-to-end, 0 failed (was 32 + 12).
New `http_allows_a_cross_origin_browser_client`: the preflight returns 204 with the three headers
and no body, and `GET /`, `/audio`, and `/audio/{hash}` all carry `Access-Control-Allow-Origin: *`.

**Verified by hand with curl.** `OPTIONS /audio` returned 204 with the three headers; a GET with
`Origin: http://example.com` returned 200 with `Access-Control-Allow-Origin: *`.

**Also recorded.** IPFS and Reticulum are deliberately not built: IPFS needs a Kubo node (external
infrastructure) or an embedded libp2p node (heavyweight), and Reticulum needs a physical interface.
Neither can be tested against a real network from this repository, so the honest status is "not
built", not "built and untested".

**Commit identifiers:** `oag-listend` `a2188fe`; this entry's commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** A carrier that can be tested from this repository and does not need
external infrastructure; then continued hardening.
---

## 2026-11-12 — oag-listend runs as a service and stops cleanly

**Repository:** `oag-listend`, commit `995c9b5` on `master`; records in `open-agent-graph`.

**What changed.** `main.rs` handles SIGTERM as well as SIGINT, so `systemctl stop` and `kill` take
the clean path: the accept loops return, the Unix socket is removed, and the process exits 0.
Previously only SIGINT was handled, so a service stop bypassed cleanup and left the socket for the
next start to remove. Added `scripts/oag-listend.service` (all four carriers, persistent store,
dedicated user, `Restart=on-failure`, `StateDirectory`/`RuntimeDirectory`) and a README section
with the exact install and start commands.

**Tests executed.** `cargo test -p oag-listend`: 32 unit + 12 end-to-end, 0 failed (unchanged; this
is signal handling and an operational unit, not a protocol change).

**Verified by hand.** Started the daemon with `--unix`, sent SIGTERM: exit code 0, socket removed,
"oag-listend shutting down" printed. Repeated with SIGINT sent from a Python-spawned process (a
background job in a non-interactive shell inherits SIG_IGN for SIGINT, which is a harness artifact,
not the daemon): exit code 0, socket removed.

**Commit identifiers:** `oag-listend` `995c9b5`; this entry's commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** The remaining imagination carriers (IPFS, Reticulum), each reusing
`AudioStore::accept` and adding no operations.
---

## 2026-11-12 — oag-listend's listing order is reproducible across a restart

**Repository:** `oag-listend`, commits `0f5b83f` and `41b30f3` on `master`; records in
`open-agent-graph`.

**What changed.** `AudioStore::hashes()` returned insertion order, which is reproducible within a
run but not across one: a restart rebuilds the store in directory-iteration order, which is
filesystem-dependent. The listing is now sorted by content hash, so the walk a client sees is the
same always. `SPEC.md` documents the order as part of the contract.

**Tests executed.** `cargo test -p oag-listend`: 32 unit + 12 end-to-end, 0 failed (was 32 + 11).
The server listing test now asserts a sorted listing rather than insertion order, and a new
persistence test publishes four sounds, restarts, and asserts the walk is identical.

**Verified by hand.** `GET /audio` before and after a restart produced the same four hashes in the
same (sorted) order (`cmp` clean).

**Commit identifiers:** `oag-listend` `0f5b83f`, `41b30f3` (the second corrects a test count in the
README: 32 unit, not 33 -- the listing test was renamed, not added). This entry's commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** The remaining imagination carriers (IPFS, Reticulum) and a documented
one-command way to run the daemon.
---

## 2026-11-12 — oag-listend serves over a Unix domain socket

**Repository:** `oag-listend`, commit `a1619ab` on `master`; records in `open-agent-graph`.

**What changed.** `unix.rs`: `--unix PATH` serves the same frame protocol over a filesystem socket.
`serve_session`, `read_frame`, and `write_response` in `server.rs` are now generic over
`AsyncRead + AsyncWrite`, so the Unix carrier shares the TCP session loop -- same protocol, same
bounds, same inbound rule -- by construction rather than by copying. A stale socket file from a
killed daemon is removed on bind and on shutdown. `main.rs` gained `--unix PATH`.

**Tests executed.** `cargo test -p oag-listend`: 32 unit + 11 end-to-end, 0 failed (was 32 + 9).
New `a_unix_socket_serves_the_same_audio` (hello, publish, fetch, non-audio rejection, socket file
removed on shutdown) and `the_unix_and_tcp_carriers_serve_one_store` (publish on the socket, fetch
over TCP).

**Verified by hand.** Over AF_UNIX: hello count, publish returned a 64-character hash, fetch
returned the same bytes, HTML was rejected. A daemon restarted over a socket left by `kill -9`
bound successfully. (The first hand attempt failed because my client declared a 10-byte type for
the 9-byte string `audio/wav`; the server was correct to reject it.)

**Commit identifiers:** `oag-listend` `a1619ab`; this entry's commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** The listing order is insertion order within a run but
directory-iteration order after a restart, so it is not reproducible across a restart; sorting by
hash would fix it. Then the remaining imagination carriers (IPFS, Reticulum) and a documented
one-command way to run the daemon.
---

## 2026-11-12 — oag-listend enforces its store bound on the way in

**Repository:** `oag-listend`, commit `1ef956d` on `master`; records in `open-agent-graph`.

**What changed.** Two unbounded paths on the store. `AudioStore::accept` now refuses bytes larger
than `max_bytes` with a new `ListendError::TooLarge { len, max }` instead of writing them and
immediately evicting them. `AudioStore::with_directory` reads the entry's metadata first and skips
a file larger than `max_bytes` without reading it, counting it in `LoadReport::skipped`. A bound
checked only after the read or write is not a bound.

**Tests executed.** `cargo test -p oag-listend`: 32 unit + 9 end-to-end, 0 failed (was 30 + 9).
New `an_item_larger_than_the_store_bound_is_refused` and
`a_file_larger_than_the_store_bound_is_skipped_without_being_read` (the latter also confirms a
small item in the same directory still loads).

**Verified by hand.** A 4044-byte WAV posted to a daemon with `--max-bytes 100` returned
`400` with `audio is 4044 bytes, larger than the 100 byte store bound`; a small WAV was accepted
with `201` and a 64-character hash.

**Commit identifiers:** `oag-listend` `1ef956d`; this entry's commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** The remaining imagination carriers (IPFS, Reticulum), then a
documented one-command way to run the daemon.
---

## 2026-11-12 — oag-listend's HTTP carrier is self-describing

**Repository:** `oag-listend`, commit `7fbb86a` on `master`; records in `open-agent-graph`.

**What changed.** `GET /audio` lists the content hashes one per line -- the list operation, on
HTTP -- so a client that only fetches can discover what exists without already knowing a hash;
`GET /` now points at it. A `POST` carrying `Expect: 100-continue` is answered with
`HTTP/1.1 100 Continue` before the body is read, so a client does not commit a large upload to a
server that might refuse it.

**Tests executed.** `cargo test -p oag-listend`: 30 unit + 9 end-to-end, 0 failed (was 30 + 8).
New `http_lists_the_audio_and_honours_expect_continue`: `GET /audio` returns exactly the published
hash and a newline, `GET /` mentions it, and a raw `POST` with `Expect: 100-continue` reads
`HTTP/1.1 100 Continue

` before sending the body, then gets 201.

**Verified by hand.** `curl http://…/audio` listed the four startup hashes and, after a POST, five
lines. A raw socket POST with `Expect: 100-continue` got `HTTP/1.1 100 Continue` then
`HTTP/1.1 201 Created` whose body was the 64-character hash. (curl no longer sends `Expect`
automatically, so the interim response is exercised by the raw request and the test.)

**Commit identifiers:** `oag-listend` `7fbb86a`; this entry's commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** The remaining imagination carriers (IPFS, Reticulum), then a real
listening address and a documented one-command way to run the daemon.
---

## 2026-11-12 — oag-listend's HTTP carrier streams: ranges, HEAD, and the type the bytes imply

**Repository:** `oag-listend`, commit `92395fa` on `master`; records in `open-agent-graph`.

**What changed.** The HTTP carrier became a real audio endpoint rather than a file dump.
`Range: bytes=...` is parsed (first-byte, open-ended, and suffix forms) and answered 206 with
`Content-Range`, or 416 with `Content-Range: bytes */len` when unsatisfiable; multiple ranges are
ignored rather than answered with a multipart body. `HEAD` answers with the entity's
`Content-Length` and no body (the length is carried explicitly, since an emptied body would
otherwise advertise zero). The served `Content-Type` now comes from the format sniff, not from a
declared type: a WAV posted as `application/x-www-form-urlencoded` was served as that and a browser
would not play it. `AudioFormat::media_type()` maps each recognised format to its canonical audio
MIME type.

**Tests executed.** `cargo test -p oag-listend`: 30 unit + 8 end-to-end, 0 failed (was 30 + 6).
`http_range_requests_and_head` covers no-range (200 + `Accept-Ranges: bytes`), `bytes=0-9`,
`bytes=-10`, `bytes=100-`, an unsatisfiable start (416), multiple ranges (ignored -> 200), and
HEAD (Content-Length = entity length, empty body). `the_served_content_type_comes_from_the_bytes`
posts a WAV labelled `application/x-www-form-urlencoded` and asserts `content-type: audio/wav`.

**Verified by hand with curl** against a running daemon: `HEAD` returned `Content-Type: audio/wav`,
`Content-Length: 4044`, no body; `-r 0-9` returned 10 bytes matching the source with
`Content-Range: bytes 0-9/4044`; `-r -10` matched the tail; `-r 999999-` returned 416 with
`Content-Range: bytes */4044`; a WAV posted without a proper content type is served as `audio/wav`.

**Commit identifiers:** `oag-listend` `92395fa`; this entry's commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** The remaining imagination carriers (IPFS, Reticulum), then a real
listening address and a documented one-command way to run the daemon.
---

## 2026-11-12 — oag-listend persists: a restart serves what was accepted

**Repository:** `oag-listend`, commit `0341be5` on `master`; records in `open-agent-graph`.

**What changed.** `AudioStore` gained an optional backing directory. `with_directory(dir,
max_bytes)` loads `<dir>/<hash>` (audio) and `<dir>/<hash>.json` (declared media type), re-checking
every entry on the way in: the name must be the BLAKE3 of the bytes and the bytes must still sniff
as audio. The load is reported as `LoadReport { loaded, skipped }`, never silent. `accept` writes
the item to `<dir>/<hash>` *before* it is answerable, so a write failure is the new
`ListendError::Persist` refusal rather than a silent loss. Eviction deletes the file, so the
directory and the store agree and `--max-bytes` bounds disk as well as memory. `main.rs` gained
`--dir PATH` and prints the load report.

**Tests executed.** `cargo test -p oag-listend`: 30 unit + 6 end-to-end, 0 failed (was 25 + 5). New
unit tests: reopen serves the same bytes and media type; a file that is not its hash is skipped; a
non-audio file in the directory is skipped; eviction deletes the file; a persist failure refuses
the inbound. The end-to-end `audio_accepted_before_a_shutdown_is_served_after_it` binds, publishes
over TCP, stops, reopens the directory, and serves the same bytes on a new listener.

**Verified by hand.** Posted a 1644-byte WAV over HTTP, stopped the daemon, restarted with
`--dir /tmp/oag-store-hand`: `loaded 5 skipped 0`, and `curl` returned the same 1644 bytes (`cmp`
clean).

**Commit identifiers:** `oag-listend` `0341be5`; this entry's commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** The remaining imagination carriers (IPFS, Reticulum), then a real
listening address and a documented one-command way to run the daemon. The format sniff is still a
closed list that grows only when a real carrier needs a format.
---

## 2026-11-12 — oag-listend adds the UDP carrier (the imagination seam's datagram transport)

**Repository:** `oag-listend`, commit `8b669e3` on `master`; records in `open-agent-graph`.

**What changed.** `udp.rs`: one datagram is one request, the reply returns to the sender. The frame
codec is reused unchanged, so the four operations, the 16 MiB frame cap, and the inbound rule are
identical to TCP; a datagram that is not the protocol gets a typed rejection rather than silence,
and a fetch whose response exceeds 65,507 bytes is answered with a rejection rather than a silent
truncation. `wav::generate_n` exposes a chosen sample count so a carrier bound can be tested with a
real container. `examples/push_pull.rs` now works over TCP (`--addr`) and UDP (`--udp --addr`); its
TCP path had double-decoded the frame, which is fixed. `main.rs` gained `--udp ADDR`.

**Tests executed.** `cargo test -p oag-listend`: 25 unit + 5 end-to-end, 0 failed (was 20 + 4).
`udp_carries_the_same_audio` runs a real socket: hello, publish a generated tone, fetch it back,
refuse HTML, list. Unit tests prove a tone round-trips, a 40,000-sample recording is refused as too
large for a datagram, and non-audio and non-protocol datagrams are rejected without storing.
Verified by hand: `push_pull` over TCP and over UDP both fetched the tone and noise with
`verified=true` and both refused HTML labelled `audio/wav`.

**Commit identifiers:** `oag-listend` `8b669e3`; this entry's commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** The store is still in-memory: a restart loses every sound. Persistence
(content-addressed files under a directory, loaded at startup) is the top priority. The remaining
imagination carriers (IPFS, Reticulum) come after.
---

## 2026-11-12 — oag-listend adds HTTP, and a browser can now reach the audio

**Repository:** `oag-listend`, commit `57c4c24` on `master`; records in `open-agent-graph`.

**What changed.** `http.rs`: a hand-rolled HTTP/1.1 request/response loop over the same
`Arc<Mutex<AudioStore>>` as the native carrier. `GET /` says what it is and how much audio it
holds; `GET /audio/{hash}` serves the bytes with the item's media type (404 if unknown); `POST
/audio` runs the body through `AudioStore::accept` (201 with the content hash, 400 if the bytes
are not audio); anything else is 404. No framework, no keep-alive -- one request, one response,
then close -- with a 16 KiB head cap, the same 16 MiB body cap as the native frame, 10 s bounds,
and a 64-session semaphore. `ListendServer::bind_shared` / `HttpCarrier::bind_shared` bind both
carriers around one store, and `main.rs` gained `--http ADDR`.

**Tests executed.** `cargo test -p oag-listend`: 20 unit + 4 end-to-end, 0 failed (was 20 + 2).
`http_serves_audio_and_refuses_the_rest` posts a generated WAV, fetches it back byte-identical with
`Content-Type: audio/wav`, refuses HTML labelled `audio/wav`, and 404s an unknown hash and an
unknown route. `the_two_carriers_serve_the_same_audio` publishes over HTTP and fetches over the
native protocol, proving one store. Verified by hand with curl against a running daemon: a
1644-byte WAV returned byte-identical (`cmp` clean), an empty body and HTML labelled `audio/wav`
were both refused with "input is not audio", and `GET /` reported 5 items.

**Commit identifiers:** `oag-listend` `57c4c24`; this entry's commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** The store is in-memory, so a restart loses the audio; persistence is
the honest next step after the next carrier. The next carrier is whatever the imagination layer's
`Transport` seam offers (UDP, IPFS, Reticulum), reusing `AudioStore::accept` and adding no
operations.
---

## 2026-11-12 — oag-listend listens: TCP carrier, seeded sounds, and a daemon you can turn on

**Repository:** `oag-listend` (https://github.com/webdawg/oag-listend), commit `a97c71f` on `master`;
cross-project records in `open-agent-graph`.

**What changed.** The minimum bar in `DIRECTIVE.md` is green. New modules:
`protocol.rs` (length-prefixed frames, four operations only -- hello, publish, fetch, list -- a
16 MiB frame cap enforced before the body is allocated, typed `WireError`s for a bad hash or an
unknown operation), `wav.rs` (seeded 16-bit PCM WAV: silence, tone, noise, via SplitMix64; the
header and the determinism are tested), `server.rs` (`ListendServer`: bind, accept, 10s read and
write timeouts, 64-session semaphore, watch-channel shutdown; every publish through
`AudioStore::accept`), `main.rs` (the `oag-listend` binary with `--listen`, `--seed`, `--max-bytes`,
`--no-generate`), `examples/push_pull.rs` (operator client), and `tests/back_and_forth.rs`.
`AudioStore` gained `hashes()` in insertion order, and the startup generation goes through
`AudioStore::accept` too, so generated sound is not a way around the inbound rule.

**Tests executed.** `cargo test -p oag-listend`: 20 unit + 2 end-to-end, 0 failed. The acceptance
test pushes a generated tone, fetches it back, verifies the BLAKE3, lists it, refuses five
non-audio inputs however labelled (HTML, JSON, PNG, empty labelled `audio/wav`, text labelled
`audio/mpeg`) and observes not-found; the second test proves a publish on one connection is served
on another. Both are bounded at 10s. Also verified by hand: daemon on `127.0.0.1:7199`, then
`push_pull` fetched the tone and the noise with `verified=true` and printed
`Rejected { reason: "input is not audio; only audio is accepted" }` for HTML labelled `audio/wav`.

**Defect fixed along the way.** `target/` had been swept into the founding commit; it is now
untracked (668 files removed from the index) and ignored.

**Commit identifiers:** `oag-listend` `a97c71f`; this entry's commit pending.
**Push status:** `oag-listend` pushed to `master`; this entry pending.
**Remaining defects / next.** The HTTP carrier (`GET /audio/{hash}`, `POST /audio`) is next; it
must reuse `AudioStore::accept` and not grow the operation set. The format sniff is a closed list
that grows only when a real carrier needs a format.
---

## 2026-11-12 — test the budget sync scheduler

**Repository:** `oag-mesh`
**Files modified:** `crates/oag-mesh/src/transport.rs`.

**Gap.** `spawn_periodic_sync_with_budget` was added in the tick-bound work without a test. It is
a thin wrapper over `sync_with_all_peers_bounded` (whose bound is tested by
`a_bounded_tick_stops_at_the_budget_instead_of_waiting_out_every_peer`), so the new test proves
the wrapper is still a *scheduler* -- the same periodic behaviour as `spawn_periodic_sync`, not a
behaviour change behind a new name.

**Tests executed.** `cargo test -p oag-mesh`: 197 passed, 0 failed (was 196). New test
`spawn_periodic_sync_with_budget_still_syncs`.

**Commit identifiers:** `1c289d0`.
**Push status:** pushed.
**Remaining defects.** None known for this function; the broader mesh priorities are unchanged.

---

## 2026-11-12 — the data-pad gather fans out with a bounded concurrency

**Repository:** `oag-imagination-random-data-compute`
**Files modified:** `crates/oag-imagination/src/socket.rs`, `spec/03-transports.md`.

**Gap.** `gather_file_with` asked each node for its pad sequentially. A file split across the
default `DEFAULT_SHARD_COUNT` (1000) pads -- the design case -- cost a thousand sequential
round-trips. The gather is all-or-nothing (`datapad::gather` needs every shard), so fanning out is
the only lever.

**Fix.** `MAX_CONCURRENT_GATHERS` (32) caps how many peers are asked at once, via a semaphore over
a `JoinSet`. `datapad::gather` sorts by the manifest's `pad_ids`, so the fan-out does not reorder
the shards. `gather_file_with` takes the carrier as an `Arc` so the spawned requests can share it.

**Tests executed.** `cargo test -p oag-imagination`: 55 passed, 0 failed (was 54). New test
`gathering_fans_out_with_a_bounded_concurrency` uses a tracking carrier to assert both that every
shard still arrives and that the peak concurrency is exactly the bound (not a timing race).

**Commit identifiers:** `40d4fa8`.
**Push status:** pushed.
**Remaining defects.** The bound (32) is a fixed constant, not configurable. A manifest naming
more nodes than shards still asks every node (the loop is over `nodes`, not `manifest.pad_ids`),
which is the caller's contract rather than a defect but is worth noting.

---

## 2026-11-12 — every outbound HTTP client in the workspace is now bounded

**Repository:** `open-agent-graph`
**Files modified:** `crates/oag-api/src/state.rs`, `crates/oag-api/src/lib.rs`,
`crates/oag-cli/src/commands.rs`, `crates/oag-embeddings/src/provider.rs`,
`crates/oag-crawler/src/llm_extract.rs`, `crates/oag-ipfs/src/provider.rs`,
`spec/18-rate-limiting-and-resource-limits.md`.

**Gap.** The peer-to-peer request paths were unbounded: the API's gateway quorum fan-out and
relayed transfer used a plain `reqwest::Client::new()`, and the CLI's `fetch` command did too. A
peer that accepted but never responded would hold a gateway fan-out or a relayed transfer open
indefinitely. The operator-configured integrations (LLM extractor, embeddings, Kubo) were also
unbounded, and `spec/18` recorded that as a deliberate carve-out.

**Why the carve-out did not hold.** A timeout is not a trust decision -- it bounds the wait
regardless of who configured the integration. And the embeddings provider sits on the search
path, where a hung model server would hold a search open. `spec/18`'s reasoning was about the
*attack surface* (the p2p paths are reachable by an admitted peer; the integrations are not), not
about whether a bound is needed.

**Fix.** Named connect and request timeouts on every outbound client:

| client | constants | values |
|---|---|---|
| API gateway / relayed transfer | `API_CONNECT_TIMEOUT` / `API_REQUEST_TIMEOUT` | 10s / 120s |
| sync | `SYNC_CONNECT_TIMEOUT` / `SYNC_REQUEST_TIMEOUT` | 10s / 120s |
| LLM extractor | `EXTRACTOR_CONNECT_TIMEOUT` / `EXTRACTOR_REQUEST_TIMEOUT` | 10s / 120s |
| embeddings | `EMBEDDINGS_CONNECT_TIMEOUT` / `EMBEDDINGS_REQUEST_TIMEOUT` | 10s / 60s |
| Kubo IPFS | `KUBO_CONNECT_TIMEOUT` / `KUBO_REQUEST_TIMEOUT` | 10s / 60s |

The embeddings provider gains a test-only `with_timeouts` constructor (mirroring `oag_sync`'s)
and a test proving a non-responding server is abandoned at the bound.

**Tests executed.** `cargo test -p oag-api`: 65 passed. `-p oag-crawler`: 75 passed.
`-p oag-ipfs`: 15 passed. `-p oag-embeddings`: 12 passed (was 11). `cargo build -p oag-cli`: clean.

**Commit identifiers:** `2c2e23a`.
**Push status:** pushed.
**Remaining defects.** The timeouts are fixed constants, not configurable per deployment. An
operator running a genuinely slow self-hosted model may need a longer embeddings timeout; that
would need a config field, which is not built here. The `serve.rs` test client is still unbounded,
which is correct (a test client is bounded by the test harness).

---

## 2026-11-12 — classify the mesh's heads failure so the fallback is only paid when it can help

**Repository:** `oag-mesh`
**Files modified:** `crates/oag-mesh/src/transport.rs`.

**Gap.** A pull whose heads request fails falls back to a full five-store pull (`spec/10`).
That fallback exists for a peer whose heads endpoint is broken while its stores serve. For an
unreachable peer it is six timeouts of pure waste -- measured at 1.8s in the tick-bound work's
test (the heads request plus five store requests, at the test client's 300ms).

**Fix.** `PullError` already distinguishes the two cases: `Request` (transport) versus `Rejected`
(HTTP status). A `Request` that is a connect error or a timeout now records the failure and
returns; a `Rejected` (or a non-connect transport error, e.g. a malformed response) still pays the
fallback, because the peer answered and its stores may serve.

**Tests executed.** `cargo test -p oag-mesh`: 196 passed, 0 failed (was 195). New tests:
`an_unreachable_peer_does_not_pay_the_five_store_fallback` (counts connections through an
`AtomicUsize`: one, not six) and `a_rejected_heads_request_still_pays_the_five_store_fallback`
(the complementary case, identified by the fallback recording no heads). The full suite dropped
from 30.6s to 28.7s on the previous commit and the new unreachable test runs in 0.26s.

**Commit identifiers:** `cb05d26` (the classification), `e1bc3d8` (the complementary test).
**Push status:** pushed.
**Remaining defects.** The classification is on `reqwest::Error::is_connect()` / `is_timeout()`;
a slow-but-alive peer whose heads request times out will still skip the fallback (its stores would
likely time out too, so the skip is defensible, but it is a judgement rather than a measurement).

---

## 2026-11-12 — bound the mesh's anti-entropy tick, not just each pull

**Repository:** `oag-mesh`
**Files modified:** `crates/oag-mesh/src/transport.rs`, `crates/oag-mesh/src/lib.rs`,
`spec/10-anti-entropy.md`.

**Gap.** Each pull is bounded (`HTTP_CONNECT_TIMEOUT` + `HTTP_REQUEST_TIMEOUT`), but a tick is
a loop over every peer, so unreachable peers cost their timeout each. A pull against a black-holed
peer costs the heads request *plus* the five-store fallback -- six timeouts, measured at 1.8s in
the new test. `link_quality` asks the live peers first, so their data arrives early, but the live
peers' next refresh waits for the dead ones to be waited out, and the next tick cannot start until
this one finishes.

**Fix.** `sync_with_all_peers_bounded(state, budget)` and
`spawn_periodic_sync_with_budget(state, interval, tick_budget)`. The budget is checked between
pulls, so the pull in flight completes and the rest are left for the next tick. This bounds the
tick rather than dropping anyone -- the skipped peers are retried, so `OPEN_QUESTIONS.md`'s "a
peer that is down is asked last, never not asked" still holds. The unbounded forms remain.

**Tests executed.** `cargo test -p oag-mesh`: 194 passed, 0 failed (was 193). New test
`a_bounded_tick_stops_at_the_budget_instead_of_waiting_out_every_peer`, which counts attempts
through the `link_quality` ledger rather than the clock so it is not a timing race.

**Commit identifiers:** `a621322`.
**Push status:** pushed.
**Remaining defects.** The budget is checked between pulls, not during one, so a single slow
pull can exceed it -- bounding that would mean cancelling an in-flight pull. The per-peer fallback
(heads failure -> five-store full pull) is the larger cost and is a deliberate design choice
(`spec/10`); changing it is not done here.

---

## 2026-11-12 — EXP-026/027: the recall is base-dependent, and the multi-probe stabilises it

**Repository:** `oag-collisions`
**Files modified:** `crates/oag-collisions/src/lib.rs`,
`crates/oag-collisions/examples/dual_hash.rs`,
`crates/oag-collisions/examples/recall_variance.rs`, `EXPERIMENTS.md`, `README.md`.

**EXP-026 (negative result).** A second independent hash (a domain-separated BLAKE3) was tested as
a recall mechanism: two hashes of one corpus recalled 73.1% and 22.5%, and their union 75.6%.
The misses are roughly independent (the union is close to `1 - (1-0.731)(1-0.225) = 0.791`), so
the gain is bounded by the better hash. Doubling the index memory for 2.5 points is a bad trade.
`simhash_with_domain` stays as a public primitive because it is the honest way to ask the
question, and the answer is recorded rather than left as an assumption.

**EXP-027 (the actual finding).** The recall is a property of the *base document*, not of the
edit. A near-duplicate's flipped bits are the base's near-zero majority-vote positions, which are
essentially random per document. Across 24 base documents, the same 32-token edit model:

| scheme | min | median | max | std dev |
|---|---|---|---|---|
| standard ladder | 20.8% | 50.0% | 95.8% | 4.2 variants |
| 1-flip multi-probe | 66.7% | 91.7% | 100.0% | 2.5 variants |
| 2-flip multi-probe | 91.7% | 100.0% | 100.0% | 0.7 variants |

The multi-probe does not merely raise the recall; it makes it nearly deterministic. The 2-flip
scheme's standard deviation falls from 4.2 to 0.7 variants.

**Tests executed.** `cargo test -p oag-collisions`: 31 passed, 0 failed. `cargo build --examples`:
clean (one pre-existing `unused variable: shuffle` warning in `ranking_tradeoff.rs`, untouched).

**Commit identifiers:** `4173f31`.
**Push status:** pushed.
**Remaining defects.** The 1-flip versus 2-flip choice is now a stated trade (probe budget and
false positives against predictability) but neither is made the default in the API -- the caller
chooses. The recall variance is measured on one synthetic corpus shape; a real corpus could differ.

---

## 2026-11-12 — EXP-025: widening the prefix filter's recall with a 1-flip multi-probe

**Repository:** `oag-collisions`
**Files modified:** `crates/oag-collisions/src/lib.rs`,
`crates/oag-collisions/examples/recall.rs`, `EXPERIMENTS.md`, `README.md`.

**Question.** EXP-024 fixed the ranking signal but not the candidate set: the prefix filter
excluded 61% of variants, and the exclusion was steepest for the far near-duplicates. An item
whose first differing bit is above bit 8 is invisible at every level, however few bits moved.

**Measurement.** `examples/recall.rs`: an 800-token base, window 3, budget 200, 20 variants at
each edit distance from 2 to 256 (160 near-duplicates) and 160 unrelated documents.

| scheme | probes | near found | unrelated found |
|---|---|---|---|
| standard | 57 | 62/160 (38.8%) | 1/160 (0.6%) |
| 1 flip | 65 | **118/160 (73.8%)** | 5/160 (3.1%) |
| 2 flips | 93 | 143/160 (89.4%) | 22/160 (13.8%) |

The 1-flip scheme nearly doubles the recall for 8 extra probes, at **14 near-duplicates recovered
per unrelated document admitted**. The 2-flip scheme's ratio is 3.9 near per unrelated, and the
unrelated rate quadruples.

**The recall that matters is at the near end.** The 1-flip scheme recovers every variant at 2, 4
and 8 tokens (20/20 at all three; the standard ladder gets 14, 14 and 12). A lost near-duplicate
is a false negative a deployment feels; a far one recovered at 128 tokens is a weaker inference.

**Fix.** `CollisionIndex::collide_multi_probe` (and `_text`) probes the query's prefixes at every
level, plus the length-8 prefixes differing from the query's in up to `max_flips` of the first
`probe_bits` bits. Two tests pin the contract: `max_flips = 0` is exactly the standard ladder, and
extra probes never narrow the candidate set.

**Tests executed.** `cargo test -p oag-collisions`: 31 passed, 0 failed (was 30). New test
`multi_probe_zero_flips_matches_the_ladder_and_extra_flips_never_narrow_it`. `cargo build
--examples`: clean.

**Commit identifiers:** `c1dac22`.
**Push status:** pushed.
**Remaining defects.** The far end is still limited: no probe count short of a full scan recovers
a 256-token variant reliably (15/20 with 2 flips). The unrelated documents are synthetic, so the
false-positive rate is a property of this corpus.

---

## 2026-11-12 — EXP-024: the index ranks by a weaker signal than it stores

**Repository:** `oag-collisions`
**Files modified:** `crates/oag-collisions/src/lib.rs`,
`crates/oag-collisions/examples/ranking_margin.rs`, `EXPERIMENTS.md`, `README.md`.

**Question.** EXP-023 left open whether the thin margin limits the ranking. The question hides a
distinction: the index ranks by **longest shared prefix** (the position of the first differing
bit), while EXP-023 measured **Hamming distance** (the number of bits moved). A variant one bit
away at the most significant position shares nothing; one bit away at the least significant
position shares 63.

**Measurement.** `examples/ranking_margin.rs`: an 800-token document, window 3, budget 200, 20
variants at each edit distance from 2 to 256 tokens.

| signal | Spearman with edit distance |
|---|---|
| Hamming distance | **+0.916** |
| shared prefix (the index's ranking signal) | **-0.443** |

The prefix ordering inverts **22.3%** of the 10,409 comparable pairs relative to the Hamming
ordering (29.7% among the 62 candidates the prefix filter admits). The Hamming ordering inverts
0%, by construction.

**Fix.** `CollisionIndex::collide_by_hamming` (and `_text`) re-ranks the candidates by the
full-hash Hamming distance. The index already stored every item's full hash and already held the
candidate set in memory, so the re-rank is `O(candidates)` with no extra memory. `Collision` gains
a `hamming` field alongside `shared_bits`. The candidate set is unchanged -- the prefix buckets
still decide recall, and they exclude 61% of variants here (70% of 2-token variants are
candidates; 5% of 256-token ones).

**Result.** On the measured corpus the top-1 improves from a 4-token edit to the 2-token edit
that is the closest detectable document.

**Tests executed.** `cargo test -p oag-collisions`: 30 passed, 0 failed (was 29). New test
`collide_by_hamming_orders_the_candidates_by_full_hash_distance`. `cargo build --examples`: clean.

**Commit identifiers:** `e069c28`.
**Push status:** pushed.
**Remaining defects.** The recall limit of the prefix filter is described, not fixed: closing it
would require scanning every item, which is the cost the index exists to avoid. The Spearman
coefficients are over one synthetic corpus.

---

## 2026-11-12 — EXP-023 finding 4: the margin is set by the edit, not the budget

**Repository:** `oag-collisions`
**Files modified:** `crates/oag-collisions/examples/distribution.rs`, `EXPERIMENTS.md`.

**Question.** EXP-023 separated the two causes of a miss -- the coverage gap and the
majority-vote margin -- and left the second described but not modelled. If the thin margin were a
budget problem, a larger budget would thicken it.

**Measurement.** Panel 4 of `examples/distribution.rs`: at a fixed budget of 200, vary the edit
length over 100 random positions x 5 seeds.

| edit | detected | mean bits moved |
|---|---|---|
| 1 token | 56.8% | 1.12 |
| 2 tokens | 84.2% | 1.68 |
| 4 tokens | 89.6% | 2.13 |
| 8 tokens | 96.4% | 3.12 |
| 16 tokens | 99.2% | 4.06 |

The margin grows with the number of changed units, and even a 16-token edit out of 800 moves only
about 4 bits; the 11+ bucket stays essentially empty. The mechanism is the majority vote: an edit
crosses only the margins of the units whose windows it overlaps.

**Consequence.** A high detection rate does not mean a confident detection. The rate saturates
near 100% for a 16-token edit while the mean margin is still 4 bits; at the common 2-token edit
the index detects 84% at a mean of 1.68 bits. A consumer that treats "detected" as "these
documents differ" is reading a 1-bit signal at the common edit size.

**Tests executed.** `cargo test -p oag-collisions`: 29 passed, 0 failed. `cargo build --examples`:
clean. Example runtime 22s.

**Commit identifiers:** `32a683e`.
**Push status:** pushed.
**Remaining defects.** The margin measurement and the ranking measurement are still unjoined:
if a detection is 1-2 bits, the longest-shared-prefix rank is 62-63 bits, and whether that orders
 two near-duplicates at different distances is the question EXP-004 raised and EXP-023 does not
 answer.

---

## 2026-11-12 — EXP-023 corrected: the detection rate is a step function of the stride

**Repository:** `oag-collisions`
**Files modified:** `crates/oag-collisions/examples/distribution.rs`, `EXPERIMENTS.md`.

**Defect in the previous entry's own measurement.** The first EXP-023 run sampled edit positions
with a systematic step. A step resonates with the window stride `ceil(798/budget)`: a step of 16
read 99.8% at budget 50 (the coverage of window *starts*, not of edits), and a step of 7 was
coprime with most strides but shared a factor with the strides at budgets 60, 120 and 130. The
reported curve was contaminated at those points. Positions are now sampled pseudo-randomly, which
has no resonance with any stride.

**Three findings from the clean measurement.**

1. **The detection rate is a function of the stride, not the budget.** Budgets sharing a stride
   give identical rates (spread exactly 0.0): stride 8 -> 41.0%, 7 -> 48.2%, 6 -> 65.8%, 5 -> 75.0%,
   4 -> 84.2%. The curve is a step function; the "jaggedness" EXP-022 could not resolve is the
   step, and there is no budget to tune between strides.
2. **The window scheme has a periodic blind spot, and it is total.** At budget 100 (stride 8), the
   residues modulo the stride are missed at 2.0%, 0.0%, 0.0%, **100.0%, 100.0%, 100.0%, 100.0%**,
   4.7%. The gap is `(stride - window - 1)/stride` of positions, zero only when
   `stride <= window + 1` -- at 800 tokens and window 3, a budget of at least 200. This refines
   EXP-016's "no blind spot" claim, which was measured at budget 200 and holds there but not below.
3. **Most detections are thin-margin.** At budget 200, 62.2% of detections move only 1-2 bits; the
   6-10 and 11+ buckets are nearly empty at every budget.

**Tests executed.** `cargo test -p oag-collisions`: 29 passed, 0 failed. `cargo build --examples`:
clean (one pre-existing `unused variable: shuffle` warning in `ranking_tradeoff.rs`, untouched).

**Commit identifiers:** `3e70a6a`.
**Push status:** pushed.
**Remaining defects.** The blind-spot formula is derived for a 2-token edit; the general form for
an `edit_len`-token edit is recorded in EXP-023's limits but not measured. The margin-failure
mechanism (why a covered edit does not flip a bit) is described, not modelled.

---

## 2026-11-12 — the pruner no longer races cargo; EXP-023 measures detection as a distribution

**Repository:** `open-agent-graph` (scripts) and `oag-collisions` (experiment).
**Files modified:** `scripts/prune-target.py`, `scripts/rustc-wrapper.sh`,
`scripts/test_prune_target.py`, `scripts/README.md` (all four repositories);
`oag-collisions/crates/oag-collisions/examples/distribution.rs`, `oag-collisions/EXPERIMENTS.md`.

**Defect repaired (introduced by the earlier prune hook).** The pruner deleted
`incremental/`, `.fingerprint/`, and `build/` directories that cargo had created for the units it
was about to compile, and cargo then failed with `failed to write .../invoked.timestamp: No such
file or directory`. Reproduced by `cargo build --examples` after clearing the fingerprint
directory: 2 of 3 runs failed. The wrapper now reads the cargo process's own start time
(`stat -c %Y /proc/<pid>`) and passes it as `--protect-after`; the pruner skips anything at or
after that horizon. The generation being built is never pruned; old generations still are.

**Tests executed.** `python3 scripts/test_prune_target.py`: 16 passed, 0 failed (was 14). New:
`test_protect_after_skips_generations_cargo_is_using`,
`test_protect_after_skips_dependency_outputs_cargo_is_writing`. Also `cargo build --examples` in
`oag-collisions`: 3 consecutive clean runs after the fix, 2 of 3 failing before it.

**Experiment EXP-023.** `examples/distribution.rs` implements the follow-up EXP-022 left open:
the distribution of detection outcomes over many (budget, position, seed) triples, not a peak.
An 800-token document, a 2-token edit, window sampling at width 3, a fine budget grid, 114 edit
positions, 5 seeds. Three findings, all measured:

- **The jaggedness is the normal case.** 10 of 19 adjacent budget pairs differ by more than their
  combined Wilson half-widths. Budget 120 detects 96.1%; budget 140 detects 62.5% -- increasing
  the budget can cost 34 points.
- **Most detections are thin-margin.** At budget 200, 60.4% of detections move only 1-2 bits.
- **The misses are positionally concentrated.** At budget 100 the per-position miss rate ranges
  0-100% with a 49-point standard deviation, so the window scheme does have positional blind
  spots after all.

**Methodological trap recorded.** The first version sampled edit positions every 16 tokens,
which is exactly the window stride at budget 50, so every tested position was a sampled window's
start and the rate read 99.8% -- the coverage of window starts, not of edits. A position step
must be coprime with the stride (7 is coprime with 16, 8, and 4).

**Commit identifiers:** `89014ff` (scripts fix, open-agent-graph), `adcee5a`/`8eb1863`/`8e9df64`
(same fix, mesh/imagination/collisions), `dd56214` (EXP-023, collisions).
**Push status:** all pushed.
**Remaining defects.** The positional blind spot EXP-023 found is unexplained: why the window
scheme, which has no periodic blind spot, still concentrates its misses at particular positions.
The pre-existing `unused variable: shuffle` warning in `ranking_tradeoff.rs` is untouched.

---

## 2026-11-12 — bound the imagination pad's recovery from a deep lock-in

**Repository:** `oag-imagination-random-data-compute`
**Files modified:** `crates/oag-imagination/src/engine.rs`,
`crates/oag-imagination/examples/convergence.rs`, `spec/02-tensor-pad-routing.md`.

**Defect.** The pad's exploration floor was an absolute `1e-6`, which is meaningless against a
pad normalized to `PAD_MEAN` over `PAD_DIM` slots. A slot penalized without limit was multiplied by
`hop_failed_penalty` every decision and fell to denormal `f32` (about `1e-45`), where the constant
left it a share of about `1e-8` -- effectively frozen. Measured in `examples/convergence.rs`: a
switch at decision 10,000 took **1,061** decisions to recover, because the stored ratio between the
strongest and weakest routing slot was about `1e45` and the ratio decays by `exp(-HOP_STEP)` per
decision. Recovery time grew with the depth of the lock-in, unbounded.

**Fix.** `PAD_FLOOR_FRAC` (0.01) floors every routing slot at a fraction of the routing slots'
mean. Applied twice: in `route` (so any pad, including one loaded from the layer below, is
explorable) and in the stored pad after every reinforcement via `floor_routing_slots` (so the
*stored* ratio -- the thing recovery is measured against -- is bounded). The floor is relative to
the routing slots' own mean, not `PAD_MEAN`, because the pad is normalized over all `PAD_DIM`
slots and the first `neighbours.len()` of them can sum to more or less than `neighbours.len()`.

**Measurement.** The same switch now recovers in **69** decisions, matching the predicted bound
`ln(neighbours / PAD_FLOOR_FRAC) / HOP_STEP` ~ 60 for four neighbours. The cost is that a
neighbour the pad has learned to avoid keeps `PAD_FLOOR_FRAC / neighbours` of the traffic; the A/B
control still separates cleanly (signal 99.1% on the answering neighbour vs control 59.3% on an
arbitrary one).

**Tests executed.** `cargo test -p oag-imagination`: 54 passed, 0 failed (was 51). New tests:
`a_penalized_neighbour_keeps_a_bounded_share`, `the_stored_pad_keeps_a_bounded_ratio`,
`recovery_from_a_deep_lock_in_is_bounded_by_the_floor`.

**Commit identifiers:** pending.
**Push status:** pending.
**Remaining defects.** None known. The floor's cost in exploration traffic is a tuning
question, not a defect; `PAD_FLOOR_FRAC` is a named constant for that reason.

---

## 2026-11-09 — cap the imagination engine's route hops

**Repository:** `oag-imagination-random-data-compute`
**Files modified:** `crates/oag-imagination/src/engine.rs`, `spec/02-tensor-pad-routing.md`.

**Gap closed.** The engine has no visited-set — the pad picks the next hop by weight, so a cycle
between two nodes is a possible routing outcome, not an impossible one. Without a hop budget such a
request would accumulate one contribution per hop and be re-sent in full each time, amplifying
bandwidth and memory until the carrier's `MAX_MESSAGE_BYTES` happened to reject it.

**Fix.** `MAX_ROUTE_HOPS` (64), well above `spec/02`'s own "6, 10, or 50 nodes" intent and matching
the spirit of the base project's `relay_hops_remaining`. A node that has exhausted the budget
returns `TooManyHops` instead of forwarding. It still records its own contribution: the bound is on
forwarding, not on acknowledging participation.

**The budget bounds forwarding, not answering.** A node that can answer does so however long the
chain behind the request is — refusing an answer would turn a bounded-chain problem into a failed
request.

**Tests executed.** `cargo test --workspace` — 45 passed (oag-imagination) + 3 passed (adapter).

**Test results.** oag-imagination 43 → 45. New: `a_request_at_the_hop_budget_is_not_forwarded`
(two below the budget still routes; one below reaches it and is refused) and
`the_hop_budget_does_not_stop_a_node_that_can_answer`.

**Performance measurements.** None. The bound costs one length comparison per handled request.

**Commit:** `548580d` — pushed.
**Remaining defects.** The engine still has no visited-set, so a request can revisit the same node
many times within the budget — the budget bounds the damage but does not detect the cycle. A
visited-set would cost memory proportional to the chain, which is the same trade the dream log
already makes.

---

## 2026-11-09 — bound the imagination dream log

**Repository:** `oag-imagination-random-data-compute`
**Files modified:** `crates/oag-imagination/src/engine.rs`, `spec/02-tensor-pad-routing.md`.

**Defect fixed.** `DreamLog` was an unbounded `Vec<Request>`, and `Engine::handle` records every
answered request as a clone. Any caller who can make this node answer a request -- any peer that can
route one here -- could grow its memory without limit, amplified per request by the caller-controlled
JSON in `target`/`payload` and the contribution chain gathered along the route.

**Fix.** Both a count cap (1,000) and a serialized-byte cap (8 MiB), evicting the oldest until both
hold. A count cap alone would not have been enough: one request may be up to the carrier's
`MAX_MESSAGE_BYTES` (16 MiB over TCP), so "keep the last 1,000" still permits tens of gigabytes. An
entry larger than the whole byte budget is not retained at all -- otherwise a single request would
defeat the bound.

**Tests executed.** `cargo test --workspace` — 43 passed (oag-imagination) + 3 passed (adapter).

**Test results.** oag-imagination 40 → 43. New: `the_dream_log_evicts_the_oldest_entry_past_its_
count_cap`, `the_dream_log_evicts_past_its_byte_cap`, `a_single_entry_larger_than_the_budget_is_not_
retained`. The log previously had no tests at all, which is part of why this went unnoticed.

**Performance measurements.** None. The bound adds one serialization per answered request (cached
at record time) and a `pop_front` on eviction. No benchmark was run.

**Commit:** `9d77e3a` — pushed.
**Remaining defects.** The log is still in-memory only: "recording generated scenarios" holds for the
running process, not across restarts, and that is a scope statement rather than an oversight.
`MAX_MESSAGE_BYTES` itself is 16 MiB over TCP, which is generous for an imagination request; it is
bounded but not small, and a per-request cap would be the next tightening if memory mattered more.

---

## 2026-11-09 — new layer: oag-collisions (inference by collision)

**Repository:** `oag-collisions` (new; the operator created it as a fourth layer on top of the
three foundations, explicitly overriding the earlier workspace confinement)
**Files created:** `Cargo.toml`, `crates/oag-collisions/{Cargo.toml,src/lib.rs}`,
`crates/oag-collisions/src/aberration.rs`, `crates/oag-collisions/examples/aberration.rs`,
`SPEC.md`, `EXPERIMENTS.md`, `LICENSE`.

**What it is.** A collision is an inference result, not an error: two independently produced values
that land in the same slot are not a coincidence to discard, the collision *is* the answer, and its
strength is how much of the slot they share. Implemented as locality-sensitive hashing (SimHash over
BLAKE3 token hashes) with every prefix indexed, so a query collides partially with everything and the
longest shared prefix ranks the result. No vector database, no model, no training.

**The experiment (PART 0).** The operator's statement "the sun is travelling at light speed" was
taken literally and tested. Speed alone gives a constant aberration -- an offset indistinguishable
from the coordinate origin -- but an *accelerated* observer sees the sky slide, and a circular orbit
is continuous acceleration, at `a / c` radians per second for `a = v^2 / r`. That quantity is the
published secular aberration drift (VLBI/Gaia, ~5 microarcsec/year).

**Result.** The control passes: 230 km/s reproduces 4.539 microarcsec/year against ~5 published.
The claim does not survive: at light speed the drift would be **7.7 arcseconds per year**, 1.7
million times the measurement and 0.17 degrees across an 80-year lifetime -- visible to the naked
eye, never recorded. The discrepancy is a factor of ~1300 in velocity.

**Second control (EXP-002).** Earth's orbital speed is the AU over the Julian year -- both defined
quantities, known to four digits independently of any star catalogue -- and it produces the
classical aberration constant every star catalogue is corrected by: computed 20.48939 arcsec against
the IAU's 20.49552 (the 0.006 gap is exactly the 29.780 vs 29.7859 km/s difference). Earth's drift
is 128.716 arcsec/year, 2.836e7 times the galactic one. One un-tuned formula landing on two
independently known quantities, seven and a half orders of magnitude apart, is what makes the first
comparison a control rather than a fitted curve.

**Tests executed.** `cargo test -p oag-collisions` — 14 passed, 0 failed.

**Commits:** `cc7613a`, `04572c7`, `5e83384`, `e0bea3d`, `693ab0b`, `3bb7127`, `1dde7b6`,
`1a80111`, `2712e73`, `550525d`, `4fcd431` — pushed.

**Follow-on work (same layer).**
- `2712e73` — third measurement, the CMB dipole: 369.8 km/s against a published 369.8 ± 0.9, a
  completely independent method from the other two.
- `550525d` — the collision index was bounded. It stored every item under all 64 prefixes of its
  hash with no cap, so memory was O(items × 64) and grew without limit. Now
  `with_capacity_and_levels(n, levels)` with oldest-first eviction, plus a `bucket_entries()`
  accessor because the item count is the wrong quantity to reason about.
- `4fcd431`, `89044f9` — the prefix-level trade-off was measured rather than assumed
  (`examples/ranking_tradeoff.rs`). Two corrections to the first reading:
  - *Ties.* At 16 and 8 levels the correct variant is still in the top set but tied, so the first
    answer is arbitrary. The index has not lost the right answer; it has lost the ability to choose
    it. 32 is the fewest levels that gets top-1 right without a tie.
  - *Length.* The crossover does not survive moving the document length. At 200 tokens the correct
    variant wins on 51 shared bits; at 60 the best hit is an unrelated variant on 11 bits (chance);
    at 20 and 8 nothing crosses the 8-bit threshold at all. For short documents **no** level count
    works, because unigram SimHash is a majority vote over bit positions and with few tokens each
    bit's vote is decided by noise.
  The default stays at 64: one synthetic corpus, and changing a default on one result is the
  unmeasured trade the experiment exists to prevent.
- `362ca03` — the recorded follow-up was tested and **falsified**: hashing word bigrams rather than
  unigrams does not fix the short-document failure and *degrades* the long-document case (at 200
  tokens the unigram index returns the correct variant at 51 shared bits; the bigram index returns
  one result, an unrelated variant at 8 bits, with the correct answer absent). The `shingle` knob
  stays but defaults to 1, with the negative result recorded beside it — nobody should "fix"
  short-document performance with bigrams on the strength of an intuition.
- `3d70ffd` — character n-grams were falsified too (they also degrade the working case), but the
  diagnostic reframed the problem. Character trigrams give 887 units per document, and a query plus
  a variant sharing 885 of them produced hashes differing in **zero** bits. Measuring SimHash's
  sensitivity directly — fix a 2-token change, vary the unit count — gives 17 bits moved at 10
  units, 4 at 50, 0 at 200, 1 at 800. Sensitivity is **inversely** proportional to the unit count,
  so short documents are *over*-sensitive, contradicting the mechanism claimed in EXP-004. That
  write-up's measurements were right and its explanation was wrong; `EXPERIMENTS.md` now says so.
  Consequence: the unit count is a sensitivity dial as well as a memory dial, coupled in the
  opposite direction from intuition — indexing more text can silently freeze the index. Why 60
  tokens fails is recorded as an open question, not explained.
- `29458ce` — the sensitivity window mapped (`examples/sensitivity.rs`): units against edit
  distance, bits moved, averaged over 8 seeds. The ~19.5-bit plateau is the unrelated-document
  distance. Detection degrades gracefully, but the **ranking** gap between e=2 and e=4 collapses
  from 5.1 bits at 10 units to **0.1 bits at 800** — so at 800 units two near-duplicates at
  different distances cannot be ordered at all. The window is a ranking window, not a detection
  window. The surface also falsifies the sensitivity explanation for the 60-token failure: 60
  tokens sits comfortably inside the resolvable band, so that question stays open — narrowed.
- `6f472bf` — unigram SimHash is **exactly invariant to word order** (the EXP-007 gap). A 200-token
  document and its full reversal collide at **64 shared bits** through the index; an adjacent swap is
  likewise invisible. Structural, not tunable. Bigrams detect order (24 bits) but EXP-005 measured
  them degrading substitution, so the layer faces a measured either/or: unigrams are blind to
  order, bigrams cost substitution sensitivity. Also qualifies EXP-004 — at the same 200 tokens a
  2-token replacement moved 0 bits here, so the earlier “works at 200 tokens” was one draw of
  replacement tokens, not a property of the size.
- `0760b15` — **real defect**: `insert_text` ignored the index's tokenizer (it called the free
  `tokenize`, while `collide_text` honoured it), so any bigram or character index hashed unigrams on
  insert and the configured scheme on query. Found because identical trigram hashes produced *zero*
  collisions, which is impossible for a matching pair. Fixed with weighted units
  (`Tokenizer::tokenize` returns `(text, weight)`, `insert_units` is the single insert path,
  `simhash_weighted` added); the regression test is verified to fail when the fix is reverted.
  EXP-005 and EXP-008's character numbers are marked invalid in place; EXP-008's word-level
  order-invariance result is unaffected. Corrected: character trigrams freeze the hash completely
  (all 60 variants hash identically); word bigrams still do not fix short documents.
- `91534a3` — **bigram vote weight 2 improves both sensitivities**, overturning EXP-005. At 200
  tokens: weight 0 (unigrams) gives order 0 bits / substitution 1.5; weight 1 gives 24 / **0.9**
  (worst of both); weight 2 gives 31 / **4.2**; weight 4 gives 33 / 4.4. A 2-token substitution
  changes ~3 bigrams rather than 2 unigrams, so bigrams carry more substitution information than
  they cost in vote weight. And weight 2 **recovers the 60-token top-1** that unigrams miss and
  weight 1 loses, while keeping 200; weight 4 freezes and loses all. The default
  `with_capacity_levels_and_shingle` weight of 1 is now known to be the measured worst setting —
  recorded for follow-up rather than changed on one synthetic result. 8 tokens fails at every
  weight.
- `057be25` — fine sweep after exposing `unigram_weight` (the knob was integer-only with unigrams
  pinned at 1, so no ratio between 0 and 1 was expressible). Order sensitivity is smooth and
  monotone (0→33 reversal bits across ratios 0→4); substitution sensitivity is not monotone; and
  **top-1 is not a function of the ratio at all** — yes at 0.00 and 2.00, no at 0.10/0.20/0.25/0.50/
  1.00/3.00/4.00. That qualifies EXP-010: "weight 2 is the optimum" overstates it. The
  sensitivities are real and the 60-token gain stands, but top-1 flips between adjacent settings,
  so tuning by top-1 on one corpus is not a reliable method. Recorded as a methodological finding.
- `10f78f6` — replaced the knife-edge top-1 with a **margin measured over 8 corpora** (correct
  variant's shared bits minus the best other). It retires the parameter-tuning line rather than
  resolving it: mean margins 1-6 bits against standard deviations of 15-28, so every tokenizer
  setting is indistinguishable from zero within noise. EXP-010's “weight 2 improves both” and
  EXP-011's “optimum between 1 and 4” were both readings of a metric too knife-edge to support
  them. The dominant failure is **visibility, not ranking** — in most corpora the correct variant
  never reaches the collision threshold at all (0 to 8 of 8 invisible depending on the setting),
  which matches the EXP-007 surface: at 200 units a 2-token edit moves ~1.5 bits on average,
  frequently zero. The unit count is the binding constraint, not the tokenizer choice.
- `4d4aef2` — repeated the margin measurement at 40/60/100/200 tokens to test whether the
  tokenizer settings separate in the resolvable band. **They anti-separate.** At 100 tokens,
  where unigrams give a positive mean margin of 14.8, both bigram settings give **negative**
  margins (-8.5, -5.2) across six corpora each with small standard deviations. The mechanism is
  in the unit count: bigram settings hash roughly double (79 units for 40 tokens, 199 for 100),
  and EXP-007 measured that doubling the unit count collapses sensitivity. So the tokenizer
  choice and the unit-count constraint are the *same* constraint. This closes the
  parameter-tuning line with a mechanism rather than a shrug, and points at the real lever:
  **reducing** the unit count (hashing a sample of tokens rather than all of them), which is
  unmeasured and is recorded as the follow-up.
- `40f588c` — implemented that lever as `Tokenizer::WordsBudget`: a deterministic stride sample,
  stride `ceil(len / target)`, so the unit count is bounded regardless of document length. **The
  surface flattens**: with a budget of 50, document sizes 100/200/400/800 all land in a 2.4-4.9 bit
  band for e=1..2, against the unbudgeted 800-token row's 0.5-0.9. And the budget is the knob: at a
  fixed 800-token document, a 2-token edit moves 5.2/4.9/2.6/1.2/1.0/0.9 bits at budgets
  20/50/100/200/400/800 — monotone in the budget alone. Honest limit: budgeted sensitivity at a
  given budget is *below* an unbudgeted document of that unit count (5.2 at budget 20 vs 9.8 for a
  20-token document), because the stride sample has gaps and a small edit can miss the sampled
  units.
- `ca9c2f4` — measured stride against shuffle sampling at the same budget
  (`Tokenizer::WordsShuffledBudget`, Fisher-Yates from a deterministic LCG with document order
  restored). **The stride is a step function**: 1.0 bits for edits on a stride boundary, 0.0 for
  edits between them, and averaging over seeds does not remove it because the mean over seeds is
  the same step function. **The shuffle has no blind spot** — positive and smooth at every
  position, variance as the price. Also found a bias in the stride's favour: it always samples
  index 0, so every document's opening is over-weighted by construction. For a crawler comparing
  a page against its previous fetch, edits are arbitrary rather than periodic, so the shuffle's
  random failure is the better failure mode.
- `a819f12` — measured the third scheme: `Tokenizer::WordsWindowBudget` hashes a fixed budget of
  *windows* (runs of consecutive tokens) instead of individual tokens. A 2-token edit touches 2 of
  800 token-units but every window overlapping it. **The window scheme has no zeros**: 1.0 bits at
  on-stride positions and 1.0 bits between them, against the stride's 0.0 at between-stride
  positions. It combines the stride's spread coverage with the shuffle's lack of a periodic blind
  spot. Cost: a unit is a 3-token window, so the index says *which window* changed, not which
  token. Limits: one window width, one document size, one budget, one seed.
- `b402cdd` — measured the **false-negative rate** over every second position of an 800-token
  document, 8 seeds each, budget 200. Stride 35.4%, shuffle 40.6%, windows **86.5%**. This
  **corrects EXP-015/016's framing**: the window scheme wins on *coverage* (each unit spans 3
  tokens, so 600 of 800 token positions are covered), not on lacking a periodic blind spot — the
  shuffle has no blind spot and is barely better than the stride, because both sample the same
  fraction of tokens. Periodicity decides *which* edits are missed; coverage decides *how many*.
  Also explains the stride's 35.4% being below the naive 50%: a single-unit change is often
  absorbed by the majority vote.
- `819cc25` — swept the window width (2, 3, 5, 8) at a fixed budget of 200 on an 800-token
  document. Rates: 40.7%, 86.5%, 97.2%, 95.4% against coverage predictions of 50%, 75%, 100%,
  100%. **The trend confirms coverage but the absolutes do not.** The reason: the majority vote
  absorbs a changed unit -- a 2-token edit changes 3 of 799 windows under width 2, so with a budget
  of 200 the expected number of changed sampled units is under one, and one changed unit out of 200
  almost never flips a bit. The decisive quantity is therefore the fraction of *sampled* units that
  changed, not the fraction of positions covered. The absorption floor was inferred from the
  saturation residual, not measured directly.
- `8ddc2fa` — measured that floor by fixing the edit and sweeping the unit budget (window 3,
  800 tokens, 8 seeds). The rate is **not monotone**: 0.5%, 2.5%, 5%, 10%, 25%, 48.9%, **86.5%
  at budget 200**, then 56.2% and 44.9% at budgets 400 and 800. **Hashing every window is worse
  than sampling 25% of them.** Below the peak the limit is coverage; above it, absorption --
  SimHash's per-bit vote margin scales with `sqrt(N)`, so a fixed-size edit is relatively larger
  in a smaller sample and flips more bits. The unit budget is therefore a sensitivity dial as well
  as a memory dial, and "index everything" is the wrong default.
- `da45c8e` — re-ran the budget sweep at 800/1600/3200 tokens on a coarser budget grid. The
  1600 peak moved from 25.1% to 49.9% — a 25-point swing from the sampling, against ~2-point
  run-to-run reproducibility at 800 tokens. The peak is a property of the budget grid, not the
  document. **Withdraws the previous entry's conclusion**: neither the constant-fraction nor the
  drifting reading is supported; the three sizes read 33.1%, 49.9%, 41.6%, which is no trend.
  The unit budget's optimum is not a stable constant fraction; a budget in the 25-50% range is
  defensible, a specific fraction is not. Cause: detection is a single bit flip in a 200-unit
  vote, so the rate is a step function of which units the budget includes.
**Remaining defects.** SimHash is a majority vote over bit positions, so with very short inputs
(2–4 tokens) it is decided by noise; the tests were corrected to use realistic-length texts rather
than asserting on a property the mechanism does not have at that scale. ~~The collision index has no
eviction policy~~ — fixed in `550525d`: the index is bounded by item count (default 100,000,
evicting oldest-first), because each item costs 64 bucket entries and an unbounded index is a
memory-exhaustion vector.

---

## 2026-11-09 — record every pin path in `pinned_cids`

**Repository:** `open-agent-graph` (`rust-ai`)
**Files created:** `crates/oag-storage/migrations/0015_pinned_cids.sql`,
`crates/oag-storage/src/repo/pinned_cids.rs`.
**Files deleted:** `crates/oag-storage/src/repo/pins.rs` (replaced by `pinned_cids`; the endpoint
now reads one indexed table instead of scanning `evidence.metadata`).
**Files modified:** `crates/oag-storage/src/repo/mod.rs`, `crates/oag-graph/src/{pins,lib,assert,
tests}.rs`, `crates/oag-cli/src/commands.rs`, `crates/oag-graph/src/assert.rs`,
`spec/29-ipfs-pinning.md`, `OPEN_QUESTIONS.md`.

**Gap closed.** The discovery surface shipped reading only `evidence.metadata`'s `ipfs_cid` key,
which covered the crawler's page pins and nothing else — `oag ipfs pin`, `oag backup --pin`, and
`oag ipfs host-page` left no queryable trace. So it answered "what did this peer pin as evidence"
rather than "what does it hold".

**Fix.** One `pinned_cids` table recording every pin path: `evidence` (the assert/evidence choke
point, which is how the crawler's pin lands), `cli-pin`, `backup-pin`, `host-page`. `source` is
descriptive metadata for an operator, never a permission or trust signal. The migration backfills
existing evidence CIDs and is idempotent, so a long-running peer does not report an empty list
after upgrading and a re-run cannot duplicate rows.

**Best-effort recording on the assert path**, with a warning, same reasoning as
`observe_sync_outcome`: the pin record is a derived index, not evidentiary state, and the evidence
row keeps its own `ipfs_cid` metadata either way — a failure there must not fail the evidence write
it is observing. The CLI paths warn too, since a failure to index must not un-pin content.

**Tests executed.** `cargo test --workspace` — all 31 test binaries pass, no failures.

**Test results.** oag-storage 33 → 34, oag-graph 95 → 96. New: idempotent recording and
most-recent-first listing, the limit, the backfill's idempotence, `record_pin`'s own cap. The
existing endpoint, MCP, and service tests continue to pass against the new table.

**Performance measurements.** None. The change replaces a full scan of `evidence` with an indexed
lookup, so it is strictly cheaper than what it replaced — no benchmark was run and none is claimed.

**Commit:** `d52dc8f` — pushed.
**Remaining defects.** Recording on the assert path is best-effort, so a peer that fails to index a
pin will under-report until a later pin of the same CID or a rebuild. `pinned_cids` has no
garbage-collection: a peer that unpins content through its IPFS node directly still lists the CID,
which is the same "record, not live check" limit already documented.

---

## 2026-11-09 — CID discovery surface

**Repository:** `open-agent-graph` (`rust-ai`)
**Files created:** `crates/oag-storage/src/repo/pins.rs`, `crates/oag-graph/src/pins.rs`.
**Files modified:** `crates/oag-storage/src/repo/mod.rs`, `crates/oag-graph/src/lib.rs`,
`crates/oag-api/src/{dto,handlers,lib,tests}.rs`, `crates/oag-mcp/src/{params,server,tests}.rs`,
`crates/oag-graph/src/tests.rs`, `spec/29-ipfs-pinning.md`, `docs/api.md`, `OPEN_QUESTIONS.md`.

**Feature implemented.** The discovery query that `spec/31` and `spec/32` both name as missing:
"how would a peer even learn *which* upstream peer has a given CID without being told?" A caller can
now ask a peer directly.

- `GET /api/v1/pins?cid=<cid>` -> `{"cid": ..., "holds": bool}`.
- `GET /api/v1/pins?limit=<n>` -> `{"pins": [...], "total": N, "limit": n}`, clamped to
  `[0, MAX_PIN_LIST_LIMIT]` at the service layer, matching search's cap.
- MCP `graph_pins`, same two modes. `graph:read`, like every other read surface.

**Scope, stated rather than implied.** The only CID this codebase records today is the projector's
`ipfs_cid` key in `evidence.metadata`, written when the crawler pins a page. So the surface answers
"which CIDs has this peer recorded pinning *as evidence*" — not "what does the IPFS node hold".
Pins from `oag ipfs pin`, `oag backup --pin`, and `oag ipfs host-page` are not recorded anywhere
queryable; closing that needs a `pinned_cids` table plus instrumenting three paths, and is
deliberately not done speculatively.

**And it is a record, not a live check.** Nothing fetches the content to confirm retrievability; a
peer that garbage-collected a CID still answers "yes". Same honesty property as a DNS name.

**Tests executed.** `cargo test -p oag-storage` (33), `-p oag-graph` (95), `-p oag-api` (62),
`-p oag-mcp` (20).

**Test results.** oag-storage 31 → 33, oag-graph 94 → 95, oag-api 61 → 62, oag-mcp 19 → 20.
New: storage tests for distinct-CID listing/counting/holding and the limit; a service test that
seeds real assertions with `pinned_cid` evidence and checks the duplicate collapses, `holds_pin`,
and the limit clamp; an HTTP test for the route's auth and both response shapes; and an MCP test for
the tool.

**Performance measurements.** None. The storage query is a full scan of `evidence` with a
`json_extract`, bounded by `LIMIT` — acceptable for an operator-driven query, and documented as
needing a real index if it ever becomes a hot path.

**Commit:** `8c676a0` — pushed.
**Remaining defects.** The surface under-reports what a peer holds (see scope above). No consumer
uses it yet: the relay still tries configured upstreams in order, and wiring a pre-check there adds
a failure mode (a failed check would skip a peer that might have worked) for a modest saving. That
trade is deliberately not made.

---

## 2026-11-09 — replication_bytes_in/out

**Repository:** `open-agent-graph` (`rust-ai`)
**Files modified:** `crates/oag-sync/src/{client,service}.rs`,
`crates/oag-api/src/{state,metrics}.rs`, `crates/oag-cli/src/serve.rs`,
`crates/oag-sync/src/client.rs` tests, `spec/19-observability.md`, `OPEN_QUESTIONS.md`.

**Feature implemented.** The last two named request-duration metrics. `SyncClient` counts the
streamed bodies of every replication read (`hello`/`heads`/`events`/`peers`) into `bytes_in`,
including ones later rejected as oversized or invalid, and each serialized POST body (`presence`,
`cluster`) into `bytes_out`. Process-lifetime counters shared by `Arc`.

**Why the sharing mattered as much as the counting.** `AppState` holds a *separately constructed*
`SyncService` that never runs the sync loop, so its own client's counters would always read zero --
the same trap `peer_sync_errors` already documents. `oag serve` hands `AppState` the real service's
`Arc`s at startup, exactly as it does for `sync_errors`.

**Tests executed.** `cargo test -p oag-sync` — 41 passed. `cargo test -p oag-api` — 61 passed.

**Test results.** oag-sync 40 → 41. New: `replication_bytes_counters_move_with_real_traffic`,
which fetches a real response and sends a real signed heartbeat against a fixture server and
asserts both counters moved.

**Not counted, deliberately.** The tiny unread responses to the fire-and-forget POSTs: reading them
would buffer a body for a few bytes. The `# HELP` line states the boundary rather than leaving it
to be discovered.

**Performance measurements.** None. The counters cost one atomic add per read body and per POST.

**Commits:** `78b46d9`, `defda80`, `1b61899` — pushed.
**Remaining defects.** `events_by_origin` (a labeled metric needing a decision on label cardinality)
and `blob_store_size` (no blob store) remain unbuilt. `bytes_in` does not cover unread POST
responses, by design and documented.

---

## 2026-11-09 — onion endpoint rate limit

**Repository:** `oag-mesh`
**Files modified:** `crates/oag-mesh/src/transport.rs` (`onion_limiter` field on `MeshState` plus
both constructors; limiter check at the top of `receive_onion_layer`; restored a doc comment that an
earlier edit had left attached to the wrong constant), `spec/15-onion-routing.md`,
`OPEN_QUESTIONS.md`.

**Gap closed.** The onion endpoint is the one endpoint in this crate that makes the node send
traffic to an address chosen by the caller — the peeled `next_hop` is forwarded to, relay or final
target. Inherent to the feature, but with no bound it also made the relay a constrained proxy for
directing anonymous-event pushes at third parties.

**Fix.** A node-wide token bucket (16 capacity, 0.5/s refill), the same shape as route search's.
Deliberately a volume bound and not a permission system: the endpoint stays unauthenticated (onion
routing's point is that the relay cannot authenticate the sender), the payload is still limited to
an identity-stripped event plus proof-of-work or one more layer, and the target still enforces its
own proof-of-work.

**Tests executed.** `cargo test -p oag-mesh` — 188 passed, 0 failed.

**Test results.** oag-mesh 187 → 188. New: `onion_rate_limiter_rejects_a_burst_and_recovers`. The
limiter check runs before peeling, so a dummy layer exercises it: below the limit a request is
rejected as unpeelable (400), past it as rate-limited (429 + `Retry-After`), and it recovers after
the bucket refills.

**Performance measurements.** None. No benchmark was run. The limiter costs one token check per
request.

**Commit:** `f47bf83` — pushed.
**Remaining defects.** The unauthenticated, arbitrary-target design is unchanged and deliberate.
The bound is node-wide, so one caller can exhaust the bucket and starve legitimate ones — there is
no per-caller key because there is no caller identity. In-memory, so a restart resets it.

---

## 2026-11-09 — crawler dual-stack DNS pinning

**Repository:** `open-agent-graph` (`rust-ai`)
**Files modified:** `crates/oag-crawler/src/fetch.rs`, `crates/oag-api/src/gateway.rs`,
`OPEN_QUESTIONS.md`.

**Defect fixed.** `build_pinned_client` looped over the resolved address set calling reqwest's
`resolve()`, which maps a host to exactly one SocketAddr and **overwrites** on every call — so only
the last entry survived. A dual-stack host resolved to `[IPv4, IPv6]` with no outbound IPv6 route
failed outright, with zero fallback to the working IPv4 address, purely because IPv6 happened to be
resolved last.

This was already recorded in `OPEN_QUESTIONS.md` as a latent bug found via the gateway feature and
deliberately left unfixed in the crawler. It is now fixed there too.

**Fix.** `build_pinned_client` pins exactly one address and documents why the loop form was wrong.
`safe_fetch` reduces its resolved set through a new `oag_crawler::fetch::prefer_ipv4` before pinning,
so the choice is deliberate rather than resolver-order-dependent; an IPv6-only host has no A record,
so it is unaffected. `oag-api`'s gateway now calls that shared helper instead of keeping its own
copy — the same logic in two places is how they drift.

**Tests executed.** `cargo test -p oag-crawler` — 75 passed. `cargo test -p oag-api` — 61 passed.

**Test results.** oag-crawler 69 → 75. New: `prefer_ipv4` picks the IPv4 address in both
resolver orders, leaves an IPv6-only set untouched, passes an empty set through;
`build_pinned_client_pins_the_first_address_in_the_set` (a set with a dead IPv6 second entry still
fetches — this is the regression); `pinning_an_unusable_first_address_fails_without_fallback`
(characterises the rule, and is why `safe_fetch` filters first); and
`the_safe_fetch_path_reduces_a_dual_stack_set_before_pinning`.

**Performance measurements.** None. No benchmark was run. The change alters *which* address is
pinned, not how many connections are attempted.

**Commit:** `33563d4` — pushed.
**Remaining defects.** The same overwrite behaviour would affect any other caller that loops
`resolve` over an address set; `build_pinned_client` is the only one in this codebase. IPv4-first is
a deliberate bias for dual-stack-broken networks, not a correctness property — an IPv6-only host is
handled, but a host whose IPv4 is firewalled and whose IPv6 works would now prefer the broken IPv4.

---

## 2026-11-09 — events_pending_chain

**Repository:** `open-agent-graph` (`rust-ai`)
**Files modified:** `crates/oag-storage/src/repo/stats.rs`, `crates/oag-graph/src/metrics.rs`,
`crates/oag-api/src/metrics.rs`, `crates/oag-storage/src/{tests,backup}.rs`,
`spec/19-observability.md`, `OPEN_QUESTIONS.md`.

**Feature implemented.** `events_pending_chain` (spec section 86): events whose `previous_event_id`
is set but whose predecessor is not present locally. `events_total` counts rows and cannot tell a
complete chain from a pile of fragments; during replication a fragment is the normal state, and a
persistent non-zero value means a predecessor is missing for good. The self-join is on the primary
key, so it is `O(n log n)`.

**Defect found and fixed while doing it.** The earlier `search()` signature change (adding
`offset`) had broken four tests in `oag-storage` -- three in `tests.rs`, one in `backup.rs` -- and
that crate was not in the set run when the change landed. All four are fixed, and the full
workspace suite now passes: 419 tests across 16 crates.

**Tests executed.** `cargo test --workspace` — all crates pass.

**Test results.** oag-storage 30 → 31. New: `pending_chain_counts_events_whose_predecessor_is_
missing`, which inserts a genesis event, a child whose predecessor IS present, and a child whose
predecessor never arrives. That dangling state is exactly what normal ingest never produces, so it
is built by hand rather than driven through the service.

**Performance measurements.** None. The query's complexity is stated (`O(n log n)` via the primary
key); no timing was measured and none is claimed.

**Commit:** `7086294` — pushed.
**Remaining defects.** `events_by_origin` (a labeled metric needing a decision on label
cardinality), `replication_bytes_in`/`out`, and `blob_store_size` remain unbuilt. The lesson from the
broken storage tests is recorded rather than just fixed: a signature change to a shared storage
function warrants running that crate, and the crate list in the engineering log should include it.

---

## 2026-11-09 — api_latency and search_latency histograms

**Repository:** `open-agent-graph` (`rust-ai`)
**Files created:** `crates/oag-graph/src/latency.rs`, `crates/oag-api/src/latency.rs`.
**Files modified:** `crates/oag-graph/src/{lib,service,search,semantic_search,metrics}.rs`,
`crates/oag-api/src/{lib,state,metrics}.rs`, `crates/oag-graph/src/tests.rs`,
`crates/oag-api/src/tests.rs`, `spec/19-observability.md`, `OPEN_QUESTIONS.md`.

**Feature implemented.** The two duration histograms spec section 86 names. `search_latency` is
recorded inside `GraphService::search` and `semantic_search` (so REST, MCP, CLI, and the Human
Interface all measure the same thing); `api_latency` is recorded by an axum middleware that sees
routing, auth, handler, and response as a whole. Fixed buckets, atomic counters, no dependency.
Values are milliseconds, matching `duration_ms` on every span.

**Why.** Every other metric here is a count, and a count answers "how much", not "how slow". The
per-request duration existed only as a `tracing` field, which is right for one request in a log and
useless for "is this peer getting slower?" across a million of them.

**Tests executed.** `cargo test -p oag-graph` — 94 passed. `cargo test -p oag-api` — 61 passed.

**Test results.** oag-graph 87 → 94 (6 histogram unit tests + `search_latency_records_every_
search_this_process_serves`). oag-api 60 → 61 (`api_latency_counts_rest_requests_end_to_end`,
which serves a real request and confirms the histogram counted it — a unit test of the histogram
would not prove the middleware is wired to it).

**Performance measurements.** None. No benchmark was run and none is claimed. The histograms cost
one atomic increment per observation by construction.

**Commit:** `fafc44a` — pushed.
**Remaining defects.** `events_by_origin` (a labeled metric needing a decision on label cardinality),
`events_pending_chain`, `replication_bytes_in`/`out`, and `blob_store_size` remain unbuilt. The
histogram sum is accumulated as integer microseconds, so a peer with extremely long uptime could in
principle lose precision in the sum; the bucket counts are unaffected.

---

## 2026-11-09 — order the mesh's anti-entropy pull loop

**Repository:** `oag-mesh`
**Files modified:** `crates/oag-mesh/src/transport.rs` (`sync_with_all_peers` orders its peers by
the `link_quality` ledger), `spec/10-anti-entropy.md`.

**Feature implemented.** The sequential pull loop now pulls best-quality peers first, so a periodic
tick gets the useful data before spending its time on peers that are probably down.

**Why.** `sync_with_all_peers` is sequential, so pulling in configuration order means every
black-holed peer is waited out -- a full connect/request timeout each, since the timeout fix landed
-- before a single live peer is reached. With a tick interval shorter than the accumulated
timeouts, the loop is permanently behind.

**Tests executed.** `cargo test -p oag-mesh` — 187 passed, 0 failed.

**Test results.** oag-mesh 186 → 187. New: `an_ordered_pull_loop_gets_live_data_before_paying_a_
dead_peers_timeout`, a two-arm control experiment. Same hanging peer, same live peer, same client;
the only difference is whether the ledger has an opinion. Without history the live data does not
arrive within the 1s window while the hanging peer blocks; with history it does.

**Performance measurements.** None. The total tick time is unchanged by ordering; what changes is
when the useful data arrives. No benchmark was run and none is claimed.

**Note on suite wall-clock.** The mesh's PoW tests are CPU-bound and the full suite's duration
varies substantially with machine load — the `corroboration::` tests alone measured 18s, 34s and
60s across three consecutive runs. That variance is environmental, not from this change.

**Commit:** `740576b` — pushed.
**Remaining defects.** Ordering does not reduce the total time a tick spends on dead peers; a
per-tick time budget would, and is not built. The ledger is still forgotten on restart, so the first
tick after a restart always pulls in configuration order.

---

## 2026-11-09 — sync client timeouts (base project)

**Repository:** `open-agent-graph` (`rust-ai`)
**Files modified:** `crates/oag-sync/src/client.rs`, `spec/18-rate-limiting-and-resource-limits.md`.

**Defect fixed.** The same one just fixed in the mesh: `SyncClient` was built from
`reqwest::Client::default()`, which sets no timeout. Its response *size* was already capped and
streamed (4 MiB, `MAX_RESPONSE_BYTES`), but nothing bounded how long a peer could make this node
wait. A peer that silently drops packets held a sync round open until the OS gave up.

**Fix.** `SYNC_CONNECT_TIMEOUT` (10s) and `SYNC_REQUEST_TIMEOUT` (120s). The total is deliberately
longer than the mesh's 30s: an `/events` response is a real batch of signed events, not a small
snapshot, so a legitimate peer on a slow link needs more room. `SyncClient::new()` uses the bounded
defaults; `sync_client_with_timeouts` takes the values as parameters so a test can prove the bound
without waiting the production value out; `with_http` lets an embedder supply its own transport.

**Tests executed.** `cargo test -p oag-sync` — 40 passed, 0 failed.

**Test results.** oag-sync 38 → 40. New: `a_peer_that_accepts_but_never_responds_is_abandoned_at_
the_timeout` and `the_default_client_is_bounded_and_reaches_a_real_peer`.

**Performance measurements.** None beyond the bound itself. No throughput benchmark was run.

**Commit:** `b2e2182` — pushed.
**Remaining defects.** The operator-configured external-service clients (LLM extractor, embeddings
provider, IPFS provider) still have no timeouts. They are a lower trust tier and are recorded in
`spec/18` rather than fixed silently. The CLI and gateway one-shot clients are likewise unbounded.

---

## 2026-11-09 — mesh outbound HTTP timeouts

**Repository:** `oag-mesh`
**Files modified:** `crates/oag-mesh/src/transport.rs` (new `HTTP_CONNECT_TIMEOUT` /
`HTTP_REQUEST_TIMEOUT` constants and `build_http_client`; both `MeshState` constructors now use
it), `spec/04-transport.md`, `OPEN_QUESTIONS.md`.

**Defect fixed.** Every outbound request used `reqwest::Client::new()`, which sets no timeout at
all. A peer that is unreachable *and silently dropping packets* — rather than refusing the
connection — holds the request open until the OS gives up, which can be minutes.

**Why it mattered more here than a slow pull.** `sync_with_all_peers` pulls peers sequentially, so
one black-holed peer stalls the whole tick before any live peer is reached. And
`search_for_route`'s fan-out calls `push_route_search` directly, on the path of an *inbound*
`POST /oag-mesh/v1/route-search` — so one broken peer can hang a request from a third party.

**Fix.** A 5s connect timeout (handshake never completes) and a 30s total request timeout
(connection succeeds, then silence). Both named constants; `build_http_client` takes them as
parameters so a test can prove the bound without waiting the production value out. A timed-out
request is an ordinary failed delivery: `pull_and_merge` reports it and `link_quality` records it.

**Tests executed.** `cargo test -p oag-mesh` — 186 passed, 0 failed.

**Test results.** oag-mesh 184 → 186. New: `a_peer_that_accepts_but_never_responds_is_abandoned_
at_the_timeout` (raw client, 50ms/200ms, asserts an error well inside 5s) and
`pull_and_merge_against_a_black_holed_peer_is_bounded_and_recorded` (end to end through the mesh's
own pull path, ~1.3s, and the failure is recorded).

**Performance measurements.** None beyond the bound itself: the pull returned in ~1.3s against a
200ms request timeout. No throughput benchmark was run and none is claimed.

**Commit:** `531b6f9` — pushed.
**Remaining defects.** The base project (`rust-ai`) has the same missing-timeout defect in
`oag-sync` and several CLI/API clients; not yet fixed there. The mesh's push-side broadcasts use
the same bounded client, so they are covered. The 30s total timeout is a starting value, not
derived from a deployment's link characteristics.

---

## 2026-11-09 — mesh link-quality fan-out ordering

**Repository:** `oag-mesh`
**Files created:** `crates/oag-mesh/src/link_quality.rs`.
**Files modified:** `crates/oag-mesh/src/lib.rs`, `crates/oag-mesh/src/transport.rs`
(`MeshState.link_quality`; `pull_and_merge` now returns whether it delivered; route search orders
peers and records both outcomes), `spec/13-distributed-route-search.md`, `OPEN_QUESTIONS.md`.

**Feature implemented.** A per-peer exponential moving average of whether each interaction
actually delivered. Route search asks its peers best-first, so the fan-out budget -- the one
scarce resource a search has -- is not spent on peers that have been refusing connections for
hours. Ordering only: the function is a permutation of the input, asserted as a property, so a
down peer is asked last rather than dropped.

**Design decisions.** Alpha 0.1, the same learning rate as the base project's tensor pad: a
single dropped packet moves a healthy peer by a tenth of the gap, so routing does not oscillate,
while a consistently-dead peer is demoted within a handful of rounds. Unseen peers score 0.5
(neutral, not optimistic) so a node with no history spreads its fan-out rather than preferring
configuration order. Both successes and failures are recorded -- a peer that did not answer is
exactly the observation the next search needs. In-memory only: losing it costs a little wasted
fan-out, and persisting it would add an eighth store and change the commit-marker set for a value
that is by nature ephemeral. It is a liveness signal, not a trust signal.

**Tests executed.** `cargo test -p oag-mesh` — 184 passed, 0 failed.

**Test results.** oag-mesh 175 → 184. New: 6 ledger unit tests (neutral unseen peer, dead sorts
last, ordering is a permutation that never drops, smoothed score, recovery, budget-of-one goes to
the proven peer) and 3 real-network transport tests (`a_failed_fan_out_is_recorded_and_the_next_
search_spends_its_budget_better`, `pull_and_merge_records_a_failure_against_an_unreachable_peer`,
`pull_and_merge_records_a_success_against_a_live_peer`).

**Performance measurements.** The ordering effect was measured on a real loopback network, not
modelled: with a dead peer and a fan-out budget of one, the first search returns `None` and the
second returns a path after a single observation. See `EXPERIMENTS.md` EXP-013. No throughput or
latency benchmark was run, and none is claimed.

**Commit:** `67ad91e` — pushed.
**Remaining defects.** Not exercised over radio, only loopback TCP. The end-to-end delivery-rate
effect is unmeasured -- only the budget-slot effect is. A peer that answers with garbage scores
exactly as well as one that answers correctly, which is deliberate (liveness, not trust) but means
this cannot be used as a reputation input.

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

---

## 2026-11-09 — keyset pagination for search

**Repository:** `open-agent-graph` (`rust-ai`)
**Files modified:** `crates/oag-storage/src/repo/nodes.rs`, `crates/oag-graph/src/search.rs`,
`crates/oag-graph/src/tests.rs`, `crates/oag-api/src/dto.rs`, `crates/oag-api/src/handlers.rs`,
`crates/oag-api/src/tests.rs`, `crates/oag-mcp/src/params.rs`, `crates/oag-mcp/src/server.rs`,
`crates/oag-mcp/src/tests.rs`, `crates/oag-cli/src/main.rs`, `crates/oag-cli/src/commands.rs`,
`spec/07-search.md`, `spec/12-cli.md`, `docs/api.md`, `OPEN_QUESTIONS.md`,
`PROJECT_STATE.md`, `ARCHITECTURE_DECISIONS.md`.

**Feature implemented.** `cursor` / `--cursor` on REST, MCP, and CLI, backed by
`CollisionIndex`-free keyset search in the storage layer and a single `GraphService::search_page`
so the surfaces cannot drift. `ORDER BY rank` became `ORDER BY rank, nodes.node_id`.

**Tests executed.** `cargo test --workspace -j 2`: 398 passed, 0 failed (was 393). New tests:
`search_cursor_walk_covers_every_match_once_across_an_insertion` (service),
`search_cursor_rejects_a_malformed_token_by_starting_over` (service),
`search_cursor_walk_does_not_repeat_a_row_when_a_match_arrives_mid_walk` (REST).

**Performance measurements.** None. The cursor path adds one indexed lookup per page (resolving
the cursor row's current rank); the offset path is unchanged.

**Commit identifiers:** `2d28af3`.
**Push status:** pushed to `origin/main`.
**Remaining defects.** A corpus change that *reorders* rows across the cursor boundary still
repeats or skips a row; unfixable by any pagination scheme and documented as such. The Human
Interface keeps offset-only paging.

---

## 2026-11-09 — the embedded IPFS node keeps a stable dial identity

**Repository:** `open-agent-graph` (`rust-ai`)
**Files modified:** `crates/oag-ipfs/src/embedded.rs`, `crates/oag-ipfs/Cargo.toml`,
`spec/29-ipfs-pinning.md`, `ARCHITECTURE_DECISIONS.md`.

**Feature implemented.** `EmbeddedIpfsProvider::new` loads or creates `<repo>/libp2p.key` and
builds with `DefaultIpfsBuilder::with_keypair` instead of `new()`, which generated a fresh
keypair per call. Added `peer_id()` and `shutdown()` accessors; `shutdown()` is needed because
dropping the handle does not release the repo lock.

**Tests executed.** `cargo test -p oag-ipfs`: 14 passed, 0 failed (was 11). New tests:
`the_libp2p_identity_survives_a_restart`, `a_corrupt_identity_file_is_refused_rather_than_regenerated`,
`the_persisted_identity_is_owner_only`.

**Performance measurements.** None; the change is a file read on startup.

**Commit identifiers:** `274ff31` (code), `9007284` (lockfile).
**Push status:** pushed to `origin/main`.
**Remaining defects.** None known. The key file is a secret and is written `0o600` on Unix; on
other platforms it relies on the containing directory's permissions, same as OAG's own
`identity.key`.

---

## 2026-11-09 — source independence accounts for mirrored content

**Repository:** `open-agent-graph` (`rust-ai`)
**Files modified:** `crates/oag-graph/src/corroboration.rs`, `crates/oag-graph/src/tests.rs`,
`spec/06-corroboration-and-authority.md`, `ARCHITECTURE_DECISIONS.md`.

**Feature implemented.** `source_independence` now groups evidence by the connected components of
two relations -- same control group, or same `content_hash` -- instead of the control group alone.
One article syndicated to several domains was previously scored as several independent sources.

**Tests executed.** `cargo test -p oag-graph`: 101 passed, 0 failed (was 98). New tests:
`identical_content_on_two_domains_is_one_source_not_two`,
`distinct_content_on_one_domain_is_still_one_source`, `union_find_joins_transitively_and_is_deterministic`.

**Performance measurements.** None; the extra work is one hash map per corroboration query, which
was already O(evidence items) and still is.

**Commit identifiers:** `26b45da`.
**Push status:** pushed to `origin/main`.
**Remaining defects.** A caller can assert distinct `content_hash` values for identical content and
regain the independence the grouping removes; `content_hash` is caller-asserted on the REST/MCP
path. Documented in `spec/06`.

---

## 2026-11-09 — per-store heads in the mesh anti-entropy pull

**Repository:** `oag-mesh`
**Files modified:** `crates/oag-mesh/src/transport.rs`, `spec/10-anti-entropy.md`,
`OPEN_QUESTIONS.md`.

**Feature implemented.** `pull_and_merge` recorded a peer's `StoreHeads` only when *every*
attempted store pull succeeded. With one persistently failing store that condition never became
true, so no heads were ever recorded and all five stores were re-pulled in full on every tick,
forever. Heads are now recorded per store: a failed store keeps its previous version, or
`UNKNOWN_VERSION` (`u64::MAX`, which no monotonic sequence reports) when there is none, so it is
retried while the healthy stores are not.

**Tests executed.** `cargo test -p oag-mesh`: 190 passed, 0 failed (was 188). New tests:
`a_failing_store_is_retried_without_re_pulling_the_healthy_ones` (integration -- the failing
endpoint is fetched on both ticks, the healthy one on the first only),
`merge_heads_records_each_store_independently` (unit).

**Performance measurements.** Not benchmarked, but the effect is a full five-store snapshot fetch
becoming a single failing-store fetch on each tick once one store is persistently broken. The
healthy case is unchanged.

**Commit identifiers:** `04869e5` (`oag-mesh`).
**Push status:** pushed to `origin/main`.
**Remaining defects.** The in-memory merges across a pull are still not atomic, and `save_to_dir`
remains the embedder's call. Found while re-reading the open question: **no code in either
repository calls `save_to_dir`/`load_from_dir`** -- `open-agent-graph` does not depend on
`oag-mesh` yet, so the persistence layer is correct and tested but unreachable in production.
Recorded in `oag-mesh/OPEN_QUESTIONS.md`.

---

## 2026-11-09 — the tensor pad can form pathways, and its adaptation is measured

**Repository:** `oag-imagination-random-data-compute`
**Files modified:** `crates/oag-imagination/src/engine.rs`, `spec/02-tensor-pad-routing.md`,
`RESEARCH_JOURNAL.md` (mission-wide records live in `open-agent-graph`).

**Feature implemented.** `Engine::with_source` seeds the pad to one slot per neighbour, filling
missing slots with the mean of the existing ones. The pad and the neighbour list come from different
layers and nothing reconciled them, so a neighbour past the end of the pad could be routed to (the
`1e-6` floor keeps it reachable) but never reinforced, because `reinforce` had no slot to add to.
That deadlock meant the pad could strengthen a path and let others lose share but could never *form*
one.

**Tests executed.** `cargo test -p oag-imagination`: 47 passed, 0 failed (was 45). New tests:
`reinforcing_a_neighbour_shifts_routing_toward_it` (the pad's central claim, measured as a
distribution over 256 fixed seeds -- `route` is deterministic given pad/neighbours/seed, so it is
an exact measurement, not a sample),
`a_neighbour_beyond_the_pad_is_explorable_not_frozen`.

**Performance measurements.** None; the change is a `Vec::resize` at engine construction.

**Commit identifiers:** `48c619a` (`oag-imagination-random-data-compute`).
**Push status:** pushed to `origin/master`.
**Remaining defects.** `reinforce` only adds, so the pad cannot weaken or dissolve a pathway: a
path that stops carrying traffic keeps its absolute weight forever. Recorded in `spec/02` and in
`RESEARCH_JOURNAL.md` as a proposed design (decay) with the measurement that would justify it,
rather than implemented on speculation.

---

## 2026-11-09 — the tensor pad is normalized, and a zero slot is seeded

**Repository:** `oag-imagination-random-data-compute`
**Files modified:** `crates/oag-imagination/src/engine.rs`, `spec/02-tensor-pad-routing.md`,
`RESEARCH_JOURNAL.md` (mission-wide records live in `open-agent-graph`).

**Feature implemented.** Pads are normalized to a mean of `PAD_MEAN` (1.0) at construction and
after every reinforcement. Routing is unchanged -- it reads weight ratios only -- but the pad is no
longer traffic-volume-weighted where it is published and where `combine_pads` adds two pads
element-wise. A slot that is missing *or zero* is seeded to the mean of the slots carrying
information; a zero slot was the same frozen-neighbour trap as a missing one, reachable from a pad
that came back from the layer below with never-used slots.

**Tests executed.** `cargo test -p oag-imagination`: 49 passed, 0 failed (was 47). New tests:
`two_nodes_with_the_same_preference_produce_the_same_pad`, `a_zero_slot_is_seeded_like_a_missing_one`;
`a_neighbour_beyond_the_pad_is_explorable_not_frozen` updated for normalization.

**Performance measurements.** None; the added work is one O(pad length) pass per routing decision,
and the pad is small.

**Commit identifiers:** `239b49c` (`oag-imagination-random-data-compute`).
**Push status:** pushed to `origin/master`.
**Remaining defects.** The pad cannot forget: a path reinforced once keeps its weight forever, so a
topology change dilutes it rather than removing it. Restated precisely in `RESEARCH_JOURNAL.md`,
which also corrects the previous entry's over-claim.

---

## 2026-11-09 — bound every wait on the imagination TCP carrier

**Repository:** `oag-imagination-random-data-compute`
**Files modified:** `crates/oag-imagination/src/socket.rs`, `spec/03-transports.md`.

**Feature implemented.** `INBOUND_READ_TIMEOUT` (30s) closes a connection whose peer completes the
handshake and then sends nothing; `UPSTREAM_TIMEOUT` (30s) bounds the connect to the next hop and
the wait for its reply. Before these, a stalling client held a spawned task and a socket for as long
as it liked, and a black-holed next hop held the task for the OS connect timeout (minutes).
`serve_on_with_timeouts` exposes the values so a deployment can retune them and a test can make
them short; `serve`/`serve_on` keep the defaults.

**Tests executed.** `cargo test -p oag-imagination`: 50 passed, 0 failed (was 49). New test:
`a_client_that_sends_nothing_is_dropped_at_the_inbound_timeout` (50ms timeout, 2s assertion bound).

**Performance measurements.** None; a timeout wrapper per read.

**Commit identifiers:** `c7f64db`.
**Push status:** pushed to `origin/master`.
**Remaining defects.** Nothing capped concurrent connections, so many slow clients could still
exhaust tasks and sockets even though each was finite. Closed in `0d21ee0`; recorded in
`spec/03-transports.md` rather than left implied.

---

## 2026-11-09 — cap concurrent connections on the imagination carrier

**Repository:** `oag-imagination-random-data-compute`
**Files modified:** `crates/oag-imagination/src/socket.rs`,
`crates/oag-imagination/src/lib.rs`, `spec/03-transports.md`.

**Feature implemented.** `DEFAULT_MAX_CONNECTIONS` (1024) bounds how many connections the TCP
carrier serves at once, via a semaphore acquired *before* `accept`, so a full server stops taking
new connections rather than queueing them. `Limits` replaces the `Timeouts` struct from the previous
commit (the cap is not a timeout); `serve_on_with_limits` exposes it, `serve`/`serve_on` keep the
defaults. Closes the remaining defect the previous entry named.

**Tests executed.** `cargo test -p oag-imagination`: 51 passed, 0 failed (was 50). New test:
`a_full_server_does_not_serve_a_second_connection_concurrently`.

**Performance measurements.** None.

**Commit identifiers:** `0d21ee0`.
**Push status:** pushed to `origin/master`.
**Remaining defects.** None known for this surface. The UDP carrier has no equivalent cap, and
`serve_udp` is documented as single-datagram-in/single-datagram-out, so it is a different shape.

---

## 2026-11-09 — the crawler pins its side-fetches too

**Repository:** `open-agent-graph` (`rust-ai`)
**Files modified:** `crates/oag-crawler/src/crawler.rs`, `crates/oag-crawler/src/tests.rs`,
`spec/29-ipfs-pinning.md`, `OPEN_QUESTIONS.md`.

**Feature implemented.** The `llms.txt`, ARD, and agent-card bodies are now pinned and their
`pinned_cid` recorded on the evidence they back, best-effort exactly like the main page's. Extracted
assertions clone that evidence, so they inherit the CID. The pin block was factored into
`pin_best_effort` so all four call sites share one failure posture.

**Why the deferred decision was reversed.** The recorded reasoning was that the side-fetches are
metadata-discovery mechanisms rather than primary content. But they are the *evidence* for every
relationship extracted from them, and the evidence row carries their URI and content hash. Citing
an assertion's backing content by URI while leaving the bytes unretrievable makes it unverifiable by
a third party -- the reason to record the evidence at all. The decision was right about the trade
and wrong about which side of it those files were on.

**Tests executed.** `cargo test -p oag-crawler`: 75 passed, 0 failed (was 74). The test that
encoded the old behaviour was rewritten as
`pinning_provider_cid_lands_on_the_page_and_every_side_fetch`: every evidence item carries the CID
and exactly four bodies are pinned (the page plus three side-fetches).

**Performance measurements.** Up to three extra pins per crawled page, each an HTTP call to the
pinning provider or a local add. Best-effort, so a pin failure costs a log line and nothing else.

**Commit identifiers:** `b8e1f71`.
**Push status:** pushed to `origin/main`.
**Remaining defects.** None known. The crawl's side-fetch pins are sequential with the main page's,
so a slow pinning provider adds latency to a crawl; parallelising them would change the failure
ordering for no correctness gain.

---

## 2026-11-09 — the mesh gains a connection to the layer below

**Repository:** `oag-mesh`
**Files modified:** `crates/oag-mesh/src/node.rs` (new), `crates/oag-mesh/src/lib.rs`,
`crates/oag-mesh/Cargo.toml`, `Cargo.toml`, `Cargo.lock`, `spec/08-persistence.md`,
`OPEN_QUESTIONS.md`.

**Feature implemented.** `node::MeshNodeAdapter` builds a `MeshState` from an existing `oag`
deployment: identity from `<data-dir>/identity.key` (generated if absent, the same file `oag`
uses), peers from the base peer's own peer tables, mesh stores from `<data-dir>/mesh/`, and saves
them back. Base-project addresses are scheme-less and `oag-sync` prepends `http://`; the adapter
applies the same convention so the addresses are dialable.

**Why this matters.** The mesh had no code connecting it to the layer below. `MeshState::new` takes
an identity and a peer list, but nothing constructed them from a real deployment, and -- as found
two invocations ago -- nothing in either repository called `save_to_dir` or `load_from_dir`. The
commit marker and torn-set refusal had therefore never run outside a test. The adapter is the first
real caller and makes the mesh standable up next to an existing peer with no new configuration.

The identity is shared deliberately: a `BridgeDeclaration` signed by this node is then attributable
to a peer that already exists, and the mesh's peer id matches the id the base project uses for that
node. The adapter reads the base project's files and never writes them.

**Tests executed.** `cargo test -p oag-mesh`: 193 passed, 0 failed (was 190). New tests:
`the_adapter_loads_the_identity_and_peers_from_an_oag_data_dir`,
`an_address_that_already_has_a_scheme_is_left_alone`,
`save_then_load_keeps_the_mesh_state_and_leaves_the_base_files`.

**Performance measurements.** None; the adapter runs at startup and on an operator-triggered save.

**Commit identifiers:** `d4c52a7` (`oag-mesh`).
**Push status:** pushed to `origin/main`.
**Remaining defects.** `save_to_dir` still does not create its directory, so the adapter does; a
caller using `save_to_dir` directly still has to. The base project's identity is loaded or
generated, so a data directory without `identity.key` gets a fresh mesh identity -- correct for a
first run, but it means the mesh node and a later-created base peer would not share one.

---

## 2026-11-09 — measured what the tensor pad converges to

**Repository:** `oag-imagination-random-data-compute`
**Files modified:** `crates/oag-imagination/examples/convergence.rs` (new),
`spec/02-tensor-pad-routing.md`, `RESEARCH_JOURNAL.md`.

**Artifact.** `cargo run --example convergence [-- neighbours N] [-- decisions N] [--offset N]`
runs the real engine over a deterministic source (`BLAKE3(counter)`) with a resolver that never
answers, and prints the chosen-neighbour share at checkpoints. Deterministic, so a run is
reproducible and comparable.

**Result.** With 4 neighbours and 20,000 decisions, one neighbour takes ~96% of routing (0.568 at
1,000; 0.866 at 5,000; 0.933 at 10,000; 0.966 at 20,000). The pad mean stays exactly 1.0000, which
also confirms the normalization added earlier is stable under reinforce-then-renormalize.

**The control.** `--offset` shifts the source's start: offsets 0-3 crown `node-2`, offsets 4-5 crown
`node-3`, at the same ~96%. So the winner is decided by the early random draws, not by any property
of the neighbours.

**What it establishes.** `reinforce` adds a constant 1.0 to whichever hop was taken, regardless of
outcome, so the reinforcement carries no information about hop quality. The pad is a rich-get-richer
random walk that locks onto an arbitrary neighbour -- not learning. This corrects the previous
research entry's "bandit with no exploration term": a bandit's reward varies with the outcome, and
here it does not vary at all. It also reframe the decay question: decay would add forgetting without
adding signal, so the missing piece is a utility signal from the hop's outcome, which the TCP carrier
already observes and `Engine::handle` currently discards.

**Tests executed.** No test added; the artifact is the example, run by hand. `cargo test -p
oag-imagination` remains 51 passed, 0 failed.

**Performance measurements.** 20,000 decisions in ~0.47s (~42,000 decisions/sec), single-threaded,
debug build.

**Commit identifiers:** `f7ea069` (`oag-imagination-random-data-compute`).
**Push status:** pushed to `origin/master`.
**Remaining defects.** The pad has no utility signal, so its documented "adaptation" is lock-in
rather than learning. Recorded as a proposed design in `RESEARCH_JOURNAL.md`, not implemented.

---

## 2026-11-09 — the pad learns from observed utility

**Repository:** `oag-imagination-random-data-compute`
**Files modified:** `crates/oag-imagination/src/engine.rs`,
`crates/oag-imagination/src/socket.rs`, `crates/oag-imagination/src/lib.rs`,
`crates/oag-imagination/examples/convergence.rs`, `spec/02-tensor-pad-routing.md`,
`RESEARCH_JOURNAL.md`.

**Feature implemented.** `Engine::handle` no longer reinforces the hop it chose. The TCP carrier
forwards the request, observes the outcome, and calls the new `Engine::observe_hop(neighbour,
HopOutcome)`: `Answered` multiplies the neighbour's weight by `1.05`, `Failed` by `0.90`, and
`Forwarded` leaves it alone, followed by normalization. Multiplicative rather than additive so a
penalty can never make the sum negative and invert the ordering.

**Why.** The convergence measurement showed the pad locking onto an arbitrary neighbour at ~96%
with the winner decided by the source offset: the constant reinforcement carried no information
about hop quality. The carrier is the only place that knows the outcome, and it was discarding it.

**Tests executed.** `cargo test -p oag-imagination`: 51 passed, 0 failed. The test that asserted
"the pad strengthened for the chosen hop" now asserts the opposite -- routing alone leaves the pad
untouched -- and that only `observe_hop` moves it. Workspace including Reticulum: 54 passed.

**Performance measurements.** `examples/convergence.rs`, 4 neighbours, 20,000 decisions: signal arm
converges to the answering neighbour at 99.8%, control arm (old constant reinforcement) to an
arbitrary one at 59.6%. Both arms share their first few hundred draws, so the difference is the
signal, not the luck. ~42,000 decisions/sec single-threaded debug build.

**Commit identifiers:** `d862492` (`oag-imagination-random-data-compute`).
**Push status:** pushed to `origin/master`.
**Remaining defects.** The gain and penalty constants (`1.05`, `0.90`) are round numbers, not tuned;
a sweep is the obvious next measurement. Only the TCP carrier feeds back, because it is the only
carrier that forwards -- a UDP-served node keeps a pad that does not learn. And the reward is binary
and delayed by the whole downstream chain, so credit assignment across a multi-hop path is coarse.

---

## 2026-11-09 — one log-space step for the pad, and a measured recovery time

**Repository:** `oag-imagination-random-data-compute`
**Files modified:** `crates/oag-imagination/src/engine.rs`, `crates/oag-imagination/src/lib.rs`,
`crates/oag-imagination/examples/convergence.rs`, `spec/02-tensor-pad-routing.md`,
`RESEARCH_JOURNAL.md`.

**Feature implemented.** `HOP_ANSWERED_GAIN` (1.05) and `HOP_FAILED_PENALTY` (0.90) -- two
independent round numbers -- are replaced by one log-space step, `HOP_STEP = 0.1`, applied as
`exp(+HOP_STEP)` for an answered hop and `exp(-HOP_STEP)` for a failed one. The answered/failed
ratio per comparison is `exp(2 * HOP_STEP)` (~1.22) and the weight-ratio half-life is
`ln 2 / HOP_STEP` (~6.9 observations). The ratio is what carries meaning, and one step makes the
memory horizon derivable rather than tuned.

`examples/convergence.rs` gained `--switch-at N`, which changes the answering neighbour mid-run and
reports when the new neighbour's *weight* overtakes the old one's.

**Tests executed.** `cargo test -p oag-imagination`: 51 passed, 0 failed.

**Performance measurements.**
- A/B, 4 neighbours, 20,000 decisions, node-0 answering and the rest unreachable: signal arm
  99.9% on node-0, control arm (old constant reinforcement) 59.6% on an arbitrary node-2.
- Recovery: `--switch-at 10000` -- the new neighbour's weight overtakes the old one's **1,061
  decisions** later. That is ~150x the 6.9-observation half-life, and the gap is real: the
  half-life describes how fast a weight *ratio* moves per observation, but a deeply locked-in
  neighbour is selected almost every time, so its selection rate falls as its weight falls and the
  decay slows. Recovering from a ~1000:1 lock-in takes ~1,000 decisions, not ~66.

**Commit identifiers:** `00208cf` (`oag-imagination-random-data-compute`).
**Push status:** pushed to `origin/master`.
**Remaining defects.** Choosing `HOP_STEP` is now a stated trade with a measured number attached
(small step = noise-stable, slow to abandon a failing path; large step = the reverse) rather than two
unexplained constants. It is still a single global constant -- a per-neighbour or per-path step
would be the next refinement. And only the TCP carrier feeds outcomes back.

---

## 2026-11-09 — the chaos test measures its errors, and 429s say when to retry

**Repository:** `open-agent-graph` (`rust-ai`)
**Files modified:** `crates/oag-sync/tests/distributed_convergence.rs`,
`crates/oag-sync/src/rate_limit.rs`, `crates/oag-sync/src/tests.rs`,
`crates/oag-api/src/rate_limit.rs`, `spec/18-rate-limiting-and-resource-limits.md`,
`RESEARCH_JOURNAL.md`.

**What the ignored test surfaced.** `fifty_peers_converge_after_chaos` is `#[ignore]`d as slow and
had never been run in this session. It passes: 50 peers, 40 chaos rounds, 5 kills, 3 restarts, 5
partitions, 5 heals, 64 events converged, 0 timeouts. Its printed label for the error count --
"transient SQLite-contention errors" -- was an assumption: the test discarded the messages.

**Feature implemented.** The test now keeps the error strings, classifies them, and prints a
breakdown with a sample per category. Both rate limiters (sync's global and REST/MCP's per-key) now
send `Retry-After`, computed from the window they already track.

**Tests executed.** `cargo test -p oag-sync --test distributed_convergence -- --ignored`:
1 passed (50 peers, 77s). `cargo test -p oag-api --lib`: 64 passed (was 63; new
`a_rejected_key_is_told_when_to_retry`). `cargo test -p oag-sync --lib`: 41 passed.

**Performance measurements.** One chaos run: 4,565 recoverable errors, of which **2,594 were 429s
from the limiter itself**, 1,971 SQLite lock contention (`database is locked`, codes 5 and 517),
105 event fetch failures. So the old label was ~43% right. The count varies between runs (2,486 in
an earlier run) because it is timing-dependent. None of it blocked convergence.

**Commit identifiers:** `cb3d6f4`.
**Push status:** pushed to `origin/main`.
**Remaining defects.** `SQLITE_BUSY_SNAPSHOT` (517) is not helped by `busy_timeout` -- WAL mode
returns it immediately when a read transaction cannot upgrade to a write while another connection
holds the writer. A single-writer queue would remove it and is not built. No sync client honours
`Retry-After` yet either; the limiter now states it and the client still treats a 429 as a generic
failure retried on the next cycle.

---

## 2026-11-09 — the sync client consumes Retry-After and the gossip loop backs off

**Repository:** `open-agent-graph` (`rust-ai`)
**Files modified:** `crates/oag-sync/src/client.rs`, `crates/oag-sync/src/service.rs`,
`crates/oag-sync/src/gossip.rs`, `crates/oag-sync/src/tests.rs`, `spec/18-rate-limiting-and-resource-limits.md`,
`RESEARCH_JOURNAL.md`.

**Feature implemented.** `SyncClient` now turns a 429 into `SyncClientError::RateLimited {
retry_after }` instead of letting `error_for_status` flatten it into a generic failure, carrying the
peer's `Retry-After` (or a 30s default when absent). `SyncService` records a per-address deadline
from it -- keeping the later of two deadlines -- and `gossip::sync_round` skips any address still
inside that window.

**Why.** The previous entry made both limiters *send* `Retry-After`; nothing consumed it. A client
that cannot tell "rate limited, retry at T" from "peer down" has to guess, and the guessing is what
produced 2,594 retries against a limiter that had already answered.

**Tests executed.** `cargo test -p oag-sync --lib`: 43 passed, 0 failed (was 41). New:
`a_rate_limited_peer_is_distinct_from_a_dead_one_and_is_backed_off` (the 429 is distinct and
carries a non-zero `Retry-After`; the service records the backoff),
`sync_round_leaves_a_peer_in_its_rate_limit_backoff_window_alone` (a dead address increments the
shared error counter; a backed-off one does not).

**Performance measurements.** None added. The chaos test still drives `sync_with_peer` directly
and so still exercises the limiter rather than the backoff -- stated in `spec/18` rather than left
for a reader to assume the loop was measured.

**Commit identifiers:** `5772e11`.
**Push status:** pushed to `origin/main`.
**Remaining defects.** A 429 on `fetch_events` is recorded as a backoff *and* counted as a
per-event fetch failure, so it is double-counted. `Retry-After` is honoured by the sync client only;
`oag-api`'s own outgoing calls (the gateway, the crawler) do not consume it either.

---

## 2026-11-09 — full verification pass, and the SQLITE_BUSY_SNAPSHOT question recorded not guessed

**Repositories:** all three.

**Tests executed.**
- `cargo test --workspace -j 2` in `open-agent-graph`: **445 passed, 0 failed**.
- `cargo test -j 2` in `oag-mesh`: **193 passed, 0 failed**.
- `cargo test --workspace -j 2` in `oag-imagination-random-data-compute`: **54 passed, 0 failed**
  (includes the Reticulum carrier).
- `oag-collisions`: **29 passed, 0 failed**.

**Why a full pass.** This closed a long run of increments (keyset pagination, persisted libp2p
identity, content-based source independence, side-fetch pinning, per-store sync heads, the mesh node
adapter, the pad's utility signal and step, rate-limit measurement and backoff) across three
repositories. A full pass after that many changes is the check that they compose.

**Commit identifiers:** none; verification only.
**Push status:** n/a.

**SQLITE_BUSY_SNAPSHOT (517), recorded rather than guessed.** The chaos test reports 1,971
`database is locked` errors, codes 5 and 517. `busy_timeout(5s)` is set, and the usual cause of
517 -- a deferred transaction that reads and then upgrades to a write -- is *not* present: the
storage layer opens no explicit transactions and issues no `RETURNING`. So the trigger is not
identified, and the three candidate explanations (sqlx holding a read snapshot across statements on
a pooled connection, the FTS external-content triggers reading the index before writing, or the
interaction of 50 peers' worth of concurrent writers) are untested. **Status: open question, not a
fix.** A single-writer queue is the standard remedy for 517 but would serialise every write in the
process; committing to it without knowing the cause and without measuring the write-throughput cost
would be exactly the kind of uninformed change this session has been correcting.

---

## 2026-11-09 — BEGIN IMMEDIATE: the SQLite lock contention goes to zero

**Repository:** `open-agent-graph` (`rust-ai`)
**Files modified:** `crates/oag-events/src/ingest.rs`, `crates/oag-events/src/commit.rs`,
`crates/oag-graph/src/redaction.rs` (two sites), `crates/oag-graph/src/service.rs`,
`crates/oag-storage/src/tests.rs`, `crates/oag-events/src/tests.rs`,
`crates/oag-sync/tests/distributed_convergence.rs`, `ARCHITECTURE_DECISIONS.md`,
`RESEARCH_JOURNAL.md`.

**Root cause found, not guessed.** The previous entry recorded `SQLITE_BUSY_SNAPSHOT` as an open
question because the usual cause -- a deferred transaction reading then writing -- appeared absent.
It was present; the earlier grep searched `oag-storage` and `oag-sync` and the transactions live in
`oag-events` and `oag-graph`. Four read-then-write sites used `pool.begin()`, which takes the write
lock only at the first write, so a concurrent commit left the reader unable to upgrade.

**Feature implemented.** All four use `pool.begin_with("BEGIN IMMEDIATE")`. `rebuild` keeps a
deferred begin: its first statement is a write, so it never upgrades, and holding the write lock
across a whole rebuild would cost more.

**Tests executed.**
- `cargo test -p oag-storage`: 36 passed (was 35). New
  `begin_immediate_removes_the_busy_snapshot_failures_a_deferred_begin_causes` -- an A/B at 96
  concurrent writers: deferred 35x code-5 + 32x code-517, immediate 0 failures.
- `cargo test -p oag-events --lib`: 22 passed. `commit_local_event_itself_is_not_safe_for_concurrent_same_peer_calls`
  **reversed**: it asserted the commits must race, and its own message said to rerun before
  concluding if that changed. Reran three times, then reversed -- it now asserts 0 errors and
  contiguous sequences 1..=20.
- `cargo test -p oag-graph`, `-p oag-sync`: 101 and 43 passed.
- `cargo test -p oag-sync --test distributed_convergence -- --ignored`: 1 passed.

**Performance measurements.** The chaos run before this change: 4,565 recoverable errors (2,594
rate-limit 429s, 1,971 SQLite locked, 105 fetch failures). After: **635 errors, all of them the rate
limiter; `sqlite locked` 0** (was 1,971). Counts are timing-dependent between runs, so the
comparison is the same test on the same seed before and after, not two runs after.

**Commit identifiers:** `419c7bc`.
**Push status:** pushed to `origin/main`.
**Remaining defects.** `GraphService::commit_lock` is no longer load-bearing for 517; kept as
defence in depth and documented as such. `rebuild`'s deferred begin is correct only because its
first statement writes -- a reorder would reintroduce the hazard, and there is no test pinning that
ordering.

---

## 2026-11-09 — rebuild takes BEGIN IMMEDIATE too, and every transaction is now uniform

**Repository:** `open-agent-graph` (`rust-ai`)
**Files modified:** `crates/oag-graph/src/rebuild.rs`, `crates/oag-graph/src/tests.rs`,
`ARCHITECTURE_DECISIONS.md`.

**Feature implemented.** `rebuild_projection` opens `BEGIN IMMEDIATE`. ADR-015 exempted it on the
reasoning that its first statement writes, so the write lock was taken immediately anyway and the
exemption cost nothing -- which was right about the cost and wrong about the risk: correctness
rested on `clear_projection_tables` staying first, and nothing tested that. A deferred transaction
that writes holds the lock until commit regardless, so taking it up front costs nothing here either.

**Tests executed.** `cargo test -p oag-graph`: 102 passed, 0 failed (was 101). New
`a_rebuild_concurrent_with_local_commits_produces_no_locking_error`: 200 seeded events, a rebuild
running concurrently with 32 commits through a *second* service handle (its own commit lock, so the
serialisation is the database's, not the application's). Both must succeed. `oag-events` 22,
`oag-storage` 36.

**Performance measurements.** None; the change is which statement acquires the write lock.

**Commit identifiers:** `2bc1537` (code and test), `d791a84` (ADR addendum).
**Push status:** pushed to `origin/main`.
**Remaining defects.** The new test asserts the property, not the overlap: the rebuild may finish
before the commits start. It would not have caught the original deferred-begin bug either, because
that bug needed a read *before* the first write -- which is exactly the ordering the change removes.

---

## 2026-11-09 — the relay chain of custody exists and is now tested; spec/22 corrected

**Repository:** `open-agent-graph` (`rust-ai`)
**Files modified:** `crates/oag-api/src/transfer.rs`, `spec/22-future-work.md`,
`RESEARCH_JOURNAL.md`.

**Finding.** `spec/22-future-work.md` claimed "No `hop_count`/`via_peer` field or multi-hop
relay-with-witness structure exists anywhere today." That is stale twice over:
`UnsignedTransferRequest` has `relay_hops_remaining` and `relay_path`, `UnsignedGatewayRequest` has
`hops_remaining` and `path`, each hop appends itself and refuses to forward to a peer already in the
chain, and the server records `relayed_via` on its evidence. The existing relay test exercises the
chain but never asserts the path.

**Feature implemented.** `a_relay_refuses_to_forward_to_a_peer_already_in_the_chain` -- a unit test
of `relay_fetch` with a dead relay address, so a `None` can only come from the path check. No network,
no timing.

**Tests executed.** `cargo test -p oag-api`: 65 passed, 0 failed (was 64).

**Commit identifiers:** pending.
**Push status:** pending.
**Remaining defects.** An attempt to write an *integration* test for a cyclic relay configuration
(two peers relaying to each other, neither holding the content) hung past a 20s bound. The path check
is not the cause -- the unit test above proves it refuses. The hang is undiagnosed and is recorded
here rather than claimed as a defect; the prime suspect is `EmbeddedIpfsProvider::fetch`, which has
no timeout and may block on a CID nobody can serve. That is a separate question and is not asserted.
