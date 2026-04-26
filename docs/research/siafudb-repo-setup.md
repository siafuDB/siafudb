# SiafuDB Repository Setup Prompt

Use this prompt to transform the forked KuzuDB repository into SiafuDB. Run these steps against the repo at `github.com/nyuchi/siafudb`.

---

## Context for AI Assistant / Developer

You are setting up **SiafuDB** — an embedded, high-performance property graph database for device, edge, and Web3 environments. SiafuDB is forked from KuzuDB v0.11.3 (MIT-licensed, archived by Apple in October 2025). All new code is Apache 2.0. The project is maintained by Nyuchi Africa (Pvt) Ltd and governed by the The Bundu Foundation (Zimbabwe).

SiafuDB is named after the African army ant (_Dorylus_), known in Swahili as _siafu_ — small, embedded, unnoticed, but the ecosystem collapses without it.

**Key facts:**

- Original source: `github.com/kuzudb/kuzu` (archived, MIT License)
- New repo: `github.com/nyuchi/siafudb`
- New licence: Apache 2.0 for all new code. Original KuzuDB MIT notice preserved.
- Maintainer: Nyuchi Africa (Pvt) Ltd, Harare, Zimbabwe
- Governance: The Bundu Foundation (Zimbabwean Company Limited by Guarantee)
- Website: `siafudb.org` (primary), `siafudb.dev`, `siafudb.io`, `siafudb.com`
- Part of the Mukoko platform ecosystem — Africa's super app targeting one billion users

---

## Step 1: Replace the LICENSE file

Replace the existing `LICENSE` file with an Apache 2.0 licence. The copyright holder is "Nyuchi Africa (Pvt) Ltd and SiafuDB Contributors". The year is 2026.

Create the standard Apache 2.0 LICENSE file with:

```
Copyright 2026 Nyuchi Africa (Pvt) Ltd and SiafuDB Contributors
```

## Step 2: Create THIRD_PARTY_NOTICES file

Create a `THIRD_PARTY_NOTICES` file in the repo root that preserves the original KuzuDB MIT copyright:

```
This project is based on KuzuDB, originally developed by Kùzu Inc.
The original KuzuDB source code is licensed under the MIT License.

---

MIT License

Copyright (c) 2020-2025 Kùzu Inc.

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## Step 3: Replace the README.md

Replace the entire README.md with the following:

````markdown
<div align="center">

# SiafuDB

**The embedded graph database for device, edge, and Web3.**

*Named after the African army ant (Dorylus) — small, embedded, unnoticed,*
*but the ecosystem collapses without it.*

[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)
[![GitHub Stars](https://img.shields.io/github/stars/nyuchi/siafudb.svg)](https://github.com/nyuchi/siafudb/stargazers)

[Website](https://siafudb.org) · [Documentation](https://siafudb.org/docs) · [Getting Started](https://siafudb.org/docs/getting-started) · [Community](https://github.com/nyuchi/siafudb/discussions)

</div>

---

## What is SiafuDB?

SiafuDB is an embedded, high-performance property graph database purpose-built for environments where server-side databases cannot reach — mobile devices, edge runtimes, Web3 nodes, and browsers.

SiafuDB is forked from [KuzuDB](https://github.com/kuzudb/kuzu) v0.11.3, which was archived when Apple acquired Kùzu Inc. in October 2025. The original MIT-licensed codebase has been relicensed under **Apache 2.0** — and will never change. This is a structural guarantee enforced by the **The Bundu Foundation**, not a corporate promise.

### Why SiafuDB?

Every major platform — Facebook, Google, Amazon, Netflix — started with relational databases and spent billions building graph layers on top of them. The relational model was designed in 1970 for accounting ledgers. The human brain is a graph. AI systems reason over graphs. The world is connected through relationships, not rows.

Server-side graph databases (Neo4j, JanusGraph, TigerGraph) solve this for the cloud. But the device, the edge, and the decentralised web have been left behind — still running SQLite, still thinking in tables.

SiafuDB brings graph-native intelligence to every environment where data lives:

- **On your phone** — your AI assistant reasons over your personal knowledge graph locally, even offline
- **At the edge** — Cloudflare Workers and Durable Objects hold cached regional subgraphs via WASM
- **In your Web3 node** — sovereign personal data stored as a graph, cryptographically bound to your identity
- **In your browser** — WASM-compiled graph engine running client-side with zero server dependency

### Key Features

**Inherited from KuzuDB v0.11.3:**
- Embedded C++ engine — runs in-process, zero network overhead, sub-millisecond queries
- OpenCypher query language — the same Cypher used by Neo4j and the emerging GQL standard
- Columnar storage with SIMD-vectorised execution — 374x faster than Neo4j on path queries
- Vector search (ANN) — semantic similarity queries for AI/ML workloads
- Full-text search — built-in text indexing
- Graph algorithms — PageRank, community detection, shortest path
- Multi-language bindings — Python, Java, Node.js, Rust, C, C++
- WebAssembly support — runs in browsers and WASM runtimes
- Single-file database format — easy backup, replication, encryption

**Built by Nyuchi (in development):**
- **Graph Sync Protocol** — CRDT-inspired bidirectional subgraph replication between SiafuDB instances and server-side graph databases (JanusGraph)
- **WASM edge runtime** — optimised compilation for Cloudflare Durable Objects and Workers
- **Web3 pod integration** — embedded graph store for decentralised personal data pods
- **Multi-model extensions** — document, key-value, and enhanced vector storage alongside graph
- **Native mobile bindings** — Swift/iOS, Kotlin/Android, ArkTS/HarmonyOS

## Quick Start

### Python
```bash
pip install siafudb
````

```python
import siafudb

db = siafudb.Database('./my_graph.db')
conn = siafudb.Connection(db)

# Create schema
conn.execute("CREATE NODE TABLE Person(name STRING, age INT64, PRIMARY KEY (name))")
conn.execute("CREATE REL TABLE Knows(FROM Person TO Person, since INT64)")

# Add data
conn.execute("CREATE (:Person {name: 'Tatenda', age: 28})")
conn.execute("CREATE (:Person {name: 'Rumbi', age: 25})")
conn.execute("MATCH (a:Person {name: 'Tatenda'}), (b:Person {name: 'Rumbi'}) CREATE (a)-[:Knows {since: 2020}]->(b)")

# Query
result = conn.execute("MATCH (a:Person)-[:Knows]->(b:Person) RETURN a.name, b.name, a.age")
while result.has_next():
    print(result.get_next())
```

### Node.js

```bash
npm install siafudb
```

### Rust

```bash
cargo add siafudb
```

### Java

```xml
<dependency>
    <groupId>com.siafudb</groupId>
    <artifactId>siafudb</artifactId>
</dependency>
```

## Architecture

SiafuDB is designed as one half of a two-engine graph fabric:

| Environment     | Engine                           | Query Language   | Role                                         |
| --------------- | -------------------------------- | ---------------- | -------------------------------------------- |
| Server (cloud)  | JanusGraph on ScyllaDB/Cassandra | Gremlin + Cypher | Platform knowledge graph (billions of nodes) |
| Device (mobile) | **SiafuDB** (native)             | Cypher           | Personal subgraph, offline AI reasoning      |
| Edge (CDN)      | **SiafuDB** (WASM)               | Cypher           | Cached regional subgraphs                    |
| Web3 (pod)      | **SiafuDB** (embedded)           | Cypher           | Sovereign personal data graph                |
| Browser (web)   | **SiafuDB** (WASM)               | Cypher           | Client-side graph cache                      |

The **Graph Sync Protocol** connects SiafuDB instances to the server-side JanusGraph, enabling bidirectional subgraph replication. Write on your phone, sync to the cloud. Update on the platform, sync to your device. One graph, expressed everywhere.

## Building from Source

### Prerequisites

- CMake 3.15+
- C++20 compiler (GCC 11+, Clang 14+, MSVC 2022+)
- Python 3.9+ (for Python bindings)

### Build

```bash
git clone https://github.com/nyuchi/siafudb.git
cd siafudb
make release
```

### Run Tests

```bash
make test
```

For detailed build instructions, see the [Developer Guide](https://siafudb.org/docs/developer-guide).

## Roadmap

### Phase 1 — Foundation (Current)

- [x] Fork KuzuDB v0.11.3 under Apache 2.0
- [ ] Rebrand codebase (package names, imports, documentation)
- [ ] Publish initial SiafuDB releases (Python, Node.js, Rust, Java)
- [ ] Set up CI/CD pipeline
- [ ] Launch siafudb.org documentation site

### Phase 2 — Graph Sync Protocol

- [ ] Design graph change log format (vertex/edge CRUD events)
- [ ] Implement local change log capture
- [ ] Implement bidirectional sync with JanusGraph
- [ ] CRDT-based conflict resolution for concurrent edits
- [ ] Integration with CouchDB replication protocol

### Phase 3 — Edge & WASM

- [ ] Optimised WASM compilation for Cloudflare Workers/DOs
- [ ] Geographic subgraph caching in Durable Objects
- [ ] User subgraph caching in Durable Objects
- [ ] Browser-based graph engine improvements

### Phase 4 — Web3 & Pod

- [ ] Nyuchi Honeycomb node integration
- [ ] Cryptographic binding to identity tokens
- [ ] Pod replication across Honeycomb nodes
- [ ] Heritage graph transformation (PII stripping on ancestral transition)

### Phase 5 — Multi-Model Extensions

- [ ] Document storage (JSON/JSONB as vertex properties)
- [ ] Key-value operations
- [ ] Enhanced vector search (multiple dimensions, configurable metrics)
- [ ] Time-series support

### Phase 6 — Native Platform Bindings

- [ ] Swift/SwiftUI binding (iOS) via C interop
- [ ] Kotlin/JVM binding (Android) improvements
- [ ] ArkTS/ArkUI binding (HarmonyOS) via N-API
- [ ] React Native bridge

## Contributing

We welcome contributions to SiafuDB. Whether it's bug fixes, performance improvements, documentation, or new features — every contribution strengthens the colony.

Please read our [Contributing Guide](CONTRIBUTING.md) before submitting a pull request. By contributing to SiafuDB, you agree that your contributions will be licensed under the Apache 2.0 License.

### Code of Conduct

SiafuDB is built on the Ubuntu philosophy — _I am because we are_. We are committed to providing a welcoming and inclusive environment for everyone. Please read our [Code of Conduct](CODE_OF_CONDUCT.md).

## Licence

SiafuDB is licensed under the [Apache License, Version 2.0](LICENSE).

The original KuzuDB source code is licensed under the [MIT License](THIRD_PARTY_NOTICES). The MIT copyright notice is preserved as required.

**The Apache 2.0 licence will never change.** SiafuDB is governed by the **The Bundu Foundation** (Zimbabwean Company Limited by Guarantee) — a legal entity with no shareholders that exists for the community. The Foundation's charter structurally prevents relicensing. This is not a promise. It is a legal guarantee.

## About

SiafuDB is maintained by [Nyuchi Africa](https://nyuchi.com) and governed by the **The Bundu Foundation**.

Nyuchi Africa is building [Mukoko](https://mukoko.com) — Africa's super app, targeting one billion users across 54 African countries. SiafuDB is the embedded graph engine that powers every device, every edge node, and every sovereign data pod in the Mukoko ecosystem. Built in Africa. Shared with the world.

---

<div align="center">

_The army ant carries the graph._

**[Website](https://siafudb.org)** · **[Documentation](https://siafudb.org/docs)** · **[GitHub](https://github.com/nyuchi/siafudb)** · **[Community](https://github.com/nyuchi/siafudb/discussions)**

</div>
```

## Step 4: Create CONTRIBUTING.md

Create a `CONTRIBUTING.md` file:

````markdown
# Contributing to SiafuDB

Thank you for your interest in contributing to SiafuDB. Every contribution strengthens the colony.

## How to Contribute

### Reporting Issues
- Use GitHub Issues to report bugs or request features
- Include your environment details (OS, compiler version, language binding)
- Provide a minimal reproduction case when reporting bugs

### Pull Requests
1. Fork the repository
2. Create a feature branch (`git checkout -b feature/your-feature`)
3. Write tests for your changes
4. Ensure all tests pass (`make test`)
5. Submit a pull request

### Development Setup
```bash
git clone https://github.com/nyuchi/siafudb.git
cd siafudb
make release
make test
````

## Contribution Areas

We especially welcome contributions in these areas:

- **Graph Sync Protocol** — CRDT-based subgraph replication
- **WASM optimisation** — performance improvements for edge runtimes
- **Native bindings** — Swift, Kotlin, ArkTS platform bindings
- **Graph algorithms** — new algorithms for the `algo` extension
- **Documentation** — tutorials, guides, API reference improvements
- **Testing** — expanded test coverage, fuzzing, benchmarks

## Licence

By contributing to SiafuDB, you agree that your contributions will be licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE) for details.

## Code Style

- C++ code follows the existing KuzuDB style (clang-format configuration is included)
- Python code follows PEP 8
- All public APIs must have documentation comments

## Community

- GitHub Discussions for questions and ideas
- GitHub Issues for bugs and feature requests

Built with Ubuntu — _I am because we are_.

````

## Step 5: Create CODE_OF_CONDUCT.md

Create a standard Contributor Covenant Code of Conduct adapted with the Ubuntu philosophy:

```markdown
# Code of Conduct

## Ubuntu — I Am Because We Are

SiafuDB is built on the Ubuntu philosophy. We are committed to providing a welcoming, inclusive, and respectful environment for everyone, regardless of age, body size, disability, ethnicity, gender identity and expression, level of experience, nationality, personal appearance, race, religion, sexual identity and orientation, or geographic location.

## Our Standards

**Expected behaviour:**
- Using welcoming and inclusive language
- Being respectful of differing viewpoints and experiences
- Gracefully accepting constructive criticism
- Focusing on what is best for the community
- Showing empathy towards other community members

**Unacceptable behaviour:**
- Harassment, intimidation, or discrimination in any form
- Trolling, insulting or derogatory comments, and personal or political attacks
- Publishing others' private information without explicit permission
- Other conduct which could reasonably be considered inappropriate in a professional setting

## Enforcement

Instances of abusive, harassing, or otherwise unacceptable behaviour may be reported to the project maintainers at conduct@siafudb.org. All complaints will be reviewed and investigated promptly and fairly.

## Attribution

This Code of Conduct is adapted from the [Contributor Covenant](https://www.contributor-covenant.org/), version 2.1.
````

## Step 6: Update package references

The following files in the repository contain "kuzu" or "kuzudb" references that should be updated to "siafudb". This is a gradual process — do NOT attempt to rename everything in one commit, as it will break the build. Instead, create a tracking issue for the rebrand and address it incrementally:

**Priority 1 — User-facing (do immediately):**

- `README.md` (replaced in Step 3)
- `LICENSE` (replaced in Step 1)
- Package metadata files: `setup.py`, `pyproject.toml`, `package.json`, `Cargo.toml`, `pom.xml`
- Documentation files in `docs/`

**Priority 2 — Build system (do carefully with CI):**

- CMakeLists.txt project name
- CI/CD workflow files in `.github/workflows/`
- Docker files

**Priority 3 — Source code (do incrementally):**

- C++ namespace references
- Python module names
- Java package names
- Import paths across all language bindings

Create a GitHub Issue titled "Rebrand: KuzuDB → SiafuDB" to track all renaming work.

## Step 7: Enable GitHub features

In the repository settings:

**Topics:** `graph-database`, `embedded-database`, `property-graph`, `cypher`, `openCypher`, `wasm`, `web3`, `edge-computing`, `mobile-database`, `vector-search`, `graph-sync`, `apache-2`, `open-source`, `ai`, `knowledge-graph`, `embedded-graph`, `offline-first`, `decentralized`, `africa`, `tinkerpop`

**Features to enable:**

- Issues (for bug reports and feature requests)
- Discussions (for community Q&A and ideas)
- Projects (for roadmap tracking)
- Wiki (optional — may prefer external docs site)

**Branch protection on `main`:**

- Require pull request reviews before merging
- Require status checks to pass before merging
- Do not allow force pushes

**Description:** Embedded property graph database for device, edge, and Web3. Forked from KuzuDB v0.11.3, relicensed Apache 2.0 — never to change. Cypher queries, vector search, WASM-ready. Named after the African army ant — small, embedded, but the ecosystem collapses without it. Maintained by Nyuchi Africa.

**Website:** <https://siafudb.org>

## Step 8: Create initial GitHub Issues

Create these issues to establish the public roadmap:

1. **Rebrand: KuzuDB → SiafuDB** — Track all renaming across package metadata, build system, source code namespaces, and documentation.

2. **Design: Graph Sync Protocol specification** — Design the CRDT-inspired graph change log format and bidirectional sync protocol between SiafuDB instances and JanusGraph.

3. **Feature: Optimised WASM build for Cloudflare Workers** — Improve the existing WASM compilation for deployment inside Cloudflare Durable Objects and Workers runtime.

4. **Feature: Web3 pod integration** — Embedded graph store for decentralised personal data pods on the Nyuchi Honeycomb network.

5. **Feature: Swift/iOS native binding** — C interop bridge for embedding SiafuDB in iOS applications (Swift/SwiftUI).

6. **Feature: ArkTS/HarmonyOS binding** — N-API bridge for embedding SiafuDB in HarmonyOS applications.

7. **Feature: Multi-model extensions** — Document (JSON/JSONB), key-value, and enhanced vector storage alongside graph.

8. **CI/CD: Set up build and release pipeline** — GitHub Actions for building, testing, and publishing SiafuDB packages across Python, Node.js, Rust, Java, and WASM.

9. **Docs: Launch siafudb.org** — Documentation site with getting started guide, API reference, architecture overview, and contributing guide.

10. **Community: Outreach to LadybugDB and KuzuDB fork ecosystem** — Introduce SiafuDB and establish collaborative relationship with other KuzuDB forks.
