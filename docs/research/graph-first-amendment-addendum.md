# Graph-First Amendment — Addendum

### The Graph Fabric: Two Engines, Seven Layers, One Sync Protocol

**Date:** April 2026
**Status:** DRAFT — Pending Bryan's review
**Extends:** Graph-First Architectural Amendment (April 2026)
**Scope:** Affects all three canonical documents and adds a new open-source project to Nyuchi's portfolio
**Decision Owner:** Bryan Fawcett, Founder & CEO

---

## The Decision

The Graph-First Amendment established JanusGraph on ScyllaDB/Cassandra as the platform's primary data engine (Layers 3 and 7). This addendum completes the vision: **every layer of the seven-layer architecture that holds data adopts a graph-first approach.** The platform does not use graph databases in some layers and relational/document databases in others. The graph paradigm is universal.

To achieve this, the architecture adopts **two graph engines** purpose-built for their deployment contexts, connected by a **graph sync protocol** that replicates subgraphs across the entire stack.

**Engine 1 — JanusGraph (server-side).** Apache 2.0. Distributed graph database on ScyllaDB (hot) and Cassandra (cold). Handles Layers 3 and 7 — the platform's primary knowledge graph and the anonymised analytical graph. Proven at 20-billion-node scale. Gremlin query language via TinkerPop. Already specified in the Layer 3 Amendment and the Graph-First Amendment.

**Engine 2 — SiafuDB (device, edge, pod).** Apache 2.0. Forked from the archived KuzuDB project (MIT-licensed, v0.11.3). An embedded, lightweight, high-performance property graph database purpose-built for constrained environments — mobile devices, edge runtimes (any WASM-capable environment), browsers, and decentralised network nodes. Cypher query language (openCypher compatible). Handles Layers 1, 5, and 6 — the personal sovereign pod graph, the edge cache graph, and the device local graph. Extended by Nyuchi with a graph sync protocol, WASM compilation, Web3 pod integration, and multi-model capabilities.

**The Graph Sync Protocol** connects the two engines. Subgraphs replicate between the platform's JanusGraph (Layers 3/7) and every embedded instance (Layers 1/5/6) through a CRDT-inspired change log carried by Layer 4's orchestration infrastructure. The protocol is the nervous system of the graph fabric — it ensures that a person's identity, relationships, and context are consistently represented across their device, the nearest edge node, their sovereign pod, and the platform's knowledge graph.

Layer 2 (Supabase) is unaffected — it remains the narrow ACID ledger as specified in the Graph-First Amendment.

---

## The SiafuDB

### Origin and Licence

KuzuDB was an embedded property graph database developed by Kùzu Inc., based on research from the University of Waterloo. It was licensed under the MIT License. In October 2025, Apple acquired Kùzu Inc. and the GitHub repository was archived. The MIT-licensed source code remains permanently available to anyone who forks it — an MIT licence cannot be revoked retroactively.

Nyuchi Africa forks KuzuDB at version 0.11.3 (the final release) directly from the archived `kuzudb/kuzu` repository. All new code contributed by Nyuchi is licensed under **Apache 2.0** — the same licence as JanusGraph, Cassandra, CouchDB, Flink, Doris, Maestro, Ray, MLflow, Feast, and Great Expectations. The original KuzuDB MIT copyright notice is preserved in a `THIRD_PARTY_NOTICES` file as required by the MIT licence terms.

The project is governed by the **Mukoko Foundation** (Mauritius, Foundations Act 2012) — the same legal entity that governs MIT tokens, MXT tokens, and pod sovereignty. The Foundation has no shareholders and exists for the community. The Apache 2.0 licence is protected by the Foundation's Reserved Powers — it cannot be changed to a proprietary or restrictive licence without triggering the same governance process that protects the token economics and the platform charter. This is a structural guarantee, not a corporate promise.

The "Kuzu" trademark is not used (Apple likely holds the trademark through the acquisition of Kùzu Inc.). The project is named **SiafuDB** — after the African army ant (*Dorylus*), known in Swahili as *siafu*. Small, embedded, unnoticed, but the ecosystem collapses without it. The Swahili name is pan-African (serving 54 countries, not a single-language choice), and the metaphor is precise: millions of small instances, each holding a piece of the graph, collectively forming the intelligence infrastructure of the platform. The name follows the universal naming principle (utility apps and tools use universal names, not Shona) since SiafuDB is cross-continental infrastructure, not a concept with irreplaceable Shona cultural weight. Repository: `github.com/nyuchitech/siafudb`. Primary domain: `siafudb.org`.

### Architecture: Grafeo Core + SiafuDB Extensions

SiafuDB follows the distribution model — analogous to how Ubuntu extends Debian. The core embedded graph engine is [Grafeo](https://grafeo.dev), a pure-Rust, Apache 2.0 graph database with no C dependencies. SiafuDB takes Grafeo's core and adds the capabilities that make it unique: the Graph Sync Protocol, Web3 pod storage, edge deployment profiles, and native mobile bindings. SiafuDB does not maintain the core graph engine — Grafeo's community does. SiafuDB inherits all core improvements automatically and focuses engineering effort on the extensions that no other embedded graph database offers.

The decision to build on a Rust core (rather than maintaining the C++ KuzuDB codebase) is a frontier decision: Rust provides memory safety without garbage collection, native WASM compilation via `wasm-bindgen`, fearless concurrency for simultaneous read/write operations on device, and a growing ecosystem of database tooling. C++ is the language of the previous era. Rust is the language of the next.

### Core Capabilities (via Grafeo)

**Pure Rust engine.** No separate server process. No C dependencies. The database runs in-process, in the same memory space as the application. Zero network overhead. Memory-safe at compile time. Sub-millisecond query latency for local traversals.

**Six query languages.** GQL (ISO standard), Cypher (openCypher), Gremlin (TinkerPop), GraphQL, SPARQL, and SQL/PGQ. Choose the query language that fits the project. A developer can write Cypher queries on device and Gremlin queries on the server — or use the same language everywhere.

**ACID transactions.** Full transactional support with configurable isolation levels including serializable. Data integrity guaranteed even under concurrent access.

**Columnar storage with vectorised execution.** SIMD-accelerated query processing. Adaptive chunking. High performance on both OLTP and OLAP workloads.

**Vector search (HNSW).** Approximate Nearest Neighbor search with scalar, binary, and product quantization (8-32x compression). Combine graph traversal with semantic similarity for AI reasoning.

**BM25 full-text search.** Built-in full-text indexing with Unicode tokenization and stop word removal.

**Graph algorithms.** PageRank, single-source shortest path, centrality, community detection, and more as built-in capabilities.

**Multi-language bindings.** Python (PyO3), Node.js/TypeScript (napi-rs), Go (CGO), C (FFI), C# (.NET 8 P/Invoke), Dart (dart:ffi), WebAssembly (wasm-bindgen).

**Dual data model.** Labeled Property Graph (LPG) and Resource Description Framework (RDF) in the same engine. Choose the model that fits the domain.

**Embedded and server modes.** Same engine, different deployment. Embed in a mobile app or run as a standalone server with REST API.

### SiafuDB Extensions (new development)

SiafuDB's extensions are general-purpose infrastructure capabilities — available to any developer building applications that need embedded graph with sync, edge, or Web3 support. They are not product-specific features.

**Extension 1 — The Graph Sync Protocol**

The single most important contribution and the capability that does not exist anywhere in the open-source ecosystem. The Graph Sync Protocol enables bidirectional subgraph replication between embedded instances (device, edge, pod) and the platform's JanusGraph knowledge graph (Layer 3).

The protocol operates on **graph change logs** — an ordered sequence of graph mutations:

```
VERTEX_CREATED   {id: uuid, label: "Person", properties: {...}, timestamp: t1}
EDGE_CREATED     {id: uuid, label: "INTERESTED_IN", from: uuid, to: uuid, properties: {...}, timestamp: t2}
PROPERTY_UPDATED {target: uuid, key: "bio", old_value: "...", new_value: "...", timestamp: t3}
VERTEX_DELETED   {id: uuid, tombstone: true, timestamp: t4}
EDGE_DELETED     {id: uuid, tombstone: true, timestamp: t5}
```

Each embedded instance maintains a local change log of all mutations since the last sync. The sync protocol exchanges change logs between instances and the platform graph, applying mutations in causal order with conflict resolution rules:

**Vertex conflicts:** Same UUID across all instances (the person's UUID is identical in Supabase, JanusGraph, device LadybugDB, edge DO, and pod). If two instances create a vertex with the same UUID (shouldn't happen — UUIDs are globally unique), the earlier timestamp wins.

**Property conflicts:** Last-writer-wins on property updates, with vector clocks for causal ordering. If the device updates `bio` while the web app simultaneously updates `bio`, the later timestamp wins and the losing update is preserved in a conflict log for potential manual resolution.

**Edge conflicts:** Last-writer-wins on edge property updates. Edge creation is idempotent (creating an edge that already exists is a no-op). Edge deletion uses tombstones with a configurable retention period.

**Subgraph scoping:** Not every instance holds the entire graph. Each instance holds a **scoped subgraph** defined by sync rules:

The **device** holds the person's vertex, their direct edges (interests, memberships, follows, credentials), their immediate neighbourhood (the vertices those edges connect to — the organisations they belong to, the places they're located in, the interest categories they follow), and cached content vertices for offline access. Shamwari context-building works entirely within this local subgraph.

The **edge DO** (geographic) holds a regional subgraph — all verified organisations, places, events, and content vertices for its geographic scope, plus the edges between them. This powers search and discovery for users in that region without round-tripping to the platform graph.

The **edge DO** (user) holds the same scope as the device — the person's vertex and immediate neighbourhood. This serves web users who don't have native apps with device-local graphs.

The **pod** holds the person's sovereign subgraph — everything the device holds, plus personal-only data that never syncs to the platform (Digital Twin memory, AI conversation context, learned preferences, engagement patterns that the person has chosen to keep private).

**Sync transport:** The Graph Sync Protocol is transport-agnostic. In the Mukoko architecture, change logs are carried by:

CouchDB's replication protocol (Layer 4) for device ↔ platform sync — extending CouchDB's existing change feed mechanism to carry graph change logs alongside document changes.

Redpanda event topics (Layer 4) for platform → edge sync — geographic and user DOs receive graph change events through the same event streaming infrastructure that carries service bus events.

The Nyuchi Honeycomb protocol (Layer 1) for platform ↔ pod sync — graph change logs are replicated across Honeycomb nodes as part of the pod's storage operations, governed by NST allocation and NHC gas fees.

**Extension 2 — WASM Compilation (Edge Runtime)**

The SiafuDB core, extended with the Graph Sync Protocol, compiles to WebAssembly for execution inside any WASM-capable edge runtime — Cloudflare Workers and Durable Objects, Deno Deploy, Fastly Compute, Vercel Edge Functions, browsers, or any V8/WASM environment. A geographic edge instance holds a WASM-compiled embedded graph engine containing the cached subgraph of a region's entities, relationships, and context. A user edge instance holds the person's cached personal subgraph.

This replaces SQLite in edge layers with a graph-native store. Edge graph queries use Cypher — the same query language as the device and the server. The edge instance can answer "show me verified restaurants within 5km that my contacts have reviewed" as a local graph traversal without calling the platform's server-side graph. The WASM compilation is platform-agnostic — SiafuDB does not lock to any single edge provider.

**Extension 3 — Web3 Pod Integration (Sovereign Graph)**

The embedded graph engine becomes the pod's database on the Nyuchi Honeycomb network. Each person's pod runs one embedded instance, hosted on a Honeycomb node, containing their sovereign personal graph — their identity vertex, personal edges, Digital Twin memory, AI conversation context, and their sovereign copy of shared platform edges.

The pod graph is cryptographically bound to the person's MIT token. The single-file storage format enables efficient replication across Honeycomb nodes (NST governs storage allocation). Pod graph operations consume NHC gas. The static gas parameters for MIT holder pod operations are encoded in the Honeycomb protocol as specified in the Nyuchi Honeycomb Protocol v1.0.

When a person verifies and their pod is provisioned (progressive decentralisation), the migration from platform-held personal data to the sovereign pod is a subgraph extraction from JanusGraph → serialisation → write to the pod's embedded instance. Graph-to-graph. No relational-to-graph transformation required.

When a person's MIT transitions to ancestral status, the pod graph undergoes heritage transformation: PII is stripped by Flink, anonymised graph patterns flow to Doris (Layer 7), and the heritage graph is preserved in Cassandra (cold tier) as part of Africa's digital heritage archive. The pod's embedded instance is decommissioned, but the graph structure lives on — anonymised — in the heritage tier.

**Extension 4 — Multi-Model Extensions**

The device, edge, and pod don't only need graph storage. They also need document storage (Shamwari conversation history, cached content bodies), key-value storage (session state, configuration, feature flags), and potentially time-series storage (activity patterns, health data points). Rather than running the embedded graph engine alongside SQLite for non-graph data, Nyuchi extends the engine with:

**Document storage:** JSON/JSONB document support as vertex properties or standalone document collections. Shamwari conversation messages (currently in ScyllaDB) can be cached on-device as document vertices connected to the Conversation vertex by CONTAINS edges.

**Key-value storage:** Lightweight KV operations for configuration, session state, and cached values. Implemented as a specialised vertex type with `key` and `value` properties and a hash index on `key`.

**Vector storage:** Enhanced ANN capabilities beyond what KuzuDB 0.11.3 provides — multiple embedding dimensions, configurable distance metrics (cosine, euclidean, dot product), and incremental index updates. Critical for on-device Shamwari semantic reasoning.

These extensions mean one database per deployment context — one on device, one in each edge DO, one per pod — handling all local data needs in a single embedded engine.

**Extension 5 — Native Platform Bindings**

SiafuDB needs native bindings for every major mobile platform:

**iOS (Swift/SwiftUI):** Rust FFI bridge via `cbindgen`. Rust's C-compatible FFI exports are called from Swift through the standard C interop mechanism. No JVM, no bridging runtime, minimal overhead.

**Android (Kotlin/Jetpack Compose):** JNI bridge from the Rust core, or the existing Java binding via Grafeo's napi-rs pattern. Native performance on Android.

**HarmonyOS (ArkTS/ArkUI):** Native bridge via ArkTS's C/C++ interop (N-API compatible). HarmonyOS supports calling native libraries from ArkTS.

**Web (browser):** WASM compilation via `wasm-bindgen` (native to Rust). The web app runs the embedded graph engine in the browser. Combined with the Graph Sync Protocol, this gives web users a local graph cache that persists across sessions.

---

## The Seven-Layer Graph Fabric

With two graph engines and the Graph Sync Protocol, every data-holding layer of the architecture is graph-native:

### Layer 1 — The Pod (SiafuDB)

**Graph role:** Personal sovereign graph. The person's Digital Twin data — identity vertex, personal edges, AI conversation context, learned preferences, engagement patterns, Digital Twin memory.

**Engine:** SiafuDB (Apache 2.0, forked from KuzuDB).

**Storage:** Single-file format on Nyuchi Honeycomb node. Replicated across nodes per NST allocation.

**Sync:** Graph Sync Protocol via Honeycomb replication protocol. Bidirectional with Layer 3 (JanusGraph) for shared edges. Personal-only data never leaves the pod.

**Query language:** Cypher. Shamwari reasons over the personal graph natively.

**Sovereignty:** The person owns their pod graph. Cryptographically bound to MIT token. Accessible only with their keys. Apache 2.0 engine governed by the Mukoko Foundation.

### Layer 2 — The ACID Ledger (Supabase/PostgreSQL)

**Graph role:** None. This layer is the financial ledger, service bus, RBAC enforcement, and platform configuration. No graph. Postgres does what Postgres does.

### Layer 3 — The Knowledge Graph (JanusGraph on ScyllaDB/Cassandra)

**Graph role:** Platform primary knowledge graph. All entities (people, organisations, places, content, communities, interest categories), all relationships (membership, employment, authorship, interest, location, family, credentials, follows, transactions, contributions), all profile and preference data.

**Engine:** JanusGraph (Apache 2.0) on ScyllaDB (hot) and Cassandra (cold).

**Sync:** Authoritative source. Publishes graph change logs to Layer 4 (Redpanda topics and CouchDB change feeds) for downstream sync to Layers 1, 5, and 6. Receives change logs from devices and pods for upstream sync.

**Query language:** Gremlin (native), Cypher (via openCypher bridge). The Graph API wraps both in typed FastAPI endpoints.

### Layer 4 — The Orchestration Layer (CouchDB + Redpanda + Maestro)

**Graph role:** Graph sync transport. Layer 4 does not hold graph data — it carries graph change logs between layers. CouchDB's replication protocol carries device ↔ platform change logs. Redpanda topics carry platform → edge change logs. Maestro orchestrates sync workflows (conflict resolution, subgraph extraction for new device/DO/pod provisioning, heritage graph migration on ancestral transition).

**Engine:** No graph engine. Layer 4 is pipes, not warehouses.

### Layer 5 — The Edge Layer (SiafuDB via WASM)

**Graph role:** Cached regional and personal subgraphs at the edge. Geographic DOs hold regional subgraphs (verified entities, places, events for their geographic scope). User DOs hold personal subgraphs (the person's vertex and immediate neighbourhood for web users).

**Engine:** SiafuDB compiled to WASM, running inside any WASM-capable edge runtime.

**Storage:** Durable Object persistent storage (replaces SQLite).

**Sync:** Graph Sync Protocol via Redpanda event topics (Layer 4). Geographic DOs receive regional graph updates. User DOs receive personal graph updates. Changes flow upstream for web users who create content or update their profiles.

**Query language:** Cypher. Edge graph queries power search and discovery for users routed through that geographic DO.

### Layer 6 — The Device Layer (SiafuDB, native)

**Graph role:** Person's local subgraph for offline access and on-device AI reasoning. The person's vertex, their direct edges, their immediate neighbourhood, cached content vertices, and Shamwari's local context.

**Engine:** SiafuDB embedded natively — C++ with Swift bridge (iOS), JNI/Java binding (Android), N-API bridge (HarmonyOS), WASM (web browser).

**Storage:** Single-file database on device storage (replaces SQLite/RxDB).

**Sync:** Graph Sync Protocol via CouchDB replication (Layer 4). Bidirectional — device changes sync up to the platform graph, platform changes sync down to the device graph.

**Query language:** Cypher. On-device Shamwari traverses the local graph for context-building, semantic search (via vector embeddings), and offline reasoning. The Three intelligence layers (personal, community, platform) operate over the device graph when connectivity is unavailable.

### Layer 7 — The Open Data Layer (JanusGraph + Doris)

**Graph role:** Anonymised analytical graph for ML training, community detection, influence analysis, and open data research. A PII-stripped mirror of the platform's relationship structure, purpose-built for analytical workloads.

**Engine:** JanusGraph (Apache 2.0) on ScyllaDB/Cassandra, alongside Doris for columnar analytics.

**Sync:** One-directional — Flink consumes events from Layer 4's Redpanda, strips PII, writes anonymised graph structures to the Layer 7 JanusGraph instance and columnar data to Doris.

**Query language:** Gremlin (for graph traversals — community detection, influence propagation, recommendation graph features). SQL (for Doris columnar analytics). The two engines complement each other: Doris handles "how many users in Kenya engaged with BushTrade last quarter" (columnar aggregation). JanusGraph handles "what community clusters emerged among Kenyan BushTrade users" (graph algorithm).

---

## Why Not One Engine Everywhere

The question arises: why not use JanusGraph everywhere, or the Nyuchi Embedded Engine everywhere?

**JanusGraph cannot run on devices or at the edge.** JanusGraph is a distributed server-side graph database that requires ScyllaDB/Cassandra as a storage backend, a JVM for the Gremlin server, and network connectivity to its storage cluster. It cannot be embedded in a mobile app, compiled to WASM for a Durable Object, or bundled into a Honeycomb pod. It is the right engine for server-side graph workloads at billion-node scale. It is the wrong engine for constrained environments.

**The Nyuchi Embedded Engine cannot replace JanusGraph at platform scale.** An embedded graph engine is designed for single-user or bounded-scope workloads — one person's subgraph on a device, one region's subgraph in a DO, one person's pod graph on a Honeycomb node. It is not designed for a unified graph of one billion people with tens of billions of edges, distributed across a multi-datacenter cluster with sub-millisecond latency requirements. JanusGraph on ScyllaDB is the right engine for that workload.

**Two engines, one query language, one sync protocol.** Both engines speak Cypher (JanusGraph via the openCypher bridge, SiafuDB natively). A graph query written for the device works on the server with minor adjustments for scope. The Graph Sync Protocol makes the two engines feel like one distributed graph to the application layer — data written on the device appears in the platform graph after sync, and data written on the platform appears on the device after sync. The developer thinks in terms of "the graph" — not "which graph engine am I talking to."

---

## Changes Per Canonical Document (Addendum to Graph-First Amendment)

### THE MUKOKO ORDER v4

**Section 5 — Technology Table**

**INSERT** the SiafuDB:

```
| SiafuDB | Apache 2.0 | Embedded graph — device, edge, pod |
```

**Section 7 — The Covenants**

The graph fabric deepens multiple covenants. No covenant text changes are required, but the commentary should note: the first covenant ("Your data is yours") is now fulfilled by a graph-native sovereign pod. The fifth covenant ("The edge is fast") is now fulfilled by graph-native edge caches. The sixth covenant ("The device is capable") is now fulfilled by a graph-native device store with offline AI reasoning.

### MUKOKO ARCHITECTURE v4

**Section 5 — The Three Sources of Truth**

**UPDATE** the Web3 Pod paragraph:

```
**The Web3 Pod** — the personal source of truth. Your sovereign graph database on the
Nyuchi Honeycomb decentralised network. Powered by the SiafuDB
(Apache 2.0, forked from KuzuDB) — a lightweight, high-performance property graph
database embedded in each Honeycomb node. Your personal knowledge graph: identity,
relationships, preferences, Digital Twin memory, AI context. Cryptographically bound
to your MIT token. Accessible only with your keys. Synced with the platform's
knowledge graph via the Graph Sync Protocol. The personal graph and the platform
graph speak the same language (Cypher) and share the same vertex UUIDs — your
identity is one graph, expressed in three places (device, pod, platform).
```

**Section 6 — The Seven Data Layers**

**UPDATE** Layer 5 description to replace SQLite references with the SiafuDB:

```
Geographic DOs and user DOs hold cached subgraphs in WASM-compiled instances of
the SiafuDB, replacing SQLite. Edge graph queries use Cypher.
Synced via the Graph Sync Protocol carried over Layer 4's Redpanda event topics.
```

**UPDATE** Layer 6 description to replace SQLite/RxDB references:

```
The device holds a native instance of the SiafuDB — the
person's local subgraph for offline access and on-device Shamwari reasoning.
Replaces SQLite and RxDB. Synced via the Graph Sync Protocol carried over
Layer 4's CouchDB replication. Cypher queries on device match Cypher queries
on the server.
```

**Section 6 — The Application Stack, Infrastructure table**

**INSERT:**

```
| SiafuDB | Apache 2.0 | Embedded graph (device, edge, pod) |
```

**Section 11 — What Is Built vs. What Is Designed**

**INSERT** into "Designed, Not Yet Built":

```
SiafuDB: fork of KuzuDB v0.11.3 under Apache 2.0. Graph Sync
Protocol (CRDT-inspired subgraph replication). WASM compilation for edge runtimes.
Web3 pod integration with Honeycomb protocol. Multi-model extensions (document, KV,
enhanced vector). Native bindings: Swift (iOS), JNI (Android), N-API (HarmonyOS),
WASM (web). CouchDB extension for graph change log transport. Redpanda topic
schemas for graph sync events.
```

### MUKOKO MANIFESTO v4

**Section 05 — Open Source & Sovereign**

**INSERT:**

```
The embedded graph database that powers every device, every edge node, and every
sovereign pod is forked from KuzuDB — originally built by researchers at the
University of Waterloo, archived when Apple acquired its creator, and reborn under
Apache 2.0 governance by the Mukoko Foundation. The Foundation's charter guarantees
the licence can never change. No corporation can acquire, archive, or relicense the
database that holds your personal graph. Your device, your edge, your pod — all run
on sovereign graph infrastructure that the community owns permanently.
```

**Covenant Six — The Capability**

The sixth covenant corresponds to Layer 6. Add the graph dimension:

```
*The Capability:* Your device is not a dumb terminal waiting for the cloud. Your
device holds your personal graph — your relationships, your interests, your context,
your Digital Twin's local intelligence. When connectivity drops, your AI assistant
still knows who you are, what you care about, and what you were working on, because
your graph is local. The device graph syncs with the platform graph when connectivity
returns, but it never depends on it for your core experience. Capability lives where
you are.
```

---

## Sovereignty Audit (Addendum)

| Component | Licence | Layer(s) | Source of Truth? | Sovereign? |
|-----------|---------|----------|-----------------|------------|
| JanusGraph | Apache 2.0 | 3, 7 | Yes (platform graph, analytical graph) | Fully sovereign (Apache foundation) |
| SiafuDB | Apache 2.0 | 1, 5, 6 | Yes (pod — personal), No (edge, device — caches) | Fully sovereign (Mukoko Foundation) |
| Graph Sync Protocol | Apache 2.0 (Nyuchi) | 4 (transport) | No (sync mechanism) | Fully sovereign (Nyuchi proprietary → open-source) |
| ScyllaDB | Source-available | 3 (hot) | Yes (storage backend) | Cassandra fallback (zero-cost migration) |
| Apache Cassandra | Apache 2.0 | 3 (cold) | Yes (storage backend) | IS the sovereign fallback |
| Supabase/PostgreSQL | PostgreSQL licence | 2 | Yes (ACID ledger) | Self-hostable if needed |

Every graph engine in the architecture is Apache 2.0. JanusGraph is governed by the Apache Foundation. SiafuDB is governed by the Mukoko Foundation. The Nyuchi Honeycomb is governed by Nyuchi Africa. No component of the graph fabric can be relicensed, acquired, or held hostage by a corporation.

---

## SiafuDB as Independent Infrastructure

SiafuDB is not proprietary infrastructure and is not product-specific tooling. It is an independent open-source project that Nyuchi Africa maintains and contributes to the global graph database ecosystem. The capabilities Nyuchi builds — the Graph Sync Protocol, WASM edge compilation, Web3 pod integration — are general-purpose infrastructure contributions that benefit anyone building applications that need embedded graph with sync, edge, or decentralised capabilities.

SiafuDB is governed by the **Mukoko Foundation** (licence governance and community stewardship). It is maintained by **Nyuchi Africa** (engineering and release management). The **Nyuchi Honeycomb** decentralised network is separately managed and governed by Nyuchi Africa. **Mukoko** the product is independently governed and operates as one of many applications that may be built on this infrastructure. These are three distinct governance domains.

Any developer, company, or project can use SiafuDB without any dependency on Mukoko, the Honeycomb, or Nyuchi's services. SiafuDB is infrastructure for the world. Nyuchi happens to be the company that builds and maintains it. Mukoko happens to be the flagship product that demonstrates its capabilities.

Nyuchi's relationship with the Grafeo upstream project and the broader KuzuDB fork ecosystem (LadybugDB, Bighorn, Vela Engineering's concurrent-write fork) is collaborative, not competitive. Core engine improvements contributed to Grafeo benefit everyone. SiafuDB's extensions are Apache 2.0 and available for any project to adopt.

This positions Nyuchi Africa as a contributor to foundational open-source infrastructure — the company that maintains the embedded graph database the industry needs. Frontier infrastructure built in Africa, shared with the world.

---

## What This Preserves

The seven data layers remain seven. The three sources of truth remain three. The locked counts are unaffected. The mathematical order is intact. The graph-first paradigm, established in the Graph-First Amendment, is extended from two layers (3 and 7) to five layers (1, 3, 5, 6, 7) — with Layer 2 as the ACID ledger and Layer 4 as the sync transport. Every layer that holds data is now graph-native. The only layer that isn't graph-native (Layer 2) is the layer that shouldn't be — financial ledgers are inherently tabular.

The tri-mode principle is preserved across the entire graph fabric:

**Live Graph (Musha):** Real-time graph queries on device (Layer 6), at the edge (Layer 5), on the platform (Layer 3), and in the pod (Layer 1). The user's experience is powered by graph traversals at every layer.

**Intelligence Graph (Basa):** Graph algorithms run on the platform graph (Layer 3) and the analytical graph (Layer 7) for recommendation features, Ubuntu scores, and model training. On-device graph algorithms power local Shamwari reasoning (Layer 6).

**Heritage Graph (Nhaka):** Ancestral subgraphs are preserved in Cassandra (Layer 3, cold tier). Anonymised graph patterns flow to Doris (Layer 7) for the open data commons. The heritage graph preserves the network of relationships, contributions, and community connections that an ancestor built during their lifetime — across all layers, synced into a coherent heritage archive.

---

*Graph-First Amendment — Addendum: The Graph Fabric*
*April 2026*
*Drafted for Bryan Fawcett*
*Nyuchi Africa / The Bundu Family*

*"Two engines. Seven layers. One graph. The relational era is over. The graph era begins here — on every device, at every edge, in every pod, across the entire platform. Built in Africa. Shared with the world."*
