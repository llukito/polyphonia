# Role 4: macOS app

## Goal

A SwiftUI menu bar app that calls the Rust core through UniFFI: pick a
contact, type a message, get a carrier file to send; drop a received file in
and read the message.

## Owned files

- `app/` (the Xcode project; create it there)
- `crates/poly-ffi/` (UniFFI wrapper, `uniffi.toml`)
- `scripts/build-xcframework.sh`

## First task: prove the Rust → Swift path

This is unverified so far: the machine that set up the repo had only the
Command Line Tools, not full Xcode. The Rust builds, `lipo` and Swift binding
generation ran there, and the bindings type-check with `swiftc`. The
`xcodebuild -create-xcframework` step and loading the framework into an app
have **not** been tested.

1. Install full Xcode and select it: `sudo xcode-select -s /Applications/Xcode.app/Contents/Developer`.
2. Run `scripts/build-xcframework.sh`. It produces
   `build/PolyphoniaCore.xcframework` and `build/swift/PolyphoniaCore.swift`.
3. Create the Xcode project in `app/` (macOS App, SwiftUI). Add the
   XCFramework (Frameworks, Libraries…: "Do Not Embed", it's a static library)
   and add `PolyphoniaCore.swift` to the target.
4. Call it from a button and print the result:
   ```swift
   let me = try generateIdentity(algorithm: .fake)
   let file = try encrypt(request: EncryptRequest(
       message: "hello", decoy: nil, passphrase: nil,
       recipientPublicKey: me.publicKey, algorithm: .fake,
       carrier: .rawDebug, options: CarrierOptions()))
   let out = try decrypt(request: DecryptRequest(
       file: file.bytes, mySecretKey: me.secretKey, myPublicKey: me.publicKey, passphrase: nil))
   print(out.text, out.slot)   // "hello" A
   ```
5. Fix the script if anything fails (e.g. modulemap location, link errors) and commit.

## The Swift API (generated from `crates/poly-ffi/src/lib.rs`)

```
generateIdentity(algorithm:) throws -> Identity            // { publicKey, secretKey: Data }
contactId(publicKey:) -> Data                              // 8 bytes
encrypt(request: EncryptRequest) throws -> EncodedFile     // { bytes, extension, mime, carrier, algorithm }
decrypt(request: DecryptRequest) throws -> Decrypted       // { text, slot, carrier, algorithm }
inspect(file:) throws -> Inspected                         // { carrier, algorithm, contactId, payloadLen }
enum Algorithm { mlKem768, rsa2048, fake }
enum CarrierKind { rawDebug, stealthPng, robustPng, musicWav, musicMidi }
enum Slot { a, b }
enum PolyError: Error { .ChecksumMismatch, .DecryptFailed, .WrongRecipient,
                        .CarrierTooSmall(needed:available:), .NotImplemented(what:), ... }  // UniFFI keeps Rust's capitalization for error cases
```

Only `fake` + `rawDebug` work today; the rest throw `notImplemented` until roles 1–3 land.

## Done criteria

- The app builds from a clean checkout after `scripts/build-xcframework.sh`.
- Encrypt → file → decrypt works in the app with fake + raw, including hidden mode.
- `ChecksumMismatch` shows the friendly message below.

## Test commands

```sh
cargo test -p poly-ffi
cargo clippy --workspace --all-targets -- -D warnings
scripts/build-xcframework.sh
```

## Order of work

1. **First task above** (framework loads in a real Xcode project).
2. **MVP UI:** SwiftUI `MenuBarExtra` with `.window` style popup.
   - Secret keys in the **Keychain**; never write them to disk or logs.
   - Contacts list: name + public key file (import/export `.pub`).
   - Encrypt screen: text, hidden-mode toggle with decoy + passphrase fields,
     contact, algorithm, carrier → file → drag out + copy to clipboard + a
     "Send as Document" hint (messengers recompress media).
   - Decrypt screen: drop zone + an **always-visible** passphrase field. The UI
     must look **identical** whichever slot opens; never display the slot.
   - On `ChecksumMismatch`: "This file was compressed in transit. Ask them to resend it as a document."
3. Global hotkey with the `KeyboardShortcuts` Swift package.
4. macOS Services menu item "Encrypt with Polyphonia" for selected text.
5. **Stretch:** AI buttons (with role 5), live microphone decoding (with role 3).

## Pitfalls

- The XCFramework holds a static library: link it, don't embed it.
- Regenerate the bindings whenever `poly-ffi` changes; the Swift file and the
  library must come from the same build (UniFFI checks this at startup with checksums).
- Call Rust off the main thread: Argon2id and the music carriers are slow.
- Showing which slot opened would defeat hidden mode. Use `slot` only in debug builds.
- Sandboxed app: file drag in/out needs the user-selected-files entitlement.
- Changes to the engine API go through the lead; `poly-ffi` only wraps it.
