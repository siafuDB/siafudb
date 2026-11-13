# Feasibility spike: findings

Date: 2026-11-12, updated 2026-11-13. Branch: `claude/siafudb-feasibility-spike-dhgusd`.
Scope: the four questions in the direction brief, plus the relational-data and
query-language investigation. The spike itself changed no source code; every
format and trait change below was a proposal awaiting sign-off.

> **What changed since the first draft.** (1) The engine is no longer an
> upstream dependency: it is a **fork of GrafeoDB frozen at 0.6.2**, vendored
> in `engine/` ([ADR-0001](../adr/0001-fork-grafeo-and-freeze-upstream.md)), so
> statements below about waiting on, negotiating with, or bumping upstream are
> superseded. (2) The stack and priorities are fixed in
> [ADR-0002](../adr/0002-stack-and-target-priorities.md): web first, UniFFI to
> Swift/Kotlin, no React Native, Node 24. (3) The browser is now **measured**
> in headless Chromium ([01](01-wasm.md#browser-measurements)). (4) 0.5.x
> encryption at rest silently did nothing; 0.6 fixes it and adds a store
> contract ([02](02-storage-backend.md)).

All claims were run, not read from docs, unless marked _unverified_. Probe
sources are in [`probes/`](probes/) so each result can be reproduced.

## Answers

| #   | Question                                      | Answer                                                           | Detail                                                       | Effort to productise                                      |
| --- | --------------------------------------------- | ---------------------------------------------------------------- | ------------------------------------------------------------ | --------------------------------------------------------- |
| 1   | Does GrafeoDB compile to `wasm32`?            | **Yes**, and it runs in headless Chromium, with 4 small changes  | [01-wasm.md](01-wasm.md)                                     | Done; committed browser tests are Phase 1                 |
| 2   | Is storage swappable (OPFS/IDB/encrypted)?    | **No at page level out of the box; yes at snapshot level**       | [02-storage-backend.md](02-storage-backend.md)               | Phase 1: 2-3 weeks. Phase 2: about 4-8 weeks, in our fork |
| 3   | Do mutations/change log carry algorithm tags? | **No.** Nothing is signed or verified today                      | [03-mutation-format.md](03-mutation-format.md)               | 2-3 weeks                                                 |
| 4   | iOS/Android build and UniFFI?                 | **Yes** to build and bindings. Not yet linked or run on a device | [04-mobile-uniffi.md](04-mobile-uniffi.md)                   | 1-2 weeks per platform                                    |
| +   | Relational data, SQL/PGQ, typed schema        | **Mostly already there in Grafeo**                               | [05-relational-and-queries.md](05-relational-and-queries.md) | Days                                                      |

Effort figures are estimates from reading the code, not commitments.

## Where the brief and the repo disagree

You asked to be told rather than have me pick silently.

1. **`siafudb-kuzu` is not in the brief.** It is an Apache-2.0 fork of archived
   KuzuDB (C++), unchanged from upstream apart from licence files, described as
   "the high-performance counterpart". The brief says one engine, GrafeoDB.
   _Question: is the Kuzu fork a hedge, a future server engine, or to be
   dropped?_ I did not touch it.
2. **README said `grafeo` 0.4; `Cargo.toml` said `0.5`; the lockfile had
   0.5.43. _Resolved:_** the engine is now a fork frozen at 0.6.2 under
   `engine/` ([ADR-0001](../adr/0001-fork-grafeo-and-freeze-upstream.md)). 0.6
   changes query semantics (e.g. `'5' = 5` is now false, `COMMIT` through
   `execute()` is refused); those are now simply our semantics. There is no
   further "bump" decision: we do not track upstream.
3. **`Cargo.toml` `repository` points to `github.com/nyuchi/siafudb`**; the
   repo lives under `siafuDB/`. _Still open._
4. **`SiafuConfig` / `DeploymentProfile` are exported but never consumed.**
   `SiafuDB::open` and `in_memory` take no config, so the memory limits and
   profile defaults documented in `config.rs` do nothing.
5. **`siafudb-access` is three 18-line stubs.** The KV and document patterns
   actually live as methods on `SiafuDB`. The brief's "KV, document,
   time-series are access patterns in `siafudb-access`" is the intent, not the
   current code.
6. **Replication replays raw query text** (`InProcessReplicator` re-executes
   `ChangeLogEntry.query` on the destination). That is incompatible with signed,
   content-addressed mutations. See 03.
7. **Two unrelated `ChangeLog` types exist** (core, query-text, live; sync,
   structured, `#![allow(dead_code)]`). Both are in-memory only: a restart loses
   the log.
8. **Security: `kv_set`/`kv_get`/`kv_delete` built Cypher by string
   interpolation** and escaped only `'` (not `\`). I confirmed the quote
   breakout: input ending in `\'` reaches the parser as code (probe in
   [05](05-relational-and-queries.md#kv-injection)). _Fixed since:_ commit
   `c394b30` binds query parameters and closes the kv/doc injection.
9. **Upstream bug (0.5.44):** `grafeo-engine` 0.5.44 did not compile with the
   `sql-pgq` feature unless `gremlin` or `graphql` was also on (`VarGen` is
   gated on them). With upstream no longer tracked, an upstream issue is a
   courtesy, not a dependency; _not re-checked on the 0.6.2 fork_.

## What needs your sign-off

Nothing below is merged or built.

1. **Storage trait** (`PageStore`, `KekProvider`, `EncryptedPageStore`): 02.
2. **Mutation envelope** (algorithm registry, multi-signature, content-addressed
   id, DID author): 03.
3. **Phase 1 first, Phase 2 deferred**: persist as encrypted snapshot + signed
   log now; build page-level storage in the engine fork (a page-backed
   `GraphBackend`) only if a real graph outgrows memory. 0.6's `GraphBackend`
   contract makes this more feasible than first estimated (02).
4. **Capture source for mutations**: engine CDC versus SiafuDB-level structured
   writes. CDC now works on wasm in Chromium (01), so browser sync need not
   have its own capture. See 03.
5. **The Kuzu fork's role** (above). The 0.5 to 0.6 timing question is closed
   by ADR-0001.

## Safe to do now (no sign-off needed)

- Fix the `kv_*` injection with parameterised queries.
- Gate `SiafuDB::open` behind `cfg(not(wasm32))`, swap `SystemTime` for
  `web-time`, add `uuid/js`, add a wasm CI job (01).
- Wire `SiafuConfig` into the constructors or delete it.
- Correct the README (done in the fork update; "pure Rust, no C" is true for
  the current tree but not for `blake3` defaults, see 03).

## Caveats on this spike

- The original standalone probes in `probes/` have no lockfile, so
  `grafeo-engine` resolved to 0.5.44 there; the repo's lockfile then pinned
  0.5.43. The wasm result was reproduced in a copy of the real workspace, and
  the browser probe (`probes/browser`) now builds the real `siafudb-core`
  against the 0.6.2 fork. The relational/query probes (05) were not re-run on
  the fork.
- The first wasm runs were in **Node 22**. The browser run is in **headless
  Chromium only**: OPFS, IndexedDB, Worker, encryption, tamper and CDC were
  exercised ([01](01-wasm.md#browser-measurements)), but
  `navigator.storage.persist()` returned `false`, and Firefox and WebKit were
  not tested.
- iOS and Android were built as Rust libraries (`cargo build --target`). I did
  not link an XCFramework or `.so`, and no Xcode/NDK was available, so nothing
  ran on a device or simulator.
- PQ crates (`ml-kem` 0.3.4, `ml-dsa` 0.1.2) compile on all three targets, but
  I did not test correctness, performance, or review their audit status.
  `ml-dsa` is a 0.1 release: _unverified_ whether it is fit for production.
