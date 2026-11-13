# 1. Does GrafeoDB compile to `wasm32-unknown-unknown`?

**Yes. It also runs, in a real browser.** The engine is our fork of GrafeoDB
0.6.2 ([ADR-0001](../adr/0001-fork-grafeo-and-freeze-upstream.md)); its
`browser` feature profile executes Cypher, and snapshot export/import
round-trips. This was first shown under Node 22 and has since been **measured
in headless Chromium** (see [Browser measurements](#browser-measurements)).
SiafuDB needed four small changes to get there, now applied in
`siafudb-core` (`native` and `browser` features).

## Evidence

| Build                                                                               | Result                                                                                                    |
| ----------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------- |
| `siafudb-core` as-is, `wasm32-unknown-unknown`                                      | **Fails**: `getrandom 0.4` needs the `wasm_js` feature (pulled in by Grafeo's `embedded` default profile) |
| `grafeo` with `default-features=false, features=["browser","cypher"]`               | Builds                                                                                                    |
| Same + `uuid` v4, `ed25519-dalek` 3, `sha2` 0.11                                    | Builds once `uuid` has the `js` feature                                                                   |
| `grafeo` with SiafuDB's current features (`gql, cypher, wal`) + `getrandom/wasm_js` | **Fails**: `fs2` and `tokio` (`full`) do not support wasm                                                 |
| `siafudb-core` with `browser` profile                                               | One error: `SiafuDB::open` needs `GrafeoDB::open`, which needs `wal`                                      |

Runtime, `wasm-bindgen --target nodejs`, release build (4.5 MB unoptimised
`.wasm`):

```text
graph_roundtrip      => rows=2                  # CREATE x2, MATCH
snapshot_roundtrip   => snapshot_bytes=40 rows_after_import=1
uuid_v4              => 2a3aa3b0-...            # works with uuid/js
sha256_hex           => c7294d15...             # works
systemtime           => FAIL RuntimeError: unreachable
siafu_execute        => FAIL RuntimeError: unreachable   # SiafuDB::execute
```

**`SiafuDB::execute` panics on wasm** because `changelog.rs:57` calls
`std::time::SystemTime::now()`, which is unimplemented on
`wasm32-unknown-unknown`. After swapping to the `web-time` crate:
`siafu_execute => ok rows=1`. The `siafudb-core` tests still pass natively with
the same edits (checked in a scratch copy).

## Browser measurements

Run in headless Chromium driven by Playwright, against the real workspace crate
`siafudb-core` built with the `browser` feature. Raw output:
[`probes/browser/results.json`](probes/browser/results.json); driver:
[`probes/browser/run.js`](probes/browser/run.js).

| Check                            | Result (from `results.json`)                                                                                                                       |
| -------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| wasm in the main thread          | Cypher `CREATE` x2 then `MATCH` returns both rows (`Amara`, `Tatenda`)                                                                             |
| wasm in a dedicated Worker       | Same query, same rows, 1 ms                                                                                                                        |
| OPFS persistence, encrypted      | Snapshot 3248 B plaintext, 3276 B encrypted (28 B overhead); the plaintext contains the test secret, the stored bytes do not                       |
| IndexedDB persistence, encrypted | Same sizes, same result                                                                                                                            |
| Survives page reload             | Both backends: restored 50 of 50 documents and the secret value after `page.reload()`; a fresh page in the same context also restores from OPFS    |
| Survives browser process restart | Persistent profile, close and relaunch: OPFS restored 50 of 50 documents, stored bytes still ciphertext                                            |
| Key handling                     | Key-encryption key created as a **non-extractable** WebCrypto key (`kekExtractable: false`); the 60-byte wrapped data key is what is stored        |
| Tamper                           | Flipping one bit in the middle of the stored blob is rejected on both backends (`OperationError`, AES-GCM tag check); altered data is never loaded |
| Change capture (CDC)             | Works at runtime on wasm: 3 `Create` events with before/after images and epochs 1 to 3 (probe `cdc`, grafeo `browser` + `cdc` + `cypher`)          |
| 10,000-node insert               | 897 ms                                                                                                                                             |
| Snapshot export                  | 38.1 ms for 468,148 B; encrypt 1.3 ms; write 7.7 ms; restore 57.5 ms (10,000 nodes recovered)                                                      |
| `navigator.storage.persist()`    | Returned **`false`** in headless Chromium, before and after the call, in both the default and the persistent profile                               |

Reading the numbers honestly:

- **Persistence is not guaranteed by the browser.** `persist()` returned
  `false`, so on this setup the origin's storage is best-effort and may be
  evicted. That is a headless-Chromium result; desktop Chrome, Safari and
  Firefox grant it by different heuristics and were not tested. The store
  must record this (`StoreCaps.evictable`, see 02) and the app must warn the
  user. Quota reported was 4 GiB (default context) and 10 GiB (persistent
  profile).
- **Key handling is stronger in the probe than in the page-level design.** The
  probe encrypts the whole snapshot with WebCrypto AES-GCM (12 B nonce plus
  16 B tag, which is the 28 B overhead), and the data key is imported
  non-extractable. A page-level store (02) needs a synchronous cipher in
  wasm, so there the data key would sit in wasm memory during a session and
  only the key-encryption key stays non-extractable.
- The benchmark is one machine and one run, a single-snapshot model (whole
  graph exported, encrypted and written), not page-level I/O.
- Only Chromium was tested. Firefox and WebKit remain to do, and OPFS sync
  access handles differ most there.

## The four changes

1. **Features.** Add a `native` feature (default on) that enables Grafeo's
   `embedded`/`wal`; a `browser` feature that enables Grafeo's `browser` only.
   Today `Cargo.toml` pins `features = ["gql","cypher","wal"]` with default
   features on, which drags in `parallel` (rayon), `mmap`, `fs2` and
   `tokio/full`.
2. **`SiafuDB::open`** gated `#[cfg(not(target_arch = "wasm32"))]` (or behind
   `native`). Wasm uses `in_memory()` + snapshot persistence (see 02).
3. **Clock.** `web-time` in place of `std::time` in `changelog.rs`. Better: an
   injected `Clock` so tests and the signed-mutation work (03) get deterministic
   time.
4. **`uuid` `js` feature** (and `getrandom/wasm_js` if the `embedded` profile is
   ever used on wasm).

## Limits to know about

- **Change tracking is not in the `browser` profile by default.** Upstream's
  grafeo-web README says `changesSince()` throws: "the WASM engine has no
  change tracking yet". We no longer depend on that: the `cdc` feature
  compiles for wasm and **works at runtime in Chromium** (3 events captured,
  see above). The engine is our fork, so enabling it in the browser profile is
  our decision, not an upstream request. This matters for 03.
- **Memory.** grafeo-web documents a 768 MiB engine ceiling and an IndexedDB
  quota of about 500 MB. Fine for personal fragments, not for a platform graph.
- **Binary size.** 4.5 MB unoptimised; grafeo-web ships a 5 MB engine and a
  "lite" GQL-only build. Run `wasm-opt -Oz` before quoting a number.
- Upstream's `grafeo-web` ships IndexedDB persistence (whole-snapshot,
  debounced). It is a useful reference, but it has no encryption hook; ours
  adds the encrypted store.

## Effort

The four changes and a wasm CI job are done
(`cargo build -p siafudb-core --no-default-features --features browser --target wasm32-unknown-unknown`
passes). The reload-survival check was run in Chromium as a probe; turning it
into a committed Playwright suite on Node 24, plus a `wasm-bindgen` wrapper,
is Phase 1 work (ADR-0002).

Reproduce: [`probes/wasm`](probes/wasm/).
