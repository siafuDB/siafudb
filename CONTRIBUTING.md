# Contributing to SiafuDB

Thank you for your interest in contributing to SiafuDB. Every contribution strengthens the colony.

SiafuDB is open-source infrastructure — an embedded graph database for device, edge, and Web3 environments. It is maintained by [Nyuchi Africa](https://nyuchi.com) and governed by the **The Bundu Foundation**. We welcome contributions from everyone — whether you're fixing a typo, improving documentation, reporting a bug, or building a major new feature.

## Getting Started

### 1. Fork and Clone

```bash
git clone https://github.com/your-username/siafudb.git
cd siafudb
```

### 2. Build from Source

**Prerequisites:**

- Rust 1.75+ (with cargo)
- CMake 3.15+ (for native bindings)
- Python 3.9+ (for Python bindings)
- wasm-pack (for WASM builds)

```bash
cargo build --release
```

### 3. Run Tests

```bash
cargo test
```

All tests must pass before submitting a pull request.

## How to Contribute

### Reporting Bugs

- Use [GitHub Issues](https://github.com/nyuchi/siafudb/issues) to report bugs
- Search existing issues first to avoid duplicates
- Include your environment details: OS, Rust version, language binding, SiafuDB version
- Provide a minimal reproduction case — the smallest possible code that demonstrates the bug
- Include the full error message or unexpected output

### Suggesting Features

- Use [GitHub Discussions](https://github.com/nyuchi/siafudb/discussions) for feature ideas and design discussions
- Describe the problem you're trying to solve, not just the solution you want
- Explain how the feature fits SiafuDB's mission: embedded graph with sync, edge, and Web3 capabilities

### Submitting Pull Requests

1. **Create an issue first** for anything beyond trivial fixes. This lets the community discuss the approach before you invest time coding.
2. **Fork the repository** and create a feature branch from `main`:
   ```bash
   git checkout -b feature/your-feature-name
   ```
3. **Write tests** for your changes. New features require tests. Bug fixes require a test that would have caught the bug.
4. **Follow the code style** (see below).
5. **Ensure all tests pass:**
   ```bash
   cargo test
   ```
6. **Commit with a clear message** and sign off (see below).
7. **Push to your fork** and submit a pull request against `main`.

### Commit Messages

Write clear, descriptive commit messages. Use the following format:

```
<component>: Short summary of the change (max 72 chars)

Longer description if needed. Explain what changed and why,
not how (the code shows how). Wrap at 72 characters.

Signed-off-by: Your Name <your.email@example.com>
```

**Components:** `core`, `sync`, `edge`, `pod`, `wasm`, `vector`, `fts`, `algo`, `gql`, `cypher`, `gremlin`, `python`, `nodejs`, `rust`, `go`, `swift`, `kotlin`, `arkts`, `dart`, `docs`, `ci`, `build`

**Examples:**

```
sync: Implement graph change log capture for vertex mutations

Implements local change log capture for VERTEX_CREATED and
VERTEX_DELETED events. Edge mutations will follow in a
subsequent PR. Part of the Graph Sync Protocol (Phase 2).

Signed-off-by: Tatenda Moyo <tatenda@example.com>
```

```
edge: Optimise WASM binary size for edge runtime deployment

Reduce WASM output from 4.2MB to 1.8MB by disabling unused
query language features in the edge build profile. Constrained
runtimes have memory limits; this gives comfortable headroom.

Signed-off-by: Rumbi Chikwanha <rumbi@example.com>
```

```
core: Fix memory leak in HNSW vector index during concurrent reads

The HNSW index was not releasing pinned buffer pages when
a vector search terminated early due to a top-k limit. This
caused gradual memory growth on long-running embedded instances.

Signed-off-by: Amara Diallo <amara@example.com>
```

### Sign-Off (DCO)

SiafuDB uses the [Developer Certificate of Origin](https://developercertificate.org/) (DCO). Every commit must include a `Signed-off-by` line certifying that you have the right to submit the code under the Apache 2.0 licence.

Add it automatically with:

```bash
git commit -s -m "your commit message"
```

Or add it manually to your commit message:

```
Signed-off-by: Your Name <your.email@example.com>
```

## Contribution Areas

SiafuDB has two layers: the **Grafeo core** (the upstream embedded graph engine) and the **SiafuDB extensions** (sync, edge, pod, bindings). Contributions to either layer are welcome, but they follow different paths.

### Grafeo Core (upstream contributions)

Bug fixes, performance improvements, and enhancements to the core graph engine — storage, query parsing, query execution, ACID transactions, vector search, graph algorithms, multi-language bindings — should ideally be contributed upstream to [Grafeo](https://github.com/GrafeoDB/grafeo). This ensures the entire Grafeo ecosystem benefits, and SiafuDB inherits the improvements automatically.

If you're unsure whether a change belongs in Grafeo core or SiafuDB extensions, open a discussion and we'll help you figure out the right home for it.

### SiafuDB Extensions (this repo)

These are the capabilities that make SiafuDB unique. We especially welcome contributions in:

**Graph Sync Protocol (highest priority)**
The most architecturally significant feature in development. The protocol that enables bidirectional subgraph replication between SiafuDB instances and server-side graph databases. Areas include:

- Graph change log format design and implementation
- CRDT-based conflict resolution for concurrent edits
- Transport adapters (HTTP, WebSocket, CouchDB replication, Kafka-compatible streaming)
- Subgraph scoping rules (what syncs where)
- Sync with JanusGraph, Grafeo Server, and other server-side graph databases
- Performance optimisation for constrained environments (mobile bandwidth, edge memory)

Start with the [Graph Sync Protocol design discussion](https://github.com/nyuchi/siafudb/discussions).

**Edge & WASM**

- WASM binary size and performance optimisation for edge runtimes (Cloudflare, Deno, Fastly, Vercel, browser, any WASM environment)
- Browser-based graph engine improvements
- Edge subgraph caching with configurable scoping rules
- Memory optimisation for constrained runtimes

**Web3 & Pod**

- Decentralised pod storage engine design
- Cryptographic identity binding for pod graphs
- Pod replication across decentralised networks
- Heritage graph transformation (PII stripping, anonymisation on lifecycle transitions)

**Native Platform Bindings**

- Swift/SwiftUI binding (iOS) via Rust FFI
- Kotlin/JVM binding (Android) improvements
- ArkTS/ArkUI binding (HarmonyOS) via N-API
- React Native and Flutter bridges

**Always Welcome**

- Documentation — tutorials, getting started guides, API reference, architecture docs
- Testing — expanded test coverage, edge cases, fuzzing, benchmarks
- CI/CD — build pipeline improvements, release automation, cross-platform testing
- Examples — sample applications demonstrating SiafuDB in real use cases

## Code Style

### Rust (core and extensions)

- Follow standard Rust conventions and the existing code style in the repository
- Use `cargo fmt` before committing
- Use `cargo clippy` to catch common mistakes
- Prefer safe Rust; use `unsafe` only when necessary and document why
- All public APIs must have documentation comments (`///`)
- Use meaningful type names and avoid abbreviations

### Python (bindings)

- Follow PEP 8
- Use type hints for function signatures
- Format with `black` and lint with `ruff`

### TypeScript/JavaScript (Node.js bindings)

- Use TypeScript for all new code
- Follow the existing code style
- Use `prettier` for formatting

### General

- No trailing whitespace
- End files with a newline
- Use UTF-8 encoding
- Keep lines under 100 characters where practical
- Write tests alongside your code, not as an afterthought

## Pull Request Review

All pull requests require at least one review from a project maintainer before merging. Reviewers will look for:

- **Correctness** — Does the code do what it claims?
- **Tests** — Are there sufficient tests? Do they cover edge cases?
- **Performance** — Does the change introduce regressions? (Benchmark for hot-path changes)
- **Safety** — Does the code use `unsafe` Rust? Is it justified and documented?
- **Style** — Does the code follow the project's conventions?
- **Documentation** — Are new features documented? Are changes reflected in relevant docs?
- **Scope** — Is the PR focused on a single concern? (Split large changes into smaller PRs)

Don't be discouraged by review feedback — it's how we maintain quality together. Every contributor's code gets reviewed, including maintainers'.

## Governance

SiafuDB is governed by the **Bundu Foundation** (Zimbabwean Company Limited by Guarantee), which protects the Apache 2.0 licence and ensures the project remains community-governed. SiafuDB is maintained by **Nyuchi Africa** (Pvt) Ltd, which provides engineering resources and release management. These are separate responsibilities — the Foundation governs the licence and community stewardship, Nyuchi provides the engineering.

Major architectural decisions (new extensions, breaking API changes, new supported platforms) are discussed openly in GitHub Discussions before implementation. The community has a voice in the project's direction.

## Community

- **[GitHub Discussions](https://github.com/nyuchi/siafudb/discussions)** — Questions, ideas, design discussions
- **[GitHub Issues](https://github.com/nyuchi/siafudb/issues)** — Bug reports, feature requests, task tracking
- **conduct@siafudb.org** — Code of Conduct concerns

Please read our [Code of Conduct](CODE_OF_CONDUCT.md) before participating in any community space.

## Licence

By contributing to SiafuDB, you agree that your contributions will be licensed under the [Apache License, Version 2.0](LICENSE). You retain copyright over your contributions.

The Apache 2.0 licence will never change. This is structurally guaranteed by the Bundu Foundation's charter — not a corporate promise that can be reversed.

---

_Every contribution strengthens the colony. Every ant matters._

_Built with Ubuntu — I am because we are._
