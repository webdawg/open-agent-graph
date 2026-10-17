# Service Receipts

A peer-to-peer content transfer, recorded as an ordinary graph fact — `(peer:<server>, sent_to,
peer:<client>)` — backed by evidence both sides could independently verify but neither side could
have fabricated alone. One step toward a larger, explicitly deferred goal (a residential-CDN-style
economy where peers get paid for what they serve, `USER_INPUT_RECORD.md` Entry 21); the user drew a
firm line against designing payment/settlement now and asked only for this record-keeping layer.
See `spec/22-future-work.md`'s matching section and `USER_INPUT_RECORD.md` Entries 21-23 for the
full design conversation this implements Phase 1 of.

## What this is not

- **Not payment or settlement.** Nothing here moves value. A receipt is a record that a transfer
  happened, nothing more.
- **Not the `/ipfs-site/{cid}` browser-serving path.** That path (`spec/29-ipfs-pinning.md`) is a
  browser fetching content directly over bitswap/libp2p via a vendored Helia bundle — bitswap's
  wire protocol is third-party (`rust-ipfs`/libp2p), and this feature does not fork it to inject
  nonces into individual block transfers. This is a separate, small, fully OAG-owned mechanism for
  two real OAG peers (not a browser) to exchange content directly.
- **Not multi-hop relay or the flood-based corruption-attribution audit** named in the same
  `spec/22-future-work.md` conversation. Both remain unbuilt. This spec produces the single,
  independently-verifiable transfer unit a future chain/audit pass would correlate across many
  paths by `content_hash`/`receipt_hash` — nothing about this design precludes that, and nothing
  here builds it.

## The mutual receipt

The user's own framing: "some type of mutual hash based on client and server randomness and the
file they served — build this into the transfer and reception mechanism." Both sides contribute a
random nonce; the resulting hash binds both of them to the transfer's actual content:

```
receipt_hash = blake3("OAG:RECEIPT:v1:" || client_nonce || server_nonce || content_hash)
```

(`oag_crypto::mutual_receipt_hash`, `crates/oag-crypto/src/hashing.rs`.) Neither nonce is
predictable to the other party in advance, so neither side can produce a matching `receipt_hash`
for content it didn't actually send or receive without the other party's real-time participation —
this is the "neither side could have fabricated it alone" property the user asked for, not just a
content digest (a content digest alone only proves *what* was transferred, not that both parties
were really both there for it).

## The protocol: `POST /oag/transfer/v1/fetch`

Authenticated by the request's own Ed25519 signature — self-certifying `client_peer_id`/
`client_public_key` pairing, the same posture `oag-sync`'s `hello`/`events` endpoints already use
— rather than the REST API's bearer-token layer, which has no natural way to authenticate one peer
to another. Deliberately unauthenticated by that layer and placed before it in the router, same
reasoning as `/ipfs-site/*`.

1. **Client → server** (`crates/oag-api/src/transfer.rs::build_and_sign_request`): `{cid,
   client_peer_id, client_public_key, client_nonce, requested_at, signature}`, signed under domain
   `OAG:TRANSFER_REQUEST:v1:`.
2. **Server**: verifies the request (signature, `client_peer_id`/`client_public_key` pairing,
   clock skew within 300s — same tolerance as presence heartbeats); checks a per-`client_peer_id`
   rate-limit budget (`AppState.transfer_rate_limiter`, the same fixed-window `RateLimiter` the
   bearer-token middleware uses, keyed differently); calls `PinningProvider::fetch(cid)`
   (`AppState.pinning_provider`, `None` by default — answers `503` with a clear message when
   `[ipfs].pinning_provider` is `"disabled"`); hashes the content
   (`oag_crypto::blake3_content_hash`); generates its own `server_nonce`; builds and signs a
   receipt.
3. **Server → client**: raw content bytes as the response body; the signed receipt as JSON in an
   `x-oag-transfer-receipt` header (not the body, so the content itself stays binary with no
   base64/JSON overhead).
4. **Client verification** (`transfer::verify_receipt`): recomputes `content_hash` from the bytes
   actually received and checks it against the receipt's claim (integrity); recomputes
   `receipt_hash` from its own `client_nonce` + the receipt's `server_nonce` + that recomputed
   `content_hash` and checks it matches (the mutual-binding check — this is what actually proves
   the exchange, not a signature check alone); verifies the `server_public_key`/`server_peer_id`
   pairing matches the peer it meant to contact; verifies the `server_signature`. The receipt also
   carries the original `cid` and `client_nonce`, so a receipt for a different exchange or a
   replayed nonce can't be mistaken for this one.

Only if every check passes does the client write anything. A failed check writes nothing and the
CLI reports the mismatch — there is no dispute/boot logic here; see "What this is not," above.

## The graph record

Both sides call the same `transfer::record_transfer` with different `peer_role`/
`extraction_method`:

- **Edge**: `(peer:<server_peer_id>, sent_to, peer:<client_peer_id>)` — `sent_to` is a new entry in
  `docs/predicates.md`'s open vocabulary, no schema change (`Predicate` is an open string type).
  Both `peer:<id>` nodes get created automatically as a side effect of asserting the edge
  (`oag-events`' `resolve_or_create_node`) — `peer` is now in `NodeType::KNOWN_NODE_TYPES`
  (`crates/oag-core/src/node.rs`), the node-level sibling of the `peer:` canonical identifier
  scheme `canonicalize.rs::canonical_identifier` already documented but never used until now.
- **Assertion actor**: each side's own `ActorType::Peer` actor (declared idempotently via
  `oag_graph::sign_actor_key_proof` + `GraphService::declare_actor` on every call) — the first real
  construction of `ActorType::Peer` anywhere in this codebase; it existed in `oag-core` from the
  start (spec section 26: "distinct from a network peer — a single peer serves many actors") but
  nothing had ever actually declared one.
- **Extraction method**: `Direct` for the server (it directly did the serving), `Verification` for
  the client (it independently verified the receipt rather than just trusting the server's claim).
- **Evidence**: a new `EvidenceType::ServiceReceipt` variant (13th case alongside `WebPage`/`File`/
  `UserObservation`/...). `content_hash` and `observed_at` use `Evidence`'s existing dedicated
  fields; `pinned_cid` reuses the exact `{"ipfs_cid": ...}` metadata convention Phase 1's IPFS
  pinning already established. `metadata` (a new, generic `Option<JsonValue>` passthrough on
  `EvidenceInput`/`AddEvidencePayload`/`project_add_evidence` — previously nothing external could
  set `Evidence.metadata` at all) carries:

  ```json
  {
    "protocol": "oag-transfer-v1",
    "ipfs_cid": "<cid>",
    "byte_size": 360448,
    "peer_role": "server" | "client",
    "counterparty_peer_id": "<the other side's PeerId>",
    "client_nonce": "<hex32>",
    "server_nonce": "<hex32>",
    "receipt_hash": "blake3:<hex>"
  }
  ```

Each side writes into its own, separate local database — there is no new sync mechanism here.
`oag-sync`'s existing gossip is what later lets a third party see both sides' corroborating
records on the same edge, exactly like any other event.

**`ServiceReceipt`'s corroboration weight** (`oag-graph/src/corroboration.rs::evidence_type_weight`)
is set at `0.85`, the same tier as `SourceCode`/`ApiResponse` — a mutual-nonce receipt is bound by
both parties' randomness plus the transferred content's hash, a harder-to-fake property than most
other evidence types get for free.

## Live-verified end-to-end

Two real `oag` processes, no other process running anywhere: a server peer with
`[ipfs].pinning_provider = "embedded"` running `oag backup <out> --pin` to get a real pinned CID,
then `oag serve`; a client peer, in a completely separate data directory and identity, running
`oag peer fetch <address> <server_peer_id> <cid> --out <path>`. Confirmed: the fetched bytes are
byte-identical to the original file; both peers' independent `oag.sqlite` databases end up with a
`sent_to` assertion and `service_receipt` evidence carrying the exact same `receipt_hash`,
correctly swapped `peer_role`/`counterparty_peer_id` on each side.

## CLI surface

| Command | Behavior |
|---|---|
| `oag peer fetch <address> <server_peer_id> <cid> [--out <path>] [--data-dir <dir>]` | Fetch `cid` from the peer at `address`, verify the receipt, record a corroborating assertion locally, optionally write the bytes to `--out`. Refuses to record anything if the receipt is signed by anyone other than `<server_peer_id>`. |

No REST/MCP surface for *initiating* a fetch beyond the peer-to-peer endpoint itself — this is a
peer operation (`oag peer fetch`), not a REST client-facing one, same category as `oag peer sync`.

## Explicitly deferred

- **Multi-hop relay chains.** "the server might be servicing another node server... we pass
  information across a thousand hosts" (`USER_INPUT_RECORD.md` Entry 21). Each hop here is
  independent — a chain is just several of these records sharing a `content_hash`, reconstructed
  by whoever queries for it later. No signed, multi-party chain-of-custody structure is built.
- **The flood-based, ~50-path redundant-delivery corruption-attribution audit** (`USER_INPUT_RECORD.md`
  Entries 22-23) — confirmed to be the same push-flood mechanism as "fire like neuron mode"
  (`spec/22-future-work.md`'s flood-propagation section), not a separate one. Unbuilt; this spec's
  receipts are exactly what that mechanism would later compare across paths.
- **Any trust-score consequence of a verified mismatch.** `spec/24-ephemeral-peer-trust.md`'s
  "degrade, don't drop" stands untouched — this spec never writes a dispute or touches
  `trust_score`. The user explicitly confirmed no boot, ever, even for a verified-bad peer.
- **Payment or settlement of any kind.** Already explicitly declined.
- Running `oag peer fetch`/the transfer endpoint concurrently with `oag ipfs host-page`/`oag backup
  --pin` against the same data dir's embedded IPFS repo — same family of single-writer-at-a-time
  caveats already documented for `spec/29-ipfs-pinning.md`, not newly introduced here.
