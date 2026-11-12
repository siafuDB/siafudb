# 1. Does GrafeoDB compile to `wasm32-unknown-unknown`?

**Yes. It also runs.** Under Node 22, GrafeoDB's `browser` feature profile
executes Cypher, and snapshot export/import round-trips. SiafuDB needs four
small changes to get there. Not yet tested in a real browser.

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

- **No change tracking in the WASM engine upstream.** The grafeo-web README
  states `changesSince()` throws: "the WASM engine has no change tracking yet".
  Grafeo's `cdc` feature is outside the `browser` profile. I did not test
  whether `cdc` compiles for wasm. This matters for 03.
- **Memory.** grafeo-web documents a 768 MiB engine ceiling and an IndexedDB
  quota of about 500 MB. Fine for personal fragments, not for a platform graph.
- **Binary size.** 4.5 MB unoptimised; grafeo-web ships a 5 MB engine and a
  "lite" GQL-only build. Run `wasm-opt -Oz` before quoting a number.
- Grafeo's own `grafeo-web` already ships IndexedDB persistence (whole-snapshot,
  debounced). It is a useful reference, but it has no encryption hook.

## Effort

2-3 days: the four changes, a wasm CI job, a `wasm-bindgen` wrapper, and a
Playwright test in Chromium that reloads the page and checks the data survived.
The Playwright test is the unfinished part of this spike.

Reproduce: [`probes/wasm`](probes/wasm/).
