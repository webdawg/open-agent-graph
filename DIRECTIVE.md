PROJECT OAG — PERPETUAL DIRECTIVE
THE AUDIO SERVING DAEMON (oag-listend)

You will receive this exact directive repeatedly, for a long time. Every invocation is a continuation. Do not restart. Do not re-audit the build environment. Do not write another architecture essay. Leave the project measurably closer to a daemon that is turned on, exchanges audio, and exchanges nothing else.

---

THE ONLY GOAL

Build oag-listend until this is true, and then keep it true:

1. The daemon turns on and listens.
2. Anything that connects can send audio in, and receive audio out.
3. The only accepted input is audio. Everything else is rejected before it is stored.
4. What goes out is audio and nothing else: random generated sounds, and sounds that arrived as input.
5. A second process can connect, push random audio, and pull audio back, and both sides can prove the bytes are the bytes they claim.

"Turn this thing on, and then audio back and forth and nothing else — random generated data, random input data, sounds."

That sentence is the acceptance test. A spec, a sniff function, or a store with no listener is not done.

Founding statements, verbatim. Do not paraphrase them in records:

> this is the global daemon that serves all audio information to all networks

> we are building a reall audio serving service that serves audio to any present and future technology - any way that anyone connects to this service - it just serves audio

> the service also listens too all incoming but only accepting audio as an input and nothing else

License of oag-listend and of every OAG repository: GNU Affero General Public License, version 3 or any later version (AGPL-3.0-or-later). v3 is the current AGPL. Do not relicense.

---

WHAT "WORKS" MEANS

Minimum bar, in this order. Do not skip ahead.

1. A listener. One carrier first: TCP is enough. Bind a local address. Accept connections. Bound every read and write (timeout). Never an unbounded read.
2. A frame. Length-prefixed messages so one connection can do many exchanges. The frame carries an operation and bytes. Operations are only: publish audio, fetch audio by content hash, list known hashes, and a small hello that says what this daemon is. No chat, no JSON-RPC kitchen sink, no arbitrary command channel.
3. Inbound rule on the wire. Every publish goes through `AudioStore::accept`. The check is on the bytes (WAV, AIFF, FLAC, Ogg, MP3, M4A signatures already in the crate), never on a declared type. A connection that sends HTML, JSON, an empty body, or an MP4 video brand gets a typed rejection and nothing is stored.
4. Outbound is audio bytes plus the BLAKE3 of those bytes. The receiver verifies. A fetch of an unknown hash is a typed error, not a hang.
5. Random sounds. A generator in the crate (or `examples/`) that writes valid WAV: silence, a tone, and noise, from a seed, so tests are reproducible. The daemon can publish generated audio on startup (a small bounded set) so a client that only fetches still receives sound.
6. Back and forth. One integration test, time-bounded: start the listener on 127.0.0.1:0, connect, publish a generated WAV, fetch it back, `AudioItem::verify` passes, publish non-audio and observe rejection, fetch a hash that was never published and observe not-found. Then shut the listener down. No sleep-and-hope; wait on the bound address.
7. A binary. `oag-listend` (or the crate's example/bin) that an operator can run: listen address, optional seed for generated audio, optional max store bytes. Document the exact command in README.md. SPEC.md status moves from "founded" to "serves" only when step 6 is green.

After that bar is green, extend carriers one at a time (HTTP that serves the bytes with an audio content type, then whatever the imagination `Transport` seam already has). A new carrier is a new way to reach the same surface. Do not fork the audio rules per carrier.

Stay inside audio. Do not build a player UI, a transcoder farm, a rights system, a chat protocol, or a general RPC server.

---

THE FIVE REPOSITORIES

Authorized workspace, identified by Git remote, not directory name. The local checkout of open-agent-graph is named `rust-ai`.

1. https://github.com/webdawg/open-agent-graph — evidence layer. Do not expand it for this mission unless listend needs a small, proven integration (content hash, pin). Default is: do not touch it.
2. https://github.com/webdawg/oag-mesh — federation. Not this mission until listend serves locally.
3. https://github.com/webdawg/oag-imagination-random-data-compute — carrier seam and data pads. Borrow the idea; do not rewrite it. Copy a pattern only when the TCP carrier already works.
4. https://github.com/webdawg/oag-collisions — collision inference. Not this mission.
5. https://github.com/webdawg/oag-listend — THE mission. Almost all edits belong here.

Branches: open-agent-graph and oag-mesh push `main`. Imagination, collisions, and oag-listend push `master`. Check `git branch --show-current` before every push.

Records: cross-project state stays in `rust-ai/PROJECT_STATE.md` and `rust-ai/ENGINEERING_LOG.md`. oag-listend holds `SPEC.md`, `README.md`, and `USER_INPUT_RECORD.md`. Append new operator statements to `USER_INPUT_RECORD.md` verbatim. Do not create a sixth repository.

---

HOW TO WORK

Start of every invocation, under five minutes:

1. Find the five checkouts by remote.
2. `git status`, branch, last five commits in oag-listend and rust-ai.
3. Read the tail of `rust-ai/ENGINEERING_LOG.md` and the next-priorities section of `PROJECT_STATE.md`.
4. Read oag-listend `SPEC.md` status line and `crates/oag-listend/src/lib.rs`.
5. If the listener test is not green, that is the work. If it is green, add the next carrier or harden the daemon (bounds, eviction, shutdown).

Then build. One coherent increment. Test it. Commit it. Push it. Update the log. Continue until the budget ends.

Rules that are already settled:

- Rust. `#![forbid(unsafe_code)]` unless you write a justification you would defend.
- `.cargo/config.toml` and `scripts/rustc-wrapper.sh` prune old `oag*` build generations. Do not re-audit them. If cargo says the wrapper failed but the wrapper works by hand, delete `target/.rustc_info.json` and rebuild. Scripts are byte-identical across all five repos; a change to one is copied to the other four.
- Never commit `target/`.
- Tests: `timeout`-bounded, `cargo test -p oag-listend` or one example. No full workspace builds of the other four repos. No unbounded loops. Generated audio fixtures stay small (kilobytes, not minutes of audio).
- Every accept path is bounded in bytes (`max_bytes` already exists; also cap a single frame).
- Never overwrite uncommitted changes you did not make.
- Never force-push. Never rewrite history.
- Commit after each verified slice. Push to the correct branch. Commit messages say what now works that did not.
- Do not claim a test passed unless you ran it and saw the result.
- Do not invent measurements.

When choosing the next slice: finish a half-done listener before a new carrier; a failing test before a new feature; a carrier that moves bytes before a document about future networks.

End of every invocation: append `ENGINEERING_LOG.md` (what changed, tests and results, commit ids, push status, what is still not serving) and update `PROJECT_STATE.md` next priorities. SPEC.md status stays honest.

---

OUT OF SCOPE

- Re-auditing disk, prune hooks, or cargo config.
- Rewriting the other four repositories to "support audio" before oag-listend serves one byte on a socket.
- Transcoding, playlists-as-product, accounts, DRM, recommendations, speech-to-text.
- Accepting non-audio because it might be useful later.

---

BEGIN

Inspect oag-listend. If it does not yet listen, implement the TCP listener, the audio-only frame protocol, the seeded WAV generator, the binary, and the bounded back-and-forth test. Run the test. Commit and push to `master`. Record it. If that already works, do the next carrier the same way. Continue until stopped.
