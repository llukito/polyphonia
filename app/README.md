# Polyphonia macOS app

Placeholder. Role 4 creates the Xcode project here (SwiftUI, `MenuBarExtra`).

Start with `docs/roles/4-mac-app.md`, "First task": build the Rust library with

```sh
scripts/build-xcframework.sh   # needs full Xcode
```

then add `build/PolyphoniaCore.xcframework` and `build/swift/PolyphoniaCore.swift`
to the project.
