<div align="center">

# SiafuDB

**The embedded graph database for device, edge, and Web3.**

_Named after the African army ant (Dorylus) — small, embedded, unnoticed,_
_but the ecosystem collapses without it._

[![CI](https://github.com/siafuDB/siafudb/actions/workflows/ci-rust.yml/badge.svg)](https://github.com/siafuDB/siafudb/actions/workflows/ci-rust.yml)
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](https://www.apache.org/licenses/LICENSE-2.0)
![Rust](https://img.shields.io/badge/Rust-2024_edition-000000?logo=rust&logoColor=white)

**Version:** 0.1.0 (pre-release, unpublished) | **Website:** [siafudb.org](https://siafudb.org) | **Docs:** [siafudb.org](https://siafudb.org) · [`siafuDB/docs`](https://github.com/siafuDB/docs) | **Licence:** Apache 2.0

</div>

---

## What is SiafuDB?

SiafuDB is an embedded, high-performance property graph database for environments where server-side databases cannot reach — mobile devices, edge runtimes, Web3 nodes, and browsers. It brings graph-native intelligence to every layer of the stack, from the phone in your pocket to the decentralised node on the other side of the continent.

The relational model was designed in 1970 for accounting ledgers. The human brain is a graph. AI systems reason over graphs. The world is connected through relationships, not rows. Server-side graph databases solve this for the cloud. SiafuDB solves it everywhere else.

SiafuDB's engine is a **fork of [GrafeoDB](https://grafeo.dev) (Apache-2.0), frozen at 0.6.2** and vendored under [`engine/`](engine/). We do not track or depend on upstream ([ADR-0001](docs/adr/0001-fork-grafeo-and-freeze-upstream.md)). On top of it SiafuDB adds capabilities that no other embedded graph database offers: a graph sync protocol for subgraph replication, Web3-native pod storage, and optimised edge runtime deployment. SiafuDB is open-source infrastructure — free to use, extend, and deploy in any application, on any platform, for any purpose.

> **Status: 0.1.0, pre-release, Phase 1 in progress.** This repository is a
> Cargo workspace (five crates today; the web, store and FFI crates of Phase 1
> are being added) plus the vendored engine fork in `engine/`. The engine is
> **GrafeoDB 0.6.2, forked and pinned** (see [ADR-0001](docs/adr/0001-fork-grafeo-and-freeze-upstream.md)):
> `engine/UPSTREAM.md` records the provenance and CI fails if `engine/` drifts
> from the crates.io package except for changes logged in `engine/CHANGES.md`.
> The browser build is **web first** ([ADR-0002](docs/adr/0002-stack-and-target-priorities.md)):
> measured in headless Chromium (wasm in the main thread and a Worker, encrypted
> OPFS and IndexedDB persistence across reload, tamper rejection), see the
> [feasibility spike](docs/spike/README.md). Nothing is published yet: there is
> no `siafudb` on crates.io, npm or PyPI, and no Go module. Build from source.
> The capabilities below are split into what the engine provides today and what
> SiafuDB is building; read the split carefully before depending on it.

### What makes SiafuDB different?

**Graph Sync.** Every other embedded graph database is an island. SiafuDB instances sync with each other and with server-side graph databases through a CRDT-inspired Graph Sync Protocol. Write on your phone, sync to the cloud. Update on the server, sync to the device. Subgraphs replicate bidirectionally across any number of instances.

**Web3-native.** SiafuDB is designed to run as the storage engine for decentralised personal data pods. Cryptographic identity binding, pod replication across decentralised networks, sovereign data ownership. Your graph, your keys, your data.

**Edge-ready.** WASM-compiled builds run inside any edge runtime — Cloudflare Workers, Deno Deploy, Fastly Compute, Vercel Edge, or any WASM-capable environment. A cached regional subgraph at the edge serves queries in microseconds without round-tripping to the origin.

**AI-native.** Built-in vector search (HNSW with quantization), graph algorithms (PageRank, community detection, centrality), and semantic similarity queries. Your AI assistant reasons over a local knowledge graph — even offline.

**Pure Rust.** No C dependencies in the engine. Memory-safe, thread-safe, no garbage collection pauses. The core builds for `wasm32-unknown-unknown`, `aarch64-apple-ios` and `aarch64-linux-android` (CI-checked); Swift and Kotlin come from generated UniFFI bindings, not hand-written wrappers.

### Key Features

**Core engine (our GrafeoDB 0.6.2 fork):**

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

**SiafuDB extensions (in development — see the roadmap for status):**

- **Graph Sync Protocol** — CRDT-inspired bidirectional subgraph replication between SiafuDB instances and server-side graph databases
- **Web3 pod storage** — embedded graph store for decentralised personal data pods with cryptographic identity binding
- **Edge deployment profiles** — optimised WASM builds for edge runtimes, browsers, and constrained environments
- **Native mobile bindings** — Swift/iOS and Kotlin/Android generated by UniFFI from the Rust core (no React Native module)

## Languages and tooling

We maintain **Rust, TypeScript and Astro**; Swift and Kotlin exist only as
generated UniFFI bindings. JavaScript tooling requires **Node 24 or newer**
(`.nvmrc`, `engines`, CI). The browser is the first deliverable, then native
([ADR-0002](docs/adr/0002-stack-and-target-priorities.md)).

## Quick Start

SiafuDB is not published to any registry yet. Depend on it by path or by git:

```toml
[dependencies]
siafudb = { git = "https://github.com/siafuDB/siafudb" }
```

```rust
use siafudb_core::SiafuDB;

fn main() -> Result<(), siafudb_core::SiafuError> {
    // Open or create a database — the same shape as opening a SQLite file.
    let db = SiafuDB::open("my_app.siafu")?;

    // Execute graph queries with Cypher.
    db.execute("CREATE (:Person {name: 'Amara', city: 'Accra'})")?;

    Ok(())
}
```

Python, Node.js and Go bindings are planned, not built. When they ship they
will be listed here with the registry name; until then, `pip install
siafudb`, `npm install siafudb` and `cargo add siafudb` all fail, because
nothing has been published under those names.

### Workspace layout

| Path                    | What it is                                                                        |
| ----------------------- | --------------------------------------------------------------------------------- |
| `engine/`               | The graph engine: GrafeoDB 0.6.2 fork, six vendored crates, frozen (ADR-0001)     |
| `crates/siafudb`        | The unified crate — re-exports everything. What users depend on.                  |
| `crates/siafudb-core`   | Wraps the engine with fragments, cryptographic identity, changelog                |
| `crates/siafudb-sync`   | The Graph Sync Protocol — mutations, conflict resolution, GSPA/GSPI/GSPN adapters |
| `crates/siafudb-access` | Document (JSON), KV and time-series access patterns over the graph                |
| `crates/siafudb-server` | Optional standalone server for development and testing                            |
| `docs/`                 | ADRs (`docs/adr`) and the feasibility spike (`docs/spike`)                        |

Phase 1 adds, as they land: `crates/siafudb-wasm` (browser bindings),
`crates/siafudb-store` (page store and encryption), `crates/siafudb-ffi`
(UniFFI for Swift and Kotlin), `packages/*` (`@siafudb/web`, TypeScript) and
`apps/*` (Astro PWA demo). The Documentation site is Mintlify
(`siafudb-docs`).

## Use Cases

SiafuDB is general-purpose infrastructure. Some examples of what you can build:

**Offline-first mobile apps.** Your app holds a local knowledge graph on the device. Users query and traverse relationships without connectivity. Changes sync when they come back online.

**AI agents with persistent memory.** Your AI agent stores its knowledge graph in SiafuDB — entities it knows, relationships it has learned, context it has built. The graph travels with the agent. No external database required.

**Edge-native search and discovery.** Deploy SiafuDB in edge runtimes to serve regional subgraphs. Users query nearby entities, relationships, and recommendations without round-tripping to a central database.

**Decentralised personal data.** Each user's data lives in their own SiafuDB instance on a Web3 node. Sovereign, encrypted, portable. The user controls who can read their graph and what syncs to the platform.

**IoT device networks.** Model device relationships, sensor readings, and network topology as a graph on the device itself. Sync with a central graph when connected.

## Architecture

SiafuDB is designed as one half of a two-engine architecture. Server-side graph databases (JanusGraph or any graph database) handle the platform-scale graph. SiafuDB handles the embedded graph — on device, at the edge, in the pod, in the browser.

| Environment     | Engine                     | Role                                    |
| --------------- | -------------------------- | --------------------------------------- |
| Server (cloud)  | JanusGraph or any graph DB | Platform graph (billions of nodes)      |
| Device (mobile) | **SiafuDB** (native)       | Personal subgraph, offline AI reasoning |
| Edge (CDN)      | **SiafuDB** (WASM)         | Cached regional subgraphs               |
| Web3 (pod)      | **SiafuDB** (embedded)     | Sovereign personal data graph           |
| Browser (web)   | **SiafuDB** (WASM)         | Client-side graph cache                 |

The **Graph Sync Protocol** connects SiafuDB instances to each other and to server-side graph databases. The protocol is transport-agnostic — it works over HTTP, WebSocket, CouchDB replication, Kafka-compatible streaming, or any custom transport.

## Building from Source

### Prerequisites

- Rust **1.99 or newer** (edition 2024; the engine fork's MSRV)
- Node **24 or newer** and npm, for the TypeScript packages and browser tests
- The wasm target for browser builds: `rustup target add wasm32-unknown-unknown`

CMake and Python are listed in older drafts of this file; neither is needed
to build the workspace as it stands, because there are no native or Python
bindings in the tree yet.

### Build

```bash
git clone https://github.com/siafuDB/siafudb.git
cd siafudb
cargo build --release
```

### Run Tests

```bash
cargo test --workspace
# browser profile
cargo build -p siafudb-core --no-default-features --features browser --target wasm32-unknown-unknown
# verify engine/ still matches the crates.io 0.6.2 packages
scripts/ci/check-engine-pin.sh
```

For detailed build instructions, see
[CONTRIBUTING.md](https://github.com/siafuDB/siafudb/blob/main/CONTRIBUTING.md).

## Roadmap

The authoritative phase plan is [ADR-0002](docs/adr/0002-stack-and-target-priorities.md);
the list below keeps the long-range themes.

### Phase 0/1 — Foundation and web first (Current)

- [x] Establish SiafuDB project under Apache 2.0
- [x] Stand up the Cargo workspace
- [x] Fork GrafeoDB 0.6.2 and pin it under `engine/` ([ADR-0001](docs/adr/0001-fork-grafeo-and-freeze-upstream.md))
- [x] Node 24 baseline; CI for Rust, wasm, mobile libs and the engine pin
- [ ] `@siafudb/web`: wasm + TypeScript API + encrypted OPFS/IndexedDB persistence
- [ ] `siafudb-store` (page store trait + encryption), `gsp-wire` (signed op envelope), `siafudb-ffi` (UniFFI)
- [ ] Publish initial SiafuDB releases (Rust and npm first)
- [x] Launch siafudb.org documentation site

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

- [ ] Swift binding (iOS) generated by UniFFI from the Rust core
- [ ] Kotlin binding (Android) generated by UniFFI from the Rust core
- [ ] Real-device smoke tests
- Not planned: React Native / Expo native module (ADR-0002), hand-written FFI

## Engine

The engine is a **fork of GrafeoDB 0.6.2** (Apache-2.0), vendored in
[`engine/`](engine/) and frozen: no upstream tracking, no automatic bumps,
upstream fixes only as individually recorded cherry-picks
([ADR-0001](docs/adr/0001-fork-grafeo-and-freeze-upstream.md),
[`engine/UPSTREAM.md`](engine/UPSTREAM.md)). Every change we make to it is
listed in [`engine/CHANGES.md`](engine/CHANGES.md), as Apache-2.0 section 4(b)
requires, and a CI job checks that nothing else differs from the crates.io
packages. Upstream is a courtesy relationship: we thank it and may send
patches, but nothing in our build or roadmap waits on it.

Owning the engine lets us add what the browser and mobile targets need: a
page-level storage seam (OPFS, IndexedDB, encrypted pages), wasm-first
profiles, change capture that behaves the same on every target, and cipher
agility. Those are scheduled in the phases of ADR-0002.

The longer-term research direction (bringing published designs from KuzuDB,
LadybugDB and ArcadeDB into Rust around the engine) is unchanged in spirit but
no longer waits on a convergence stage: because we own the fork, such work
lands as ordinary reviewed changes, each recorded in `engine/CHANGES.md`.

## Contributing

We welcome contributions to SiafuDB. Whether it's bug fixes, performance improvements, documentation, or new features — every contribution strengthens the colony.

Please read our [Contributing Guide](CONTRIBUTING.md) before submitting a pull request. By contributing to SiafuDB, you agree that your contributions will be licensed under the Apache 2.0 License.

### Code of Conduct

SiafuDB is built on the Ubuntu philosophy — _I am because we are_. We are committed to providing a welcoming and inclusive environment for everyone. Please read our [Code of Conduct](CODE_OF_CONDUCT.md).

## Licence

SiafuDB is licensed under the [Apache License, Version 2.0](LICENSE).

**The Apache 2.0 licence will never change.** SiafuDB is governed by the
**Bundu Foundation** (Zimbabwean Company Limited by Guarantee) — a legal
entity with no shareholders that exists for the community. The Foundation's
charter structurally prevents relicensing. This is not a promise. It is a
legal guarantee.

© Bundu Foundation, operated by Nyuchi Africa (Pvt) Ltd.

## About

SiafuDB is open-source infrastructure governed by the **Bundu Foundation**
and operated by [Nyuchi](https://nyuchi.com).

Sibling Foundation projects include [NTL](https://openntl.org), the Neural
Transfer Layer, and [Mzizi](https://mzizi.dev), the open architecture and
design system. [Mukoko](https://mukoko.com) operates independently as a
product built on this infrastructure. Nyuchi Honeycomb — a decentralised
compute and storage network — appears in Foundation ecosystem copy but has
no public repository or address yet, so it is not linked here.

SiafuDB is not a product-specific tool. It is infrastructure for anyone building applications that need graph-native intelligence on device, at the edge, or in decentralised networks. Build what you need. The graph is yours.

---

<div align="center">

_The army ant carries the graph._

**[Website](https://siafudb.org)** · **[Documentation](https://siafudb.org)** · **[GitHub](https://github.com/siafuDB/siafudb)** · **[Docs repo](https://github.com/siafuDB/docs)**

</div>
