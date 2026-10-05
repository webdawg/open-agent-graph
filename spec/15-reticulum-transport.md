# Reticulum Transport

**Source**: `crates/oag-reticulum/src/{listener,handlers,client,framing,identity}.rs`.

An optional *additional* peer transport over the Reticulum mesh-networking protocol, alongside —
never replacing — `oag-sync`'s HTTP path. Disabled by default.

## What it carries

The same three calls `SyncService::sync_with_peer`'s pull loop makes over HTTP (`Hello`, `Events`,
`Peers`), carried over a Reticulum `Link` instead of `reqwest`. Deliberately duplicates
`oag-sync/src/server.rs`'s handler bodies (`handle_request` mirrors, does not call into, the HTTP
handlers) rather than sharing code — this keeps the proven HTTP path completely untouched while
this transport is still new.

## Framing

A `Link`'s `data_packet` carries at most `PACKET_MDU` (2048) bytes once encrypted, and JSON
request/response messages routinely exceed that. Manual framing: a 4-byte big-endian length prefix
followed by the JSON body, split into `CHUNK_SIZE` (1400)-byte pieces, reassembled from the pieces
on the other end. `MAX_MESSAGE_BYTES` (8 MiB) bounds how large a single logical message can declare
itself to be, independent of how many chunks it takes to arrive — checked against the *declared*
length before buffering, so a malicious peer can't claim an enormous length and force unbounded
allocation; this was already correctly designed with its own test
(`oversized_declared_length_is_rejected`) before the session that produced this spec audited it.

## Rate limiting — found missing, fixed

The listener answers the identical `hello`/`events`/`peers` calls `oag-sync`'s HTTP endpoints do,
but unlike those endpoints — which have a 600-req/60s global `SyncRateLimiter` specifically for
"a valid signature alone must never guarantee unlimited replication" — the Reticulum listener had
no rate limiting on this path at all. `listen_tcp` means this transport isn't inherently
bandwidth-capped by mesh radio either; it can run over plain TCP at full network speed, so "radio
is slow enough to self-limit" isn't a real mitigation.

**Fix**: reuses the exact same `SyncRateLimiter` directly (its `check()` made `pub`, not
`pub(crate)`, specifically so this non-axum listener loop can share it) rather than reimplementing
the logic. The per-message handling was extracted into a small `rate_limited_handle_request`
function specifically so this is unit-testable without driving 600+ real round-trips through a
live Reticulum `Link`.

## CLI surface

| Command | Behavior |
|---|---|
| `oag peer reticulum-address` | Print this peer's Reticulum destination address |
| `oag peer add-reticulum <address_hash> <via_tcp>` | One-shot sync with a peer reachable over Reticulum |

Enabled via `[reticulum]` in `config.toml` (`listen_tcp` to accept incoming, `uplink_tcp` to dial
out to a shared transport node, `announce_interval`).

## Tests

`crates/oag-reticulum/src/framing.rs` (chunking/reassembly, oversized-length rejection);
`crates/oag-reticulum/src/listener.rs`'s `requests_beyond_the_window_get_a_rate_limit_error_instead_of_being_handled`;
`crates/oag-reticulum/tests/two_peer_sync.rs`'s `event_created_on_peer_a_syncs_to_peer_b_over_reticulum`
(a real event, over a real TCP-backed Reticulum `Link`, no public Reticulum network needed).
