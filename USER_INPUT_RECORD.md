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

---

## Entry 2 — 2026-10-01

> we need to intake the md files at hackspherelabs.com http site, record that we intaked them, and
> add them to some sort of software architecutre space inside of this project, it will eventually be
> a blockchain, but only for the sake of public record - anyone can try to commit a block to the
> chain to fight for what should be built next - this code should always be changing, and part of
> what we will store inside of the actual storage service we have created is the code itself i
> guess? - we do not want to follow the .md files inside of hackspherelabs.com but istead use it as
> an example of an architecture around running the type of sofware we are building - small ant like
> software almost like creatures building and linking together - clusters of mutually cooperating
> nodes and groups of nodes that are distributed and do things to help the real world make tech work
> possible or physical things possible - commit this to the user input too, but think and plan on
> this - lets see what you come up with

### Response

**The intake itself**: fetched all nine markdown files live from `https://hackspherelabs.com/md/`
(`The_HACK_COMMANDMENTS.md`, `REQUIREMENTS.md`, `DO.md`, `LAW.md`, `UTOPIA.md`, `COLLECTIVE.md`,
`GLASS_HOUSE.md`, `BOOK.md`, `GOALS.md`) on 2026-10-01. Worth noting plainly: `hackspherelabs.com`
links directly to `github.com/webdawg` on its own front page — this is not a stranger's site, it's
the project creator's own parallel philosophical/collective project. The nine files split roughly
into two groups: broader ethical/political philosophy not really about software architecture at all
(`LAW.md`'s surveillance-accountability and direct-democracy proposals, `UTOPIA.md` and `GOALS.md`'s
supply-chain and climate ethics, `The_HACK_COMMANDMENTS.md`'s seven-point ethical manifesto), and a
smaller set that *is* about how software/work gets organized (`DO.md` — a living "what's being
worked on" list, explicitly "spec for an AI"; `COLLECTIVE.md` — individuals not organizations,
transparent/broadcast-live operation, work within-but-testing legal bounds; `GLASS_HOUSE.md` — all
code public and readable, deployment pipelines inspectable, test/prod parity; `REQUIREMENTS.md` — a
competitive, non-monopolistic internet root where "whoever registers a TLD first maintains the
compute resources behind it, for a small maintenance fee" and anyone can take over by offering
better terms).

Per the user's explicit instruction, none of this is being adopted as a spec — "we do not want to
follow the .md files... but instead use it as an example of an architecture." Per the user's
instruction, it's been ingested as actual graph data inside OAG itself (not copied into `docs/`) —
see the `software-architecture-reference` concept node described below, which is the literal
"software architecture space inside of this project" the user asked for: a durable, hash-verified,
provenance-tracked record that this intake happened, living in the same signed event log as every
other fact OAG holds, rather than a special-cased markdown dump.

**On "it will eventually be a blockchain... anyone can commit a block to fight for what should be
built next"**: recommend *against* building a second, separate chain for this. OAG's own event log
is already a per-peer hash-chained, signed, append-only structure (spec sections 27-30) — a second
blockchain bolted on for roadmap governance would duplicate machinery that already exists one layer
down. What this proposal actually wants — a public, permissionless, non-monetary record where
competing proposals for "what gets built next" are visible and can be judged on their merits — maps
directly onto assertions the project already knows how to make about itself: anyone holding a
`graph:assert` key could assert a roadmap-proposal node (e.g. predicate `proposed_next`, object a
concept node describing the proposed work), back it with evidence (a design doc, a working
prototype, a link), and the *existing* corroboration machinery (`source_independence`,
`identity_assurance`, `evidence_strength` — already computed, already public) does the "fighting for
it" ranking work a bespoke voting chain would otherwise have to build from scratch. This is also the
more legacy-compatible path (Entry 1): reuse the standard already built rather than inventing a
second one. Concretely this would mean OAG governing its own development *using its own graph* —
genuinely interesting, consistent with "the code itself" idea below, and worth treating as a real
future-work candidate rather than pure speculation. Not building it now; recorded as a future
direction (memory + `docs/architecture.md`).

**On "part of what we will store inside of the actual storage service we have created is the code
itself"**: this is less speculative than it sounds — nothing about `oag-crawler`'s fetch pipeline
cares whether a URL serves HTML, JSON-LD, or a plain-text source file, and `oag-storage`'s
content-hash-per-evidence model (already built) is naturally a versioning mechanism for *any* text
artifact, source code included, and ties directly into Entry 1's granular-versioning idea: each
commit/version of a source file is just another evidence blob with its own content hash, hung off a
node representing the file or module. Not building this now either (no reason to duplicate git), but
worth keeping in view as a concrete, already-mostly-supported capability rather than a hypothetical
one, should the project ever want to make its own source history queryable through the same graph it
uses for everything else.

**On "small ant like software... clusters of mutually cooperating nodes... help the real world make
tech work possible or physical things possible"**: this extends, rather than introduces, two things
already tracked. First, [[future_ephemeral_peer_trust]]: an ant colony's trust model is exactly
"identity is disposable, only the colony's accumulated behavior matters" — individual ants are
expendable and interchangeable, but the colony's structures (built over many ant-lifetimes) persist.
That is precisely the shape already captured there (session-scoped identity, trust earned through
behavior/uptime, not a persisted key) — this image is a strong confirming metaphor for a direction
already queued, not a new one. Second, genuinely new: today's peer model (spec section 3) treats
peers as a flat set — there's no notion of a *cluster* or *group* of peers that cooperate as a unit
distinct from the network as a whole. "Clusters of mutually cooperating nodes... doing physical
things" implies peer *grouping* as a first-class future concept (a sensor cluster on one traffic
intersection, say, cooperating tightly with each other and more loosely with the rest of the graph) —
recorded as a new, distinct future-work memory, cross-linked to ephemeral-peer-trust rather than
folded into it, since grouping/clustering and identity-lifetime are separable concerns.

**Connection to existing memory**: reinforces [[future_ephemeral_peer_trust]] (ant-colony framing of
disposable identity / persistent collective behavior) and [[future_long_word_subdomain_addressing]]
(`REQUIREMENTS.md`'s competitive-registrar proposal is close kin to the subdomain-addressing idea —
both are about minting/governing address space without a single permanent gatekeeper). Introduces
two new future-work threads, recorded separately: self-hosted roadmap governance via the existing
assertion/corroboration machinery instead of a second blockchain, and peer clustering/grouping as a
concept distinct from flat peer identity.

---

## Entry 3 — 2026-10-09

> the next layer is a blackhole, a force pulling down everywhere slowing each node down - one at a
> time

### Response

Read against [23-tensor-pad-and-evolution-layer.md](spec/23-tensor-pad-and-evolution-layer.md),
which already describes two forces acting on the same per-peer tensor pad: `oag-tensor` (a peer
nudging its own pad toward a signal derived from its own sync experience) and `oag-brain` (one
peer's pad reprogramming another's via pairwise self-attention, two pads at a time). This entry
describes a third, structurally different force on the same pad — call it the **gravity layer**
(the "blackhole" is the colloquial name, matching this project's running physics-metaphor naming:
ant memory, evolution layer, now gravity):

- **"a force pulling down everywhere"**: unlike both existing forces, this one is not driven by any
  particular sync event or any particular peer's attention — it's ambient and universal, acting on
  every pad regardless of whether that peer has synced recently. The natural reading of "pulling
  down" is decay toward zero/collapse, not growth: `TensorPad::update` already does an EMA step
  toward a *signal* (`v = (1-lr)*v + lr*s`); a gravity pull is the same mechanism with the signal
  fixed at zero (or some small constant below the pad's current magnitude) rather than
  sync-derived — so a pad that nothing nudges for a while loses magnitude instead of holding state
  forever. This needs no new dependency; it's a second entry point into `oag-tensor`'s existing EMA
  math (e.g. `TensorPad::pull(strength)` beside `update`), not new machinery.
- **"slowing each node down"** has a second, not-mutually-exclusive reading worth flagging rather
  than collapsing: either the pad's own magnitude is what shrinks (decay, as above), or the
  *rate* at which a node's own sync/processing loop ticks is what gravity throttles directly
  (gravitational time dilation — more accumulated "mass" slows a node's own clock). The first is
  buildable today with zero new state; the second would need a new per-peer rate signal and is a
  real, separate design question. Recording both readings here rather than picking one
  unilaterally — status: open.
- **"one at a time"** already has a concrete precedent in this codebase, not just a metaphor:
  `spawn_gossip_loop` (`crates/oag-sync/src/gossip.rs`) already iterates every known address
  sequentially inside one loop (`for addr in &addrs { service.sync_with_peer(addr).await }`), never
  concurrently. Gravity should piggyback on that exact shape rather than introduce a new scheduler:
  once per gossip tick, pull exactly one peer's pad (round-robin through `list_peers`, independent
  of whether that peer was actually reached this round) instead of decaying every row in the table
  in one pass. This keeps gravity's own write pattern consistent with this project's existing
  "no unbounded/bursty global operation" precedent (the rate limiter's threshold-triggered sweep,
  `MAX_TOTAL_KNOWN_PEERS`) — one small write per tick, not a full-table pass.

Not implemented yet — this entry and the corresponding future-work note are the capture, not the
build. See [22-future-work.md](spec/22-future-work.md) for the tracked item.

---

## Entry 4 — 2026-10-09

Elicited through four direct clarifying questions while planning ephemeral peer identity/trust
(`future_ephemeral_peer_trust.md`), each question included verbatim below its answer so the context
is legible later.

> [Should ephemeral identity be the new default behavior of `oag serve`, or an opt-in config flag?]
>
> Once a network is established they should have a voting right to keep the network alive if a new
> peer upgrades and comes in - we build trust by monitoring peers and seeing if they are doing
> trustful things, we should adjust the time limit on trust checks live. Different things would
> change the trust level, and trust is not about paranoia, trust is about reliance. A node can
> become untrusted because it is operating on bad hardware or buggy, just the same as a malicous
> actor. If we are creating something where something needs to be restarted, upgraded, etc - they
> we have to inform the network first, do the work, and when we come back online we would have a
> low trust level, and then the trust level gets rebuild as time goes on. We want a network where
> almost everything can be leaked, and even a malicous actor could come in, or memory could become
> corrupt, but nothing bad can happen.

> [When identity rotates, how should the old peer_id's event history be treated?]
>
> see what i wrote before and build architecutre around that - use common software sense though -
> get this all into a creator log - we have something somewhere

> [What should the v1 uptime/trust signal actually be, mechanically?]
>
> Signed presence heartbeat

> [Should this milestone wire the new trust signal into FederationPolicy's enforcement, or just
> compute and expose it?]
>
> Wire it into enforcement now

### Response

Four load-bearing design decisions, folded into the Phase 1 ephemeral-peer-trust plan
(`spec/24-ephemeral-peer-trust.md` once built):

**Trust is reliance, not paranoia** — a peer made unreliable by bad/buggy hardware is scored
identically to a malicious one. One continuous trust signal, computed from observed behavior only,
with no attempt to infer intent. This directly shapes `trust_score`: it never branches on *why* a
peer looks bad, only on what was actually observed (heartbeat continuity, the existing `forked`
flag).

**Announced vs. unannounced absence** — a peer that broadcasts a `GoingOffline` presence heartbeat
before restarting/upgrading gets treated more gently on return than one that simply vanished. Since
ephemeral identity means the returning peer's `peer_id` is new and cryptographically unlinkable to
the one that left (no succession proof was wanted — "use common software sense" is read as: don't
build new crypto machinery for a feature whose point is needing *less* identity machinery), the
leniency can't attach to the key. It attaches to the network *address* instead: every peer that
directly witnessed the announcement independently remembers, for a configurable grace window, that
*that address* said it was coming back — and extends a gentler trust-rebuild curve to whatever new
`peer_id` next greets it from there. This is this plan's reading of "voting right": many independent
observers, each extending the same leniency on their own judgment with no coordination between them,
rather than a literal ballot or quorum. Flagged clearly in the plan as the single biggest
interpretive call, open to correction.

**Live-adjustable trust-check windows** — landing in v1 as ordinary `config.toml` values (consistent
with every other tunable in this project), not true hot-reload-without-restart; the latter is
real, separate future work, not bundled into this milestone.

**"Nothing bad can happen" even under full compromise** — safety has to come from the protocol's
own structure, not secrecy or assumed good faith. Read together with the existing PKT-inspired
"degrade, don't drop" framing already in `future_ephemeral_peer_trust.md`: federation enforcement
gets a hard-reject floor only at zero trust (forked, or never-seen); everything above that is
accepted but visibly tiered, never silently dropped outright.

See `future_ephemeral_peer_trust.md` memory for the full Phase 1 scope this entry fed into.

---

## Entry 5 — 2026-10-09

> no you need to build that - gravity is different everywhere by some small random amount (use a
> really good randomness generator for this for now, but hardware later), the speed at witch a
> node can process is impacted by its gravity level, but very very slightly

### Response

Resolves the open question `spec/22-future-work.md`/`USER_INPUT_RECORD.md` Entry 3 deliberately
left unresolved (whether "slowing each node down" meant the tensor pad's own magnitude decaying,
or the node's own clock being throttled) in favor of the second reading, and adds a detail that
entry didn't have at all: gravity is a *per-node* quantity, randomly rolled, not a single global
force.

Implemented as `oag-sync`'s `gravity.rs` plus a new `node_gravity` table
(`crates/oag-storage/migrations/0010_node_gravity.sql`): each peer rolls its own
`gravity_level` once, with `rand::rngs::OsRng` — the OS's own CSPRNG, not the deterministic
seeded-RNG pattern this project otherwise uses for reproducibility (`oag-tensor`/`oag-brain`'s
fixed-seed weights, `oag-petname`'s deterministic naming) — and persists it, so it survives
restarts for a permanent identity the same way a real physical trait would, rather than being
re-rolled every time anyone asks. "Hardware later" is recorded here verbatim as the explicit future
upgrade path: swap `OsRng` for a real hardware entropy/measurement source without changing anything
downstream, since every consumer only ever sees a `f32` gravity level, never the source it came
from.

"The speed at which a node can process is impacted ... very very slightly": applied to the gossip
loop (`spawn_gossip_loop`), which already processes known peers sequentially, "one at a time" —
the same anchor Entry 3's capture used. A gravity level of `1.0` (the theoretical maximum) costs at
most half a millisecond per peer address processed per gossip tick; most nodes, with a
small-to-middling random level, see a small fraction of that. Full reasoning and the exact mapping:
`spec/25-gravity.md`.

See also `spec/23-tensor-pad-and-evolution-layer.md` for the two other forces already acting on
the same tensor pad this entry's gravity reading deliberately does *not* touch.

---

## Entry 6 — 2026-10-11

Elicited through three direct clarifying questions while planning peer clustering
(`future_peer_clustering.md`), each question included verbatim below its answer so the context is
legible later.

> [Should a 'cluster' be a first-class graph concept (a Node with member_of edges from
> peer-nodes, reusing the existing assertion/evidence/corroboration pipeline for free) or a purely
> networking-layer construct (a new FederationPolicy-style concept with no graph representation)?]
>
> Pure networking-layer construct

> [For this first phase, what's the one concrete behavioral effect of two peers sharing a
> cluster?]
>
> Trust/federation effect now

> [Who can assert that a peer belongs to a cluster?]
>
> Any peer can self-assert

### Response

Overrides this session's own recommendation on the first question (graph-level representation,
since it was free given the existing assertion pipeline) in favor of keeping clustering entirely
inside `oag-sync`/`oag-storage` — no `oag-core`/`oag-graph` change at all, see
`spec/26-peer-clustering.md`.

**The three answers compose into a real, explicit security trade-off, stated plainly rather than
assumed safe**: self-assertion with no gatekeeping, combined with a pure-networking construct
with no corroboration mechanism (that's what declining the graph-level option costs — the graph's
`source_independence`/`identity_assurance` signals were the thing not adopted), combined with a
*full* trust/federation override (not just a cadence change) means a cluster name functions as a
shared, unauthenticated label: any peer that learns or guesses it can claim the same cluster and
receive the same full-trust override from anyone configured with that name. This was flagged
explicitly before the decision was made (`future_peer_clustering.md`'s own open question: "does
intra-cluster replication need the same rigor as `FederationPolicy::Open`/`Allowlist`") and chosen
anyway, deliberately — not an oversight, and documented prominently in the implementation rather
than quietly assumed safe.

See `future_peer_clustering.md` memory for the full Phase 1 scope this entry fed into.

---

## Entry 7 — 2026-10-11

Unlike Entries 4 and 6, this entry records a design judgment made without a new round of direct
questions — Entry 1 above already specified the mechanism precisely enough ("every single artifact
of data has a version number, and if we are accessing it we can convert it perm, or we can leave it
as is and use api interface code to access it") that building spec/27-artifact-versioning.md's
Phase 1 only required resolving how literally to take `docs/architecture.md`'s own earlier
speculation that payload variants should *also* eventually carry a version, not just projected
rows.

**Judgment call, recorded explicitly rather than silently assumed**: Entry 1 describes exactly one
mechanism — a version number on each *stored artifact* (node/edge/assertion/evidence), with a
read-time choice between permanently converting it or serving it through an adapter. That's what
got built. `docs/architecture.md`'s additional speculation that `EventPayload` variants themselves
should eventually carry a version — so a future *breaking* change to e.g. `AssertRelationPayload`
doesn't orphan already-signed historical events — was deliberately **not** built this phase: no
real second payload shape exists anywhere in this codebase today, so a versioned-variant-dispatch
system would have nothing real to migrate and nothing to prove it correct beyond synthetic test
fixtures standing in for a shape that doesn't exist. The existing `Option<T>`/`#[serde(default)]`
convention (already proven, already used throughout `oag-core`) remains the mechanism for additive
payload evolution; `UnsignedEvent::version: u16` (already part of every signed event's hashed
bytes, already inert) is documented as the reserved hook for the day a genuinely breaking payload
change is needed — not wired up until then.

A second, smaller judgment call: the plan for this phase originally called for `oag migrate
<artifact>`'s "already current" case to report immediately without requiring `--force`, since
nothing is mutated either way. Implemented instead with `--force` required unconditionally, mirroring
`redact_evidence`'s existing discipline exactly — because the "no real mutation happens" fact is
only true *today*, when every row genuinely is already current; a peek-then-gate design that
skips `--force` for "nothing to do" would, the day a real second schema version exists, let a
caller who forgot `--force` discover that the tool quietly migrated their row anyway. Consistency
with the one other irreversible-action command already in this CLI was chosen over saving an
operator a flag in a case that is, today, universal.

See `future_legacy_compatibility_and_versioning.md` memory and `spec/27-artifact-versioning.md` for
the full mechanism this entry fed into.

---

## Entry 8 — 2026-10-12

Like Entry 7, a self-made design judgment rather than an elicited answer — `spec/22-future-work.md`'s
"Self-hosted roadmap governance" section (itself derived from Entry 2's "we build things... and we
decide what to do with it") already specified the shape precisely enough ("the genuinely new part,
if pursued, is a convention for which predicate/concept namespace roadmap proposals live under,
and possibly a dedicated Human Interface view for browsing/ranking them") that building
`spec/28-roadmap-governance.md` only required two judgment calls, both recorded here rather than
silently assumed.

**First**: which predicate to use for the convention. Rather than inventing a new one (e.g.
`roadmap_candidate`), reused the already-documented `instance_of` — `(proposal, instance_of,
concept:oag-roadmap-proposal)`. This keeps the feature at genuinely zero new vocabulary, which is
as literal a reading of "no new consensus mechanism to build" as the words allow, and mirrors
`concept:software-architecture-reference`'s existing hub-node precedent exactly (an external page,
`example_of`, that hub).

**Second**: whether `GraphService::list_roadmap_proposals` should be a generic "list every
instance of any hub node" capability, or scoped narrowly to roadmap proposals specifically. Built
narrowly scoped — the future-work text asked for a roadmap-proposal view, not a general
hub-browsing primitive, and nothing else in this codebase currently needs the generic version. The
underlying code is nearly identical either way, so generalizing later, if a second real use ever
appears, costs almost nothing; building it speculatively now would have been exactly the kind of
unrequested abstraction this project's own conventions warn against.

See `future_self_hosted_roadmap_governance.md` memory and `spec/28-roadmap-governance.md` for the
full mechanism this entry fed into.

---

## Entry 9 — 2026-10-12

Another self-made design judgment, same pattern as Entries 7 and 8 — `future_ipfs_static_data_hosting.md`
(2026-09-27) had already resolved the hardest two questions ("OAG's own hash stays authoritative,
the CID is just a retrieval pointer"; "opt-in, works fully disabled") well enough that building
Phase 1 (`spec/29-ipfs-pinning.md`) required three further judgment calls, recorded here.

**First**: `rust-ipfs`/`ipfs-api` (both flagged in the memory as worth evaluating) were declined in
favor of a hand-rolled `reqwest`-based client (`oag-ipfs::KuboHttpProvider`) — `rust-ipfs` is
alpha-quality and far heavier than two HTTP calls justify; `ipfs-api` is a thin wrapper over those
same two calls. Same "small hand-rolled implementation over a heavyweight crate" posture already
used for the Prometheus metrics endpoint and `oag-embeddings::OpenAiCompatibleProvider`.

**Second**: only the crawler's main page gets auto-pinned, not the `llms.txt`/ARD/agent-card
side-fetches a crawl also makes. Those are metadata-discovery mechanisms, not primary content a
human would want independently retrievable, and skipping them kept Phase 1's scope tight —
trivially extendable later if a real need shows up.

**Third**: the CID threads through as a narrowly-named `pinned_cid: Option<String>` field on
`EvidenceInput`/`AddEvidencePayload`/the REST and MCP evidence request shapes, landing in the
*existing* `Evidence.metadata` JSON column as `{"ipfs_cid": "..."}` — not a generic, open-ended
metadata API exposed to callers. This needed zero schema migration (`metadata` already existed,
unused) and keeps the new caller-facing surface exactly as validate-able as `content_hash` already
is, rather than opening arbitrary JSON injection into evidence metadata from any caller.

One real bug, not a judgment call, found during this feature's own live verification against a
fixture kubo stand-in (no real kubo daemon available in this environment): `reqwest::multipart::
Part::bytes` with no `.file_name(...)` produced a part that a stricter multipart parser silently
treated as empty, pinning the hash of zero bytes instead of the real page content. Fixed by always
setting a filename on the uploaded part, with a regression-guarding unit test added
(`oag-ipfs/src/provider.rs`) and reverified live end-to-end (crawl → correct CID in evidence
metadata → `oag ipfs cat` round-trips the exact original page bytes).

See `future_ipfs_static_data_hosting.md` memory and `spec/29-ipfs-pinning.md` for the full
mechanism this entry fed into.

---

## Entry 10 — 2026-10-12

Another self-made design judgment, same pattern as Entries 7-9 — `spec/27-artifact-versioning.md`'s
own "still future work" note already named this Phase 2 precisely ("the same mechanism for
actors/aliases/disputes/retractions/supersessions/observations"), so building it only required two
judgment calls, both recorded here.

**First**: `assertion_supersessions` was excluded entirely, not just deferred for a later phase.
Grepping the whole workspace found `SupersessionRow` (`oag-storage/src/models.rs`) has zero usages
anywhere outside its own definition — `insert_supersession` writes using a separate plain struct
built directly, and the only place supersession info is ever displayed (the Human Interface's
assertion page) reads it from the event log (`get_history`), not this table. There is no
row-to-domain-type read path for a migration shim to attach to, so a `schema_version` column here
would have nothing to condition on at read time — building one anyway would be versioning a table
nothing actually reads, the same kind of speculative construction Phase 1's own "what's
deliberately not built yet" section already declined for payload-variant versioning.

**Second**: of the remaining five, only `actors` got the explicit "migrate in place" CLI action
(`oag migrate actor <id> --force`). It's the only one of the five with a pre-existing singular
get-by-id lookup to extend (`repo::actors::get_by_id`, already backing `oag actor get`) — exactly
the precedent Phase 1's four commands followed. `node_aliases` has no single id at all (composite
primary key); `observations`/`assertion_disputes`/`assertion_retractions` each have a real `id`
column but no existing singular lookup anywhere (only `list_*` keyed by `assertion_id`).
Inventing one solely to attach a migrate command would have been scope beyond "the same
mechanism" — the same reasoning Entry 8 used to decline generalizing `list_roadmap_proposals`. All
five still get the automatic shim (`row_to_*` migrating on every read), which fully satisfies "the
same mechanism" for the read side regardless; only the explicit write-side convenience is narrower
for four of them.

See `spec/27-artifact-versioning.md` for the full mechanism this entry fed into.

---

## Entry 11 — 2026-10-12

A self-made correction, not quite the same shape as Entries 7-10's judgment calls — those each
picked between two reasonable designs; this one reverses a reasoning step from the immediately
preceding phase once a new fact made it wrong.

`spec/29-ipfs-pinning.md`'s Phase 1 (this session, same day) put IPFS pinning config under
`[crawler]` (`ipfs_pinning_provider`/`ipfs_api_base_url`/`ipfs_api_key`), reasoned at the time as
"IPFS pinning is crawler-specific... belongs in the section that already owns crawl-time
behavior." Phase 2 — `oag backup --pin`, picking up the original memory's shape #2 ("publish
read-only snapshots... to IPFS") — needed the exact same `PinningProvider`, and backup has nothing
to do with crawling. Rather than have `oag backup --pin` read a `[crawler]`-scoped setting, or
duplicate the provider-resolution logic under a second name, moved it to its own top-level
`[ipfs]` section (`crates/oag-cli/src/config.rs`'s new `IpfsSection`), with `CrawlerSection` and
`ResolvedConfig`/`resolve_crawler_config` updated to read from it unchanged otherwise.

**Why this isn't a legacy break**: the `[crawler].ipfs_*` keys existed for exactly one commit,
earlier the same session, with no real `config.toml` anywhere ever having depended on that path —
"legacy must flow" (Entry 1) is about data/clients/protocols actually in use, not about a config
key nobody has had the chance to write yet. Fixing a wrong assumption before anyone depends on it
is the cheap moment to fix it; leaving a known-wrong section name in place "for compatibility" with
zero real users would have been cargo-culting the principle rather than applying it.

See `spec/29-ipfs-pinning.md` for the full mechanism this entry fed into.

---

## Entry 12 — 2026-10-12

Self-made judgment calls, same pattern as Entries 7-11 — `spec/24-ephemeral-peer-trust.md`'s own
"Explicitly deferred" list (and `docs/architecture.md`/`spec/22-future-work.md`'s matching
bullets) named "live config reload for the trust-check windows" precisely enough to build directly,
unlike its two siblings on the same list (a real voting/consensus protocol, PacketCrypt-style
proof-of-data-volume) — both of which have no concrete shape named anywhere in this project's own
docs or memory, and would mean inventing a consensus/proof-of-work scheme from scratch. Those two
were deliberately left alone this session as exactly the kind of open-ended design decision better
surfaced to the user than decided unilaterally; only the third, mechanical item was picked up.

Three smaller calls within that scoped item, all reversible/low-stakes:

1. **Polling, not a file-watcher (inotify/similar).** A fixed 30s poll needs no new dependency and
   is far more responsive than this setting's own effect (trust decay/rebuild curves, which operate
   on minutes) requires.
2. **The poll interval itself is a hardcoded constant, not a config value.** Making it configurable
   would raise "how often do we check for changes to the check-interval" — hot-reloading a
   hot-reload setting has no natural base case.
3. **Unconditional overwrite every tick, no diffing.** Re-applying an unchanged `TrustConfig` is a
   harmless no-op, so there's no diff-then-apply logic to write or get wrong — the simplest correct
   behavior, consistent with this project's general preference for the smallest implementation that
   is actually correct.

Also scoped deliberately narrow: only `[identity]`'s three trust-window fields get this treatment.
`[cluster]`, `[federation]`, `[network]`, etc. all stay startup-only, exactly matching today's
behavior — the backlog item named the trust-check windows specifically, not config hot-reload as a
general capability, and generalizing further wasn't asked for.

See `spec/24-ephemeral-peer-trust.md` for the full mechanism this entry fed into.

---

## Entry 13 — 2026-10-13

Unlike Entries 7-12, this one started with a real elicited answer, not a self-made judgment call —
a sign the earlier entries' own instinct was right. After Phase 2 of ephemeral peer trust shipped,
the remaining named backlog items (a real voting/consensus protocol and PacketCrypt-style
proof-of-data-volume for ephemeral trust, a corroboration mechanism for peer-cluster claims, and
long-word subdomain addressing) were surfaced via `AskUserQuestion` rather than picked
unilaterally — each genuinely needed either a resource only the user could provide (a domain) or a
design decision with real security/architecture implications (building "real consensus" cuts
against this project's own stated "no global consensus" founding architecture; "corroboration" for
a cluster claim doesn't actually add security against a copyable bare string the way corroboration
of independent evidence sources does for the main graph).

**The user picked long-word subdomain addressing**, then a follow-up question resolved the first
of its three open questions directly: domain ownership/cost is the user's own concern, designed
*domain-agnostically* — `[addressing].domain` as a plain config value with no real default, no
specific registrar assumed, nothing hardcoded.

**Two further scoping decisions made without a fresh round of questions, same pattern as Entries
7-12**:

1. **Scoped to `PeerId`, not `NodeId`**, despite the original idea's "NodeId/PeerId" phrasing — a
   graph node has no network address to resolve to, so only the half that names something actually
   reachable (a running peer) got built.
2. **No automatic DNS record publishing.** The operator publishes the TXT record themselves (by
   hand, or with their own dynamic-DNS tooling); OAG only computes the name and can resolve it.
   Automating publication would mean picking one specific registrar/provider API nobody asked for,
   plus a credential-storage pattern this project has no precedent for — a separate, later
   decision if ever wanted, not bundled into deciding what to resolve *to*.

See `spec/30-long-word-addressing.md` for the full mechanism this entry fed into.
