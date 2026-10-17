# Platform tests

Owner: role 5. Which messengers deliver a Polyphonia file byte-for-byte, and
how to send it so they do.

## Why platform profiles

The app can't see inside a messenger. Once the user drags a file into
Telegram or WhatsApp, we don't know whether it went as a lossless *file* or as
a recompressed *photo*. So we decide up front, per platform:

- **Stealth** (StealthPng): looks like a normal image and holds the most, but
  the hidden bits live in the least significant bits of each pixel, so any
  recompression destroys them. Only works when the platform can send the file
  unchanged (a "file" / "document" / "without compression" option).
- **Robust** (RobustPng): survives recompression, so it's sent as a normal
  photo. It looks like abstract art and holds less (about 500 bytes).

`profiles/platforms.json` records, for each platform, whether it has a
lossless file mode, which mode to use by default, and the exact steps to show
the user. If the user still sends it the wrong way, nothing is lost silently:
the envelope's CRC-32 catches the damage on receive and the app says
"compressed in transit, ask them to resend as a document" (`ChecksumMismatch`,
`docs/SPEC.md` §2).

Every entry starts as `"verified": { "status": "research" }`. It becomes
`"verified"` (with the date) or `"failed"` only after a real send test below.

## What research says (unverified)

| Platform | Lossless file mode | Default | Summary |
|---|---|---|---|
| Telegram | yes | stealth | Sending as a file / "without compression" keeps the original (up to 2 GB); normal photos are compressed. |
| WhatsApp | yes | stealth | Attach → Document keeps the original (2 GB from Desktop/Web, 100 MB on Android per the help center); normal and HD photos are compressed. |
| Signal | yes | stealth | Files/documents skip compression (up to 100 MB); normal photos are recompressed and shrunk (reported ~1200×1600, not officially confirmed). |
| Discord | yes | stealth | Desktop uploads are generally kept; free limit 20 MB since August 2026; mobile may compress images. |
| Instagram | no | robust | DMs have no file/document attachment; photos are heavily compressed (HD is still compressed). |
| Email | yes | stealth | Attachments arrive unchanged; check the mail app isn't shrinking images (Apple Mail: Actual Size). |
| Other | n/a | user picks | |

Sources:

- Telegram: <https://telegram.org/faq>, <https://www.online-tech-tips.com/?p=82159>
- WhatsApp: <https://faq.whatsapp.com/453914586839706/?cms_platform=windows-desktop>, <https://faq.whatsapp.com/164676891531296/?locale=en_US>
- Signal: <https://support.signal.org/hc/en-us/articles/360007060212-Send-a-message>,
  <https://www.xatakamovil.com/aplicaciones/whatsapp-telegram-line-signal-facebook-imessage-este-su-limite-tamano-para-envio-archivos>,
  <https://mjtsai.com/blog/2021/04/27/beware-of-signal-recompressing-images-and-videos>
- Discord: <https://support.discord.com/hc/en-us/articles/25444343291031-File-Attachments-FAQ>,
  <https://gaminghq.eu/2026/08/14/discord-free-file-upload-limit-increased/>
- Instagram: <https://www.igeeksblog.com/how-to-send-hd-photos-on-instagram/>,
  <https://beebom.com/instagram-send-hd-photos-messages-rolling-out/amp/>
- Email (Apple Mail image size): <https://support.apple.com/guide/mail/send-attachments-mlhlp1050/mac>

## Running a test

All sending is done **by hand** in the real apps. The tool only prepares files
and checks what came back; it never touches a messenger or account.
`platform-tests/` is git-ignored.

### 1. Prepare the samples

```sh
cargo run -q -p poly-platform-check -- prepare platform-tests/samples
```

This writes:

| file | what | why |
|---|---|---|
| `sample.png` | 512×512 random-noise PNG (~787 KB) | noise can't be compressed losslessly, so any recompression is obvious |
| `sample.wav` | 2 s of sine tones (440–1320 Hz), 44,100 Hz mono 16-bit | like the MusicWav carrier |
| `sample.mid` | four-note C major arpeggio | like the MusicMidi carrier |
| `manifest.json` | SHA-256, size and PNG dimensions of each | used by `compare` |

The files are the same on every run (fixed seed), so hashes are comparable
across machines.

### 2. Send and save

For each platform, send the three samples to yourself or a test contact, once
per method, then save what arrives (use the app's save/download option, not a
screenshot). Record the OS and app version.

### 3. Folder layout for received files

```
platform-tests/received/
  <platform>/          telegram, whatsapp, signal, discord, instagram, email, ...
    <method>/          file (sent as file/document), photo (sent normally), hd, ...
      any-file-name.*
```

Example:

```
platform-tests/received/telegram/file/sample.png
platform-tests/received/telegram/photo/photo_2026-10-17.jpg
platform-tests/received/whatsapp/file/sample.wav
```

File names don't matter. Files are matched to samples by extension:
images (`png jpg jpeg webp gif heic`) → `sample.png`, audio
(`wav ogg opus m4a mp3 aac caf`) → `sample.wav`, `mid midi` → `sample.mid`.
Use the platform `id`s from `profiles/platforms.json` as folder names.

### 4. Compare

```sh
cargo run -q -p poly-platform-check -- compare platform-tests/samples platform-tests/received
```

For each received file it reports **identical** (same SHA-256) or
**changed**, the size change, and for images the detected format (still PNG
or not) and dimensions. It prints a Markdown table and writes it to
`platform-tests/received/results.md`. Copy the conclusions into the table
below. HEIC images can't be read by the tool, so format and dimensions show
as unknown.

### 5. Update the profile

For each platform tested, set `verified` in `profiles/platforms.json`:

```json
"verified": { "status": "verified", "date": "2026-10-18" }
```

Use `"failed"` if the lossless method didn't keep the file identical, and
fix `lossless_file_mode` / `default_mode` / `steps` to match what really
happened. `cargo test -p poly-platform-check` checks the file still follows
the rules.

## Results

| Platform | Method | Date | OS / app version | PNG | WAV | MID | Notes |
|---|---|---|---|---|---|---|---|
| Telegram | file | | | | | | |
| Telegram | photo | | | | | | |
| WhatsApp | file | | | | | | |
| WhatsApp | photo | | | | | | |
| Signal | file | | | | | | |
| Signal | photo | | | | | | |
| Discord | file | | | | | | |
| Instagram | photo | | | | | | |
| Email | file | | | | | | |

## For the Mac app

The app (role 4) reads `profiles/platforms.json` and bundles it as a resource.

```json
{
  "version": 1,
  "platforms": [
    {
      "id": "telegram",
      "name": "Telegram",
      "lossless_file_mode": true,
      "default_mode": "stealth",
      "steps": { "stealth": ["..."], "robust": ["..."] },
      "notes": "...",
      "sources": ["https://..."],
      "verified": { "status": "research", "date": null }
    }
  ],
  "other": { "default_mode": null, "explain": { "stealth": "...", "robust": "..." } }
}
```

| field | type | meaning |
|---|---|---|
| `version` | number | format version, currently 1 |
| `platforms[].id` | string | stable identifier (also the folder name in platform tests) |
| `platforms[].name` | string | display name |
| `platforms[].lossless_file_mode` | bool | the platform can send a file unchanged |
| `platforms[].default_mode` | `"stealth"` \| `"robust"` | mode to preselect |
| `platforms[].steps.stealth` / `.robust` | string[] | instructions to show, one step per line, for the Mac desktop apps |
| `platforms[].notes` | string | details for an info popover |
| `platforms[].sources` | string[] | research links |
| `platforms[].verified.status` | `"research"` \| `"verified"` \| `"failed"` | confirmed by real sends? |
| `platforms[].verified.date` | string \| null | ISO date of the test |
| `other.default_mode` | null | no default: the user chooses |
| `other.explain.stealth` / `.robust` | string | text explaining each choice |

**Rule:** `default_mode` is `"stealth"` when `lossless_file_mode` is true,
otherwise `"robust"`. "Other" has no default; the user chooses, with the
`explain` texts shown next to each option.

Suggested UI: the encrypt screen picks the contact's platform, preselects
`default_mode`, and shows that mode's `steps`. If `verified.status` is
`"research"`, a small "not yet tested" hint is fair; if `"failed"`, prefer
robust. On receive, `ChecksumMismatch` means the sender used the wrong method:
show the platform's stealth steps so they can resend.
