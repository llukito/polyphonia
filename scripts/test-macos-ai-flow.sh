#!/usr/bin/env bash
# Exercise the Swift draft/review flow and real Rust carriers without touching user contacts or Keychain.
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ ! -f build/swift/PolyphoniaCore.swift || ! -f build/universal/libpoly_ffi.a ]]; then
  echo 'Build the Rust bridge first: scripts/build-xcframework.sh --debug' >&2
  exit 1
fi
test_dir="$(mktemp -d "${TMPDIR:-/tmp}/polyphonia-ai-flow.XXXXXX")"
trap 'rm -rf "$test_dir"' EXIT
test_app="$test_dir/AiFlowChecks.app"
mkdir -p "$test_app/Contents/MacOS" "$test_app/Contents/Resources"
cp profiles/platforms.json "$test_app/Contents/Resources/platforms.json"
cat > "$test_app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>me.polyphonia.ai-flow-tests</string>
<key>CFBundleExecutable</key><string>AiFlowChecks</string>
</dict></plist>
PLIST
sources=()
for source_file in app/polyphonia/polyphonia/*.swift; do
  case "$source_file" in
    */PolyphoniaApplication.swift|*/AppDelegate.swift|*/ApplicationMenu.swift) continue ;;
  esac
  sources+=("$source_file")
done
xcrun swiftc -parse-as-library -target "$(uname -m)-apple-macos14.0" \
  -module-cache-path "$test_dir/module-cache" -I build/headers \
  build/swift/PolyphoniaCore.swift "${sources[@]}" app/tests/CryptoFlowChecks.swift app/tests/AiFlowChecks.swift \
  build/universal/libpoly_ffi.a -o "$test_app/Contents/MacOS/AiFlowChecks"
"$test_app/Contents/MacOS/AiFlowChecks" "$@"
