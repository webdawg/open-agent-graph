# Native IPFS Pinning

Opt-in retrieval of evidence content and database snapshots via IPFS, independent of whether the
peer that originally produced them is still online; Phase 3 extends this to hosting an actual
static website, served to a browser with zero HTTP gateway/proxy involved. Tracked as future work
since `future_ipfs_static_data_hosting.md` and `spec/22-future-work.md`'s "native IPFS
integration" entry.

## What it does

**Phase 1 (crawled evidence content)**:
- Every `AddEvidencePayload` (and the `EvidenceInput`/`EvidenceRequest`/`EvidenceParam` layers above
  it, used by REST, MCP, and the crawler) accepts an optional `pinned_cid: Option<String>` — an IPFS
  CID the caller asserts this evidence's content is already pinned under.
- When set, the projector writes it into `Evidence.metadata` as `{"ipfs_cid": "<cid>"}` — the
  existing `metadata: JsonValue` column every evidence row already has (`oag-core/src/evidence.rs`),
  so this needed **no schema migration at all**.
- `oag-crawler` automatically pins the main page's body right after fetching it, before building
  that page's evidence, whenever a `PinningProvider` other than the default `DisabledProvider` is
  configured. Every evidence item built for that page (the baseline `instance_of` assertion, any
  LLM-extraction candidate) inherits the same CID via `page_evidence.clone()`.

**Phase 2 (database snapshots)**:
- `oag backup <out> --pin` pins the resulting consistent snapshot (`oag-storage::backup_to`'s
  `VACUUM INTO` output) after writing it, printing the CID. Unlike the crawler's best-effort
  pinning, a pin failure here is surfaced loudly (`anyhow`-propagated, not swallowed) — `--pin`'s
  entire point is to pin, so there's no "continue without it" fallback that makes sense.

**Both phases**: `oag ipfs cat <cid>` retrieves previously-pinned content directly from the
configured provider and writes the raw bytes to stdout — the one shared read path either phase's
CID can be fed back into.

**Phase 3 (static page hosting)**: `oag ipfs host-page <dir>` pins a whole local directory as one
UnixFS tree (not a loop of single-file pins — a directory needs one root CID a browser can
resolve every file under), and `oag serve` gains `GET /ipfs-site/{cid}` + `GET
/ipfs-site/_bundle.js`, a tiny bootstrap page that loads a vendored, gateway-free browser bundle
(`crates/oag-api/assets/`) which fetches the whole site — not just `index.html`, every CSS/JS/
image file too — purely over bitswap/libp2p, never through an HTTP gateway. See "Static page
hosting" below for the full mechanism; `oag serve` itself never fetches from IPFS at any point in
this phase, it only ever hands the browser a CID and the bundle.

## What it deliberately doesn't do

- **Hash authority is unchanged.** `content_hash` (BLAKE3/SHA-256) stays the authoritative,
  signature-relevant identifier for a piece of evidence. `pinned_cid` is purely an additional
  retrieval pointer, exactly as caller-asserted and unverified as `content_hash` itself — OAG
  doesn't independently confirm a claimed CID any more than it reverifies a claimed hash.
- **Never a trust/ranking signal.** `pinned_cid`/`ipfs_cid` never feeds `EdgeCorroboration` or
  authority in any way.
- **Only the crawler's main page gets auto-pinned.** The `llms.txt`/ARD/agent-card side-fetches a
  crawl also makes are metadata-discovery mechanisms, not primary content a human would want
  independently retrievable, so they're left unpinned in this phase.
- **A pin failure never fails the crawl.** Same best-effort-enrichment posture already established
  for malformed LLM-extraction responses (`docs/architecture.md`) — a failed `pin()` call logs a
  warning and the crawl proceeds with `pinned_cid: None`.
- **No REST/MCP endpoint for triggering a pin or fetch directly.** The write side needs none (it's
  the existing evidence-add path, one new optional field); the read side (`oag ipfs cat`) is
  CLI-only, matching `oag redact`/`oag peer forks` precedent for operator-triggered read tooling.

## Legacy compatibility

`pinned_cid` on `AddEvidencePayload` is `#[serde(default)]` — an `ADD_EVIDENCE` event signed before
this field existed has no `pinned_cid` key in its canonical JSON at all, and still deserializes
correctly (`USER_INPUT_RECORD.md` Entry 1, "legacy must flow").

## Configuration (`[ipfs]` in `config.toml`)

```toml
[ipfs]
pinning_provider = "embedded"   # "disabled" (default) | "kubo" | "embedded"
api_base_url = "http://127.0.0.1:5001"   # only for "kubo"
api_key = ""                              # only for "kubo" -- optional bearer token
libp2p_ws_multiaddr = ""      # override: a browser dial target, e.g. for NAT/a public hostname
embedded_listen_addrs = ["/ip4/0.0.0.0/tcp/0/ws"]   # only for "embedded"; this is the default
```

`"disabled"` (the default) degrades to `DisabledProvider`; `"kubo"` with a missing `api_base_url`
does too — every call returns `IpfsError::Disabled` rather than erroring at startup, same posture
as `[search].embedding_provider`/`[crawler].llm_provider`. OAG must work, and does work, with IPFS
integration completely absent.

`"embedded"` is the no-external-process option, and the one this project's own docs/README
recommend (`"kubo"` remains a legitimate choice for an operator who already runs a dedicated Kubo
node for other reasons — traffic isolation, existing infra — just not the default
recommendation). `libp2p_ws_multiaddr` is an **override**: when `pinning_provider = "embedded"`
and this is unset, `oag serve` auto-detects the embedded node's own real, just-bound dial address
and uses that — no operator configuration needed for the common case. Set it explicitly only when
the auto-detected local address isn't what a browser outside this host should dial (behind NAT, a
different public hostname/port-forward).

**Phase 1 originally put this under `[crawler]`** (`ipfs_pinning_provider`/`ipfs_api_base_url`/
`ipfs_api_key`), reasoned at the time as crawler-specific. Phase 2 (`oag backup --pin`) falsified
that reasoning immediately — backup has nothing to do with crawling — so this moved to its own
top-level `[ipfs]` section before any real deployment depended on the old key path (not a legacy
break; see `USER_INPUT_RECORD.md` Entry 11).

## Provider implementation (`oag-ipfs` crate)

`PinningProvider` trait (`pin`, `fetch`, `pin_directory`, `dialable_multiaddrs` — the last
defaults to an empty list, so adding it didn't need to touch every existing implementation) with
three implementations:

- `DisabledProvider` — the default; every method returns `IpfsError::Disabled` (or an empty list
  for `dialable_multiaddrs`).
- `KuboHttpProvider` — hand-rolled over `reqwest` against a kubo-compatible HTTP RPC API
  (`POST /api/v0/add` multipart, `POST /api/v0/cat?arg=<cid>`). The multipart part uploaded to
  `/api/v0/add` carries an explicit filename (`.file_name("content")`, or each file's own relative
  path for `pin_directory`) — found to matter during this feature's own live verification: a part
  with no filename at all can be silently treated as empty by a stricter multipart parser, which
  would otherwise pin zero bytes instead of the real content. `pin_directory` posts
  `recursive=true&wrap-with-directory=true&cid-version=1` and reads the **last** newline-delimited
  response line's CID (kubo returns one line per file plus the wrapping directory, root last —
  genuinely different from single-file `pin`'s first-line read, not a copy-paste).
- `EmbeddedIpfsProvider` — a real IPFS node embedded directly in this process, via `rust-ipfs`
  (the actively-maintained `dariusc93` fork — confirmed live, not assumed: pushed within a day of
  this feature being built, dependabot-current). An **earlier version of this doc** reasoned
  against using `rust-ipfs` at all ("alpha-quality... far heavier than this narrow need
  justifies") — that was a sound call for Phase 1/2's narrow single-file-pin need, built against a
  thin HTTP client instead; it stopped being sound once the actual need became "be a real,
  directly-dialable IPFS node with no external process," which `KuboHttpProvider` structurally
  cannot do (it talks to a *separately run* Kubo process over HTTP — a real external-process
  dependency this project otherwise avoids everywhere else). Composed with
  `default-features = false`, enabling only `tcp`/`websocket`/`noise`/`ed25519` — deliberately
  excluding `rust-ipfs`'s own `gateway`/`pinning`(remote pinning-*service*)/`routing` features,
  which pull in `reqwest` for HTTP calls to third-party services, the same "no proxy" principle
  this feature's browser side applies. `pin_directory` uses `rust_unixfs::dir::builder::{TreeOptions,
  BufferingTreeBuilder}` directly (the same lower-level primitive `rust-ipfs`'s own single-file
  "wrap in a directory" code uses internally) — `rust-ipfs`'s high-level `add_unixfs` only adds one
  file at a time, with no public API for "add a whole local directory, get one CID," so this
  builds the UnixFS tree by hand: `put_link(relative_path, cid, size)` per file (confirmed live:
  this accepts a full `/`-containing relative path directly and creates intermediate directory
  nodes itself — no manual per-level grouping needed), then `repo.put_blocks(...)` +
  `repo.pin(root_cid).recursive()`.

## CLI

- `oag ipfs cat <cid> [--config <path>] [--data-dir <dir>]` — resolves the configured provider
  from `[ipfs]` and writes the raw retrieved bytes to stdout (not JSON-wrapped, so it can be piped
  to a file/viewer). Errors clearly (naming the config key to check) if pinning isn't configured.
  `--data-dir` (default `./data`) is only read by `pinning_provider = "embedded"`, to find its
  on-disk blockstore — must match the data dir the content was originally pinned under.
- `oag backup <out> --pin [--config <path>]` — backs up as normal, then pins the resulting file and
  prints the CID. A pin failure is a hard error (exits non-zero), unlike the crawler's best-effort
  posture.
- `oag ipfs host-page <dir> [--config <path>] [--data-dir <dir>]` — pins a local directory as one
  UnixFS tree (16 MiB / 200 files sanity cap), prints the root CID and the `/ipfs-site/<cid>` path
  to open it at. Warns (non-fatally) if no top-level `index.html` is found.

## Static page hosting (Phase 3)

`GET /ipfs-site/{cid}` (`crates/oag-api/src/ipfs_site.rs`) returns a minimal bootstrap HTML page
injecting the requested CID and (if `pinning_provider = "embedded"`) the live, auto-detected dial
multiaddr into a `<script>` block, then loads `GET /ipfs-site/_bundle.js` — the vendored browser
bundle (`crates/oag-api/assets/`, build instructions in that directory's own `README.md`) as a
module script. **Deliberately unauthenticated**, unlike `/ui/*`: a hosted site is meant to be
publicly viewable like any other website, not gated behind a `graph:read` API key a visitor would
have no way to obtain. `oag serve` itself never fetches from IPFS at any point here — these two
routes only ever hand the browser a CID, an optional dial address, and the bundle; the browser
does 100% of the real content retrieval.

The bundle builds its own Helia node composed *without* ever calling `withHTTP` (Helia's HTTP-
trustless-gateway block broker) — a structural guarantee the HTTP gateway code path isn't even
included, not a config flag — and without `@helia/verified-fetch` (confirmed live: it does its
own public-network provider/content-routing lookup before trying bitswap, which stalls for many
seconds against a peer, like our own embedded node, that was never going to show up in a public
DHT/IPNI lookup). It walks the whole pinned directory (not just `index.html` — every CSS/JS/image
file), builds `blob:` URLs for each via direct bitswap fetches, and rewrites `index.html`'s
relative `src`/`href` references to those blob URLs before injecting it into the page — the
mechanism that makes a *whole site*, not one page, load with zero HTTP involvement beyond this
bootstrap page and the bundle itself. See `crates/oag-api/assets/README.md` for the full list of
API quirks found and fixed during this feature's own live, real-browser verification (a connection
gater that silently refuses to dial local/insecure addresses by default; `fs.ls()`'s entries
carrying no usable `type` field; `dial()` needing a real `Multiaddr` object, not a string).

Live-verified end-to-end: a real headless browser, a real embedded `rust-ipfs` node, zero separate
daemon of any kind — pinned a two-file test site (`index.html` + `css/style.css`), opened
`/ipfs-site/<cid>`, and confirmed the page rendered with its CSS actually applied, fetched purely
over bitswap.

## Explicitly deferred

- Pinning the crawler's `llms.txt`/ARD/agent-card side-fetches.
- Any REST/MCP endpoint for pin/fetch (the new `/ipfs-site/*` routes are GET-only and don't pin
  anything; that stays CLI-only).
- Automatic/scheduled pinning of backups — `--pin` remains exactly as operator-triggered as
  `oag backup` itself.
- Un-pinning, pin garbage collection, backup-retention-via-IPFS policy, or pinning-service-specific
  features beyond a bearer key.
- Using `pinned_cid` as a ranking/corroboration input.
- Inlining large evidence bodies into IPFS in place of local storage (the original memory's shape
  #1) — evidence content is never fully stored in OAG itself today, so there's nothing local to
  replace.
- Running `oag ipfs host-page`/`oag ipfs cat`/`oag backup --pin` concurrently with a running `oag
  serve` against the same data dir's embedded IPFS repo — not coordinated, same family as this
  project's other single-writer-at-a-time notes; pin before starting `oag serve`, or restart it
  to pick up new content.
- Compiling the browser-side client as Rust-to-WASM using this same `rust-ipfs` crate (it does
  support `wasm32`, confirmed via its own `wasm-example`) instead of the JS/Helia bundle — a
  genuinely interesting "one codebase instead of two" direction, not pursued here since it would
  mean replacing already-working, already-tested JS with a from-scratch port for no immediate gain.
- `EmbeddedIpfsProvider`'s own libp2p keypair is not persisted across restarts (a fresh identity,
  and so a fresh dial address, every `oag serve` start) — fine for content delivery (bitswap
  addresses by CID, not by peer identity), but means a browser must reload the bootstrap page
  after a restart to get the new dial address; not solved with new persistence machinery here.
