//! Grammar-aware word completion.
//!
//! Completion reuses the tagger dictionary of a [Tokenizer] (all inflected forms with their lemmas and
//! part-of-speech tags, stored as an FST) for prefix search, the tokenizer pipeline to analyze the words
//! before the cursor, and optionally a [Frequencies] list for ranking and the [Rules] to discard candidates
//! which would introduce a grammatical error.
//!
//! ```no_run
//! use nlprule::{Tokenizer, complete::{Completer, CompletionOptions}};
//!
//! let tokenizer = Tokenizer::new("path/to/es_tokenizer.bin")?;
//! let completer = Completer::new(&tokenizer);
//!
//! let completions = completer.complete("Los niños jue", &CompletionOptions::default());
//! assert_eq!(completions.items[0].word, "juegan");
//! # Ok::<(), nlprule::Error>(())
//! ```
//!
//! The context rules are currently written for the EAGLES tag set of the Spanish dictionary,
//! see [context]. For other languages the completer still works, ranked only by frequency and length.

pub mod context;
mod freq;

pub use context::{ContextInfo, Expectation, TagPattern};
pub use freq::Frequencies;

use crate::{utils, Rules, Tokenizer};
use serde::Serialize;
use std::collections::HashMap;

/// Score boost for candidates matching a boosted pattern (e.g. imperfect subjunctive after "si").
const BOOST_WEIGHT: f32 = 3.0;

/// Options for [Completer::complete].
#[derive(Debug, Clone)]
pub struct CompletionOptions {
    /// The maximum number of completions returned.
    pub limit: usize,
    /// If set, only forms with a tag matching this tag prefix are returned (`?` / `*` are wildcards,
    /// e.g. `VMIP3P`). Replaces the tags inferred from the context.
    pub tag_filter: Option<String>,
    /// Whether to infer the expected tags from the words before the cursor.
    pub use_context: bool,
    /// Whether to discard candidates for which the rules report an error (requires [Completer::with_rules]).
    pub validate: bool,
    /// The maximum number of dictionary entries scanned per prefix.
    pub max_scan: usize,
    /// No completions are computed for shorter prefixes (in characters).
    pub min_prefix_len: usize,
}

impl Default for CompletionOptions {
    fn default() -> Self {
        CompletionOptions {
            limit: 10,
            tag_filter: None,
            use_context: true,
            validate: false,
            max_scan: 20_000,
            min_prefix_len: 1,
        }
    }
}

/// One completion candidate.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Completion {
    /// The completed word, with the casing of the typed prefix applied.
    pub word: String,
    /// The lemmas of the word.
    pub lemmas: Vec<String>,
    /// The part-of-speech tags of the word.
    pub tags: Vec<String>,
    /// Whether one of the tags fits the context (or the explicit tag filter).
    pub matches_context: bool,
    /// How well the best matching tag fits the context: 0 = not at all, 1 = a secondary reading, 2 = the main reading.
    pub context_weight: u8,
    /// The frequency of the word (0 if no frequencies are loaded or the word is unknown).
    pub frequency: u64,
    /// The ranking score (log frequency plus context boosts).
    pub score: f32,
}

/// The result of [Completer::complete].
#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct Completions {
    /// The prefix being completed (the partial word right before the cursor).
    pub prefix: String,
    /// Start of the prefix in the input, in characters.
    pub start: usize,
    /// End of the prefix in the input (= the input length), in characters.
    pub end: usize,
    /// What was inferred from the context.
    pub context: ContextInfo,
    /// The completions, best first.
    pub items: Vec<Completion>,
}

/// Grammar-aware word completion, see the [module docs][self].
#[derive(Clone, Copy)]
pub struct Completer<'a> {
    tokenizer: &'a Tokenizer,
    rules: Option<&'a Rules>,
    frequencies: Option<&'a Frequencies>,
}

struct Candidate {
    word: String,
    lemmas: Vec<String>,
    tags: Vec<String>,
}

fn is_word_char(c: char) -> bool {
    c.is_alphabetic() || c == '-' || c == '\''
}

/// Splits the text into context and the partial word at its end.
fn split_prefix(text: &str) -> (&str, &str) {
    let mut start = text.len();
    for (i, c) in text.char_indices().rev() {
        if is_word_char(c) {
            start = i;
        } else {
            break;
        }
    }
    // a word does not start with a hyphen or apostrophe
    while let Some(c) = text[start..].chars().next() {
        if c.is_alphabetic() {
            break;
        }
        start += c.len_utf8();
    }
    (&text[..start], &text[start..])
}

fn apply_case(prefix: &str, word: &str) -> String {
    let n_prefix_chars = prefix.chars().count();
    if n_prefix_chars > 1 && utils::is_uppercase(prefix) {
        word.to_uppercase()
    } else if prefix.chars().next().map_or(false, char::is_uppercase) {
        utils::apply_to_first(word, |c| c.to_uppercase().collect())
    } else {
        word.to_string()
    }
}

fn ends_sentence(text: &str) -> bool {
    text.trim_end()
        .chars()
        .last()
        .map_or(false, |c| ".!?…".contains(c))
}

impl<'a> Completer<'a> {
    /// Creates a completer using the dictionary of the given tokenizer.
    pub fn new(tokenizer: &'a Tokenizer) -> Self {
        Completer {
            tokenizer,
            rules: None,
            frequencies: None,
        }
    }

    /// Uses the given rules to validate candidates if [CompletionOptions::validate] is set.
    pub fn with_rules(mut self, rules: &'a Rules) -> Self {
        self.rules = Some(rules);
        self
    }

    /// Uses the given frequencies to rank candidates.
    pub fn with_frequencies(mut self, frequencies: &'a Frequencies) -> Self {
        self.frequencies = Some(frequencies);
        self
    }

    /// Returns the byte offset in `context` where the current sentence starts.
    fn sentence_start(&self, context: &str) -> usize {
        if context.trim().is_empty() || ends_sentence(context) {
            return context.len();
        }
        self.tokenizer
            .sentencize(context)
            .last()
            .map_or(0, |sentence| sentence.span().byte().start)
    }

    /// The expected tags of the next word given the current sentence up to the cursor.
    pub fn infer_context(&self, sentence: &str) -> Expectation {
        if sentence.trim().is_empty() {
            return Expectation::default();
        }

        let tokens: Vec<(String, Vec<String>)> = self
            .tokenizer
            .pipe(sentence)
            .last()
            .map(|sentence| {
                sentence
                    .tokens()
                    .iter()
                    .map(|token| {
                        (
                            token.word().text().as_str().to_string(),
                            token
                                .word()
                                .tags()
                                .iter()
                                .map(|data| data.pos().as_str().to_string())
                                .collect(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();

        context::infer(&tokens)
    }

    fn collect_candidates(&self, prefix: &str, options: &CompletionOptions) -> Vec<Candidate> {
        let tagger = self.tokenizer.tagger();
        let lower = prefix.to_lowercase();

        let mut by_word: HashMap<String, usize> = HashMap::new();
        let mut candidates: Vec<Candidate> = Vec::new();

        let mut add = |word: &str, data: &[crate::types::WordData]| {
            if word.to_lowercase() == lower {
                return;
            }
            let index = *by_word.entry(word.to_string()).or_insert_with(|| {
                candidates.push(Candidate {
                    word: word.to_string(),
                    lemmas: Vec::new(),
                    tags: Vec::new(),
                });
                candidates.len() - 1
            });
            let candidate = &mut candidates[index];
            for d in data {
                let lemma = d.lemma().as_str();
                let tag = d.pos().as_str();
                if !candidate.lemmas.iter().any(|l| l == lemma) {
                    candidate.lemmas.push(lemma.to_string());
                }
                if !candidate.tags.iter().any(|t| t == tag) {
                    candidate.tags.push(tag.to_string());
                }
            }
        };

        // short prefixes match too many dictionary entries for a lexicographic scan to be useful,
        // so if frequencies are available, start from the most frequent words with that prefix
        if let Some(freq) = self.frequencies {
            if lower.chars().count() < 3 {
                let mut frequent = freq.with_prefix(&lower, usize::MAX);
                frequent.sort_by(|a, b| b.1.cmp(&a.1));
                for (word, _) in frequent.into_iter().take(options.max_scan) {
                    if let Some((found, data)) = tagger.words_with_prefix(&word, 1).pop() {
                        if found == word {
                            add(found, &data);
                        }
                    }
                }
            }
        }

        for (word, data) in tagger.words_with_prefix(&lower, options.max_scan) {
            add(word, &data);
        }
        if lower != prefix {
            // e.g. proper nouns
            for (word, data) in tagger.words_with_prefix(prefix, options.max_scan) {
                add(word, &data);
            }
        }

        candidates
    }

    /// Whether the rules report an error overlapping `word` when it is appended to `sentence`.
    fn introduces_error(&self, rules: &Rules, sentence: &str, word: &str) -> bool {
        let text = format!("{}{}", sentence, word);
        let start = sentence.chars().count();
        let end = start + word.chars().count();

        rules.suggest(&text, self.tokenizer).iter().any(|s| {
            let span = s.span().char();
            span.start < end && start < span.end
        })
    }

    /// Completes the partial word at the end of `text` (the text up to the cursor).
    pub fn complete(&self, text: &str, options: &CompletionOptions) -> Completions {
        let (context, prefix) = split_prefix(text);
        let end = text.chars().count();
        let start = end - prefix.chars().count();

        let mut result = Completions {
            prefix: prefix.to_string(),
            start,
            end,
            ..Completions::default()
        };

        if prefix.is_empty() || prefix.chars().count() < options.min_prefix_len {
            return result;
        }

        let sentence = &context[self.sentence_start(context)..];

        let expectation = match &options.tag_filter {
            Some(filter) => Expectation {
                patterns: vec![TagPattern::from_prefix(filter)],
                weights: vec![2],
                boosts: Vec::new(),
            },
            None if options.use_context => self.infer_context(sentence),
            None => Expectation::default(),
        };
        result.context = expectation.info();

        let mut scored: Vec<Completion> = self
            .collect_candidates(prefix, options)
            .into_iter()
            .filter_map(|mut candidate| {
                if options.tag_filter.is_some() {
                    // a hard filter: only keep the matching readings
                    let pattern = &expectation.patterns[0];
                    candidate.tags.retain(|tag| pattern.matches(tag));
                    if candidate.tags.is_empty() {
                        return None;
                    }
                }

                let context_weight = expectation.weight(&candidate.tags);
                let matches_context = context_weight > 0;
                let boosted = matches_context
                    && candidate
                        .tags
                        .iter()
                        .any(|tag| expectation.boosts.iter().any(|p| p.matches(tag)));

                let frequency = self
                    .frequencies
                    .map_or(0, |freq| freq.get(&candidate.word.to_lowercase()));
                let mut score = (1.0 + frequency as f32).ln();
                if boosted {
                    score += BOOST_WEIGHT;
                }

                Some(Completion {
                    word: candidate.word,
                    lemmas: candidate.lemmas,
                    tags: candidate.tags,
                    matches_context,
                    context_weight,
                    frequency,
                    score,
                })
            })
            .collect();

        // compounds like "jueces-árbitro" are rarely what is meant unless typed
        let compound_penalty =
            |word: &str| !prefix.contains('-') && (word.contains('-') || word.contains(' '));

        scored.sort_by(|a, b| {
            b.context_weight
                .cmp(&a.context_weight)
                .then(compound_penalty(&a.word).cmp(&compound_penalty(&b.word)))
                .then(
                    b.score
                        .partial_cmp(&a.score)
                        .unwrap_or(std::cmp::Ordering::Equal),
                )
                .then(a.word.chars().count().cmp(&b.word.chars().count()))
                .then(a.word.cmp(&b.word))
        });

        let mut items = Vec::with_capacity(options.limit);
        let rules = if options.validate { self.rules } else { None };
        let max_checks = options.limit.saturating_mul(3);

        for (i, mut item) in scored.into_iter().enumerate() {
            if items.len() >= options.limit {
                break;
            }
            item.word = apply_case(prefix, &item.word);

            if let Some(rules) = rules {
                if i >= max_checks {
                    break;
                }
                if self.introduces_error(rules, sentence, &item.word) {
                    continue;
                }
            }
            items.push(item);
        }

        result.items = items;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_prefix() {
        assert_eq!(split_prefix("Los niños jue"), ("Los niños ", "jue"));
        assert_eq!(split_prefix("Los niños "), ("Los niños ", ""));
        assert_eq!(split_prefix("jue"), ("", "jue"));
        assert_eq!(split_prefix("(¿Qué"), ("(¿", "Qué"));
        assert_eq!(split_prefix("un -ex"), ("un -", "ex"));
        assert_eq!(split_prefix("guarda-ba"), ("", "guarda-ba"));
        assert_eq!(split_prefix(""), ("", ""));
    }

    #[test]
    fn applies_case() {
        assert_eq!(apply_case("jue", "juegan"), "juegan");
        assert_eq!(apply_case("Jue", "juegan"), "Juegan");
        assert_eq!(apply_case("JUE", "juegan"), "JUEGAN");
        assert_eq!(apply_case("Á", "árbol"), "Árbol");
    }

    #[test]
    fn detects_sentence_end() {
        assert!(ends_sentence("Hola. "));
        assert!(ends_sentence("¿Qué?"));
        assert!(!ends_sentence("Los niños "));
    }
}
