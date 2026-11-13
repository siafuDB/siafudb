# ADR-0001: Fork GrafeoDB 0.6.2 and stop depending on upstream

- Status: **Accepted** (owner decision, 2026-11-13)
- Supersedes: the "engine stays GrafeoDB as a Cargo dependency" line in the
  direction brief.

## Decision

SiafuDB's graph engine is a fork of GrafeoDB (Apache-2.0) **pinned at 0.6.2**,
vendored under `engine/`. From this point we do not track, merge, or depend on
upstream. Upstream is a courtesy relationship, not a dependency: we may read
it, thank it, and send patches to it, but nothing in our build, release or
roadmap waits on it.

## Why

Our web, PWA and mobile work needs engine changes upstream will not prioritise
(a page-level storage seam for OPFS/IndexedDB/encrypted pages, wasm-first
profiles, change capture that works the same on every target, cipher
agility). Owning the engine lets us ship those on our schedule.

## Rules

1. The baseline is exactly the crates.io 0.6.2 source packages; hashes are in
   `engine/UPSTREAM.md`.
2. No automatic upstream updates. Dependabot and `cargo update` must never
   move an engine crate; `[patch.crates-io]` in the root `Cargo.toml` forces
   every `grafeo-*` reference to the vendored copy.
3. Upstream fixes are adopted only as deliberate, individually reviewed
   cherry-picks, each recorded in `engine/UPSTREAM.md` (cherry-pick log) with
   the upstream commit/issue.
4. Every modification to `engine/` is recorded in `engine/CHANGES.md`
   (Apache-2.0 section 4(b)); licence headers and notices stay intact.
5. Published crate names must not imply upstream endorsement. Before the first
   crates.io release the vendored crates are renamed (`siafudb-engine-*`);
   until then they are workspace-internal and unpublished.
6. MSRV is inherited from the baseline: rustc 1.99.

## Consequences

- We own security fixes in the engine from today. Monitor RustSec advisories
  for the engine's third-party dependencies ourselves.
- The 0.6 query-semantics changes are now simply our semantics.
- Docs and README must say "fork of GrafeoDB 0.6.2" and never "built on
  Grafeo's latest".
