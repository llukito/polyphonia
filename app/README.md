# Polyphonia macOS app

Select text in another app and press **Command + Shift + P**. A glass ring
opens around the cursor with **Seal** on the left and **Open** on the right.
The ring uses native glass, spring motion, and a quieter animation when macOS
Reduce Motion is enabled.
The center hub features Shalva, the dancing crab in Georgian dress; his GIF pauses
when Reduce Motion is enabled.

- **Seal** expands into a second layer on the left: **Image** above and **Audio**
  below. Move through Seal into either child. Choose the recipient and optional
  hidden mode in the small controls below the ring.
- Press in the hub, drag toward an action, and release to choose it. Continue
  dragging through Seal to Image or Audio in the same gesture. A release in the
  hub or a divider gap cancels. Clicking the visible choices also works.
- If the ring appears while you are still holding **Command + Shift + P**, move
  toward a choice and release the shortcut to choose it. Some editors need the
  shortcut released before selection capture can finish; use a mouse drag once
  the ring appears in those apps.
- **Open** offers **Paste file** from the clipboard or **Browse**. Enter an
  optional passphrase before choosing a file; files can also be dropped onto
  the ring.
- **Escape**, **Command + W**, an outside click, or the shortcut again closes
  the ring. **Command + Q** quits while the ring is active. The small close
  button also has a Quit action in its context menu.

macOS asks for Accessibility permission when the app first reads selected text.
Enable **Polyphonia** in **System Settings → Privacy & Security → Accessibility**,
then quit and run the app again. This demo runs with App Sandbox disabled because
it reads selections in other apps.

Selection capture first uses Accessibility. For editors that do not expose their
selection, it sends **Copy** to the original app and restores the previous clipboard
contents before showing the ring. Press and release **Command + Shift + P**; keep
the source app active until the ring appears. Password fields and apps that block
both selection access and Copy cannot be captured. A failed capture shows the reason
under the ring instead of silently treating it as an empty selection.

The app uses **ML-KEM-768** by default, with RSA-2048 available for comparison.
The contact’s public key selects the algorithm automatically. Image creates
**Stealth PNG** or **Robust PNG**; Audio creates **Music WAV** or **Music MIDI**.
Rust encrypts the draft and embeds it in the carrier, then Swift copies the actual
file URL to the clipboard. Private keys stay in the local macOS Keychain.

Send WAV/MIDI as original files/documents, not voice messages. MIDI playback needs
a compatible player. WAV files can be large and take a moment to generate; Rust
work and temporary file creation run off the UI thread. The result shows the
carrier format and file size. Carrier capacity includes the envelope and both
message slots; if it is exceeded, shorten the message or choose Stealth PNG.

Open auto-detects the carrier and encryption algorithm. Only the recipient’s
private key can open a real encrypted file. With a decoy, an empty or incorrect
passphrase returns the decoy; the agreed passphrase returns the real message.
Both results have the same interface. A normal message needs the recipient’s
private key, without a separate passphrase.

Existing Fake identities, contacts, and files remain readable, but new sealing
to Fake contacts is blocked. Ask those contacts to export a new ML-KEM public key;
old contacts are never silently relabeled as secure.

Full messages start a fresh one-way session. After the recipient opens that file,
enable **Compact** below the contact selector to omit the large public-key block
from later files. This helps small carriers such as Robust PNG and MIDI. A reply starts its
own session in the other direction. **Start a new session** turns compact mode
off; send that next full message before sending more compact files. Sending
sessions expire after 7 days; received session keys are retained for 30 days. Session keys are
stored only in Keychain and survive app restarts. A recipient who missed the full
message sees instructions to open it or request a new full message.

Sessions do not authenticate the sender and do not provide forward secrecy.
RSA is a comparison option: its Rust implementation documents a known timing
side channel; prefer ML-KEM. See `crates/poly-crypto/src/rsa.rs`.

For your own image, select **Stealth**, then **Choose…** in the image-cover row.
Pick a PNG or JPEG, then use **Seal → Image**. Rust embeds the message in a new
PNG using that image; the original file is never overwritten. **Replace…** changes
the cover and the **×** restores the generated image. The choice lasts for the
current ring session. If the image is too small, choose a larger one or remove it.
Robust always generates its own grid, so the cover picker is disabled in that mode.

For your own music, select **WAV**, then **Choose…** in the audio-cover row.
Pick a 16-bit PCM WAV, then use **Seal → Audio**. Rust embeds the message in a
new WAV while preserving the cover's length, sample rate, and channel count;
the original is never overwritten. **Replace…** changes the cover and **×**
restores generated music. The choice lasts for the current ring session. MIDI
always generates its own music, so the cover picker is disabled in that format.
If the WAV is too short, choose a longer one or shorten the message. Other WAV
encodings must be converted to 16-bit PCM before using them as a cover.

## Destination platform and image mode

The glass selector below the ring suggests the app where the text was selected.
Recognized native messengers are matched by app identity; browser pages are
matched only when their focused web area's URL is available through Accessibility.
Otherwise the selector shows **Other**. You can always select a platform manually.
The destination and mode reset on each invocation and are not saved.

The app bundles the canonical `profiles/platforms.json` from the repository:

- Choosing a platform preselects its recommended **Stealth** or **Robust** mode.
  Both modes remain available. A profile whose transfer test failed prefers Robust.
- **Other** starts without a mode. Choose one explicitly; the profile's explanations
  appear alongside the choices. Audio does not use these image profiles.
- **Image sending steps** opens the instructions for the current platform and mode,
  with the profile's test status, notes and sources. After a successful image copy,
  the same instructions open automatically for the file just created.
- A received file with `ChecksumMismatch` shows the existing compression error
  and opens that platform's Stealth steps for resending as a file/document.

The controls sit close to the main ring and move down while the Image/Audio layer
is open. A solid background marks the whole scrollable panel. Its padding, headings,
and gaps between controls scroll along with the inputs when the content exceeds
the space below the ring.
App detection does not use browser history, inspect other
tabs, or request Automation permission. No selected text, keys, passphrases or URLs
are logged.

## Local AI and editable drafts

The original two-layer ring uses scrollable controls below it. **Message to seal**
shows the selected text and lets you edit it before choosing **Seal → Image** or
**Seal → Audio**. Those actions encode the current field, including accepted AI edits.

When LM Studio or Ollama has a model available, **Local AI** appears in the controls:

1. Choose the local server and model, then **Shorten message**.
2. Compare the inline suggestion with the current message and edit it if needed.
3. Choose **Use as message**. The suggestion replaces **Message to seal** and the
   controls return to that field. **Discard** keeps the current message.
4. Choose **Seal → Image** or **Seal → Audio** to encode the accepted text.

Choose **Decoy** next to the recipient to jump to **Decoy message**, then enable
**Use a decoy**. Write harmless text yourself or choose **Generate decoy with local AI**.
Review an AI suggestion and its permitted byte range, then choose **Use as decoy**.
Add a passphrase agreed with the recipient before sealing. Without the correct
passphrase, the recipient sees the decoy. With it, they see **Message to seal**.
Opening a normal file does not require a passphrase.

Sealing waits until you use or discard a pending suggestion. **Cancel** ignores a
pending model response. Leaving Seal or closing the ring also cancels it. Suggestions
for an outdated draft are discarded. No suggestion is applied or sent automatically.

See [the local AI guide](../docs/ai.md) for LM Studio and Ollama setup.

## Build

From the repository root on macOS with full Xcode:

```sh
scripts/build-xcframework.sh --debug
open app/polyphonia/polyphonia.xcodeproj
```

The Xcode project reads `build/PolyphoniaCore.xcframework` and
`build/swift/PolyphoniaCore.swift` generated by the script. After this initial
setup, building/running in Xcode refreshes Rust automatically through the
**Rust bridge** target. Cargo checks for source changes on each build; packaging
and binding generation are skipped when the compiled library is unchanged.
Debug builds use the Rust debug profile; Release uses release. Rust must be
installed, with Cargo available in `CARGO_HOME/bin` or `~/.cargo/bin`.

Run `bash scripts/test-macos-ai-flow.sh` to check AI review, apply, discard, cancel,
and stale-result handling using deterministic model responses and real PNG/WAV
round trips. The checks use isolated contacts and pasteboards. Add `--ui` to also
open a temporary preview, test wheel scrolling over panel gaps and padding, use the
Decoy controls and inline review buttons, and save layout captures
to `/private/tmp/polyphonia-restored-*.png`.

## Try the carriers

1. Run the **polyphonia** scheme in Xcode. Use **This Mac · ML-KEM-768** to
   receive your own file.
2. Select a short message in another app, then press **Command + Shift + P**.
3. Choose an image mode below the platform selector, then Seal → Image. Paste
   the copied PNG into a chat, or use Save to keep a local copy.
4. Invoke the ring again, choose Open → Paste file (or Browse the saved PNG).
   The selected message should appear. Repeat for both image modes.
5. Enable **Compact** after opening the full PNG, choose WAV below the ring,
   then Seal → Audio. Repeat Open with that file.
   Choose a 16-bit PCM cover WAV and repeat to hear your own music in the new
   file. Then repeat with MIDI; its file extension is `.mid`.
6. For hidden mode, set a decoy and passphrase before sealing. Open without the
   passphrase to see the decoy, then open with the agreed passphrase to see the
   real message. The interface does not label either slot.

## Sharing public keys

Use **Export my public key → ML-KEM-768** in the contact menu and share the `.pub`
file. The other person imports it, selects your contact, and seals a message for
you. Exchange keys in both directions to reply. For RSA comparison, export the
RSA option; this also adds your RSA identity as a contact for local testing.
Imported keys are validated against their declared algorithm before saving.

The integration checks run both real algorithms through PNG and WAV, check
passphrase/decoy and wrong-recipient behavior, round-trip public-key files, test
compact sessions before and after opening a full message, session expiry and
recovery, all four media carriers, custom WAVs, and reloaded identities and
sessions using an isolated in-memory vault. They also confirm legacy contact
migration and Fake read compatibility without touching the user’s Keychain.
