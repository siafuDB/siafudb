# siafudb-core

The core of [SiafuDB](https://siafudb.org), an embedded graph database for
device, edge and web. A Bundu Foundation open project, Apache-2.0.

**Pre-release (0.x).** The API will change. Features:

- `native` (default): file-backed persistence.
- `browser`: `wasm32-unknown-unknown` profile (in-memory plus snapshot
  export/import; persist the encrypted snapshot yourself, or use the
  `@siafudb/web` package).

Queries take bound parameters (`execute_with_params`, `query_with_params`);
never format values into query text.

The graph engine is a fork of GrafeoDB 0.6.2 published as `siafudb-graph*`
(see `engine/UPSTREAM.md` in the repository). Requires rustc 1.99 or newer.
