#!/usr/bin/env bash
# Verify that engine/ is byte-identical to the crates.io 0.6.2 packages, except
# for files listed in engine/CHANGES.md (ADR-0001, rule 4).
#
# 1. Download each crate from crates.io and check the .crate sha256 against the
#    value recorded in engine/UPSTREAM.md.
# 2. Extract it and diff the tree against engine/<crate>.
# 3. Any difference must be a file named (in backticks, as `engine/<path>`) in
#    engine/CHANGES.md. Anything else fails the check.
#
# Registry-only artefacts are ignored: .cargo-ok (written by cargo when it
# unpacks a crate), .cargo_vcs_info.json and Cargo.lock at a crate root.
#
# Usage: scripts/ci/check-engine-pin.sh   (run from anywhere; needs curl, tar,
# sha256sum, diff). Set ENGINE_PIN_CACHE to reuse downloads between runs.
set -euo pipefail

VERSION="0.6.2"
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
upstream_md="$root/engine/UPSTREAM.md"
changes_md="$root/engine/CHANGES.md"
crates=(grafeo grafeo-adapters grafeo-common grafeo-core grafeo-engine grafeo-storage)

work="${ENGINE_PIN_CACHE:-$(mktemp -d)}"
mkdir -p "$work"
[ -n "${ENGINE_PIN_CACHE:-}" ] || trap 'rm -rf "$work"' EXIT

# Files allowed to differ, as repo-relative paths (engine/<crate>/<path>).
allowed="$work/allowed.txt"
grep -oE '`engine/[^`]+`' "$changes_md" | tr -d '`' | sort -u >"$allowed" || true

fail=0
for crate in "${crates[@]}"; do
  expected="$(grep -E "^\| ${crate} \|" "$upstream_md" | grep -oE '[0-9a-f]{64}' || true)"
  if [ -z "$expected" ]; then
    echo "FAIL ${crate}: no sha256 recorded in engine/UPSTREAM.md"
    fail=1
    continue
  fi
  pkg="$work/${crate}-${VERSION}.crate"
  if [ ! -f "$pkg" ]; then
    curl -fsSL --retry 3 -o "$pkg" \
      "https://crates.io/api/v1/crates/${crate}/${VERSION}/download"
  fi
  actual="$(sha256sum "$pkg" | cut -d' ' -f1)"
  if [ "$actual" != "$expected" ]; then
    echo "FAIL ${crate}: downloaded .crate sha256 ${actual} != recorded ${expected}"
    fail=1
    continue
  fi
  dest="$work/src-${crate}"
  rm -rf "$dest"
  mkdir -p "$dest"
  tar -xzf "$pkg" -C "$dest"
  up="$dest/${crate}-${VERSION}"
  ours="$root/engine/${crate}"

  diffs="$(diff -rq "$up" "$ours" || true)"
  bad=0
  while IFS= read -r line; do
    [ -n "$line" ] || continue
    case "$line" in
      "Files "*" differ")
        path="${line#Files "$up"/}"
        path="${path%% and *}" ;;
      "Only in $up"*)
        dir="${line#Only in "$up"}"
        dir="${dir%%: *}"; dir="${dir#/}"
        name="${line##*: }"
        path="${dir:+$dir/}$name"
        case "$path" in .cargo_vcs_info.json|Cargo.lock) continue ;; esac ;;
      "Only in $ours"*)
        dir="${line#Only in "$ours"}"
        dir="${dir%%: *}"; dir="${dir#/}"
        name="${line##*: }"
        path="${dir:+$dir/}$name"
        [ "$path" = ".cargo-ok" ] && continue ;;
      *) path="$line" ;;
    esac
    if ! grep -qxF "engine/${crate}/${path}" "$allowed"; then
      echo "FAIL ${crate}: unrecorded difference: ${line}"
      bad=1
    fi
  done <<<"$diffs"
  if [ "$bad" -eq 0 ]; then
    echo "ok   ${crate} ${VERSION} (sha256 ${actual:0:12}..., tree matches)"
  else
    fail=1
  fi
done

# A crate directory under engine/ that is not in the pin list is also drift.
for d in "$root"/engine/*/; do
  n="$(basename "$d")"
  case " ${crates[*]} " in
    *" $n "*) ;;
    *) echo "FAIL engine/${n}: not a pinned crate"; fail=1 ;;
  esac
done

if [ "$fail" -ne 0 ]; then
  echo "engine pin check FAILED: record intentional changes in engine/CHANGES.md as \`engine/<crate>/<path>\`"
  exit 1
fi
echo "engine pin check passed: engine/ matches crates.io ${VERSION} (plus changes listed in engine/CHANGES.md)"
