#!/usr/bin/env bash
# Build libsiafudb_ffi.so for four Android ABIs with cargo-ndk, drop them into
# the Gradle library module's jniLibs, and refresh the Kotlin sources.
#
# Requires: Android NDK (ANDROID_NDK_HOME), rustup, `cargo install cargo-ndk`.
# Written on a host without the NDK: UNVERIFIED.
#
#   scripts/ffi/build-android.sh
#   PROFILE=debug scripts/ffi/build-android.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

PROFILE="${PROFILE:-release}"
CARGO_FLAGS=()
[ "$PROFILE" = "release" ] && CARGO_FLAGS+=(--release)
MODULE="$ROOT/bindings/kotlin"
JNI="$MODULE/src/main/jniLibs"

command -v cargo-ndk >/dev/null || { echo "install cargo-ndk: cargo install cargo-ndk" >&2; exit 1; }
: "${ANDROID_NDK_HOME:?set ANDROID_NDK_HOME to the NDK root}"

rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android i686-linux-android

rm -rf "$JNI"
cargo ndk \
  -t arm64-v8a -t armeabi-v7a -t x86_64 -t x86 \
  --platform 24 \
  -o "$JNI" \
  build -p siafudb-ffi --lib "${CARGO_FLAGS[@]}"

# Kotlin sources come from the host build (bindgen reads metadata only).
"$ROOT/scripts/ffi/generate-bindings.sh"

find "$JNI" -name 'libsiafudb_ffi.so' -print
echo "Gradle module: $MODULE (run: gradle -p $MODULE assembleRelease)"
