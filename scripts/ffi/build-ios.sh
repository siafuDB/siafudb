#!/usr/bin/env bash
# Build SiafuDBFFI.xcframework (iOS device, iOS simulator, macOS; arm64 only)
# and refresh the Swift sources of the Swift package in bindings/swift.
#
# Requires macOS with Xcode command line tools and rustup. NOT RUN in CI of
# this repo yet: it was written on a Linux host without Xcode and is unverified.
#
#   scripts/ffi/build-ios.sh            # release build
#   PROFILE=debug scripts/ffi/build-ios.sh
set -euo pipefail

[ "$(uname -s)" = "Darwin" ] || { echo "build-ios.sh must run on macOS" >&2; exit 1; }
command -v xcodebuild >/dev/null || { echo "xcodebuild not found" >&2; exit 1; }

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

PROFILE="${PROFILE:-release}"
CARGO_FLAGS=()
[ "$PROFILE" = "release" ] && CARGO_FLAGS+=(--release)
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
SWIFT_PKG="$ROOT/bindings/swift"
OUT="$SWIFT_PKG/SiafuDBFFI.xcframework"

TARGETS=(aarch64-apple-ios aarch64-apple-ios-sim aarch64-apple-darwin)
rustup target add "${TARGETS[@]}"

# Static libraries only: cargo rustc --crate-type staticlib skips the cdylib
# link, which is not wanted for the XCFramework.
for t in "${TARGETS[@]}"; do
  cargo rustc -p siafudb-ffi --lib --target "$t" "${CARGO_FLAGS[@]}" --crate-type staticlib
done

# Refresh generated Swift + headers from the host (macOS) build.
"$ROOT/scripts/ffi/generate-bindings.sh"

rm -rf "$OUT"
ARGS=()
for t in "${TARGETS[@]}"; do
  ARGS+=(-library "$TARGET_DIR/$t/$PROFILE/libsiafudb_ffi.a" -headers "$SWIFT_PKG/headers")
done
xcodebuild -create-xcframework "${ARGS[@]}" -output "$OUT"

echo "built $OUT"
echo "Swift package: $SWIFT_PKG (Package.swift)"
