# Changes to the vendored engine

Apache-2.0 section 4(b): files we change must carry notice that they were
changed. Record every modification to `engine/` here, newest first.

Baseline: GrafeoDB 0.6.2 unmodified (2026-11-12).

## 2026-11-13: renamed for publication (ADR-0001 rule 5)

Package names changed so the fork can be published without implying upstream
endorsement. Rust source files are unchanged; library names (`grafeo_core`,
...) are kept so no `use` path changed. Changed files per crate: the manifest
(package name, version 0.1.0, sibling dependencies now `package`/`path`/
`version`, repository/homepage/authors/description), the removed upstream
`Cargo.toml.orig`, and added `LICENSE` and `NOTICE`.

| Upstream | Published as |
| --- | --- |
| grafeo | siafudb-graph |
| grafeo-common | siafudb-graph-common |
| grafeo-core | siafudb-graph-core |
| grafeo-adapters | siafudb-graph-adapters |
| grafeo-storage | siafudb-graph-storage |
| grafeo-engine | siafudb-graph-engine |

Recorded paths (read by `scripts/ci/check-engine-pin.sh`):

- `engine/grafeo/Cargo.toml`
- `engine/grafeo/Cargo.toml.orig`
- `engine/grafeo/LICENSE`
- `engine/grafeo/NOTICE`
- `engine/grafeo-common/Cargo.toml`
- `engine/grafeo-common/Cargo.toml.orig`
- `engine/grafeo-common/LICENSE`
- `engine/grafeo-common/NOTICE`
- `engine/grafeo-core/Cargo.toml`
- `engine/grafeo-core/Cargo.toml.orig`
- `engine/grafeo-core/LICENSE`
- `engine/grafeo-core/NOTICE`
- `engine/grafeo-adapters/Cargo.toml`
- `engine/grafeo-adapters/Cargo.toml.orig`
- `engine/grafeo-adapters/LICENSE`
- `engine/grafeo-adapters/NOTICE`
- `engine/grafeo-storage/Cargo.toml`
- `engine/grafeo-storage/Cargo.toml.orig`
- `engine/grafeo-storage/LICENSE`
- `engine/grafeo-storage/NOTICE`
- `engine/grafeo-engine/Cargo.toml`
- `engine/grafeo-engine/Cargo.toml.orig`
- `engine/grafeo-engine/LICENSE`
- `engine/grafeo-engine/NOTICE`

