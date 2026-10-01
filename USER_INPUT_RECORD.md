# User Input Record

This file is a verbatim, append-only record of founding philosophical and architectural
statements from the project's creator — distinct from `OPEN_QUESTIONS.md` (day-to-day scope
decisions) and `docs/` (the living technical description of what's actually built). Entries here
are the *why* behind decisions that show up elsewhere as principles, deferred-future notes, or
memory entries. Nothing in this file is edited after the fact — new understanding gets a new entry
or a response underneath, never a rewrite of what was actually said.

---

## Entry 1 — 2026-10-01

> we are going to add long word domains I guess too - this software should hold as one of its core
> values that it allows legacy to flow - but never sets a real new standard based on its
> architecture - we always want what came before it to work - data - we version data on a
> granualar level - every single artifact of data has a version number, and if we are accessing it
> we can convert it perm, or we can leave it as is and use api interface code to access it - we
> need a way to bridge the old and the new networking - we have people with clients (browsers) - we
> have people in an old world - and if we do not let them in - then we abandon them, but the new
> internet - it may not need browsers - the new world is moving, and in smart objects, rocks, smart
> traffic lights, sensors, and movement of devices, and decision. we record data, we build things,
> and we decide what to do with it. we build things so we can continue to extend them infinitally
> with respect to science - networks of temporary work with unguessable encryption - add this
> paragraph as part of the founding USER_INPUT_RECORD just to have laying around - react to it,
> update your spec - for instance we may take a domain, and add 50 million subdomains to it to hack
> around a commercial internet (the current internet as it stands) constraint (each subdomain could
> be a node address)

### Response

Four distinct ideas are tangled together here, worth pulling apart:

**1. Legacy must always flow — OAG never becomes its own gatekeeper.** This is a *permanent*
principle, not a future feature, and it's already partially true by construction: `Predicate` is an
open string, not a closed enum (spec section 20); `canonicalize_value` passes through any
already-scheme-prefixed identifier unchanged rather than forcing it into a house format; the event
log never deletes anything (spec section 106 invariant 3). What's new in this statement is making
it *explicit and binding*: no future OAG version is allowed to require that old data, old clients,
or old protocols be upgraded or abandoned to keep working. Added to `docs/architecture.md`'s mission
statement as a named principle — see "Legacy Compatibility" there.

**2. Granular, per-artifact data versioning, with a choice at read time.** This is genuinely new,
not yet built. Today, the event log *is* a version history in the sense that nothing is ever
overwritten and `oag rebuild` can always reproduce past state — but there's no explicit version
number stamped on each artifact (node/edge/assertion/evidence), and no formal mechanism for "read
this old-versioned thing through a compatibility shim" versus "upgrade it in place." Concretely this
would mean: a schema/format version field on projected rows (and on `EventPayload` variants
themselves, so a future `AssertRelationPayload` shape change doesn't orphan old events), plus two
explicit access modes — a one-way migration that rewrites a row to the current version, or an
API-layer adapter that keeps serving the old shape forever without touching the underlying data.
Tracked as future work (see `docs/architecture.md` and memory).

**3. Bridging old and new networking — browsers vs. a browser-less world.** OAG already does this
more than it gets credit for: `oag-api` serves plain humans with browsers (REST JSON + the Human
Interface's HTML pages), while `oag-mcp` and `oag-sync` serve agents and other peers with no browser
involved at all, and `oag-reticulum` already reaches past the commercial internet entirely over mesh
radio. The "smart objects, rocks, traffic lights, sensors" framing is really describing *more peers*
of the kind this architecture already treats as first-class (spec section 3: "one program, many
peers") — a sensor is just a peer with a very small event budget. No architecture change implied
here so much as a reminder that "peer" was always meant to include non-human, non-browser
participants, and that the REST/HTML surface must never become a *requirement* for participating.

**4. Long-word domains / subdomain-as-node-address.** This extends `oag-petname`'s existing
deterministic word-based naming (a peer's cryptographic identity already gets a 128-word
human-readable display name) from *display* into *addressing*: using DNS's own namespace
depth — a single domain with tens of millions of subdomains — as a way to mint huge numbers of
addressable node identities without needing anyone's permission or a new registry, each subdomain
acting as one node's address. This is a real, usable idea (DNS already tolerates enormous
subdomain counts; a wildcard record plus content-addressed subdomain labels could map directly onto
existing `NodeId`/`PeerId` hashes), and it's the same spirit as `oag-reticulum`: use an existing,
already-deployed substrate in a way its own designers didn't require, rather than asking anyone for
new infrastructure. Tracked as future work, not yet implemented.

**Connection to existing memory**: "networks of temporary work with unguessable encryption" is the
same idea already captured in `future_ephemeral_peer_trust.md` (session-scoped identity, trust
earned through sustained connection rather than a persisted key) — this entry reinforces and
slightly extends it (the *network* itself is temporary/reconfiguring, not just any one peer's
identity within it). Cross-linked there.
