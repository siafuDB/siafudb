# Engine fork baseline

SiafuDB's graph engine is a **fork of GrafeoDB (Apache-2.0), frozen at 0.6.2**.

| Item | Value |
| --- | --- |
| Upstream | https://github.com/GrafeoDB/grafeo |
| Baseline version | 0.6.2 (published to crates.io 2026-11-08) |
| Vendored | 2026-11-12, from the crates.io source packages |
| Upstream policy | **None.** We do not track, merge, or depend on upstream. No automatic bumps; dependabot is disabled for these paths. |
| Cherry-picks | Allowed only as deliberate, reviewed, individually recorded patches (log below). Nothing is ever pulled implicitly. |
| Licence | Apache-2.0, preserved in each crate. Modifications are ours and are listed in `CHANGES.md`. |
| MSRV | rustc 1.99 (inherited from the baseline) |

## Provenance (crates.io SHA-256 of the source packages)

| Crate | sha256 |
| --- | --- |
| grafeo | `efcf30ceca036380281bad050e1b66d5b673d9f05c6baca3c17bcd919626a14c` |
| grafeo-adapters | `ca11785dbcfd3f5c9c32475ab7722e3da0fdcbfbcb535dff98a245fc245f9145` |
| grafeo-common | `aa27dcea8a70c2ce83fc751c3022fb207a2b68066ce52364f60c081863026e59` |
| grafeo-core | `c6de07617c96d44d033929522946c2238a74210c07024f3ac14fab17d6263a22` |
| grafeo-engine | `0500f98699d74a6c3aa966ade00f5ca242c20ecfbcf5fa677c2ae7a5a483c72f` |
| grafeo-storage | `31d074f2ce89c265ceae49061121191fe2ffa74e5ddffcd67f2941b96e30af92` |

Verify a vendored crate against upstream with
`cargo package -p <crate> --list` and a diff against the crates.io download of
the same version; any difference must appear in `CHANGES.md`.

## Cherry-pick log

None yet.
