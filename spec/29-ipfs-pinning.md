# Native IPFS Pinning

Opt-in retrieval of evidence content and database snapshots via IPFS, independent of whether the
peer that originally produced them is still online. Tracked as future work since
`future_ipfs_static_data_hosting.md` and `spec/22-future-work.md`'s "native IPFS integration"
entry.

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
pinning_provider = "kubo"   # "disabled" (default) | "kubo"
api_base_url = "http://127.0.0.1:5001"
api_key = ""                 # optional bearer token, for a pinning service fronting the same API
```

`"disabled"` (the default) or a missing `api_base_url` both degrade to `DisabledProvider` — every
call returns `IpfsError::Disabled` rather than erroring at startup, same posture as
`[search].embedding_provider`/`[crawler].llm_provider`. OAG must work, and does work, with IPFS
integration completely absent.

**Phase 1 originally put this under `[crawler]`** (`ipfs_pinning_provider`/`ipfs_api_base_url`/
`ipfs_api_key`), reasoned at the time as crawler-specific. Phase 2 (`oag backup --pin`) falsified
that reasoning immediately — backup has nothing to do with crawling — so this moved to its own
top-level `[ipfs]` section before any real deployment depended on the old key path (not a legacy
break; see `USER_INPUT_RECORD.md` Entry 11).

## Provider implementation (`oag-ipfs` crate)

`PinningProvider` trait (`pin`, `fetch`, `provider_name`) with two implementations:

- `DisabledProvider` — the default; both methods return `IpfsError::Disabled`.
- `KuboHttpProvider` — hand-rolled over `reqwest` against a kubo-compatible HTTP RPC API
  (`POST /api/v0/add` multipart, `POST /api/v0/cat?arg=<cid>`), not the `ipfs-api`/`rust-ipfs` crates:
  `rust-ipfs` is alpha-quality and far heavier than this narrow need justifies, `ipfs-api` is a thin
  wrapper over the same two HTTP calls this makes directly — the same "small hand-rolled
  implementation over a heavyweight crate" posture already used for the Prometheus metrics endpoint
  and `oag-embeddings::OpenAiCompatibleProvider`.

The multipart part uploaded to `/api/v0/add` carries an explicit filename (`.file_name("content")`)
— found to matter during this feature's own live verification: a part with no filename at all can
be silently treated as empty by a stricter multipart parser, which would otherwise pin zero bytes
instead of the real content.

## CLI

- `oag ipfs cat <cid> [--config <path>]` — resolves the configured provider from `[ipfs]` and
  writes the raw retrieved bytes to stdout (not JSON-wrapped, so it can be piped to a file/viewer).
  Errors clearly (naming the config key to check) if pinning isn't configured.
- `oag backup <out> --pin [--config <path>]` — backs up as normal, then pins the resulting file and
  prints the CID. A pin failure is a hard error (exits non-zero), unlike the crawler's best-effort
  posture.

## Explicitly deferred

- Pinning the crawler's `llms.txt`/ARD/agent-card side-fetches.
- Any REST/MCP endpoint for pin/fetch.
- Automatic/scheduled pinning of backups — `--pin` remains exactly as operator-triggered as
  `oag backup` itself.
- Un-pinning, pin garbage collection, backup-retention-via-IPFS policy, or pinning-service-specific
  features beyond a bearer key.
- Using `pinned_cid` as a ranking/corroboration input.
- Inlining large evidence bodies into IPFS in place of local storage (the original memory's shape
  #1) — evidence content is never fully stored in OAG itself today, so there's nothing local to
  replace.
