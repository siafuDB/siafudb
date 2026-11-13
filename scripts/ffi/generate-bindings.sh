#!/usr/bin/env bash
# Regenerate the committed Swift and Kotlin bindings from a host build of
# siafudb-ffi. Works on Linux and macOS (no Xcode or NDK needed).
#
#   scripts/ffi/generate-bindings.sh
#
# Output (committed):
#   bindings/swift/Sources/SiafuDB/siafudb_ffi.swift
#   bindings/swift/headers/siafudb_ffiFFI.h, module.modulemap
#   bindings/kotlin/src/main/kotlin/uniffi/siafudb_ffi/siafudb_ffi.kt
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
case "$(uname -s)" in
  Darwin) EXT=dylib ;;
  *) EXT=so ;;
esac
LIB="$TARGET_DIR/debug/libsiafudb_ffi.$EXT"

cargo build -p siafudb-ffi
[ -f "$LIB" ] || { echo "missing $LIB" >&2; exit 1; }
BINDGEN="$TARGET_DIR/debug/uniffi-bindgen"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

"$BINDGEN" generate --library "$LIB" --language swift --out-dir "$TMP/swift" --no-format
"$BINDGEN" generate --library "$LIB" --language kotlin --out-dir "$TMP/kotlin" --no-format

SW="$ROOT/bindings/swift"
KT="$ROOT/bindings/kotlin/src/main/kotlin/uniffi/siafudb_ffi"
mkdir -p "$SW/Sources/SiafuDB" "$SW/headers" "$KT"
cp "$TMP/swift/siafudb_ffi.swift" "$SW/Sources/SiafuDB/siafudb_ffi.swift"
cp "$TMP/swift/siafudb_ffiFFI.h" "$SW/headers/siafudb_ffiFFI.h"
# xcframework headers must expose the module as module.modulemap.
cp "$TMP/swift/siafudb_ffiFFI.modulemap" "$SW/headers/module.modulemap"
cp "$TMP/kotlin/uniffi/siafudb_ffi/siafudb_ffi.kt" "$KT/siafudb_ffi.kt"
echo "bindings regenerated under bindings/"
