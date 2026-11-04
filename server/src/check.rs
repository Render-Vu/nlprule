//! A LanguageTool-compatible `/v2/check` endpoint.
//!
//! Supported parameters: `text` or `data` (with `annotation`, as LanguageTool does for markup),
//! `language` (must match the loaded language, `auto` or a variant like `es-ES`), `disabledRules`,
//! `disabledCategories`. Offsets and lengths are in UTF-16 code units like in LanguageTool.

use crate::State;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Characters of context shown around a match, on each side.
const CONTEXT_CHARS: usize = 40;

pub fn language_name(code: &str) -> &'static str {
    match code {
        "es" => "Spanish",
        "en" => "English",
        "de" => "German",
        _ => "Unknown",
    }
}

#[derive(Deserialize)]
struct Annotation {
    text: Option<String>,
    markup: Option<String>,
    #[serde(rename = "interpretAs")]
    interpret_as: Option<String>,
}

#[derive(Deserialize)]
struct Data {
    annotation: Vec<Annotation>,
}

/// The text as written (including markup) and the plain text that is checked,
/// with a map from plain char indices to original char indices.
#[derive(Debug, PartialEq)]
pub struct Input {
    pub original: String,
    pub plain: String,
    /// `map[i]` is the original char index of plain char `i`; has one extra entry for the end.
    pub map: Vec<usize>,
}

impl Input {
    pub fn from_text(text: &str) -> Self {
        let n = text.chars().count();
        Input {
            original: text.to_string(),
            plain: text.to_string(),
            map: (0..=n).collect(),
        }
    }

    pub fn from_data(data: &str) -> Result<Self, String> {
        let data: Data = serde_json::from_str(data).map_err(|e| format!("invalid data: {}", e))?;
        let mut input = Input {
            original: String::new(),
            plain: String::new(),
            map: Vec::new(),
        };
        let mut position = 0;

        for part in data.annotation {
            match (part.text, part.markup) {
                (Some(text), None) => {
                    for c in text.chars() {
                        input.map.push(position);
                        input.plain.push(c);
                        input.original.push(c);
                        position += 1;
                    }
                }
                (None, Some(markup)) => {
                    // markup is not checked; its interpretation maps to where the markup starts
                    for c in part.interpret_as.unwrap_or_default().chars() {
                        input.map.push(position);
                        input.plain.push(c);
                    }
                    input.original.push_str(&markup);
                    position += markup.chars().count();
                }
                _ => {
                    return Err(
                        "each annotation must have exactly one of `text` or `markup`".into(),
                    )
                }
            }
        }
        input.map.push(position);

        Ok(input)
    }

    /// Maps a char range in the plain text to a char range in the original text.
    pub fn to_original(&self, start: usize, end: usize) -> (usize, usize) {
        let start_original = self.map[start];
        let end_original = if end > start {
            self.map[end - 1] + 1
        } else {
            start_original
        };
        (start_original, end_original.max(start_original))
    }
}

/// UTF-16 offset of each char index of `text` (with one extra entry for the end).
pub fn utf16_offsets(text: &str) -> Vec<usize> {
    let mut offsets = Vec::with_capacity(text.len() + 1);
    let mut offset = 0;
    for c in text.chars() {
        offsets.push(offset);
        offset += c.len_utf16();
    }
    offsets.push(offset);
    offsets
}

fn split_list(params: &HashMap<String, String>, name: &str) -> Vec<String> {
    params
        .get(name)
        .map(|list| {
            list.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Splits an nlprule source like `GRAMMAR/WAS_BEEN/1` into category, rule id and sub id.
fn split_source(source: &str) -> (&str, &str, &str) {
    let mut parts = source.splitn(3, '/');
    let category = parts.next().unwrap_or("");
    let id = parts.next().unwrap_or(category);
    let sub_id = parts.next().unwrap_or("");
    (category, id, sub_id)
}

fn issue_type(category: &str) -> &'static str {
    match category {
        "TYPOS" | "SPELLING" => "misspelling",
        "STYLE" | "REDUNDANCY" | "REPETITIONS_STYLE" => "style",
        "TYPOGRAPHY" | "PUNCTUATION" => "typographical",
        _ => "grammar",
    }
}

pub fn check(state: &State, params: &HashMap<String, String>) -> Result<Value, String> {
    let language = params.get("language").map(String::as_str).unwrap_or("auto");
    let base = language.split('-').next().unwrap_or("");
    if language != "auto" && base != state.language {
        return Err(format!(
            "language '{}' is not supported, this server only checks '{}'",
            language, state.language
        ));
    }

    let input = match (params.get("text"), params.get("data")) {
        (Some(text), _) => Input::from_text(text),
        (None, Some(data)) => Input::from_data(data)?,
        (None, None) => return Err("missing parameter: text or data".into()),
    };

    let disabled_rules = split_list(params, "disabledRules");
    let disabled_categories = split_list(params, "disabledCategories");

    let original_chars: Vec<char> = input.original.chars().collect();
    let utf16 = utf16_offsets(&input.original);
    let mut matches = Vec::new();

    for sentence in state.tokenizer.pipe(&input.plain) {
        for suggestion in state.rules.apply(&sentence) {
            let source = suggestion.source();
            let (category, id, sub_id) = split_source(source);

            if disabled_rules.iter().any(|r| r == id || r == source)
                || disabled_categories.iter().any(|c| c == category)
            {
                continue;
            }

            let span = suggestion.span().char();
            let (start, end) = input.to_original(span.start, span.end);

            let context_start = start.saturating_sub(CONTEXT_CHARS);
            let context_end = (end + CONTEXT_CHARS).min(original_chars.len());
            let context: String = original_chars[context_start..context_end].iter().collect();

            matches.push(json!({
                "message": suggestion.message(),
                "shortMessage": "",
                "replacements": suggestion
                    .replacements()
                    .iter()
                    .map(|r| json!({ "value": r }))
                    .collect::<Vec<_>>(),
                "offset": utf16[start],
                "length": utf16[end] - utf16[start],
                "context": {
                    "text": context,
                    "offset": utf16[start] - utf16[context_start],
                    "length": utf16[end] - utf16[start],
                },
                "sentence": sentence.text().trim(),
                "type": { "typeName": "Other" },
                "rule": {
                    "id": id,
                    "subId": sub_id,
                    "sourceFile": "nlprule",
                    "description": suggestion.message(),
                    "issueType": issue_type(category),
                    "category": { "id": category, "name": category },
                },
                "ignoreForIncompleteSentence": false,
                "contextForSureMatch": 0,
            }));
        }
    }

    Ok(json!({
        "software": {
            "name": "nlprule-server",
            "version": env!("CARGO_PKG_VERSION"),
            "buildDate": "",
            "apiVersion": 1,
            "status": "",
            "premium": false,
        },
        "warnings": { "incompleteResults": false },
        "language": {
            "name": language_name(&state.language),
            "code": state.language,
            "detectedLanguage": {
                "name": language_name(&state.language),
                "code": state.language,
                "confidence": 1.0,
            },
        },
        "matches": matches,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_annotations() {
        let data = r#"{"annotation":[
            {"text": "Un "},
            {"markup": "<b>"},
            {"text": "texto"},
            {"markup": "<br/>", "interpretAs": "\n"},
            {"text": "más"}
        ]}"#;
        let input = Input::from_data(data).unwrap();
        assert_eq!(input.original, "Un <b>texto<br/>más");
        assert_eq!(input.plain, "Un texto\nmás");
        // "texto" is plain chars 3..8, original chars 6..11
        assert_eq!(input.to_original(3, 8), (6, 11));
        // "más" after the <br/>
        assert_eq!(input.to_original(9, 12), (16, 19));
        assert_eq!(input.to_original(9, 9), (16, 16));
    }

    #[test]
    fn rejects_invalid_annotation() {
        assert!(Input::from_data(r#"{"annotation":[{"text":"a","markup":"b"}]}"#).is_err());
        assert!(Input::from_data("nope").is_err());
    }

    #[test]
    fn computes_utf16_offsets() {
        assert_eq!(utf16_offsets("añ😀b"), vec![0, 1, 2, 4, 5]);
    }

    #[test]
    fn splits_sources() {
        assert_eq!(
            split_source("GRAMMAR/WAS_BEEN/1"),
            ("GRAMMAR", "WAS_BEEN", "1")
        );
        assert_eq!(split_source("TYPOS"), ("TYPOS", "TYPOS", ""));
    }
}
