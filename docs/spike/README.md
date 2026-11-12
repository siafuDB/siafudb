# Feasibility spike: findings

Date: 2026-11-12. Branch: `claude/siafudb-feasibility-spike-dhgusd`.
Scope: the four questions in the direction brief, plus the relational-data and
query-language investigation. **This PR changes no source code.** Every
format and trait change below is a proposal awaiting sign-off.

All claims were run, not read from docs, unless marked _unverified_. Probe
sources are in [`probes/`](probes/) so each result can be reproduced.

## Answers

| #   | Question                                      | Answer                                                           | Detail                                                       | Effort to productise                                     |
| --- | --------------------------------------------- | ---------------------------------------------------------------- | ------------------------------------------------------------ | -------------------------------------------------------- |
| 1   | Does GrafeoDB compile to `wasm32`?            | **Yes**, and it runs (Node), with 4 small changes                | [01-wasm.md](01-wasm.md)                                     | 2-3 days                                                 |
| 2   | Is storage swappable (OPFS/IDB/encrypted)?    | **No at page level. Yes at snapshot level**                      | [02-storage-backend.md](02-storage-backend.md)               | Phase 1: 2-3 weeks. Phase 2: 6-10+ weeks, needs upstream |
| 3   | Do mutations/change log carry algorithm tags? | **No.** Nothing is signed or verified today                      | [03-mutation-format.md](03-mutation-format.md)               | 2-3 weeks                                                |
| 4   | iOS/Android build and UniFFI?                 | **Yes** to build and bindings. Not yet linked or run on a device | [04-mobile-uniffi.md](04-mobile-uniffi.md)                   | 1-2 weeks per platform                                   |
| +   | Relational data, SQL/PGQ, typed schema        | **Mostly already there in Grafeo**                               | [05-relational-and-queries.md](05-relational-and-queries.md) | Days                                                     |

Effort figures are estimates from reading the code, not commitments.

## Where the brief and the repo disagree

You asked to be told rather than have me pick silently.

1. **`siafudb-kuzu` is not in the brief.** It is an Apache-2.0 fork of archived
   KuzuDB (C++), unchanged from upstream apart from licence files, described as
   "the high-performance counterpart". The brief says one engine, GrafeoDB.
   _Question: is the Kuzu fork a hedge, a future server engine, or to be
   dropped?_ I did not touch it.
2. **README says `grafeo` 0.4; `Cargo.toml` says `0.5`; the lockfile has
   0.5.43.** Upstream is already at **0.6.2** (grafeo-web, 8 Nov 2026), and 0.6
   changes query semantics (e.g. `'5' = 5` is now false, `COMMIT` through
   `execute()` is refused). `"0.5"` will not float to 0.6. A bump is a decision,
   not housekeeping.
3. **`Cargo.toml` `repository` points to `github.com/nyuchi/siafudb`**; the
   repo lives under `siafuDB/`.
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
8. **Security: `kv_set`/`kv_get`/`kv_delete` build Cypher by string
   interpolation** and escape only `'` (not `\`). I confirmed the quote
   breakout: input ending in `\'` reaches the parser as code (probe in
   [05](05-relational-and-queries.md#kv-injection)). I did not build a working
   destructive payload, but treat it as exploitable. Fix is parameterised
   queries. Small, independent of this spike.
9. **Upstream bug:** `grafeo-engine` 0.5.44 does not compile with the `sql-pgq`
   feature unless `gremlin` or `graphql` is also on (`VarGen` is gated on them).
   Worth an upstream issue.

## What needs your sign-off

Nothing below is merged or built.

1. **Storage trait** (`PageStore`, `KekProvider`, `EncryptedPageStore`): 02.
2. **Mutation envelope** (algorithm registry, multi-signature, content-addressed
   id, DID author): 03.
3. **Phase 1 first, Phase 2 deferred**: persist as encrypted snapshot + signed
   log now; only fork or upstream a page-level VFS if a real graph outgrows
   memory.
4. **Capture source for mutations**: Grafeo CDC (native/mobile) versus
   SiafuDB-level structured writes. The WASM engine has no change tracking
   upstream, so this decides whether browser sync needs its own capture. See 03.
5. **Grafeo 0.5 to 0.6 timing**, and the Kuzu fork's role (above).

## Safe to do now (no sign-off needed)

- Fix the `kv_*` injection with parameterised queries.
- Gate `SiafuDB::open` behind `cfg(not(wasm32))`, swap `SystemTime` for
  `web-time`, add `uuid/js`, add a wasm CI job (01).
- Wire `SiafuConfig` into the constructors or delete it.
- Correct the README (grafeo version, "pure Rust, no C" is true for the current
  tree but not for `blake3` defaults, see 03).
- File the `sql-pgq` upstream issue.

## Caveats on this spike

- The standalone probes in `probes/` have no lockfile, so `grafeo-engine`
  resolved to 0.5.44 there; the repo's lockfile pins 0.5.43. The wasm result
  was also reproduced in a copy of the real workspace (0.5.43).
- Wasm was run in **Node 22**, not a real browser. No OPFS, IndexedDB, Web
  Worker, or `navigator.storage.persist()` behaviour was exercised.
- iOS and Android were built as Rust libraries (`cargo build --target`). I did
  not link an XCFramework or `.so`, and no Xcode/NDK was available, so nothing
  ran on a device or simulator.
- PQ crates (`ml-kem` 0.3.4, `ml-dsa` 0.1.2) compile on all three targets, but
  I did not test correctness, performance, or review their audit status.
  `ml-dsa` is a 0.1 release: _unverified_ whether it is fit for production.
