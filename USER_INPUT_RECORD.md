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
