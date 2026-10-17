#!/usr/bin/env bash
# Build poly-ffi into build/PolyphoniaCore.xcframework + Swift bindings (macOS only).
# Owner: role 4.
#
# Output:
#   build/PolyphoniaCore.xcframework   universal (arm64 + x86_64) static library + C headers
#   build/swift/PolyphoniaCore.swift   Swift bindings; add this file to the Xcode project
#
# Requires full Xcode (not just the Command Line Tools) for xcodebuild.
#
# Usage: scripts/build-xcframework.sh [--debug]
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "error: this script only runs on macOS" >&2
  exit 1
fi

if ! xcodebuild -version >/dev/null 2>&1; then
  cat >&2 <<'MSG'
error: xcodebuild is not available.
This script needs full Xcode, not just the Command Line Tools:
  1. Install Xcode from the App Store.
  2. sudo xcode-select -s /Applications/Xcode.app/Contents/Developer
  3. Re-run this script.
MSG
  exit 1
fi

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

PROFILE="release"
CARGO_PROFILE_FLAG="--release"
if [[ "${1:-}" == "--debug" ]]; then
  PROFILE="debug"
  CARGO_PROFILE_FLAG=""
fi

CRATE="poly-ffi"
LIB="libpoly_ffi"
MODULE="PolyphoniaCore"
FFI_MODULE="${MODULE}FFI"   # matches crates/poly-ffi/uniffi.toml
TARGETS=(aarch64-apple-darwin x86_64-apple-darwin)
BUILD="$ROOT/build"
XCFRAMEWORK="$BUILD/$MODULE.xcframework"

echo "==> Installing Rust targets (no-op if present)"
rustup target add "${TARGETS[@]}" >/dev/null

for target in "${TARGETS[@]}"; do
  echo "==> Building $CRATE for $target ($PROFILE)"
  cargo build -p "$CRATE" $CARGO_PROFILE_FLAG --target "$target"
done

rm -rf "$BUILD/universal" "$BUILD/swift" "$BUILD/headers" "$XCFRAMEWORK"
mkdir -p "$BUILD/universal" "$BUILD/swift" "$BUILD/headers"

echo "==> Combining with lipo"
lipo -create \
  "target/aarch64-apple-darwin/$PROFILE/$LIB.a" \
  "target/x86_64-apple-darwin/$PROFILE/$LIB.a" \
  -output "$BUILD/universal/$LIB.a"
lipo -info "$BUILD/universal/$LIB.a"

echo "==> Generating Swift bindings"
cargo run -q -p "$CRATE" --features bindgen --bin uniffi-bindgen -- \
  generate --library "target/aarch64-apple-darwin/$PROFILE/$LIB.dylib" \
  --language swift --out-dir "$BUILD/swift"

cp "$BUILD/swift/$FFI_MODULE.h" "$BUILD/headers/"
cp "$BUILD/swift/$FFI_MODULE.modulemap" "$BUILD/headers/module.modulemap"

echo "==> Creating $MODULE.xcframework"
xcodebuild -create-xcframework \
  -library "$BUILD/universal/$LIB.a" \
  -headers "$BUILD/headers" \
  -output "$XCFRAMEWORK"

echo "==> Done"
echo "    $XCFRAMEWORK"
echo "    $BUILD/swift/$MODULE.swift"
