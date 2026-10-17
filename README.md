# Polyphonia

Send end-to-end encrypted messages through **every messaging platform**. Powered
by post-quantum ML-KEM-768 key exchange and end-to-end encryption.

## Build

You need macOS 14 or later, full Xcode, and Rust stable with `rustup` and
`cargo` installed.

From the repository folder, build the Rust library and Swift bindings, then open
the Xcode project:

```sh
scripts/build-xcframework.sh
open app/polyphonia/polyphonia.xcodeproj
```

In Xcode, choose the **polyphonia** scheme and click **Run**. The first build
can take a few minutes while Rust dependencies are compiled.

## Use

1. The receiver opens Polyphonia, chooses **Export my public key → ML-KEM-768**
   from the contact menu, and sends the exported `.pub` file to the sender.
2. The sender chooses **Import public key…**, selects that `.pub` file, and
   selects the new contact.
3. The sender selects a message, presses **Command + Shift + P**, chooses
   **Seal → Image** or **Seal → Audio**, and sends the created file to the
   receiver as a file or document.
4. The receiver copies the file and presses **Command + Shift + P**, then
   chooses **Open → Paste file**. They can also choose **Browse** to select it.

To reply, exchange public keys in the other direction: the sender exports their
`.pub` file, and the other person imports it before sealing a reply.

For a hidden message, the sender enables **Use a decoy**, enters harmless decoy
text and a passphrase, then seals the message. The receiver sees the decoy if
they leave the passphrase empty; entering the agreed passphrase reveals the real
message. Agree on that passphrase privately.

The app can also use a custom PNG/JPEG image cover in Stealth image mode or a
16-bit PCM WAV cover in WAV audio mode. Choose the cover in the controls before
sealing. Send the resulting image or audio as an original file so the messenger
does not recompress it.
