# SiafuDB Community Outreach

Two outreach messages: one to Grafeo (upstream core engine), one to the LadybugDB/KuzuDB fork community.

---

## 1. Outreach to Grafeo Maintainers

**Channel:** GitHub Discussion on GrafeoDB/grafeo, or direct email if available
**Subject:** Introducing SiafuDB — a Grafeo distribution for sync, edge, and Web3

---

Hi Grafeo team,

I'm Bryan Fawcett, Founder & CEO of Nyuchi Africa — a technology company based in Harare, Zimbabwe, building open-source infrastructure for the next generation of applications across Africa and beyond.

We've just launched **SiafuDB** (github.com/nyuchi/siafudb), an embedded graph database for device, edge, and Web3 environments. After evaluating every embedded graph database on the market — Neo4j (licensing), KuzuDB (archived/acquired by Apple), LadybugDB (community fork, uncertain roadmap), ArcadeDB (JVM dependency) — we landed on Grafeo as the core engine we want to build on.

Grafeo is exactly what the embedded graph space needs: pure Rust, Apache 2.0, multi-model, six query languages, WASM bindings, vector search, and serious performance. The architecture is right, the language choice is right, and the licence commitment is right. We want to build on top of it, not compete with it.

**What SiafuDB is:**

SiafuDB is a distribution of Grafeo — the same relationship as Ubuntu to Debian. Grafeo is the core engine. SiafuDB extends it with capabilities that don't exist in any embedded graph database today:

- **Graph Sync Protocol** — CRDT-inspired bidirectional subgraph replication between SiafuDB instances and server-side graph databases (JanusGraph, Grafeo Server, etc.). This is the big one. Every embedded graph database today is an island. We're building the bridge.
- **Web3 pod storage** — using the embedded graph engine as the storage layer for decentralised personal data pods with cryptographic identity binding.
- **Edge deployment profiles** — optimised WASM builds for edge runtimes (Cloudflare Workers, Deno Deploy, Fastly Compute, Vercel Edge, browser, any WASM-capable environment), with configurable subgraph scoping.
- **Native mobile bindings** — Swift/iOS, Kotlin/Android, ArkTS/HarmonyOS, beyond what Grafeo currently offers.

**What we're proposing:**

We'd like to build SiafuDB as a downstream distribution that depends on Grafeo as a Rust crate. We contribute bug fixes, performance improvements, and core engine enhancements upstream to Grafeo. We maintain SiafuDB's extensions (sync, edge, Web3, mobile bindings) in our own repo.

If any of our extension work is general enough to benefit the broader Grafeo community — for example, if the Graph Sync Protocol's change log capture mechanism could be useful as a Grafeo-native CDC (Change Data Capture) feature — we'd happily contribute it upstream.

**About Nyuchi Africa:**

We're building open-source infrastructure for a continent of one billion people. Our flagship product, Mukoko, is an application built on this infrastructure — but the infrastructure itself (SiafuDB, the Nyuchi Honeycomb decentralised network, the Nyuchi API Platform) is independent and available to anyone. SiafuDB is governed by the The Bundu Foundation (Zimbabwe), a legal entity with no shareholders that structurally guarantees the Apache 2.0 licence can never change.

We'd love to understand how you see the Grafeo ecosystem developing, whether a distribution model like this aligns with your vision, and how we can be good citizens in the community.

Looking forward to the conversation.

Bryan Fawcett
Founder & CEO, Nyuchi Africa
<bryan@nyuchi.com>
github.com/nyuchi/siafudb

---

## 2. Outreach to LadybugDB / KuzuDB Fork Community

**Channel:** LadybugDB Discord, GitHub Discussion, or direct message to Arun Sharma
**Subject:** SiafuDB — another KuzuDB descendant, different approach, same goals

---

Hi everyone,

I'm Bryan Fawcett, Founder & CEO of Nyuchi Africa, based in Harare, Zimbabwe. We build open-source infrastructure for applications across Africa and globally.

Like many of you, we were using KuzuDB and were caught off guard when it was archived after Apple's acquisition. We evaluated the fork landscape — LadybugDB, Bighorn, Vela Engineering's concurrent-write fork — and have deep respect for the speed at which the community rallied.

We've taken a slightly different path and wanted to introduce ourselves and our project: **SiafuDB** (github.com/nyuchi/siafudb).

**Where we diverge:**

Rather than maintaining the C++ KuzuDB codebase long-term, we've decided to build SiafuDB as a Rust-native project, using Grafeo (grafeo.dev) as the core embedded graph engine. Our reasoning:

- Rust provides memory safety, native WASM compilation, and fearless concurrency — critical for our target environments (mobile devices, Cloudflare edge runtimes, decentralised Web3 nodes).
- Maintaining a C++ codebase that, as one community member noted, "probably six people actually understand" is a risk we'd rather not carry for infrastructure we're betting the next 25 years on.
- Grafeo already delivers six query languages (GQL, Cypher, Gremlin, GraphQL, SPARQL, SQL/PGQ), WASM bindings, vector search with quantization, and ACID transactions — in pure Rust with no C dependencies.

**What SiafuDB adds that doesn't exist anywhere:**

- **Graph Sync Protocol** — bidirectional subgraph replication between embedded instances and server-side graph databases. Every embedded graph DB today is isolated. SiafuDB connects them.
- **Web3 pod storage** — embedded graph as the storage engine for decentralised personal data with cryptographic identity binding.
- **Edge-native deployment** — optimised WASM builds for any edge runtime, browser, or constrained environment.

**Why we're reaching out:**

We're not competitors. The embedded graph space needs more players, not fewer. Apple acquiring KuzuDB and archiving it was a loss for the entire ecosystem, and the more independent projects that exist in this space, the healthier it is for everyone.

We see potential for collaboration:

- If LadybugDB develops capabilities that would benefit SiafuDB (or vice versa), we should share.
- The Graph Sync Protocol we're designing is transport-agnostic and engine-agnostic. If LadybugDB or any other embedded graph database wanted to implement sync compatibility, the protocol spec will be open and Apache 2.0 licensed.
- Research and benchmarks from the KuzuDB era (particularly the University of Waterloo work on worst-case optimal joins and factorised execution) informed all of our thinking. We want to acknowledge that lineage openly.

SiafuDB is Apache 2.0, governed by the Bundu Foundation (a legal entity in Mauritius with no shareholders — the licence structurally cannot change). It is named after the African army ant — _siafu_ in Swahili — small, embedded, unnoticed, but the ecosystem collapses without it.

Happy to answer questions, share our architectural thinking, or just connect with fellow travellers in the embedded graph space.

Bryan Fawcett
Founder & CEO, Nyuchi Africa
<bryan@nyuchi.com>
github.com/nyuchi/siafudb
siafudb.org

---

## Notes for Bryan

**Timing:** Send the Grafeo outreach first. That relationship is more important strategically — Grafeo is the upstream engine. Wait for a response (or at least a few days) before the LadybugDB outreach, so you can reference the Grafeo relationship if it's established.

**Tone:** Both messages are deliberately collaborative, not competitive. The embedded graph space is small enough that burning bridges would be counterproductive. Grafeo's team needs to see Nyuchi as a contributor, not a taker. LadybugDB's community needs to see SiafuDB as a sibling, not a rival.

**Follow-up:** If Grafeo responds positively, the next step is a technical discussion about how SiafuDB would depend on Grafeo as a crate — whether Grafeo's API surface is stable enough for a downstream distribution, whether there are feature flags or build profiles that SiafuDB needs, and how upstream contributions would flow.
