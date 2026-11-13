#!/usr/bin/env bash
# Publish the SiafuDB crates to crates.io in dependency order.
#
# Safe to re-run: a crate whose current version is already on crates.io is
# skipped, so a run that failed half way can simply be started again.
#
# Usage: scripts/ci/publish-crates.sh [--dry-run]
#   --dry-run  package and build-verify the engine crates, list what
#              siafudb-core would contain, publish nothing. Needs no token.
# Publishing needs CARGO_REGISTRY_TOKEN in the environment.
set -euo pipefail

dry_run=false
[[ "${1:-}" == "--dry-run" ]] && dry_run=true

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

# <workspace dir relative to the repo root>:<package>, in dependency order.
crates=(
  engine:siafudb-graph-common
  engine:siafudb-graph-core
  engine:siafudb-graph-adapters
  engine:siafudb-graph-storage
  engine:siafudb-graph-engine
  engine:siafudb-graph
  .:siafudb-core
)

version_of() { # <dir> <package>
  (cd "$root/$1" && cargo metadata --no-deps --format-version 1) |
    python3 -c 'import json,sys
name=sys.argv[1]
print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"]==name))' "$2"
}

is_published() { # <package> <version>
  local code
  code="$(curl -sS -o /dev/null -w '%{http_code}' \
    -A 'siafudb-publish-check' "https://crates.io/api/v1/crates/$1/$2")"
  case "$code" in
    200) return 0 ;;
    404) return 1 ;;
    *) echo "crates.io answered HTTP $code for $1 $2" >&2; exit 1 ;;
  esac
}

if $dry_run; then
  echo "Dry run: packaging and verifying the engine workspace"
  (cd "$root/engine" && cargo package --workspace)
  echo "Dry run: files siafudb-core would publish"
  (cd "$root" && cargo package -p siafudb-core --list)
  exit 0
fi

: "${CARGO_REGISTRY_TOKEN:?CARGO_REGISTRY_TOKEN is not set}"

for entry in "${crates[@]}"; do
  dir="${entry%%:*}"
  name="${entry##*:}"
  version="$(version_of "$dir" "$name")"
  if is_published "$name" "$version"; then
    echo "skip   $name $version (already on crates.io)"
    continue
  fi
  echo "publish $name $version"
  # cargo waits until the new version is visible in the index before
  # returning, so the next crate can depend on it.
  #
  # crates.io limits how fast NEW crate names can be published (a burst of
  # five, then about one every ten minutes) and answers 429 beyond that.
  # Wait and try again instead of failing the release.
  attempt=1
  until out="$(cd "$root/$dir" && cargo publish -p "$name" 2>&1)"; do
    echo "$out"
    if [[ "$out" != *"429 Too Many Requests"* ]] || ((attempt >= 8)); then
      exit 1
    fi
    echo "rate limited by crates.io; waiting 3 minutes (attempt $attempt of 8)"
    sleep 180
    attempt=$((attempt + 1))
  done
  echo "$out"
done
