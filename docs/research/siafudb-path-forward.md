# SiafuDB — Path Forward

### Building the Definitive Embedded Graph Database for Device, Edge, and Web3

**Date:** April 2026
**Status:** LOCKED — Engineering strategy
**Decision Owner:** Bryan Fawcett, Founder & CEO, Nyuchi Africa
**Governance:** Bundu Foundation (Apache 2.0 licence)
**Repository:** github.com/nyuchi/siafudb

---

## The Decision

SiafuDB will be a **standalone, Rust-native embedded graph database** that combines the best architectural ideas and research from three Apache 2.0 / MIT-licensed projects — Grafeo, KuzuDB, and ArcadeDB — into a unified engine purpose-built for device, edge, and Web3 environments. SiafuDB does not depend on any upstream project as a runtime dependency. It is not a fork of any single project. It is a new engine, written in Rust, informed by the best ideas in the space, with original capabilities (Graph Sync Protocol, Web3 pod storage, edge-agnostic WASM deployment) that no other project offers.

The community outreach to Grafeo and LadybugDB proceeds as planned — collaboration and shared contribution are welcome and valued. But SiafuDB's engineering path does not depend on the outcome of those conversations. Whether Grafeo says yes to the distribution model, no, or goes silent — SiafuDB proceeds on the same trajectory.

---

## What We Take From Each Project

All three source projects are Apache 2.0 or MIT licensed. Algorithms, architectural patterns, and design ideas from published research are not proprietary. SiafuDB credits all three projects in THIRD_PARTY_NOTICES and acknowledges the research lineage openly. The implementation is original Rust code.

### From Grafeo (Apache 2.0, Rust)

Grafeo is the closest to what SiafuDB needs as a starting foundation. It is pure Rust with no C dependencies. The specific contributions to SiafuDB's design:

**Multi-language query architecture.** Grafeo's modular translator pattern — where GQL, Cypher, Gremlin, GraphQL, SPARQL, and SQL/PGQ are each parsed into ASTs and then translated to a unified logical execution plan — is the right architecture. SiafuDB adopts this pattern. One execution engine, multiple query language frontends. A developer uses whichever query language fits their context. The execution plan is the same underneath.

**Dual data model (LPG + RDF).** Labeled Property Graphs for application data (social networks, knowledge graphs, user profiles). RDF for semantic web and linked data (open data commons, Schema.org compliance). Both in the same engine, queryable through the same API. This is relevant for Mukoko's Schema.org compliance requirement and for the open data layer.

**HNSW vector search with quantization.** Scalar, binary, and product quantization (8-32x compression). Combined graph traversal with semantic similarity. This is how on-device Shamwari (or any AI agent) reasons over a local knowledge graph — traverse relationships AND find semantically similar nodes in one query.

**Change Data Capture (CDC).** Grafeo already has CDC capabilities. This is the foundation for the Graph Sync Protocol — capturing graph mutations as a change stream that can be replicated to other instances.

**Multi-language bindings via Rust FFI.** PyO3 (Python), napi-rs (Node.js/TypeScript), CGO (Go), C FFI, .NET P/Invoke, dart:ffi (Dart), wasm-bindgen (WebAssembly). The Rust FFI story is mature and these patterns are proven.

**Embedded and server modes.** Same engine, different deployment. This is critical — SiafuDB runs embedded on a phone AND as a server in a Honeycomb node, from the same codebase.

### From KuzuDB (MIT, C++)

KuzuDB's code is C++ and will not be directly used in SiafuDB's Rust codebase. However, the research from the University of Waterloo that produced KuzuDB contains algorithmic innovations that are not language-specific. These are published academic research results that can be re-implemented in any language:

**Worst-Case Optimal Joins (WCOJ).** The algorithm that produced 374x faster performance than Neo4j on path queries. Traditional graph databases use binary joins (hash join, merge join) which have worst-case performance that degrades exponentially with the number of joins. WCOJ algorithms (specifically the Leapfrog TrieJoin variant used in KuzuDB) guarantee optimal performance in the worst case for cyclic multi-way joins. This is the single most important algorithmic innovation in embedded graph databases in the last decade. SiafuDB implements WCOJ in Rust.

**Factorised execution.** Instead of materialising full intermediate result sets (which explode in size for multi-hop graph queries), factorised execution compresses intermediate results by exploiting the structure of the join tree. KuzuDB achieved 50-100x compression on intermediate results for multi-hop queries. This is critical for constrained environments (phones, edge runtimes) where memory is limited. SiafuDB implements factorised execution in Rust.

**Morsel-driven parallelism.** A parallel query execution model (originally from the HyPer database research) where work is divided into small "morsels" and distributed across CPU cores. Each core processes a morsel independently, with no global synchronisation until the morsel is complete. This gives near-linear scaling with core count on modern multi-core devices. SiafuDB implements morsel-driven parallelism in Rust.

**Columnar storage with SIMD vectorisation.** Storing graph data in columnar format (rather than row-oriented) enables SIMD (Single Instruction, Multiple Data) operations that process multiple values in parallel at the CPU instruction level. KuzuDB demonstrated that columnar storage is viable and performant for graph workloads, not just analytical/columnar databases. SiafuDB adopts columnar storage with Rust's native SIMD support.

**Single-file database format.** One file per database. Critical for device backup, pod replication, edge deployment, and encryption. A person's entire graph is one file on their phone, one file in their pod, one file at the edge. SiafuDB uses a single-file format.

**Typed schema enforcement.** Node tables and relationship tables with explicit schemas declared at creation time. The schema is the ontology. It is enforced at write time, not application time. This prevents schema drift across millions of devices and ensures that a Person vertex on a phone in Harare has the same schema as a Person vertex on a phone in Lagos.

### From ArcadeDB (Apache 2.0, Java)

ArcadeDB is written in Java, so no code is directly usable in SiafuDB's Rust engine. However, ArcadeDB has design innovations that are valuable, particularly in the multi-model space:

**Native multi-model architecture.** ArcadeDB natively stores graphs, documents, key-value pairs, full-text search, vectors, and time-series in a single engine. No polyglot persistence required — since each model is native to the database engine, there are no translation layers and no performance penalties. SiafuDB adopts this principle. A device doesn't just need graph storage — it also needs document storage (conversation history, cached content), key-value storage (session state, configuration), and time-series (activity patterns, health data). One engine handles all of it. No SQLite alongside the graph database.

**O(1) graph traversal via direct pointers.** Graph hops via direct pointers, not index lookups. ArcadeDB stores edges as direct memory pointers between records, so traversing from node A to node B is a single pointer dereference, not an index lookup. This is the "index-free adjacency" principle that Neo4j pioneered. SiafuDB implements this in Rust, where direct memory references are both fast and safe (Rust's ownership model prevents dangling pointers).

**70+ built-in graph algorithms.** ArcadeDB ships with pathfinding, centrality, community detection, link prediction, graph embeddings, and more. SiafuDB aims for a comprehensive algorithm library — not 70+ at launch, but a growing set of algorithms that covers the core needs: PageRank, shortest path, community detection (Louvain, Label Propagation), centrality (betweenness, closeness, degree), connected components, and similarity measures. These power on-device intelligence — Ubuntu score computation, recommendation features, community analysis — without network calls.

**Time-series as a native data model.** ArcadeDB's time-series engine with columnar compression (Gorilla, Delta-of-Delta, Simple-8b) achieves as low as 0.4 bytes per sample. For on-device health data, activity patterns, weather observations, and IoT sensor data, native time-series support eliminates the need for a separate time-series database or awkward workarounds in the graph model. SiafuDB includes time-series as a native model.

**Raft consensus for clustering.** ArcadeDB uses Raft for HA replication across server nodes. While SiafuDB's primary deployment is embedded (not clustered), the Graph Sync Protocol benefits from Raft-inspired consistency models for pod replication across Honeycomb nodes. The protocol doesn't need full Raft (pods don't form a consensus cluster), but the concepts of leader election, log replication, and quorum-based writes inform the sync protocol design.

**MCP server built-in.** ArcadeDB ships with an MCP (Model Context Protocol) server that lets AI assistants query the database directly. SiafuDB includes an MCP server as a first-class feature — enabling AI agents (on-device or cloud-based) to interact with the local graph through the standardised MCP interface.

---

## The SiafuDB Engine Architecture

Combining the best from all three projects, SiafuDB's architecture is:

```
┌─────────────────────────────────────────────────────────────┐
│                     Query Languages                          │
│  GQL │ Cypher │ Gremlin │ GraphQL │ SPARQL │ SQL/PGQ        │
│  (each parsed to AST, translated to unified logical plan)    │
└──────────────────────┬──────────────────────────────────────┘
                       │
┌──────────────────────▼──────────────────────────────────────┐
│                  Execution Engine                             │
│  Worst-Case Optimal Joins (WCOJ)                             │
│  Factorised Execution (50-100x intermediate compression)     │
│  Morsel-Driven Parallelism (near-linear core scaling)        │
│  SIMD Vectorisation (columnar batch processing)              │
└──────────────────────┬──────────────────────────────────────┘
                       │
┌──────────────────────▼──────────────────────────────────────┐
│                   Data Models                                │
│  Graph (LPG) │ Graph (RDF) │ Document │ KV │ Vector │ TimeSeries │
│  (all native, no translation layers)                         │
└──────────────────────┬──────────────────────────────────────┘
                       │
┌──────────────────────▼──────────────────────────────────────┐
│                  Storage Engine                               │
│  Columnar format │ Single-file │ ACID transactions            │
│  Index-free adjacency (direct pointer traversal)             │
│  HNSW vector index (with quantization)                       │
│  BM25 full-text index │ LSM-Tree secondary indexes           │
│  Typed schema enforcement                                    │
└──────────────────────┬──────────────────────────────────────┘
                       │
┌──────────────────────▼──────────────────────────────────────┐
│                SiafuDB Extensions                            │
│  Graph Sync Protocol (CRDT change logs, transport-agnostic)  │
│  Web3 Pod Storage (crypto identity binding, Honeycomb)       │
│  Edge Profiles (WASM builds, subgraph scoping)               │
│  MCP Server (AI agent interface)                             │
│  Native Bindings (Swift, Kotlin, ArkTS, Dart, WASM)          │
└─────────────────────────────────────────────────────────────┘
```

**Language:** Pure Rust. No C, no C++, no Java, no JVM dependencies.

**Licence:** Apache 2.0. Governed by the Bundu Foundation. Structurally guaranteed to never change.

**Deployment targets:** Mobile (iOS, Android, HarmonyOS), edge (any WASM-capable runtime), Web3 (Honeycomb nodes, any decentralised network), browser (WASM), server (standalone binary for development/testing).

---

## Engineering Phases

### Phase 1 — Core Engine (Months 1-4)

Build the foundational Rust engine with the storage layer and execution engine.

- Columnar storage engine with single-file format
- ACID transaction manager with configurable isolation
- Index-free adjacency for O(1) graph traversal
- Typed schema enforcement (node tables, relationship tables)
- GQL and Cypher query parsers (the two most critical languages)
- Unified logical plan executor
- WCOJ implementation for multi-way joins
- Basic graph algorithms (PageRank, shortest path, connected components)
- Rust API (the native interface all bindings build on)
- Test suite and benchmarks (LDBC Social Network Benchmark)

**Deliverable:** A Rust crate (`siafudb-core`) that can be embedded in a Rust application with GQL/Cypher queries, ACID transactions, and benchmark-validated performance.

### Phase 2 — Search & AI (Months 3-5, overlaps with Phase 1)

Add vector search and full-text search for AI-native workloads.

- HNSW vector index with scalar, binary, and product quantization
- BM25 full-text index with Unicode tokenization
- Combined graph + vector queries (traverse relationships AND find similar nodes)
- ONNX embedding generation (optional, opt-in)
- MCP server interface for AI agent interaction

**Deliverable:** SiafuDB can serve as the knowledge graph and memory store for AI agents, with combined graph traversal and semantic similarity in one query.

### Phase 3 — Multi-Model (Months 4-6)

Extend beyond graph to handle all on-device data needs.

- Document model (JSON/JSONB collections)
- Key-value model (fast get/set with hash index)
- Time-series model (columnar compression, time-bucketed aggregation)
- Additional query languages (Gremlin, GraphQL, SQL/PGQ)
- Factorised execution for memory-constrained environments

**Deliverable:** A single embedded database that handles graph, document, KV, vector, and time-series — eliminating the need for SQLite or any other database alongside SiafuDB.

### Phase 4 — Graph Sync Protocol (Months 5-8)

The differentiating capability that no other embedded graph database offers.

- Graph change log format specification
- Local change log capture (mutation tracking)
- CRDT-based conflict resolution (last-writer-wins with vector clocks)
- Subgraph scoping rules (what syncs where)
- Transport adapters (HTTP, WebSocket, CouchDB replication, Kafka-compatible, custom)
- Bidirectional sync between SiafuDB instances
- Sync with JanusGraph (and any Gremlin/Cypher-compatible server-side graph)
- Sync with Grafeo Server (if the relationship materialises)

**Deliverable:** Two SiafuDB instances (e.g., one on a phone, one on a server) can sync subgraphs bidirectionally with conflict resolution. This is the feature that changes the embedded graph database market.

### Phase 5 — Platform Bindings & Edge (Months 6-9)

Make SiafuDB available on every target platform.

- Python binding (PyO3)
- Node.js/TypeScript binding (napi-rs)
- Go binding (CGO)
- Swift binding (C FFI bridge for iOS)
- Kotlin binding (JNI for Android)
- ArkTS binding (N-API for HarmonyOS)
- Dart binding (dart:ffi)
- WASM build via wasm-bindgen (browser, edge runtimes — platform-agnostic)
- Optimised WASM profiles for constrained runtimes (binary size, memory)
- Morsel-driven parallelism tuning for mobile CPU architectures (ARM)

**Deliverable:** SiafuDB runs natively on iOS, Android, HarmonyOS, in any browser, and in any WASM-capable edge runtime. Published packages: PyPI, npm, crates.io, Go modules, CocoaPods/SPM, Maven Central.

### Phase 6 — Web3 & Pod (Months 8-12)

The sovereign personal data store for decentralised networks.

- Pod storage engine (one SiafuDB instance per person on a Honeycomb node)
- Cryptographic identity binding (pod graph bound to identity tokens)
- Pod replication across network nodes
- Heritage graph transformation (PII stripping on lifecycle transitions)
- Static gas metering for pod operations
- Pod provisioning workflow (progressive decentralisation — platform-held → sovereign)

**Deliverable:** SiafuDB serves as the personal sovereign graph database for decentralised personal data. Each person's pod is a SiafuDB instance on a network node, replicated, encrypted, and cryptographically bound to their identity.

---

## Relationship to Upstream Projects

### Grafeo

SiafuDB approaches Grafeo collaboratively. If Grafeo is receptive to the distribution model, SiafuDB may use Grafeo as a crate dependency for the core engine (Phases 1-3), contributing improvements upstream. If Grafeo declines or diverges, SiafuDB builds its own core informed by Grafeo's architectural patterns. Either way, SiafuDB credits Grafeo in THIRD_PARTY_NOTICES and contributes bug fixes to the Grafeo project when discovered.

The outreach to Grafeo proceeds as drafted. The engineering work on SiafuDB's extensions (Phases 4-6) proceeds regardless of the Grafeo response, as these capabilities are original to SiafuDB.

### LadybugDB / KuzuDB Forks

SiafuDB maintains a collaborative relationship with the LadybugDB community and other KuzuDB forks. The Graph Sync Protocol specification will be published as an open standard — any embedded graph database (LadybugDB, Grafeo, or others) can implement sync compatibility. Research insights from the University of Waterloo (WCOJ, factorised execution) that informed SiafuDB's execution engine design are cited and acknowledged.

### ArcadeDB

ArcadeDB's multi-model architecture and O(1) traversal design inform SiafuDB's design decisions. ArcadeDB is Java/JVM and cannot be directly used in SiafuDB's Rust engine, but the design principles — native multi-model, no translation layers, direct pointer traversal, comprehensive algorithm library — are adopted. SiafuDB credits ArcadeDB's design influence in documentation.

ArcadeDB could also serve as a **server-side complement** to SiafuDB in deployments where JanusGraph is not used. ArcadeDB's HA clustering (Raft consensus), eight protocol support (HTTP, PostgreSQL, Bolt, MongoDB, Redis, Gremlin, JDBC, gRPC), and multi-model storage make it a viable server-side graph database that the Graph Sync Protocol could target alongside JanusGraph and Grafeo Server.

---

## What SiafuDB Becomes

At the end of this engineering path, SiafuDB is:

**The only embedded graph database with built-in sync.** No other project offers bidirectional subgraph replication. This alone makes SiafuDB the right choice for any application that needs graph data on device and in the cloud.

**The only embedded graph database with Web3-native pod storage.** Personal sovereign data stored as a graph, cryptographically bound to identity, replicated across decentralised networks. No other project is building this.

**A true multi-model embedded database.** Graph, document, KV, vector, and time-series in one engine. Replaces SQLite AND whatever else you were running alongside it.

**The fastest embedded graph database for multi-hop queries.** WCOJ, factorised execution, and morsel-driven parallelism — the algorithmic innovations from KuzuDB's research, implemented in Rust with SIMD vectorisation.

**Platform-agnostic infrastructure.** Runs on iOS, Android, HarmonyOS, in any browser, in any WASM edge runtime, on any Web3 node. Not locked to any platform, any cloud provider, or any ecosystem. Apache 2.0, governed by the Bundu Foundation, structurally guaranteed to never change.

**Open-source infrastructure built in Africa, shared with the world.** Nyuchi Africa maintains it. The Bundu Foundation governs it. Anyone can use it. The army ant carries the graph.

---

## THIRD_PARTY_NOTICES (Updated)

The THIRD_PARTY_NOTICES file in the SiafuDB repository will credit all three projects:

```
THIRD-PARTY NOTICES

SiafuDB's design is informed by research and architectural patterns from
three open-source projects. SiafuDB does not contain code from these
projects (the implementation is original Rust code), but acknowledges
their intellectual contributions.

---

KuzuDB (MIT License)
Copyright (c) 2020-2025 Kùzu Inc.
https://github.com/kuzudb/kuzu

SiafuDB's execution engine implements algorithms published by the
University of Waterloo research team that created KuzuDB, including
Worst-Case Optimal Joins (WCOJ) and factorised execution. The
original repository was forked by Nyuchi Africa at v0.11.3 and
served as the initial SiafuDB repository before the Rust rewrite.

[Full MIT License text]

---

Grafeo (Apache License 2.0)
Copyright Grafeo Contributors
https://github.com/GrafeoDB/grafeo

SiafuDB's multi-language query architecture (AST → unified logical
plan) and dual data model (LPG + RDF) design are informed by
Grafeo's modular translator architecture.

[Full Apache 2.0 License text]

---

ArcadeDB (Apache License 2.0)
Copyright Arcade Data Ltd
https://github.com/ArcadeData/arcadedb

SiafuDB's native multi-model architecture (graph, document, KV,
vector, time-series in one engine) is informed by ArcadeDB's
multi-model design principles.

[Full Apache 2.0 License text]
```

---

_SiafuDB Path Forward — April 2026_
_Bryan Fawcett, Founder & CEO_
_Nyuchi Africa / Bundu Foundation_

_"Three projects. Three licences. One vision. The army ant takes the best from everywhere and builds something that didn't exist before."_
