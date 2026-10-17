# Local AI: Shorten and Generate decoy

Owner: role 5. Code: `crates/poly-ai`. Prompts and word lists:
`profiles/ai-prompts.json`. Eval samples: `tests/ai-samples.json`.

Two optional helpers, both running on a language model **on the user's own
Mac**:

- **Shorten** rewrites the message shorter. Every byte saved makes the song
  0.2 s shorter (music carrier ≈ 40 bits/s) and helps a message fit a small image.
- **Generate decoy** writes a harmless everyday message for slot A in hidden
  mode, of a length that pads to the same slot size as the real message.

If no local AI server runs, the buttons are hidden and nothing else changes.

## How it works

1. **Discovery.** `detect_servers()` asks the two usual local servers for
   their models with `GET /v1/models`, 500 ms timeout each:
   LM Studio at `http://127.0.0.1:1234/v1` and Ollama at
   `http://127.0.0.1:11434/v1` (their default ports; both speak the
   OpenAI-compatible API). Servers that don't answer are left out; an empty
   list means "hide the AI buttons". A custom URL can be checked with
   `AiClient::probe`, under the same localhost rules.
2. **Propose → verify → approve.** The model only *proposes* a text
   (`POST /v1/chat/completions`, not streamed). Deterministic code then
   *verifies* it (checks below). If it fails, poly-ai tries again, telling the
   model exactly what was wrong. A suggestion that passes is shown to the
   user, who *approves* it, edits it, or cancels. Nothing is ever replaced or
   sent automatically.
3. **The decoy never sees the secret.** `generate_decoy` receives only the
   real message's **byte length** and **script** (Latin, Georgian, Cyrillic),
   which the app gets from `detect_script(text)`, a pure local function. The
   real text is never part of any decoy request.
4. **Padding match.** In hidden mode both slots pad to the longer text
   (`docs/SPEC.md` §Slots: `2 + length`, rounded up to 16 bytes). A decoy
   "fits" when it pads to the same size as the real message, so the decoy
   doesn't change the file size or the song length. poly-ai asks poly-core's
   own `padded_len` for this; it never re-implements the rule. A decoy that
   passes every other check but misses the size comes back with
   `fits = false` and the allowed byte range, so the user can edit it to fit.
5. **Temperature is a request setting, not a guarantee.** Each request sends
   a `temperature` (shorten 0.1, retries 0.3; decoy 0.8, retries 0.9). Some
   servers ignore it. None of the checks depend on it being honored.

### The retry loop

| | Shorten | Decoy |
|---|---|---|
| attempts | 3 | 3 |
| temperature (first / retries) | 0.1 / 0.3 | 0.8 / 0.9 |
| per request timeout | 30 s | 30 s |
| overall deadline per call | 45 s | 45 s |

All numbers live in `profiles/ai-prompts.json`, each with its reason.

- A failed attempt's problems go into the next prompt, for example: *"Your
  previous attempt was rejected: it dropped or changed "18:30"; keep them
  exactly; it changed the negations (the original has "not"); keep every
  negation."* Retries use the slightly higher temperature so the model doesn't
  repeat the same answer.
- **Errors.** Connection refused (no server) → `NoLocalServer` at once, no
  retry. An HTTP error status (for example an unknown model) → `Server` at
  once. A timeout, an empty answer or an unparsable response counts as a failed
  attempt and is retried.
- **Deadline.** 45 s per call across all attempts (past that, typing by hand
  is faster). Each request's timeout is cut to the time left.
- **Never a silently bad result.** After the last attempt: `Rejected { reasons }`
  with the failed check names (`numbers`, `negations`, `timeout`, `deadline`,
  ...). For decoys, the closest candidate with `fits = false` if one passed
  every check except the length.

### Cleanup and checks

Every answer is cleaned first: `<think>…</think>` blocks (reasoning models)
are removed, whitespace trimmed, and one pair of wrapping quotes dropped.

A **shortened** text is accepted only if all of these pass:

| check | rule |
|---|---|
| `empty` | not empty |
| `preamble` | doesn't start with "Here is", "Sure", "Shorter version", "As an AI"… or end with an offer ("Let me know if…"), unless the original uses that phrase |
| `markdown` | no `-`/`*` list lines, `#` headings, `**` or backticks (unless the original has them) |
| `prompt_echo` | doesn't repeat distinctive phrases of our prompts |
| `not_shorter` | strictly fewer UTF-8 bytes than the original (bytes are what carriers carry) |
| `too_short` | at least a third of the original's bytes (smaller almost certainly dropped content) |
| `numbers` | every number token (3.5, 18:30, 17/10, 2026-10-17) appears unchanged |
| `links` | every URL and email address appears unchanged |
| `names` | every capitalized Latin word of 2+ letters that doesn't start a sentence appears (skipped for Georgian text) |
| `negations` | same number of negation words (English, Georgian, Russian lists, whole words, any case) |
| `script` | same dominant script (Latin / Georgian / Cyrillic) |

A **decoy** is accepted only if it passes `empty`, `preamble`, `markdown`,
`prompt_echo`, has the requested `script`, contains no `banned_word` (secret,
decoy, hidden, password, encrypted, Polyphonia, and Georgian stems for the
same), and passes the `padding` match.

## Hard questions

**How do you guarantee nothing leaves the device?**
- Only plain `http://` URLs whose host is a literal loopback address
  (127.0.0.0/8 or `::1`) or exactly `localhost` are accepted. Everything else
  is rejected before any connection: `192.168.1.5`, `example.com`,
  `localhost.evil.com`, `http://127.0.0.1@evil.com`, any `https://`.
- `localhost` is mapped to `127.0.0.1` by poly-ai itself. No DNS lookup
  happens, so a tampered resolver or hosts file can't redirect the request.
- Proxy settings and the `HTTP_PROXY` / `HTTPS_PROXY` / `ALL_PROXY`
  variables are ignored (tested: a "proxy" set in the environment is never
  used).
- Redirects are not followed; a 3xx answer is an error (tested).
- The URL is checked again right before every request.
- poly-ai doesn't log, print or store message text, prompts or model output.
  Errors carry only check names and status codes, never text. The only
  exception is the `poly-ai-eval` tool's output, which prints the sample
  texts (made up) with `--show`, or the text you give to `try`.

**What about hallucinations?** The model can't change the message on its
own. Every suggestion must pass the checks above (numbers, links, names,
negations, script, length), and then the user sees original and suggestion
side by side and decides. Nothing is replaced or sent automatically.

**Prompt injection?** The message goes to the model between unique markers,
with the instruction that everything inside is text to rewrite, never
instructions. Marker strings inside the message are removed first so it can't
close the block early. If the model obeys an injected "reply OK" anyway, the
answer fails `too_short` and the fact checks, so it's rejected. The decoy
prompt contains no user text at all, except an optional short tone (cut to
40 characters, one line). `tests/ai-samples.json` has injection samples in
English and Georgian.

**Why local?** Sending the plaintext to a cloud API would defeat the whole
product: the message would leave the device unencrypted. A local model keeps
it on the Mac.

**What if no AI is installed?** `detect_servers()` returns an empty list
within about a second and the app hides the AI buttons. Everything else works
as before.

**Honest limits.**
- LM Studio and Ollama may keep their own logs. Ollama writes a server log
  (`~/.ollama/logs/server.log` on macOS); with `OLLAMA_DEBUG=1` it logs more
  detail, so leave that unset. LM Studio shows requests in its Developer tab's
  server log and can write log files; turn off verbose request logging in its
  server settings. Check both in your installed version; we can't control
  another program's logging.
- The `names` check is skipped for Georgian (no capital letters to find
  names by), so a dropped Georgian name is caught only by the user.
- The checks catch dropped or flipped facts (numbers, links, names,
  negations), not every subtle change in meaning or tone. That's why the
  user always approves.
- Negations are counted, not located: moving a "not" to another verb keeps
  the count and passes.
- Byte targets are hard for models; decoys often need an edit
  (`fits = false` shows the range).

## Setup

**LM Studio**
1. Install LM Studio and download a small instruct model (for example a 3–8B
   model).
2. Start the local server: Developer tab → toggle the server on (port 1234),
   or in a terminal `lms server start`.
3. Load the model (in the app, or `lms load <model>`).

**Ollama**
1. Install Ollama; the app runs the server on port 11434 (or run `ollama serve`).
2. `ollama pull <model>`, e.g. `ollama pull llama3.2:3b`.
3. The model id for poly-ai is the name shown by `ollama list`.

**Choose a model with evidence**

```sh
cargo run -p poly-ai --bin poly-ai-eval -- list
cargo run -p poly-ai --bin poly-ai-eval -- run --model <model id from list>
cargo run -p poly-ai --bin poly-ai-eval -- run --model <id> --show   # also print suggestions
cargo run -p poly-ai --bin poly-ai-eval -- try --model <id> "text to shorten"
```

`run` prints pass/fail per sample with the failed check names, the average
bytes saved, the decoy fit rate and the average latency.

## For the Mac app (role 4)

poly-ai has no UniFFI code; role 4 owns `poly-ffi`. Proposed functions to add
there (UniFFI has no `usize` or tuples, so lengths are `u64` and the range is
two fields):

```rust
#[derive(uniffi::Enum)]
pub enum AiServerKind { LmStudio, Ollama, Custom }

#[derive(uniffi::Record)]
pub struct AiServer { pub kind: AiServerKind, pub base_url: String, pub models: Vec<String> }

#[derive(uniffi::Enum)]
pub enum AiScript { Latin, Georgian, Cyrillic, Unknown }

#[derive(uniffi::Record)]
pub struct AiShortenSuggestion {
    pub text: String, pub original_bytes: u64, pub new_bytes: u64, pub saved_percent: f64,
}

#[derive(uniffi::Record)]
pub struct AiDecoySuggestion {
    pub text: String, pub fits: bool, pub min_bytes: u64, pub max_bytes: u64,
}

#[derive(uniffi::Error)]
pub enum AiFfiError {
    NoLocalServer, NotLocal { message: String }, Timeout, BadResponse,
    Server { message: String }, InvalidRequest { message: String },
    Rejected { reasons: Vec<String> },
}

#[uniffi::export] fn ai_detect_servers() -> Vec<AiServer>;                       // poly_ai::detect_servers
#[uniffi::export] fn ai_shorten(server: AiServer, model: String, text: String)
    -> Result<AiShortenSuggestion, AiFfiError>;                                   // poly_ai::shorten
#[uniffi::export] fn ai_generate_decoy(server: AiServer, model: String,
    target_text_len_bytes: u64, script: AiScript, tone: Option<String>)
    -> Result<AiDecoySuggestion, AiFfiError>;                                     // poly_ai::generate_decoy
#[uniffi::export] fn ai_detect_script(text: String) -> AiScript;                  // poly_ai::detect_script
```

UI rules:

- Hide the AI buttons when `ai_detect_servers()` is empty. Run discovery at
  launch and when the encrypt screen opens, not on every keystroke.
- **Call every AI function off the main thread** (they block: up to 500 ms for
  discovery, up to 45 s for shorten/decoy). Show a spinner and a Cancel.
- Show original and suggestion side by side with **Accept / Edit / Cancel**.
  Never auto-replace and never auto-send.
- Show the bytes saved (`original_bytes → new_bytes`, `saved_percent`).
- When encrypting fails with `CarrierTooSmall`, offer **Shorten** right there.
- Decoy: pass `target_text_len_bytes = real message's UTF-8 byte length` and
  `script = ai_detect_script(real message)`; never pass the message itself.
  If `fits` is false, show the allowed range (`min_bytes`–`max_bytes`) and a
  live byte counter while the user edits.
- On `Rejected`, say the suggestion didn't keep the facts and offer to try
  again or edit by hand; don't show the rejected text.
