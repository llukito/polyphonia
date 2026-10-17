//! Deterministic cleanup and checks applied to every model answer.
//!
//! The model only proposes. These pure functions decide whether a proposal
//! is acceptable; the user then approves it in the app.

use crate::prompts::Prompts;
use poly_core::payload::padded_len;
use std::collections::HashSet;

/// Writing system of a text, by which letters it has the most of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Script {
    Latin,
    Georgian,
    Cyrillic,
    /// No letters of the scripts above (e.g. only digits or emoji).
    Unknown,
}

/// A check that failed. `name()` is what `AiError::Rejected` reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    Empty,
    Preamble,
    Markdown,
    NotShorter,
    TooShort,
    Numbers,
    Links,
    Names,
    Negations,
    Script,
    BannedWord,
    PromptEcho,
    Padding,
    /// The request timed out (not a content check, but a failed attempt).
    Timeout,
    /// The server's answer was empty or not usable (a failed attempt).
    BadResponse,
}

impl Check {
    pub const ALL: [Check; 15] = [
        Check::Empty,
        Check::Preamble,
        Check::Markdown,
        Check::NotShorter,
        Check::TooShort,
        Check::Numbers,
        Check::Links,
        Check::Names,
        Check::Negations,
        Check::Script,
        Check::BannedWord,
        Check::PromptEcho,
        Check::Padding,
        Check::Timeout,
        Check::BadResponse,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Check::Empty => "empty",
            Check::Preamble => "preamble",
            Check::Markdown => "markdown",
            Check::NotShorter => "not_shorter",
            Check::TooShort => "too_short",
            Check::Numbers => "numbers",
            Check::Links => "links",
            Check::Names => "names",
            Check::Negations => "negations",
            Check::Script => "script",
            Check::BannedWord => "banned_word",
            Check::PromptEcho => "prompt_echo",
            Check::Padding => "padding",
            Check::Timeout => "timeout",
            Check::BadResponse => "bad_response",
        }
    }
}

/// A failed check and what exactly was wrong (missing numbers, names, a byte
/// count), used to tell the model on the next attempt. `items` may contain
/// parts of the message: it only ever goes to the local model, never to logs
/// or errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub check: Check,
    pub items: Vec<String>,
}

impl Failure {
    pub fn new(check: Check) -> Self {
        Failure {
            check,
            items: Vec::new(),
        }
    }

    fn with(check: Check, items: Vec<String>) -> Self {
        Failure { check, items }
    }
}

/// Quote items for the feedback prompt: `"18:30", "Nino"`.
fn quoted(items: &[String]) -> Vec<String> {
    items.iter().map(|i| format!("\"{i}\"")).collect()
}

/// A shortened text must keep at least 1/this of the original's bytes;
/// anything smaller almost certainly dropped content.
const MIN_SHORTEN_FRACTION_DIVISOR: usize = 3;
/// Names must have at least this many letters ("I" is not a name).
const MIN_NAME_LETTERS: usize = 2;

/// Reasoning models wrap their thinking in these tags.
const THINK_OPEN: &str = "<think>";
const THINK_CLOSE: &str = "</think>";
/// Quote pairs a model may wrap its whole answer in.
const WRAPPING_QUOTES: &[(char, char)] = &[
    ('"', '"'),
    ('\'', '\''),
    ('“', '”'),
    ('„', '“'),
    ('«', '»'),
    ('`', '`'),
];
/// Separators allowed inside a number token when a digit follows
/// (3.5, 1,000, 18:30, 17/10, 2026-10-17).
const NUMBER_SEPARATORS: &[char] = &['.', ',', ':', '/', '-'];
/// Punctuation stripped from the ends of a word before it's read as a link.
const LINK_TRIM: &[char] = &[
    '.', ',', ';', ':', '!', '?', '(', ')', '[', ']', '<', '>', '"', '\'', '«', '»', '“', '”',
];
const LINK_PREFIXES: &[&str] = &["http://", "https://", "www."];
/// Characters that end a sentence (the next word may be capitalized without being a name).
const SENTENCE_END: &[char] = &['.', '!', '?', '…'];

/// Cleanup applied to every model answer before checking: drop
/// `<think>…</think>` blocks, trim, remove one pair of wrapping quotes.
pub fn clean_output(raw: &str) -> String {
    let mut text = raw.to_string();
    // Closed blocks anywhere.
    while let Some(start) = text.find(THINK_OPEN) {
        match text[start..].find(THINK_CLOSE) {
            Some(end) => text.replace_range(start..start + end + THINK_CLOSE.len(), ""),
            // Unclosed: the model ran out of tokens while thinking; no answer.
            None => text.truncate(start),
        }
    }
    // A stray close tag: some templates open the block in the prompt, so
    // only the end shows up. Everything before it is thinking.
    if let Some(end) = text.rfind(THINK_CLOSE) {
        text = text[end + THINK_CLOSE.len()..].to_string();
    }
    let trimmed = text.trim();
    for &(open, close) in WRAPPING_QUOTES {
        if let Some(inner) = trimmed
            .strip_prefix(open)
            .and_then(|t| t.strip_suffix(close))
        {
            return inner.trim().to_string();
        }
    }
    trimmed.to_string()
}

fn script_of(c: char) -> Option<Script> {
    match c as u32 {
        // Basic Latin letters, Latin-1 Supplement through Latin Extended-B,
        // Latin Extended Additional (Unicode blocks).
        0x41..=0x5A | 0x61..=0x7A | 0xC0..=0x24F | 0x1E00..=0x1EFF if c.is_alphabetic() => {
            Some(Script::Latin)
        }
        // Georgian, Georgian Extended (Mtavruli), Georgian Supplement.
        0x10A0..=0x10FF | 0x1C90..=0x1CBF | 0x2D00..=0x2D2F => Some(Script::Georgian),
        // Cyrillic and Cyrillic Supplement.
        0x0400..=0x052F => Some(Script::Cyrillic),
        _ => None,
    }
}

/// The script most of the text's letters belong to.
pub fn detect_script(text: &str) -> Script {
    let (mut latin, mut georgian, mut cyrillic) = (0usize, 0usize, 0usize);
    for c in text.chars() {
        match script_of(c) {
            Some(Script::Latin) => latin += 1,
            Some(Script::Georgian) => georgian += 1,
            Some(Script::Cyrillic) => cyrillic += 1,
            _ => {}
        }
    }
    let best = latin.max(georgian).max(cyrillic);
    if best == 0 {
        Script::Unknown
    } else if best == latin {
        Script::Latin
    } else if best == georgian {
        Script::Georgian
    } else {
        Script::Cyrillic
    }
}

fn trim_link(word: &str) -> &str {
    word.trim_matches(LINK_TRIM)
}

fn is_url(word: &str) -> bool {
    let lower = word.to_lowercase();
    LINK_PREFIXES
        .iter()
        .any(|p| lower.starts_with(p) && lower.len() > p.len())
}

fn is_email(word: &str) -> bool {
    match word.split_once('@') {
        Some((local, domain)) => {
            !local.is_empty()
                && !domain.contains('@')
                && domain
                    .split_once('.')
                    .is_some_and(|(a, b)| !a.is_empty() && !b.is_empty())
        }
        None => false,
    }
}

/// URLs and email addresses, exactly as written (surrounding punctuation removed).
pub fn links(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(trim_link)
        .filter(|w| is_url(w) || is_email(w))
        .map(str::to_string)
        .collect()
}

/// The text without its links, so digits inside a URL aren't read as numbers.
fn without_links(text: &str) -> String {
    text.split_whitespace()
        .filter(|w| {
            let w = trim_link(w);
            !(is_url(w) || is_email(w))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Number tokens: digit runs, joined by one separator when a digit follows
/// (3.5, 18:30, 17/10, 2026-10-17). A sentence-ending dot is not included.
pub fn numbers(text: &str) -> Vec<String> {
    let chars: Vec<char> = without_links(text).chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let mut token = String::new();
        loop {
            while i < chars.len() && chars[i].is_ascii_digit() {
                token.push(chars[i]);
                i += 1;
            }
            let joins = i + 1 < chars.len()
                && NUMBER_SEPARATORS.contains(&chars[i])
                && chars[i + 1].is_ascii_digit();
            if !joins {
                break;
            }
            token.push(chars[i]);
            i += 1;
        }
        out.push(token);
    }
    out
}

/// Words for negation counting: letters, digits and apostrophes, lowercased,
/// with curly apostrophes made straight.
fn negation_words(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '’'))
        .map(|w| w.replace('’', "'").trim_matches('\'').to_lowercase())
        .filter(|w| !w.is_empty())
}

/// The negation words (any language in the prompts file) in the text, in order.
pub fn negation_list(text: &str, prompts: &Prompts) -> Vec<String> {
    let set: HashSet<String> = prompts.negations.all().collect();
    negation_words(text).filter(|w| set.contains(w)).collect()
}

/// How many negation words the text has.
pub fn count_negations(text: &str, prompts: &Prompts) -> usize {
    negation_list(text, prompts).len()
}

/// Runs of letters (apostrophes and punctuation split words).
fn letter_runs(word: &str) -> impl Iterator<Item = &str> {
    word.split(|c: char| !c.is_alphabetic())
        .filter(|w| !w.is_empty())
}

/// Latin-script names: capitalized words of 2+ letters that don't start a
/// sentence. Only meaningful for Latin text (Georgian has no capitals).
pub fn names(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let mut sentence_start = true;
        for word in line.split_whitespace() {
            let core = trim_link(word);
            if !(is_url(core) || is_email(core))
                && let Some(run) = letter_runs(word).next()
                && !sentence_start
                && run.chars().count() >= MIN_NAME_LETTERS
                && run
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_uppercase() && script_of(c) == Some(Script::Latin))
            {
                out.push(run.to_string());
            }
            let end = word.trim_end_matches(['"', '\'', ')', '»', '”']);
            sentence_start = end.ends_with(SENTENCE_END);
        }
    }
    out
}

fn starts_with_any<'a>(lower: &str, phrases: &'a [String]) -> Option<&'a str> {
    phrases
        .iter()
        .map(String::as_str)
        .find(|p| lower.starts_with(&p.to_lowercase()))
}

/// The last sentence, lowercased, without its final punctuation.
fn last_sentence(text: &str) -> String {
    let lower = text.to_lowercase();
    let body = lower.trim_end_matches(|c: char| c.is_ascii_punctuation() || c.is_whitespace());
    let start = body
        .rfind(|c: char| SENTENCE_END.contains(&c) || c == '\n')
        .map_or(0, |i| {
            i + body[i..].chars().next().map_or(1, char::len_utf8)
        });
    body[start..].trim().to_string()
}

/// Starts with a preamble ("Here is…") or ends with an offer ("Let me know
/// if…"). Phrases the original itself uses are allowed.
fn has_preamble(text: &str, original: Option<&str>, prompts: &Prompts) -> bool {
    let lower = text.to_lowercase();
    let orig_lower = original.map(str::to_lowercase);
    if let Some(p) = starts_with_any(&lower, &prompts.preamble_starts.phrases) {
        let original_too = orig_lower
            .as_deref()
            .is_some_and(|o| o.contains(&p.to_lowercase()));
        if !original_too {
            return true;
        }
    }
    let last = last_sentence(text);
    if let Some(p) = starts_with_any(&last, &prompts.offer_phrases.phrases) {
        let original_too = orig_lower
            .as_deref()
            .is_some_and(|o| o.contains(&p.to_lowercase()));
        if !original_too {
            return true;
        }
    }
    false
}

/// Markdown: list or heading lines, bold, or backticks.
fn has_markdown(text: &str) -> bool {
    text.contains("**")
        || text.contains('`')
        || text.lines().any(|l| {
            let l = l.trim_start();
            l.starts_with('#') || l.starts_with("- ") || l.starts_with("* ")
        })
}

fn echoes_prompt(text: &str, original: Option<&str>, prompts: &Prompts) -> bool {
    let lower = text.to_lowercase();
    let orig_lower = original.map(str::to_lowercase).unwrap_or_default();
    prompts.prompt_echo_phrases.phrases.iter().any(|p| {
        let p = p.to_lowercase();
        lower.contains(&p) && !orig_lower.contains(&p)
    })
}

/// Checks a, b, c (and the prompt echo) shared by shorten and decoy.
/// Returns false if the text is empty (nothing else is worth checking).
fn common_checks(
    text: &str,
    original: Option<&str>,
    prompts: &Prompts,
    failed: &mut Vec<Failure>,
) -> bool {
    if text.trim().is_empty() {
        failed.push(Failure::new(Check::Empty));
        return false;
    }
    if has_preamble(text, original, prompts) {
        failed.push(Failure::new(Check::Preamble));
    }
    if has_markdown(text) && !original.is_some_and(has_markdown) {
        failed.push(Failure::new(Check::Markdown));
    }
    if echoes_prompt(text, original, prompts) {
        failed.push(Failure::new(Check::PromptEcho));
    }
    true
}

/// The needed items that `have` lacks, without duplicates.
fn missing(needed: &[String], have: &[String]) -> Vec<String> {
    let have: HashSet<&str> = have.iter().map(String::as_str).collect();
    let mut out: Vec<String> = Vec::new();
    for n in needed {
        if !have.contains(n.as_str()) && !out.contains(n) {
            out.push(n.clone());
        }
    }
    out
}

/// Every check for a shortened text. Empty result = accepted.
pub fn check_shorten(original: &str, result: &str, prompts: &Prompts) -> Vec<Failure> {
    let mut failed = Vec::new();
    if !common_checks(result, Some(original), prompts, &mut failed) {
        return failed;
    }
    if result.len() >= original.len() {
        failed.push(Failure::with(
            Check::NotShorter,
            vec![original.len().to_string()],
        ));
    }
    if result.len() * MIN_SHORTEN_FRACTION_DIVISOR < original.len() {
        failed.push(Failure::new(Check::TooShort));
    }
    let gone = missing(&numbers(original), &numbers(result));
    if !gone.is_empty() {
        failed.push(Failure::with(Check::Numbers, quoted(&gone)));
    }
    let gone = missing(&links(original), &links(result));
    if !gone.is_empty() {
        failed.push(Failure::with(Check::Links, quoted(&gone)));
    }
    let original_script = detect_script(original);
    if original_script != Script::Georgian {
        let result_words: Vec<String> = result
            .split_whitespace()
            .flat_map(letter_runs)
            .map(str::to_string)
            .collect();
        let gone = missing(&names(original), &result_words);
        if !gone.is_empty() {
            failed.push(Failure::with(Check::Names, quoted(&gone)));
        }
    }
    let original_negations = negation_list(original, prompts);
    if original_negations.len() != count_negations(result, prompts) {
        let items = if original_negations.is_empty() {
            vec!["none".to_string()]
        } else {
            quoted(&original_negations)
        };
        failed.push(Failure::with(Check::Negations, items));
    }
    if detect_script(result) != original_script {
        failed.push(Failure::new(Check::Script));
    }
    failed
}

/// Checks a decoy except the padding match. Empty result = acceptable.
pub fn check_decoy_content(text: &str, script: Script, prompts: &Prompts) -> Vec<Failure> {
    let mut failed = Vec::new();
    if !common_checks(text, None, prompts, &mut failed) {
        return failed;
    }
    if detect_script(text) != script {
        failed.push(Failure::new(Check::Script));
    }
    let lower = text.to_lowercase();
    let banned: Vec<String> = prompts
        .banned_decoy_words
        .words
        .iter()
        .filter(|w| lower.contains(&w.to_lowercase()))
        .cloned()
        .collect();
    if !banned.is_empty() {
        failed.push(Failure::with(Check::BannedWord, quoted(&banned)));
    }
    failed
}

/// The feedback paragraph for the next attempt, from the prompts file.
pub fn feedback(failed: &[Failure], prompts: &Prompts) -> String {
    let problems: Vec<String> = failed
        .iter()
        .filter_map(|f| {
            let template = prompts.feedback.checks.get(f.check.name())?;
            Some(template.replace("{items}", &f.items.join(", ")))
        })
        .collect();
    prompts
        .feedback
        .template
        .replace("{problems}", &problems.join("; "))
}

/// True if a decoy of `decoy_len` bytes pads to the same slot size as the
/// real text, using poly-core's own padding rule.
pub fn padding_matches(decoy_len: usize, target_len: usize) -> bool {
    padded_len(decoy_len) == padded_len(target_len)
}

/// The byte lengths (inclusive) a decoy may have to pad like the target,
/// found by asking poly-core's padding rule, never re-deriving it.
/// The minimum is at least 1 (a decoy can't be empty).
pub fn allowed_len_range(target_len: usize) -> (usize, usize) {
    let padded = padded_len(target_len);
    let mut min = target_len;
    while min > 1 && padded_len(min - 1) == padded {
        min -= 1;
    }
    let mut max = target_len;
    while max < poly_core::payload::MAX_TEXT_LEN && padded_len(max + 1) == padded {
        max += 1;
    }
    (min.max(1), max)
}

/// How far a length is from the allowed range (0 = inside).
pub fn distance_to_range(len: usize, (min, max): (usize, usize)) -> usize {
    if len < min {
        min - len
    } else {
        len.saturating_sub(max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prompts::prompts;

    fn shorten_fails(original: &str, result: &str) -> Vec<Check> {
        check_shorten(original, result, prompts())
            .into_iter()
            .map(|f| f.check)
            .collect()
    }

    fn decoy_fails(text: &str, script: Script) -> Vec<Check> {
        check_decoy_content(text, script, prompts())
            .into_iter()
            .map(|f| f.check)
            .collect()
    }

    const ORIGINAL: &str = "Hi Nino, I will not be able to come to the meeting at 18:30 on 17/10 because my train from Batumi is delayed. Please check https://example.com/agenda and do not start without me.";
    const GOOD: &str = "Nino, I will not make the 18:30 meeting on 17/10: my train from Batumi is delayed. Check https://example.com/agenda and do not start without me.";

    #[test]
    fn good_shortening_passes() {
        assert_eq!(shorten_fails(ORIGINAL, GOOD), vec![]);
    }

    #[test]
    fn think_block_removed() {
        assert_eq!(
            clean_output("<think>count words...</think>\n Hello there "),
            "Hello there"
        );
        assert_eq!(
            clean_output("<think>a</think>One <think>b</think>two"),
            "One two"
        );
        assert_eq!(clean_output("reasoning here</think>Answer"), "Answer");
        assert_eq!(clean_output("<think>never finished"), "");
    }

    #[test]
    fn wrapping_quotes_removed_once() {
        assert_eq!(clean_output("  \"Hello\"  "), "Hello");
        assert_eq!(clean_output("“Hello”"), "Hello");
        assert_eq!(clean_output("\"\"Hello\"\""), "\"Hello\"");
        assert_eq!(clean_output("\"Hello\" and \"bye"), "\"Hello\" and \"bye");
    }

    #[test]
    fn empty_fails() {
        assert_eq!(shorten_fails(ORIGINAL, ""), vec![Check::Empty]);
        assert_eq!(shorten_fails(ORIGINAL, "   "), vec![Check::Empty]);
    }

    #[test]
    fn dropped_number_fails() {
        let r = GOOD.replace("18:30 ", "");
        assert!(shorten_fails(ORIGINAL, &r).contains(&Check::Numbers));
        let r = GOOD.replace("17/10", "17 Oct");
        assert!(shorten_fails(ORIGINAL, &r).contains(&Check::Numbers));
    }

    #[test]
    fn number_tokens() {
        assert_eq!(
            numbers("At 18:30 on 17/10, pay 3.5 GEL. Room 12."),
            vec!["18:30", "17/10", "3.5", "12"]
        );
        assert_eq!(numbers("2026-10-17"), vec!["2026-10-17"]);
        // Digits inside links are the link check's job.
        assert_eq!(numbers("see https://x.com/a/123 now"), Vec::<String>::new());
    }

    #[test]
    fn dropped_link_fails() {
        let r = GOOD.replace("https://example.com/agenda", "the agenda");
        assert!(shorten_fails(ORIGINAL, &r).contains(&Check::Links));
        let o = "Write to nino@example.ge about the plan for the whole weekend, thanks a lot.";
        assert!(shorten_fails(o, "Write to Nino about the weekend plan.").contains(&Check::Links));
        assert!(
            !shorten_fails(o, "Write to nino@example.ge about the weekend plan.")
                .contains(&Check::Links)
        );
    }

    #[test]
    fn link_tokens() {
        assert_eq!(
            links("See (https://a.com/x), www.b.ge. Mail me: n@c.org!"),
            vec!["https://a.com/x", "www.b.ge", "n@c.org"]
        );
    }

    #[test]
    fn removed_negation_fails_english() {
        let r = GOOD.replace("will not make", "will make");
        assert!(shorten_fails(ORIGINAL, &r).contains(&Check::Negations));
        assert_eq!(count_negations("I don’t know, nobody can't", prompts()), 3);
        assert_eq!(count_negations("Notes are nothing", prompts()), 1);
    }

    #[test]
    fn removed_negation_fails_georgian() {
        let original = "ხვალ შეხვედრაზე ვერ მოვალ, რადგან მატარებელი იგვიანებს. ჩემს გარეშე არ დაიწყოთ, გთხოვთ.";
        let good = "ხვალ ვერ მოვალ, მატარებელი იგვიანებს. ჩემს გარეშე არ დაიწყოთ.";
        let flipped = "ხვალ მოვალ, მატარებელი იგვიანებს. ჩემს გარეშე არ დაიწყოთ.";
        assert_eq!(shorten_fails(original, good), vec![]);
        assert!(shorten_fails(original, flipped).contains(&Check::Negations));
        // "არის" (is) is not the negation "არ".
        assert_eq!(count_negations("ეს არის სახლი", prompts()), 0);
    }

    #[test]
    fn switched_script_fails() {
        let original = "ხვალ შეხვედრაზე ვერ მოვალ, რადგან მატარებელი იგვიანებს, ბოდიში.";
        let english = "I can't come tomorrow, my train is late.";
        assert!(shorten_fails(original, english).contains(&Check::Script));
    }

    #[test]
    fn script_detection() {
        assert_eq!(detect_script("Hello, Nino!"), Script::Latin);
        assert_eq!(detect_script("გამარჯობა, Nino"), Script::Georgian);
        assert_eq!(detect_script("Привет, как дела?"), Script::Cyrillic);
        assert_eq!(detect_script("18:30 🙂"), Script::Unknown);
        // Mtavruli (Georgian capitals) count as Georgian.
        assert_eq!(detect_script("\u{1C90}\u{1C91}"), Script::Georgian);
    }

    #[test]
    fn preamble_fails() {
        let r = format!("Here is a shorter version: {GOOD}");
        assert!(shorten_fails(ORIGINAL, &r).contains(&Check::Preamble));
        let r = format!("Sure! {GOOD}");
        assert!(shorten_fails(ORIGINAL, &r).contains(&Check::Preamble));
        let r = format!("{GOOD} Let me know if you want it shorter.");
        assert!(shorten_fails(ORIGINAL, &r).contains(&Check::Preamble));
    }

    #[test]
    fn preamble_allowed_when_original_has_it() {
        let o = "Sure, I will bring the cake to the party on Saturday evening, see you there.";
        assert!(
            !shorten_fails(o, "Sure, I'll bring the cake Saturday evening.")
                .contains(&Check::Preamble)
        );
        let o = "I will be late tonight, sorry about that. Let me know if you need anything.";
        assert!(
            !shorten_fails(o, "Late tonight, sorry. Let me know if you need anything.")
                .contains(&Check::Preamble)
        );
    }

    #[test]
    fn markdown_fails() {
        for r in [
            format!("**{GOOD}**"),
            format!("- {GOOD}"),
            format!("# Note\n{GOOD}"),
            GOOD.replace("18:30", "`18:30`"),
        ] {
            assert!(
                shorten_fails(ORIGINAL, &r).contains(&Check::Markdown),
                "{r}"
            );
        }
    }

    #[test]
    fn longer_output_fails() {
        let r = format!("{ORIGINAL} Thanks!");
        assert!(shorten_fails(ORIGINAL, &r).contains(&Check::NotShorter));
        assert!(shorten_fails(ORIGINAL, ORIGINAL).contains(&Check::NotShorter));
    }

    #[test]
    fn too_short_output_fails() {
        let r = "Nino: not 18:30 17/10, Batumi, https://example.com/agenda, not";
        assert!(
            r.len() * 3 >= ORIGINAL.len(),
            "test text must pass the third rule"
        );
        assert!(!shorten_fails(ORIGINAL, r).contains(&Check::TooShort));
        assert!(shorten_fails(ORIGINAL, "OK").contains(&Check::TooShort));
    }

    #[test]
    fn missing_name_fails() {
        let r = GOOD.replace("from Batumi ", "");
        assert!(shorten_fails(ORIGINAL, &r).contains(&Check::Names));
    }

    #[test]
    fn name_detection() {
        assert_eq!(
            names("Tell Giorgi. Then call Ana and the UN in Paris! We go."),
            vec!["Giorgi", "Ana", "UN", "Paris"]
        );
        // "I'm" and one-letter words aren't names; links aren't either.
        assert_eq!(
            names("Yes I'm at https://Example.com now"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn name_check_skipped_for_georgian() {
        let original = "გამარჯობა, ხვალ Giorgi-სთან ერთად მოვალთ საღამოს შვიდზე, დაგვიცადეთ.";
        let r = "ხვალ საღამოს შვიდზე მოვალთ, დაგვიცადეთ.";
        assert!(!shorten_fails(original, r).contains(&Check::Names));
    }

    #[test]
    fn prompt_echo_fails() {
        let r = format!("{} text to rewrite", &GOOD[..GOOD.len() - 20]);
        assert!(shorten_fails(ORIGINAL, &r).contains(&Check::PromptEcho));
    }

    #[test]
    fn decoy_checks() {
        assert_eq!(
            decoy_fails("Did you buy milk on the way home?", Script::Latin),
            vec![]
        );
        assert!(
            decoy_fails("My secret recipe is ready", Script::Latin).contains(&Check::BannedWord)
        );
        assert!(
            decoy_fails("ჩემი პაროლი დამავიწყდა", Script::Georgian).contains(&Check::BannedWord)
        );
        assert!(
            decoy_fails("Here is an everyday message: hi", Script::Latin)
                .contains(&Check::PromptEcho)
        );
        assert!(decoy_fails("Did you buy milk?", Script::Georgian).contains(&Check::Script));
        assert!(decoy_fails("- buy milk", Script::Latin).contains(&Check::Markdown));
        assert_eq!(decoy_fails("", Script::Latin), vec![Check::Empty]);
    }

    #[test]
    fn feedback_names_what_was_dropped() {
        let p = prompts();
        let r = GOOD
            .replace("18:30 ", "")
            .replace("will not make", "will make");
        let f = feedback(&check_shorten(ORIGINAL, &r, p), p);
        assert!(f.contains("\"18:30\""), "{f}");
        assert!(f.contains("\"not\""), "{f}");
        assert!(f.starts_with("Your previous attempt was rejected"));
    }

    #[test]
    fn padding_rule_comes_from_poly_core() {
        // 2-byte prefix + text rounded up to 16: 1..=14 bytes pad to 16.
        assert_eq!(allowed_len_range(5), (1, 14));
        assert_eq!(allowed_len_range(14), (1, 14));
        assert_eq!(allowed_len_range(15), (15, 30));
        assert!(padding_matches(20, 30));
        assert!(!padding_matches(31, 30));
        assert_eq!(distance_to_range(40, (15, 30)), 10);
        assert_eq!(distance_to_range(10, (15, 30)), 5);
        assert_eq!(distance_to_range(20, (15, 30)), 0);
    }
}
