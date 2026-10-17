# User Input Record

This file is a verbatim, append-only record of founding philosophical and architectural
statements from the project's creator — distinct from `OPEN_QUESTIONS.md` (day-to-day scope
decisions) and `docs/` (the living technical description of what's actually built). Entries here
are the *why* behind decisions that show up elsewhere as principles, deferred-future notes, or
memory entries. Nothing in this file is edited after the fact — new understanding gets a new entry
or a response underneath, never a rewrite of what was actually said.

---

## Entry 0 — 2026-09-27

Numbered 0, not appended at the end, because it's chronologically the *earliest* founding
statement on record — captured mid-session while backing up/rebuilding was being built, before
this file existed at all (it was created 2026-10-01, for Entry 1). It lived only in the
`future_ephemeral_peer_trust.md` auto-memory file until backfilled here on 2026-10-14, during a
pass specifically aimed at making sure nothing load-bearing exists only in memory. Entries 1-14
keep their original numbers; nothing already cross-referenced elsewhere needed renumbering.

The user described a significant future direction for OAG's peer-identity/trust model and
explicitly said not to implement it yet — just capture the concepts (this became
[24-ephemeral-peer-trust.md](spec/24-ephemeral-peer-trust.md), built 2026-10-09 per Entry 4 below).

> identities only last the lifetime of how long they run

On what trust should attach to, since the key itself resets every restart:

> all it has is its data - if it can use that to try to prove its identity - it can try

And, as a direct follow-up, on uptime itself being a trust signal:

> things need to stay connected and online for it to form trust

The user had read the PKT Network whitepaper in full (DeLisle & Berger, Feb 2021,
<https://pkt.cash/PKT_Network_v1.0_2021.02.01.pdf>, linked in `ENVIRONMENT.md`) and named it as
conceptual inspiration, explicitly not for its blockchain/token machinery:

> we do not want to implement it yet, but it highlights the concepts that are needed around an
> ever living connected network

The specific mechanisms identified as worth stealing the *shape* of: PacketCrypt's commit-then-
random-challenge pattern (pre-commit a merkle root over data you hold, then get randomly
challenged to produce a branch proving you really hold it — "prove your identity through your
data," not a bare claim); the Route Server's signed periodic presence gossip (self-reported but
independently verifiable by anyone who receives one, no central authority needed); announcements
decaying in value until unusable (a credential that needs ongoing renewal, not a one-time check);
the free-tier/fail-open design (a zero-guarantee minimal path always stays reachable rather than
locking out low-standing peers entirely); and cjdns's address-as-key-fingerprint, no PKI (already
exactly OAG's own `PeerId` model — confirmation, not a change).

On latency tolerance, verbatim:

> things are like tcp packets, but lets slow down - it could take forever for a packet to arrive -
> nice and slow - like a slow moving storm of packets that never stop moving - we need to keep
> this thing alive, even if it is frozen in ice

Read as: the trust/connectivity model should assume very long dormancy periods are normal and
recoverable, not treated as connection failure — a peer gone quiet for a long time and later
resumed should be able to pick back up, not be judged the same as one that actively misbehaved.
This is `spec/24-ephemeral-peer-trust.md`'s "decays patiently, not as a second cliff" trust-decay
shape, directly.

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

---

## Entry 14 — 2026-10-13

After long-word addressing shipped (Entry 13), the remaining named backlog narrowed to two items,
both again surfaced via `AskUserQuestion` rather than picked unilaterally: ephemeral trust's real
voting/consensus mechanism and proof-of-data-volume (still flagged as cutting against this
project's own "no global consensus" architecture principle), and peer clustering's open questions.

**The user picked peer clustering's open questions.** On inspection before implementing anything,
both sub-questions as originally framed in `spec/22-future-work.md`'s "First-class peer clustering"
section turned out to already be resolved — Phase 1 (`spec/26-peer-clustering.md`, this record's
own Entry 6) had already decided both explicitly: a cluster stays a pure networking construct, not
a graph concept, and intra-cluster replication deliberately does *not* get extra rigor beyond the
chosen full federation override. `spec/22-future-work.md`'s section simply never got updated to
reflect Phase 1 landing, unlike every other section in that file — a documentation-staleness
finding, not a new decision needing the user's input.

**Scoping decision made without a fresh round of questions, same pattern as prior entries**: rather
than going back to the user over a question that turned out to already have an answer, built the
one item genuinely still open from Phase 1's own "Explicitly deferred" list instead —
differentiated sync *cadence* for same-cluster peers (a second, independent, faster gossip loop,
`[cluster].gossip_interval_seconds`, default 5s, alongside the unchanged standard loop). This is a
real feature gap, not a re-litigation of anything already decided, and doesn't conflict with any
stated architecture principle the way the ephemeral-trust item does.

See `spec/26-peer-clustering.md`'s new "Phase 2" section for the full mechanism this entry fed
into.

---

## Entry 15 — 2026-10-14

Recorded verbatim at the user's explicit request ("put that down as original prompt text too for
further expansion") — captured as-is for a later session to react to, not pre-interpreted here
beyond the brief pull-apart below:

> okay, so we are going to add on some more things - this can be used for instant car networks -
> group computer control of linked moving devices etc, also applications in branched infinite
> expanding quantum upgress, as new directions are made and tracked, as we expand out we build the
> network - even though a network may only last 30 milli seconds - groups of this software can
> represent one thing, and is one of the ways you can prove that, that one thing is part of the
> same unit/grouping/organism

### Response

Three things tangled together, worth separating before anything gets spec'd:

**1. New application domains, not new mechanisms.** Instant car networks (vehicle-to-vehicle),
group control of linked moving devices (drone/robot swarms, fleets of actuators), and branching
exploratory "direction" trees that expand as new paths are tried and tracked — none of these
require anything OAG doesn't already model. A car, a drone, a sensor, and an explored hypothesis
are all just nodes; "this car talked to that car for an instant" is just an edge/assertion with
evidence. This is confirmation that the graph-of-claims model generalizes past "web crawling for
AI agents," not a request for new primitives by itself.

**2. Sub-second ephemerality — well past anything built so far.** `spec/24-ephemeral-peer-trust.md`
assumes identity lasting "the lifetime of how a process runs" (Entry 0): minutes to days. Even
peer clustering's fast intra-cluster gossip loop (`spec/26-peer-clustering.md`, Phase 2) defaults
to a 5-second cadence. A network lasting 30 milliseconds is a different regime entirely — there's
no time left for a multi-round gossip handshake; forming the group and recording that it formed
would have to collapse into something closer to a single signed broadcast than `oag-sync`'s
current `hello`/`heads`/pull exchange.

**3. The one genuinely new idea: network-formation itself as proof of composite identity.**
Peer clustering already has "a self-declared, signed cluster membership" (`spec/26`) — but today
that's purely a networking-layer trust-radius knob, explicitly *not* a graph concept (considered
and declined in `spec/22-future-work.md`'s "Peer clustering" section, for lack of a corroboration
story). What's new here is recasting that same shape as an identity primitive: the fact that a
specific set of peers briefly formed a network together — even for 30ms — becomes itself an
assertion in the graph ("these N peers were, for this instant, one thing"), corroborated and
ranked the same way any other claim is, rather than a transient fact that evaporates the moment
the network disappears. That's a direct extension of "claims backed by evidence, not declarations
of truth" onto group identity itself, and it's the piece that would actually require new spec work
— not the application domains, and not the latency.

Not implemented. Added to `spec/22-future-work.md` as a new, explicitly deferred direction so a
future session has this pulled-apart version to start from rather than re-deriving it from the raw
quote above.

---

## Entry 16 — 2026-10-14

Direct follow-up to Entry 15, same session. The user asked for a new section for "inputs":

> we want to add a section for inputs - this is a sensor input

Asked where this should live and what it was for (new spec concept vs. a new
`USER_INPUT_RECORD.md` entry category vs. something else). The user picked the first, and added
the key framing detail:

> sensor input as graph data is a good one yes, each graph is a packet

### Response

"Each graph is a packet" is the missing piece that ties Entry 15's three threads together. Read
as: a *subgraph* — some bundle of nodes/edges/assertions/evidence, e.g. one car's sensor readings
at one instant, or one drone's position-and-intent snapshot — is the unit that travels across one
of Entry 15's sub-second ephemeral networks, the same way a packet travels across a wire. That
reframes "sensor input" as not a new storage primitive (no new table, no new type alongside
Node/Edge/Assertion/Evidence) but a new *ingestion/transport* concept: a self-contained, signed
subgraph small enough to form, transmit, and be corroborated inside a 30ms-scale ephemeral network.
Not implemented — captured in `spec/22-future-work.md` alongside Entry 15 rather than as its own
numbered spec file, per this project's own rule that speculative work lives in
`22-future-work.md` until it's real.

---

## Entry 17 — 2026-10-14

Direct follow-up to Entry 16, same session — a concrete sensor example for "sensor input as a
graph packet":

> add a sensor: a neutrino detector - just a thin well drilled into the ground using the
> groundwater for sensing if possible

### Response

Recorded as the first concrete sensor example for Entry 16's "each graph is a packet" concept,
alongside the car-telemetry/drone-position examples already named in Entry 15/16. Worth flagging
honestly, not just filed away: real neutrino detection (Super-Kamiokande, IceCube) relies on
Cherenkov light through a very large, heavily shielded water or ice volume plus photomultiplier
arrays — a single narrow borehole's groundwater column is a vastly smaller target mass and has no
named shielding or photodetection in this description, so as physics it's unlikely to detect
neutrinos specifically at any practical rate. That caveat doesn't matter for what this entry
actually needs to feed, though: as a *graph-packet example*, the real content is "a fixed,
physical, continuously-sampled environmental sensor at a known location, reporting a value cheaply
and often" — a borehole/groundwater probe fits that shape regardless of what physical quantity it
actually ends up measuring (conductivity, water table level, temperature, seismic/vibration, or an
honest attempt at a particle-flux proxy). Added to `spec/22-future-work.md`'s sensor-input section
as a named example sensor type, with this caveat attached so a future session doesn't take the
neutrino-detection framing at face value without re-deriving the same concern.

---

## Entry 18 — 2026-10-14

> lets build a fire like neuron mode - every transaction requires a reach out to all connected
> nodes - this is part of anonymonity - we send information everywhere, and ask trusted members to
> discard - it is how we build secure clusters of moving data - part of this is network counter
> intelligence - no one knows what anyone is saying or can track it - this is how we transmit
> messages securly and forms the basic of a never ending morphing encrpytion

### Response

Five threads here, worth separating:

**1. Flood/broadcast propagation ("fire like neuron"), not today's pull-gossip.**
`14-replication-sync.md` is pull-based end to end: a peer calls `hello`, learns heads, and only
*fetches* events past its local head from peers it already knows about — `POST /events` exists for
protocol completeness but gossip/tests use pull as the real path. "Every transaction reaches out to
all connected nodes" is the opposite shape — a push-flood to every neighbor on every write, the way
an action potential fires down every connected synapse regardless of which downstream neuron
"needs" it. This is a genuinely different propagation model, not a tuning knob on the existing one.

**2. Anonymity via indistinguishable flood, not access control.** "We send information everywhere,
and ask trusted members to discard" is a specific, real anonymity technique: when every peer
receives every message, an outside observer watching any single link can't tell who the real
sender or intended recipient is, because real traffic and discarded traffic look identical on the
wire. This is the same shape as Dining-Cryptographers-style broadcast anonymity and the cover-
traffic idea in mixnets — distinct from everything currently built, since `docs/security.md`'s
identity model is pseudonymous (a `PeerId` is derived, not chosen) but not unlinkable: today,
*which* peer sent *which* event to *which* other peer is visible to anyone watching the sync
protocol. This entry asks for metadata/traffic-analysis resistance specifically, not just content
confidentiality.

**3. "Trusted members" doing the discarding ties straight to existing trust machinery.** Who counts
as "trusted" enough to be handed everything and rely on to discard the rest is exactly
`24-ephemeral-peer-trust.md`'s trust-weighted federation and `26-peer-clustering.md`'s signed
cluster membership — this entry is a new *use* for that existing trust signal (gating who
participates in the flood) rather than a new trust mechanism by itself.

**4. "Network counter intelligence" names the threat model explicitly.** Not just "keep data
confidential" but "resist an adversary who is watching the network itself, trying to map who talks
to whom." Worth stating plainly: this is a real, large design commitment (closer to Tor/mixnet
territory than anything in `docs/security.md` today) and in real tension with
`18-rate-limiting-and-resource-limits.md` and the bandwidth-conscious pull model `14-replication-
sync.md`'s own docs justify pull over push with — flooding *every* transaction to *all* connected
nodes multiplies bandwidth by fan-out, so this likely wants scoping (e.g. cluster-only flood, not
graph-wide) rather than replacing `oag-sync` wholesale. Flagged, not resolved.

**5. "Never ending morphing encryption" is named but not yet specified.** Read as: encryption state
that keeps changing as a message propagates hop to hop, rather than one fixed key/cipher for the
message's whole lifetime — the same family as onion routing's layered re-encryption, but "never
ending" suggests something more continuous than a fixed-depth onion. Genuinely open: what state
morphs, what triggers a change, and what a receiver needs to undo it. Not designed here, just
recorded as the thing this entry is reaching for.

Not implemented. Added to `spec/22-future-work.md` as a new deferred section so the propagation-
model/anonymity-model split above doesn't need re-deriving before any of this gets designed for
real.

---

## Entry 19 — 2026-10-14

> lets store data in this thing, and start figuring out how to host a page - ipfs can do file
> stuff, and there are js libs to load stuff from ipfs instead of from a http server

### Response

Builds directly on `spec/29-ipfs-pinning.md`, which already proves the single-file half of this:
evidence content can be pinned to IPFS today (opt-in, `[ipfs].pinning_provider`), with
`Evidence.metadata.ipfs_cid` as the retrieval pointer and `oag ipfs cat <cid>` reading it back.
Three real gaps stand between that and "host a page," worth naming precisely rather than treating
this as already solved:

**1. "Store data in this thing" is not yet true in the literal sense.** Today OAG stores a
`content_hash` and, optionally, a CID — a pointer to content pinned on an external Kubo node — not
the bytes themselves. `spec/22-future-work.md`'s existing IPFS section already names this directly:
"no local full-body evidence storage exists to replace." A page's HTML/CSS/JS would need to live
*somewhere* retrievable (OAG's own storage, or the configured IPFS node) — right now neither path
stores a page's actual bytes inside OAG itself.

**2. A page is a bundle of files, and Phase 1 only pins one file at a time.**
`oag-ipfs::KuboHttpProvider` does two HTTP calls against a single file (same pattern as the
crawler's main-page pinning). A real static page is `index.html` + CSS + JS as a set — IPFS's
answer to that is a UnixFS directory CID (pin a whole directory, get back one CID that resolves
subpaths), which Phase 1 never implemented or needed to, since it only ever pinned one evidence
body at a time. This is the actual missing mechanism, not a config toggle.

**3. "JS libs to load from IPFS instead of a HTTP server" names a real choice, not one thing.**
Worth being precise about what "instead of a HTTP server" means, since the options aren't
equivalent: (a) a **public IPFS HTTP gateway** (`ipfs.io/ipfs/<cid>`, or a self-run Kubo gateway) —
still an HTTP server in the loop, just not OAG's own; (b) **Helia** (the current JS/TS IPFS
implementation, successor to the deprecated js-ipfs) running directly in the browser, fetching
over bitswap/libp2p with no HTTP gateway at all — the genuinely serverless reading of "instead of
an HTTP server"; (c) a **service-worker gateway-in-browser** pattern (fetch once through a gateway,
then intercept and resolve subsequent requests from a local/peer cache). These have very different
trust and latency properties and aren't interchangeable — which one "host a page" means decides
most of the design.

Not implemented. Extended `spec/22-future-work.md`'s existing "Native IPFS integration" section
with this as a named next direction rather than opening a new section, since it's a continuation of
the same Phase 1/2 work, not a separate concept.

---

## Entry 20 — 2026-10-15

> keep going - can this thing serv a website yet? i wanted to use ipfs to feed files to a web
> browser - there is an ipfs js library - so we still serve some http stuff, but all the
> cdn/content is from ipfs - ipfs mode should not use an ipfs proxy - iirc ipfs js libary has this
> as a config - we are going to have to model this locally so we will have to figure that out too
> - also report back - there should be one rust process here right - how big is this rust.exe
> file, and how much memory does it take when it is running, and how much cpu does it take when
> it is running

Direct follow-up (same day) to Entry 19's static-page-hosting direction, now with concrete design
choices: in-browser fetch over bitswap/libp2p, explicitly never through an HTTP gateway/proxy
(the user's own memory of a real Helia config option, confirmed correct — see below), plus a
"model this locally" requirement and a direct resource-usage question.

### Response, part 1 — resource report

Confirmed one process, no subprocess/worker fork, 11 threads (tokio runtime). Measured against a
pre-this-feature release build: ~65 MB unstripped / ~56 MB stripped binary, ~20.5 MB RSS at
startup staying flat through a full idle gossip-loop tick, and immeasurably small idle CPU (0.03s
of CPU time across 50s of wall-clock idle). 30 light REST writes cost ~1-2 CPU ticks combined.

### Response, part 2 — built this session, with one correction along the way

Built `oag ipfs host-page <dir>` (pins a directory as one UnixFS tree) and `GET /ipfs-site/{cid}`
(a bootstrap page loading a vendored, gateway-free browser bundle — Helia, composed without ever
calling `withHTTP`, so the HTTP-gateway code path isn't even included in the bundle, a structural
guarantee rather than a config flag). First implementation used `KuboHttpProvider` — pinning via
a *separately run* Kubo daemon over HTTP, same as Phase 1/2's existing provider.

**The user caught a real inconsistency before live verification**: "we want to package this in
the end... everything is supposed to go in a single exe... what are we doing with kubo?" Right —
a separate daemon contradicts this project's own "single self-contained binary" posture everywhere
else. Researched whether a real embedded Rust IPFS node was viable (an old internal note had
dismissed `rust-ipfs` as "alpha-quality... far heavier than this narrow need justifies," written
for Phase 1/2's much narrower "one HTTP client call" need) — confirmed live that the actively-
maintained `dariusc93` fork (pushed within a day of this work) supports exactly what's needed:
bitswap, Kademlia DHT, mDNS, and a browser-dialable websocket listener, composable with
`default-features = false` to exclude its own HTTP-touching features. Swapped to a new
`EmbeddedIpfsProvider` as the new default-recommended `[ipfs].pinning_provider = "embedded"`
value; kept `KuboHttpProvider` as `"kubo"`, a legitimate option for an operator with existing Kubo
infrastructure, just not the one this project's own docs point to first.

**Real bugs found only by live end-to-end testing** (a real headless browser against a real
embedded node, not just `cargo test`): the browser's default connection gater silently refuses to
dial private/insecure (`ws`, non-TLS) addresses — exactly what our own local embedded node's
address always is — with zero network activity and no error; `@helia/verified-fetch` does its own
public-network provider lookup before trying bitswap, which stalls for many seconds against a
direct, already-connected peer that was never going to show up in a public DHT lookup (dropped it
entirely in favor of `@helia/unixfs`'s direct `ls`/`stat`/`cat`); `fs.ls()`'s own yielded entries
carry no usable `type` field despite the underlying library's own type declarations suggesting
otherwise (worked around with `fs.stat()` instead); `libp2p.dial()` needs a real `Multiaddr`
object, not the plain string the server hands the page. Final state, confirmed live: a real
two-file test site (`index.html` + `css/style.css`), pinned and served entirely by one `oag`
process with zero other processes running anywhere, rendered correctly in a real headless
Chromium with its CSS actually applied, fetched purely over bitswap.

Full mechanism, all judgment calls, and the "explicitly deferred" list (notably: the browser
client could in principle be the same `rust-ipfs` crate compiled to WASM instead of a separate JS
bundle — not pursued this round) are in `spec/29-ipfs-pinning.md`'s "Static page hosting" section.

## Entry 21 — 2026-10-15

> listen man - we are building new things - so get used to new stuff - what do we have to do to
> implement this - one of the main goals of this system is to enable distributed healing services
> globally - where people get paid for what they serve - right now we just trade work - like a
> residential cdn kinda

> we dont want to go that far, for now we just want records - announcments 'i gave user
> xsdjfklasdjlaksdfjsdfal file dsjfklasdjkldsafjklsfda' - i have some ideas about this, but lets
> figure it out, recticlium can be a communication back end - i dont know - this stuff needs to
> mesh togehter

> we have rules - i guess you can document this - but servers emit events like we are talking
> about, but the thing is - and there are at least two scenerios - the server might be serviing
> another node server, might be serving another node server - get it - we pass information across
> a thousand hosts if we need. also, we have a supply chain audit mode where trusted nodes can man
> in the middle and emit if the server is lieing - the node then gets a lower trust score or can
> even be booted until it proves itself again. this is not just about malicious intent - we are
> building a network that guards against any type of corruption - bad actor corruption, bad
> hadware corruption, bug corruption etc

A real pivot named, then deliberately narrowed: "paid for what they serve" (a residential-CDN-
style incentive economy) is a genuine future goal, but the user explicitly declined to design
payment/settlement right now ("we dont want to go that far, for now we just want records") in
favor of the smaller, immediately buildable piece underneath it — a serving peer announcing what
it served, to whom. Three requirements layered on top of that basic shape, in order:

1. **Multi-hop, not just two-party.** "the server might be servicing another node server... we
   pass information across a thousand hosts if we need" — a served request is not always
   client-to-leaf; a server can be relaying to another server, and a chain of these can span many
   hops. Today's relay machinery (`oag-sync`'s gossip — see `crates/oag-sync/src/service.rs`)
   only produces *transitive event propagation* ("I learned about peer C's events through peer
   B"), never a modeled, signed chain-of-custody for one specific transfer. There is no
   `hop_count`/`via_peer` field or multi-hop relay-with-witness structure anywhere in the codebase
   today (confirmed by direct search, not assumed).
2. **Supply-chain audit mode.** Trusted nodes can sit in a position to independently observe a
   transfer ("man in the middle") and emit a record if the serving node lied about what it sent.
   This is new: `DisputeAssertionPayload` (`crates/oag-events/src/payload.rs`, `POST
   /api/v1/assertions/{id}/dispute`) already lets any actor dispute any assertion with a reason,
   but it's a bare two-party "I disagree" — there's no notion today of a *witness* independently
   verifying a specific serving-announcement and the dispute carrying that verification weight
   differently from an ordinary disagreement.
3. **Consequence: trust score drop, or a full boot, reversible by re-proving.** This directly
   contradicts the explicit design philosophy already written into `spec/24-ephemeral-peer-trust.md`
   ("degrade, don't drop" — trust is a continuum, nothing nonzero is ever hard-excluded from
   federation; the only hard floor, exactly `0.0`, is for a forked peer or one never heard from at
   all). Also structurally new: trust scoring today is computed purely from a peer's own signed
   presence heartbeats — a *third party's* witnessed report never feeds `trust_score` anywhere
   right now. Confirmed by direct code read, not assumed.
4. **Framing: corruption-resistance, not just fraud-resistance.** "not just about malicious intent
   ... bad actor corruption, bad hardware corruption, bug corruption etc" — the audit mechanism is
   meant to catch any way served content can diverge from what was promised, regardless of intent.
   Nothing in the dispute model today distinguishes *why* a served artifact was wrong (malice vs.
   bit-flip vs. bug) — today's `reason` field is free text, not a structured cause.

Reticulum was floated as a possible transport for these announcements ("recticlium can be a
communication back end - i dont know"). It already is one, structurally: `oag-reticulum`
(`spec/15-reticulum-transport.md`) carries the exact same `hello`/`heads`/`events`/`peers` calls
`oag-sync` makes over HTTP, just over a Reticulum `Link` instead — a serving-announcement event
would propagate over either transport with no new wiring, since both are just alternate carriers
for the same signed event stream. See `future_service_announcements_and_audit.md` memory and
`spec/22-future-work.md`'s matching section for the full write-up; nothing has been built yet —
this entry and its cross-references exist so a future session picks up the real open questions
(hop-chain modeling, witness-weighted disputes, and the "degrade don't drop" vs. "can be booted"
tension) rather than re-deriving them.

## Entry 22 — 2026-10-15

> trust in this new type of network is only earned when verified - you are right, we do not boot,
> but the message from start to end has to get to the end correctly, if a node has figured out
> delibertly or accidently how to corrupt a message then we fix that - messages in this network
> can go through 50 nodes just to get at the end - all transmitting the same data - see how that
> works

Resolves Entry 21's "degrade, don't drop" vs. "can be booted" tension directly: no boot, confirmed.
The real requirement is end-to-end correctness despite untrusted/corrupting intermediate hops, and
the mechanism the user named — the same message going through on the order of 50 independent
node-chains at once, all carrying identical data — makes trust judgment unnecessary for *detecting*
corruption and only necessary for *degrading* a hop that's actually been caught: a receiving
endpoint content-hashes what arrives (the same discipline this project already applies to evidence
and IPFS CIDs), so a mismatch is a deterministic fact, not an accusation; comparing which of the
many parallel paths agree on the correct hash against which don't, and which specific nodes are
common to the failing paths but absent from the succeeding ones, attributes the corruption to a
hop without needing a stationed witness or a man-in-the-middle at all. Delivery itself never
breaks (the majority-hash copy wins, the rest are discarded) — "we fix that" splits into "the
message still arrives correctly" (automatic, from redundancy) and "the verified-bad node's trust
degrades" (never booted, and only after verification, never suspicion). Folded into
`spec/22-future-work.md`'s existing section as a refinement, not a new entry in the future-work
list — this is the resolution of Entry 21's open trust question, not a separate direction.

## Entry 23 — 2026-10-15

> it's the flood model, same mechanism

Direct answer to the question left open at the end of Entry 22's write-up: the ~50-independent-
node-chain redundant delivery is not a new propagation mechanism to design — it's Entry 18's
already-named "fire like neuron mode" push-flood (`spec/22-future-work.md`'s "Flood/broadcast
propagation with traffic-analysis resistance" section). One flood mechanism now serves two
named purposes rather than two separate ones: anonymity (Entry 18's original motivation — flooding
makes real and discarded traffic indistinguishable on the wire) and corruption attribution (Entry
21/22's motivation — flooding the same message across many paths lets a receiver compare which
paths agree on the content hash and localize which hop diverged). Both future-work write-ups
updated to cross-reference each other as the same mechanism, not two designs.

### Response — Phase 1 (single-hop) implemented, same day

After the design conversation above, the user asked to "start designing the serving-announcement
predicate and evidence shape," picked `sent_to` as the predicate, and — when asked how the receipt
should be shaped — said: "some type of mutual hash based on client and server randomness and the
file they served - build this into the transfer and reception mechanism." Then: "lets go all the
way - we can plan, but lets code."

Built exactly that, scoped to the single-hop building block (multi-hop chains and the flood-based
witness audit above remain untouched, as planned): a new `POST /oag/transfer/v1/fetch` endpoint,
authenticated by the request's own Ed25519 signature rather than a bearer token; both sides
contribute a random nonce; `oag_crypto::mutual_receipt_hash(client_nonce, server_nonce,
content_hash)` is the hash neither side could produce alone. Both sides independently write
`(peer:<server>, sent_to, peer:<client>)` into their own local graph, using the first real
construction anywhere in the codebase of `ActorType::Peer` (defined since the project's start,
never instantiated) and a new `EvidenceType::ServiceReceipt`. New CLI command `oag peer fetch
<address> <server_peer_id> <cid>`.

Found and fixed a real pre-existing gap along the way, not just new feature code: nothing external
could ever set `Evidence.metadata` before this — `EvidenceInput`/`AddEvidencePayload`/
`project_add_evidence` gained a generic `metadata: Option<JsonValue>` passthrough (merged with,
never replacing, the existing `pinned_cid` → `{"ipfs_cid": ...}` convenience key), now usable by
any evidence producer, not just this one.

Live-verified end-to-end, same discipline as every other feature this session: two real `oag`
processes (a server with `[ipfs].pinning_provider = "embedded"`, a real `oag backup --pin`'d file,
and an independent client peer in a separate data directory), a real `oag peer fetch` over real
HTTP — fetched bytes byte-identical to the original, both peers' independent `oag.sqlite`
databases carrying the exact same `receipt_hash`, with correctly swapped `peer_role`/
`counterparty_peer_id` on each side. 422+ existing tests still pass; `cargo clippy --workspace
--all-targets` is clean. Full design and "what this is not" are in
`spec/31-service-receipts.md`.

### Response — Phase 2 (static multi-hop relay), same day

Continued without a new prompt, picking up Entry 21's still-unbuilt "the server might be
servicing another node server... we pass information across a thousand hosts if we need." A
server configured with `[transfer].relay_peers` that misses its own local pin on a `cid` now acts
as the *client* of one of its own statically configured upstream peers (reusing the exact same
request/verify/record machinery `oag peer fetch` already used), then continues serving downstream
as if it had the content all along. Each hop stays an entirely independent, freshly re-signed
pairwise exchange, exactly as the original spec's "each hop here is independent" language
committed to — no new signed multi-party chain-of-custody structure, consistent with Phase 1's own
"explicitly deferred" framing of this. `relay_hops_remaining`/`relay_path` (both backward-
compatible via `#[serde(default)]`) bound the chain depth and prevent a relay loop; a relaying
hop's own evidence record gets one new metadata field, `relayed_via`, so a later audit pass could
follow which hops were relays versus origins.

Deliberately *not* built: any automatic CID-to-peer discovery or routing — `relay_peers` is a
plain, hand-written, operator-configured list; a chain exists exactly as deep as each hop's own
config points onward. That's recorded as a real, still-open design question
(`OPEN_QUESTIONS.md`), not quietly assumed.

Live-verified with three real `oag` processes, no shortcuts: an origin peer with content pinned;
a relay peer with `relay_peers` pointing at the origin and nothing pinned locally itself; a client
that has never heard of the origin at all, fetching only from the relay. Confirmed: the client's
received bytes are byte-identical to the origin's; the relay's own database independently ends up
with *two* records on its own `peer:<id>` node — a `direct` one serving the client (carrying
`relayed_via` pointing at the origin) and a `verification` one from its own fetch of the origin —
with the receipt hashes correctly paired leg-by-leg (client↔relay share one `receipt_hash`,
relay↔origin share a different one); the origin's own record names the relay, never the original
client, as its counterparty, since it was never contacted directly. All existing tests still
pass; `cargo clippy --workspace --all-targets` clean.
