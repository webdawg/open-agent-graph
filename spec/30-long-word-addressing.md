# Long-Word Subdomain Addressing

Deterministic, word-based DNS addressing for a peer, built on top of `oag-petname`'s existing
display-naming approach — resolved as a second, independent path to a `PeerId` you already know,
not a replacement for `discover_peers`' own discovery role. Tracked as future work since
`spec/22-future-work.md`'s "Long-word, subdomain-based node addressing" entry; this is its Phase 1.

## Scope: `PeerId` only, not `NodeId`

A graph node (a URL, a concept, ...) has no network address to resolve to. DNS addressing only
makes sense for something actually reachable over the network — a running `oag serve` process,
identified by its `PeerId`. The original idea's "NodeId/PeerId" phrasing was exploratory; this
phase narrows it to the only half with a real target.

## The label (`oag-petname::address_label`)

`oag-petname`'s existing 128-word `peer_name` (display only) is nowhere close to fitting a DNS
label (RFC 1035: 63 bytes max per label). `address_label` draws **4 words**, hyphen-joined, from a
separate candidate set — the same wordlist, filtered to words of ≤12 characters — so the worst
case (`4 × 12 + 3 hyphens = 51` bytes) stays under the limit by construction, not just usually.
Same determinism guarantee as `peer_name`: the same `PeerId` always produces the same label,
anywhere, with no coordination.

**A label collision is not a security problem.** This is an addressing convenience, not a trust
credential — resolving `<label>.<domain>` only ever gets you an address to *try*. Any real
connection still goes through this peer's existing, unrelated signature/identity verification at
the protocol layer. A collision just means two peers can't both publish under the same name.

## Configuration (`[addressing]` in `config.toml`)

```toml
[addressing]
domain = "example.com"   # unset by default — this feature is then entirely absent
```

Domain ownership and cost are entirely the operator's own concern — deliberately not this
project's to decide or default. Unset (the default) means the feature doesn't exist at all for
that peer; **not** wired into `oag serve`'s startup sequence or `oag-sync`'s automatic
discovery/gossip/bootstrap path at all — an explicit, operator-triggered CLI capability only, same
posture as `oag ipfs cat`/`oag migrate`.

## CLI

- `oag identity show [--config <path>]` — prints the usual `peer_id`/`peer_name` lines, plus an
  `address: <fqdn>` line only when `[addressing].domain` is set (silent otherwise, matching every
  other opt-in feature's "nothing extra when absent" posture).
- `oag peer resolve <peer_id> [--domain <domain>] [--config <path>]` — a real DNS TXT lookup at
  `<peer_id's label>.<domain>` (`--domain` takes priority over config), printing every TXT value
  found (an operator may publish more than one, e.g. mid-migration to a new address). Errors
  clearly — naming what to check — when neither a domain nor a record is found, rather than a raw
  resolver error dump or a panic.

## What publishes the record

Nothing in OAG does, by design. The operator publishes a TXT record at the FQDN `oag identity
show`/a `fqdn_for_peer` call would print, pointing at this peer's actual reachable sync URL — by
hand, or with their own existing dynamic-DNS tooling. Automating that would mean picking one
specific registrar/provider API (Cloudflare, Route53, ...) nobody asked for, plus a credential-
storage pattern this project doesn't have anywhere today. If this ever needs automating, it's a
separate, later decision — not bundled into deciding what to resolve *to*.

## Explicitly deferred

- Automatic DNS record publishing / dynamic-DNS client integration.
- Wiring DNS resolution into `oag-sync`'s automatic discovery/gossip/bootstrap path.
- `NodeId` addressing (no network target to resolve to).
- Any DNSSEC validation beyond whatever `hickory-resolver`'s default resolver already does.

## Tests

`oag-petname`: `address_label` is deterministic and distinct per peer; every generated label, over
many distinct `PeerId`s, is a valid DNS label (non-empty, ≤63 bytes, exactly 4 lowercase-alphabetic
words joined by single hyphens, no leading/trailing hyphen); `address_wordlist` still has a large
candidate count (documents the real entropy margin rather than asserting a hand-computed number).
`oag-cli::addressing`: `fqdn_for_peer`'s plain string-formatting (including a trailing dot on the
domain). Live, against real already-existing public DNS (not this project's own infrastructure,
since no real domain is configured for OAG itself): a real TXT lookup against a well-known domain
succeeds, and a lookup against a deliberately-nonexistent subdomain returns a clean error rather
than a panic. Fully offline: `oag identity show --config <path with [addressing].domain set>`
prints the computed FQDN; `oag peer resolve` against that same nonexistent-subdomain case prints
the same clean not-found error; `oag peer resolve` with neither `--domain` nor config errors
clearly rather than attempting a lookup with no domain.
