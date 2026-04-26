<div align="center">

# SiafuDB

**The embedded graph database for device, edge, and Web3.**

*Named after the African army ant (Dorylus) — small, embedded, unnoticed,*
*but the ecosystem collapses without it.*

[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)
[![GitHub Stars](https://img.shields.io/github/stars/nyuchitech/siafudb.svg)](https://github.com/nyuchitech/siafudb/stargazers)

[Website](https://siafudb.org) · [Documentation](https://siafudb.org/docs) · [Getting Started](https://siafudb.org/docs/getting-started) · [Community](https://github.com/nyuchitech/siafudb/discussions)

</div>

---

## What is SiafuDB?

SiafuDB is an embedded, high-performance property graph database for environments where server-side databases cannot reach — mobile devices, edge runtimes, Web3 nodes, and browsers. It brings graph-native intelligence to every layer of the stack, from the phone in your pocket to the decentralised node on the other side of the continent.

The relational model was designed in 1970 for accounting ledgers. The human brain is a graph. AI systems reason over graphs. The world is connected through relationships, not rows. Server-side graph databases solve this for the cloud. SiafuDB solves it everywhere else.

SiafuDB is built on [Grafeo](https://grafeo.dev), a pure-Rust embedded graph engine, extended with capabilities that no other embedded graph database offers: a graph sync protocol for subgraph replication, Web3-native pod storage, and optimised edge runtime deployment. SiafuDB is open-source infrastructure — free to use, extend, and deploy in any application, on any platform, for any purpose.

### What makes SiafuDB different?

**Graph Sync.** Every other embedded graph database is an island. SiafuDB instances sync with each other and with server-side graph databases through a CRDT-inspired Graph Sync Protocol. Write on your phone, sync to the cloud. Update on the server, sync to the device. Subgraphs replicate bidirectionally across any number of instances.

**Web3-native.** SiafuDB is designed to run as the storage engine for decentralised personal data pods. Cryptographic identity binding, pod replication across decentralised networks, sovereign data ownership. Your graph, your keys, your data.

**Edge-ready.** WASM-compiled builds run inside any edge runtime — Cloudflare Workers, Deno Deploy, Fastly Compute, Vercel Edge, or any WASM-capable environment. A cached regional subgraph at the edge serves queries in microseconds without round-tripping to the origin.

**AI-native.** Built-in vector search (HNSW with quantization), graph algorithms (PageRank, community detection, centrality), and semantic similarity queries. Your AI assistant reasons over a local knowledge graph — even offline.

**Pure Rust.** No C dependencies. Memory-safe, thread-safe, no garbage collection pauses. Compiles natively to every platform — iOS, Android, WASM, Linux, macOS, Windows.

### Key Features

**Core engine (via Grafeo):**
- Embedded Rust engine — in-process, zero network overhead, sub-millisecond queries
- Six query languages — GQL (ISO standard), Cypher, Gremlin, GraphQL, SPARQL, SQL/PGQ
- ACID transactions with configurable isolation levels
- Columnar storage with SIMD-vectorised execution
- Vector search (HNSW) with scalar, binary, and product quantization
- BM25 full-text search with Unicode tokenization
- Graph algorithms — PageRank, SSSP, centrality, community detection
- Multi-language bindings — Python, Node.js, Rust, Go, C, C#, Dart, WebAssembly
- Embedded and server modes — same engine, different deployment
- LPG and RDF dual data model support

**SiafuDB extensions (in development):**
- **Graph Sync Protocol** — CRDT-inspired bidirectional subgraph replication between SiafuDB instances and server-side graph databases
- **Web3 pod storage** — embedded graph store for decentralised personal data pods with cryptographic identity binding
- **Edge deployment profiles** — optimised WASM builds for edge runtimes, browsers, and constrained environments
- **Native mobile bindings** — Swift/iOS, Kotlin/Android, ArkTS/HarmonyOS

## Quick Start

### Python
```bash
pip install siafudb
```

```python
import siafudb

db = siafudb.Database('./my_graph.db')

# Create nodes
db.execute("INSERT (:Person {name: 'Tatenda', age: 28})")
db.execute("INSERT (:Person {name: 'Rumbi', age: 25})")

# Create a relationship
db.execute("""
    MATCH (a:Person {name: 'Tatenda'}), (b:Person {name: 'Rumbi'})
    INSERT (a)-[:KNOWS {since: 2020}]->(b)
""")

# Query the graph
result = db.execute("""
    MATCH (a:Person)-[:KNOWS]->(b:Person)
    RETURN a.name, b.name
""")
for row in result:
    print(row)
```

### Rust
```bash
cargo add siafudb
```

### Node.js
```bash
npm install siafudb
```

### Go
```bash
go get github.com/nyuchitech/siafudb-go
```

## Use Cases

SiafuDB is general-purpose infrastructure. Some examples of what you can build:

**Offline-first mobile apps.** Your app holds a local knowledge graph on the device. Users query and traverse relationships without connectivity. Changes sync when they come back online.

**AI agents with persistent memory.** Your AI agent stores its knowledge graph in SiafuDB — entities it knows, relationships it has learned, context it has built. The graph travels with the agent. No external database required.

**Edge-native search and discovery.** Deploy SiafuDB in edge runtimes to serve regional subgraphs. Users query nearby entities, relationships, and recommendations without round-tripping to a central database.

**Decentralised personal data.** Each user's data lives in their own SiafuDB instance on a Web3 node. Sovereign, encrypted, portable. The user controls who can read their graph and what syncs to the platform.

**IoT device networks.** Model device relationships, sensor readings, and network topology as a graph on the device itself. Sync with a central graph when connected.

## Architecture

SiafuDB is designed as one half of a two-engine architecture. Server-side graph databases (JanusGraph, Grafeo Server, or any graph database) handle the platform-scale graph. SiafuDB handles the embedded graph — on device, at the edge, in the pod, in the browser.

| Environment | Engine | Role |
|-------------|--------|------|
| Server (cloud) | JanusGraph, Grafeo Server, or any graph DB | Platform graph (billions of nodes) |
| Device (mobile) | **SiafuDB** (native) | Personal subgraph, offline AI reasoning |
| Edge (CDN) | **SiafuDB** (WASM) | Cached regional subgraphs |
| Web3 (pod) | **SiafuDB** (embedded) | Sovereign personal data graph |
| Browser (web) | **SiafuDB** (WASM) | Client-side graph cache |

The **Graph Sync Protocol** connects SiafuDB instances to each other and to server-side graph databases. The protocol is transport-agnostic — it works over HTTP, WebSocket, CouchDB replication, Kafka-compatible streaming, or any custom transport.

## Building from Source

### Prerequisites
- Rust 1.75+ (with cargo)
- CMake 3.15+ (for native bindings)
- Python 3.9+ (for Python bindings)
- wasm-pack (for WASM builds)

### Build
```bash
git clone https://github.com/nyuchitech/siafudb.git
cd siafudb
cargo build --release
```

### Run Tests
```bash
cargo test
```

### Build WASM
```bash
wasm-pack build --target web siafudb-edge
```

For detailed build instructions, see the [Developer Guide](https://siafudb.org/docs/developer-guide).

## Roadmap

### Phase 1 — Foundation (Current)
- [x] Establish SiafuDB project under Apache 2.0
- [ ] Integrate Grafeo core as the embedded engine
- [ ] Publish initial SiafuDB releases (Python, Node.js, Rust, Go)
- [ ] Set up CI/CD pipeline
- [ ] Launch siafudb.org documentation site

### Phase 2 — Graph Sync Protocol
- [ ] Design graph change log format (vertex/edge CRUD events)
- [ ] Implement local change log capture
- [ ] Implement bidirectional sync between SiafuDB instances
- [ ] CRDT-based conflict resolution for concurrent edits
- [ ] Transport adapters (HTTP, WebSocket, CouchDB, Kafka-compatible)
- [ ] Sync with JanusGraph and other server-side graph databases

### Phase 3 — Edge & WASM
- [ ] Optimised WASM builds for edge runtimes (Cloudflare, Deno, Fastly, Vercel, browser)
- [ ] Browser-based graph engine with persistent storage
- [ ] Edge subgraph caching with configurable scoping rules
- [ ] Memory and binary size optimisation for constrained runtimes

### Phase 4 — Web3 & Pod
- [ ] Decentralised pod storage engine
- [ ] Cryptographic identity binding for pod graphs
- [ ] Pod replication across decentralised networks
- [ ] Heritage graph transformation (PII stripping, anonymisation)

### Phase 5 — Native Platform Bindings
- [ ] Swift/SwiftUI binding (iOS) via C FFI
- [ ] Kotlin/JVM binding (Android)
- [ ] ArkTS/ArkUI binding (HarmonyOS) via N-API
- [ ] React Native and Flutter bridges

## Contributing

We welcome contributions to SiafuDB. Whether it's bug fixes, performance improvements, documentation, or new features — every contribution strengthens the colony.

Please read our [Contributing Guide](CONTRIBUTING.md) before submitting a pull request. By contributing to SiafuDB, you agree that your contributions will be licensed under the Apache 2.0 License.

### Code of Conduct

SiafuDB is built on the Ubuntu philosophy — *I am because we are*. We are committed to providing a welcoming and inclusive environment for everyone. Please read our [Code of Conduct](CODE_OF_CONDUCT.md).

## Licence

SiafuDB is licensed under the [Apache License, Version 2.0](LICENSE).

**The Apache 2.0 licence will never change.** SiafuDB is governed by the [Mukoko Foundation](https://mukoko.com/foundation) (Mauritius, Foundations Act 2012) — a legal entity with no shareholders that exists for the community. The Foundation's charter structurally prevents relicensing. This is not a promise. It is a legal guarantee.

## About

SiafuDB is open-source infrastructure maintained by [Nyuchi Africa](https://nyuchi.com) and governed by the [Mukoko Foundation](https://mukoko.com/foundation).

It is part of the broader open infrastructure ecosystem built by Nyuchi Africa, alongside the [Nyuchi Honeycomb](https://nyuchi.com/honeycomb) decentralised compute and storage network and the [Nyuchi API Platform](https://nyuchi.com/api). Each is independently governed — Nyuchi Africa manages the Honeycomb network and enterprise infrastructure, the Mukoko Foundation governs SiafuDB and the Mukoko token ecosystem, and [Mukoko](https://mukoko.com) operates independently as a product built on this infrastructure.

SiafuDB is not a product-specific tool. It is infrastructure for anyone building applications that need graph-native intelligence on device, at the edge, or in decentralised networks. Build what you need. The graph is yours.

---

<div align="center">

*The army ant carries the graph.*

**[Website](https://siafudb.org)** · **[Documentation](https://siafudb.org/docs)** · **[GitHub](https://github.com/nyuchitech/siafudb)** · **[Community](https://github.com/nyuchitech/siafudb/discussions)**

</div>
