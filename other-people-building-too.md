# Other people building too

A running list of other active projects doing adjacent work — AI/agent-oriented graphs,
knowledge graphs, or graph-shaped infrastructure, in Rust or otherwise. Distinct from the
architectural prior art cited in [`docs/architecture.md`](docs/architecture.md) (SSB, Nostr,
Holochain, OrbitDB, Nanopublications), which covers established protocols OAG's design borrows
from. This file instead tracks *contemporaries*: things being built right now that overlap in
problem space, so we notice convergent ideas, worth collaborating or diverging from, early.

Each entry: what it is, why it's here, and how it compares to OAG where that's known. Add to this
list as new relevant projects turn up — don't prune without a reason, since "they stopped" is
itself useful information.

## Surveyed 2026-10-14

- **[dialectforge/lu](https://github.com/dialectforge/lu)** — "An experimental AI built on a
  knowledge graph instead of model weights. Persistent, honest ('I don't know'), with scars and
  curiosity." Rust, AGPL-3.0 + commercial. Closest match in spirit: graph-as-substrate rather than
  weights-as-substrate, and licensed the same way OAG is.

- **[scbrown/quipu](https://github.com/scbrown/quipu)** — "AI-native knowledge graph with strict
  ontology enforcement." Rust. Ontology-first where OAG is corroboration-first (OAG doesn't
  enforce a schema on claims, it ranks competing ones) — worth watching as a contrast in how much
  structure to impose up front.

- **[Fortemi/fortemi](https://github.com/Fortemi/fortemi)** — Self-hosted AI knowledge base:
  hybrid semantic search (pgvector + FTS + RRF), MCP server, multi-provider LLM inference (Ollama,
  OpenAI, OpenRouter, llama.cpp), multimodal ingestion, knowledge graph. Rust + PostgreSQL — the
  external-database dependency OAG deliberately avoids (single binary + embedded SQLite only).

- **[naukograd-software/elri](https://github.com/naukograd-software/elri)** — Local-first,
  self-hostable platform: universal knowledge graph, CRDT sync, programmable interfaces,
  automation, AI agents. CRDT merge strategy is the OrbitDB-style approach OAG's architecture doc
  explicitly contrasts itself against (shared merged state vs. kept-separate provenance).

- **[agent-grounds/grund](https://github.com/agent-grounds/grund)** — "Keep your agents grounded —
  specs, docs, and code as one knowledge graph, always in sync." Narrower scope (single-repo/project
  grounding) than OAG's federated multi-peer graph, but same instinct that agents need a graph, not
  a vector store, under them.

- **[bigai-nlco/TongGraph](https://github.com/bigai-nlco/TongGraph)** — "Lightweight graph engine
  for AI graph context, memory, and agent harness with Rust Core." Closest in framing to OAG's own
  pitch ("infrastructure for AI agents") — worth a closer look at whether it's single-peer or
  networked.

- **[astorise/Magnetar](https://github.com/astorise/Magnetar)** — Hardware-agnostic compute
  runtime for AI, written in Rust; compiles/executes computation graphs across CPUs/GPUs/NPUs/TPUs.
  Different layer entirely (ML execution graphs, not semantic/claim graphs) but same "AI + graph +
  Rust" search surface — noted for disambiguation, not overlap.

- **[rnortman/graf](https://github.com/rnortman/graf)** — Simple Markdown-based graph
  knowledge/to-do system, Rust. Personal-scale, not federated or agent-facing, but same
  graph-of-knowledge instinct at a much smaller scope.

- **[philipperenzen/open-triplestore](https://github.com/philipperenzen/open-triplestore)** — Open
  source triplestore / semantic graph database aligned with linked-data community standards.
  RDF/triplestore angle is adjacent to OAG's Node/Edge/Assertion/Evidence schema; worth comparing
  query models if OAG ever adds a SPARQL-like interface.

## How this list gets used

Before describing any OAG capability as novel, check this list (and `docs/architecture.md`'s prior
art section) first — same discipline the architecture doc already applies to protocol-level prior
art, extended to active contemporaries.
