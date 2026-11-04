//! Completion tests on the Spanish binaries. Run `scripts/download_es.sh` first.

use lazy_static::lazy_static;
use nlprule::{
    complete::{Completer, CompletionOptions, Frequencies},
    Rules, Tokenizer,
};

const TOKENIZER_PATH: &str = "../storage/es_tokenizer.bin";
const RULES_PATH: &str = "../storage/es_rules.bin";
const FREQ_PATH: &str = "tests/fixtures/freq_es.txt";

lazy_static! {
    static ref TOKENIZER: Tokenizer = Tokenizer::new(TOKENIZER_PATH).unwrap();
    static ref RULES: Rules = Rules::new(RULES_PATH).unwrap();
    static ref FREQ: Frequencies = Frequencies::from_path(FREQ_PATH).unwrap();
}

fn words(text: &str, options: &CompletionOptions) -> Vec<String> {
    Completer::new(&TOKENIZER)
        .complete(text, options)
        .items
        .into_iter()
        .map(|c| c.word)
        .collect()
}

fn top(text: &str) -> Vec<String> {
    words(text, &CompletionOptions::default())
}

#[test]
fn verb_agrees_with_plural_subject() {
    let completions =
        Completer::new(&TOKENIZER).complete("Los niños jue", &CompletionOptions::default());
    assert_eq!(completions.prefix, "jue");
    assert_eq!((completions.start, completions.end), (10, 13));
    assert!(completions
        .context
        .expected
        .iter()
        .any(|l| l.starts_with('V')));

    let first = &completions.items[0];
    assert_eq!(first.word, "juegan");
    assert!(first.matches_context);
    assert!(first.tags.iter().any(|t| t == "VMIP3P0"));
    assert!(first.lemmas.iter().any(|l| l == "jugar"));
    // a singular verb does not fit
    let juega = completions.items.iter().position(|c| c.word == "juega");
    assert!(juega.map_or(true, |i| !completions.items[i].matches_context));
}

#[test]
fn conditional_si_prefers_imperfect_subjunctive() {
    let result = top("si nosotros escrib");
    assert!(
        ["escribiéramos", "escribiésemos"].contains(&result[0].as_str()),
        "{:?}",
        result
    );
    assert!(result[..2].contains(&"escribiéramos".to_string()));
    assert!(result[..2].contains(&"escribiésemos".to_string()));
}

#[test]
fn adjective_agrees_with_noun() {
    assert_eq!(top("las casas bonit")[0], "bonitas");
    assert_eq!(top("el perro bonit")[0], "bonito");
}

#[test]
fn noun_agrees_with_determiner() {
    let result = top("una ca");
    let first = Completer::new(&TOKENIZER).complete("una ca", &CompletionOptions::default());
    assert!(first.items[0].matches_context, "{:?}", result);
    assert!(first.items[0]
        .tags
        .iter()
        .any(|t| t.starts_with("NCF") || t.starts_with("NCC") || t.starts_with("AQ")));
}

#[test]
fn no_context_returns_candidates() {
    let result = top("jue");
    assert_eq!(result.len(), 10);
    assert!(result.iter().all(|w| w.starts_with("jue")));
    // compounds are ranked last
    assert!(result.iter().all(|w| !w.contains('-')));
}

#[test]
fn frequencies_rank_candidates() {
    let completer = Completer::new(&TOKENIZER).with_frequencies(&FREQ);
    let result: Vec<_> = completer
        .complete("jue", &CompletionOptions::default())
        .items
        .into_iter()
        .map(|c| c.word)
        .collect();
    assert_eq!(&result[..3], &["juega", "juego", "juegan"]);
    let jueceo = result.iter().position(|w| w == "jueceo");
    assert!(jueceo.map_or(true, |i| i > 4));
}

#[test]
fn keeps_casing_of_prefix() {
    assert!(top("Jue").iter().all(|w| w.starts_with("Jue")));
    assert!(top("Los niños JUE")[0] == "JUEGAN");
}

#[test]
fn empty_prefix_gives_nothing() {
    assert!(top("Los niños ").is_empty());
    assert!(top("").is_empty());
}

#[test]
fn new_sentence_has_no_context() {
    let completions =
        Completer::new(&TOKENIZER).complete("Los niños. Jue", &CompletionOptions::default());
    assert!(completions.context.expected.is_empty());
}

#[test]
fn explicit_tag_filter() {
    let options = CompletionOptions {
        tag_filter: Some("VMSI1P".into()),
        ..CompletionOptions::default()
    };
    let result = words("escrib", &options);
    assert_eq!(result.len(), 2, "{:?}", result);
    assert!(result.contains(&"escribiéramos".to_string()));
    assert!(result.contains(&"escribiésemos".to_string()));
}

#[test]
fn validation_discards_errors() {
    let completer = Completer::new(&TOKENIZER).with_rules(&RULES);
    let options = CompletionOptions {
        validate: true,
        use_context: false,
        limit: 50,
        ..CompletionOptions::default()
    };
    // without context, validation still works (and does not crash)
    let result = completer.complete("El agua est", &options);
    assert!(!result.items.is_empty());
    assert!(result.items.iter().all(|c| c.word.starts_with("est")));
}
