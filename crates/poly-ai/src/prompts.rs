//! `profiles/ai-prompts.json`, embedded at compile time.

use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;

/// The prompts file, embedded so the app can't run with a missing or edited copy.
const PROMPTS_JSON: &str = include_str!("../../../profiles/ai-prompts.json");

#[derive(Debug, Deserialize)]
pub struct Prompts {
    pub text_start_marker: String,
    pub text_end_marker: String,
    pub shorten: ShortenPrompt,
    pub decoy: DecoyPrompt,
    pub limits: Limits,
    pub feedback: Feedback,
    pub languages: Languages,
    pub negations: Negations,
    pub preamble_starts: PhraseList,
    pub offer_phrases: PhraseList,
    pub banned_decoy_words: WordList,
    pub prompt_echo_phrases: PhraseList,
}

#[derive(Debug, Deserialize)]
pub struct ShortenPrompt {
    pub system: String,
    pub user_template: String,
    pub temperature: f64,
    pub retry_temperature: f64,
    pub max_tokens: u32,
    pub attempts: u32,
}

#[derive(Debug, Deserialize)]
pub struct DecoyPrompt {
    pub system: String,
    pub tone_template: String,
    pub user: String,
    pub temperature: f64,
    pub retry_temperature: f64,
    pub max_tokens: u32,
    pub attempts: u32,
    pub max_tone_chars: usize,
}

#[derive(Debug, Deserialize)]
pub struct Limits {
    pub deadline_seconds: u64,
}

#[derive(Debug, Deserialize)]
pub struct Feedback {
    pub template: String,
    /// Check name → problem description, may contain `{items}`.
    pub checks: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
pub struct Languages {
    pub latin: String,
    pub georgian: String,
    pub cyrillic: String,
}

#[derive(Debug, Deserialize)]
pub struct Negations {
    pub en: Vec<String>,
    pub ka: Vec<String>,
    pub ru: Vec<String>,
}

impl Negations {
    /// All negation words of all languages, lowercased.
    pub fn all(&self) -> impl Iterator<Item = String> + '_ {
        self.en
            .iter()
            .chain(&self.ka)
            .chain(&self.ru)
            .map(|w| w.to_lowercase())
    }
}

#[derive(Debug, Deserialize)]
pub struct PhraseList {
    pub phrases: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct WordList {
    pub words: Vec<String>,
}

/// The parsed prompts file. Parsed once; a unit test makes sure it parses.
pub fn prompts() -> &'static Prompts {
    static PROMPTS: OnceLock<Prompts> = OnceLock::new();
    PROMPTS.get_or_init(|| {
        serde_json::from_str(PROMPTS_JSON).expect("profiles/ai-prompts.json is valid")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompts_file_parses_and_is_sane() {
        let p = prompts();
        assert!(p.shorten.attempts >= 1 && p.decoy.attempts >= 1);
        assert!(p.shorten.user_template.contains("{text}"));
        assert!(p.shorten.user_template.contains(&p.text_start_marker));
        assert!(p.shorten.user_template.contains(&p.text_end_marker));
        for placeholder in ["{language}", "{chars}", "{bytes}", "{tone}"] {
            assert!(p.decoy.system.contains(placeholder), "{placeholder}");
        }
        assert!(p.decoy.tone_template.contains("{tone}"));
        assert!(!p.negations.en.is_empty() && !p.negations.ka.is_empty());
        assert!(p.shorten.retry_temperature > p.shorten.temperature);
        assert!(p.decoy.retry_temperature > p.decoy.temperature);
        assert!(p.limits.deadline_seconds > 0);
        assert!(p.feedback.template.contains("{problems}"));
        for check in crate::checks::Check::ALL {
            assert!(
                p.feedback.checks.contains_key(check.name()),
                "{}",
                check.name()
            );
        }
    }
}
