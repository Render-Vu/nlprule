//! Inference of the part-of-speech tags expected at the cursor from the preceding words.
//!
//! The rules here are written for the EAGLES tag set used by the Spanish dictionary
//! (e.g. `DA0MP0` = determiner, article, masculine plural; `VMIP3P0` = main verb,
//! indicative present, 3rd person plural). They encode simple agreement facts:
//!
//! - determiner / adjective → noun or adjective with the same gender and number,
//! - noun → agreeing adjective, or a verb in 3rd person with the same number,
//! - subject pronoun → verb with the same person and number,
//! - clitic pronoun → finite verb,
//! - conditional "si" before the verb → boost for the imperfect subjunctive.

use serde::Serialize;

/// Gender and number values which agree with anything ("common", "invariable", "unspecified").
const WILDCARDS: &str = "CN0";

#[derive(Debug, Clone, PartialEq)]
enum Part {
    /// Any character.
    Any,
    /// Exactly this character.
    Eq(char),
    /// One of these characters.
    OneOf(&'static str),
    /// Agrees with this gender / number value: equal, or one of both is a wildcard.
    Agree(char),
}

impl Part {
    fn matches(&self, c: char) -> bool {
        match self {
            Part::Any => true,
            Part::Eq(x) => *x == c,
            Part::OneOf(set) => set.contains(c),
            Part::Agree(x) => *x == c || WILDCARDS.contains(*x) || WILDCARDS.contains(c),
        }
    }

    fn label(&self) -> String {
        match self {
            Part::Any => "?".into(),
            Part::Eq(x) | Part::Agree(x) => x.to_string(),
            Part::OneOf(set) => format!("[{}]", set),
        }
    }
}

/// A pattern over a positional (EAGLES-style) part-of-speech tag.
/// Matches every tag which is at least as long as the pattern and whose characters match position by position.
#[derive(Debug, Clone, PartialEq)]
pub struct TagPattern {
    parts: Vec<Part>,
}

impl TagPattern {
    fn new(parts: Vec<Part>) -> Self {
        TagPattern { parts }
    }

    /// Builds a pattern from a user-given tag prefix. `?` and `*` are wildcards, e.g. `VMIP3P` or `V??P3P`.
    pub fn from_prefix(prefix: &str) -> Self {
        TagPattern::new(
            prefix
                .chars()
                .map(|c| {
                    if c == '?' || c == '*' {
                        Part::Any
                    } else {
                        Part::Eq(c)
                    }
                })
                .collect(),
        )
    }

    /// Whether the given tag matches this pattern.
    pub fn matches(&self, tag: &str) -> bool {
        let mut chars = tag.chars();
        self.parts
            .iter()
            .all(|part| chars.next().map_or(false, |c| part.matches(c)))
    }

    /// A human-readable representation, e.g. `NCFP*` or `V?[IS]?3P*`.
    pub fn label(&self) -> String {
        let mut out: String = self.parts.iter().map(Part::label).collect();
        out.push('*');
        out
    }

    fn noun(gender: char, number: char) -> Self {
        use Part::*;
        TagPattern::new(vec![Eq('N'), Eq('C'), Agree(gender), Agree(number)])
    }

    fn adjective(gender: char, number: char) -> Self {
        use Part::*;
        TagPattern::new(vec![
            Eq('A'),
            OneOf("QO"),
            Any,
            Agree(gender),
            Agree(number),
        ])
    }

    fn finite_verb(person: char, number: char) -> Self {
        use Part::*;
        let number = if WILDCARDS.contains(number) {
            Any
        } else {
            Eq(number)
        };
        TagPattern::new(vec![Eq('V'), Any, OneOf("IS"), Any, Eq(person), number])
    }

    fn any_finite_verb() -> Self {
        use Part::*;
        TagPattern::new(vec![Eq('V'), Any, OneOf("IS")])
    }

    fn imperfect_subjunctive() -> Self {
        use Part::*;
        TagPattern::new(vec![Eq('V'), Any, Eq('S'), Eq('I')])
    }
}

/// What the context says about the next word.
#[derive(Debug, Clone, Default)]
pub struct Expectation {
    /// Candidates matching any of these patterns fit the context.
    pub patterns: Vec<TagPattern>,
    /// The weight of each pattern in `patterns` (higher = more likely reading of the context).
    pub weights: Vec<u8>,
    /// Candidates matching these patterns get an additional score boost.
    pub boosts: Vec<TagPattern>,
}

/// A summary of the inferred expectation, used in the output.
#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct ContextInfo {
    /// Labels of the patterns a candidate should match.
    pub expected: Vec<String>,
    /// Labels of the boosted patterns.
    pub boosted: Vec<String>,
}

impl Expectation {
    /// Whether nothing is known about the next word.
    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }

    /// The highest weight of a pattern matching one of the tags, 0 if none matches.
    pub fn weight<S: AsRef<str>>(&self, tags: &[S]) -> u8 {
        self.patterns
            .iter()
            .zip(&self.weights)
            .filter(|(p, _)| tags.iter().any(|t| p.matches(t.as_ref())))
            .map(|(_, w)| *w)
            .max()
            .unwrap_or(0)
    }

    /// Labels of the inferred patterns.
    pub fn info(&self) -> ContextInfo {
        ContextInfo {
            expected: self.patterns.iter().map(TagPattern::label).collect(),
            boosted: self.boosts.iter().map(TagPattern::label).collect(),
        }
    }

    fn push(&mut self, pattern: TagPattern, weight: u8) {
        if let Some(i) = self.patterns.iter().position(|p| *p == pattern) {
            self.weights[i] = self.weights[i].max(weight);
        } else {
            self.patterns.push(pattern);
            self.weights.push(weight);
        }
    }
}

/// Whether a tag looks like an EAGLES tag (as opposed to internal disambiguation markers
/// like `_GN_MP`, `SENT_END` or `LOC_PREP`).
pub fn is_eagles_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag != "UNKNOWN"
        && tag
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

fn nth(tag: &str, i: usize) -> Option<char> {
    tag.chars().nth(i)
}

fn is_word(text: &str) -> bool {
    text.chars().any(char::is_alphanumeric)
}

/// Infers the expected tags of the next word from the preceding tokens of the same sentence.
///
/// Each token is given as `(text, tags)`; non-EAGLES tags are ignored, so the tags of a
/// disambiguated nlprule token can be passed as they are.
pub fn infer<S: AsRef<str>, T: AsRef<str>>(tokens: &[(S, Vec<T>)]) -> Expectation {
    let words: Vec<_> = tokens
        .iter()
        .filter(|(text, _)| is_word(text.as_ref()))
        .collect();

    let mut expectation = Expectation::default();

    let (_, last_tags) = match words.last() {
        Some(last) => last,
        None => return expectation,
    };

    let tags: Vec<&str> = last_tags
        .iter()
        .map(AsRef::as_ref)
        .filter(|tag| is_eagles_tag(tag))
        .collect();
    let has = |a: char, b: Option<char>| {
        tags.iter()
            .any(|t| nth(t, 0) == Some(a) && b.map_or(true, |b| nth(t, 1) == Some(b)))
    };
    let is_determiner = has('D', None);
    let is_noun = has('N', Some('C'));

    // weights: 2 = main reading, 1 = secondary reading
    for tag in &tags {
        let c = |i| nth(tag, i).unwrap_or('0');

        match (c(0), c(1)) {
            // determiner: D type person gender number
            ('D', _) => {
                expectation.push(TagPattern::noun(c(3), c(4)), 2);
                expectation.push(TagPattern::adjective(c(3), c(4)), 2);
            }
            // common noun: N C gender number
            ('N', 'C') => {
                expectation.push(TagPattern::adjective(c(2), c(3)), 2);
                expectation.push(TagPattern::finite_verb('3', c(3)), 2);
            }
            // proper noun
            ('N', 'P') => {
                expectation.push(TagPattern::finite_verb('3', 'S'), 2);
            }
            // adjective: A type degree gender number. If the word can also be a noun
            // ("los niños"), the noun reading wins and no second noun is expected.
            ('A', _) => {
                if !is_noun {
                    expectation.push(TagPattern::noun(c(3), c(4)), 2);
                }
                expectation.push(TagPattern::adjective(c(3), c(4)), 1);
                expectation.push(TagPattern::finite_verb('3', c(4)), 1);
            }
            // personal pronoun: P P person gender number case
            ('P', 'P') => match c(5) {
                'A' | 'D' => expectation.push(TagPattern::any_finite_verb(), 1),
                _ if "123".contains(c(2)) => {
                    expectation.push(TagPattern::finite_verb(c(2), c(4)), 2)
                }
                _ => {}
            },
            // copula "ser": V S mood tense person number. Expects an adjective agreeing
            // with the closest preceding noun phrase of the same number ("las niñas son bonitas").
            ('V', 'S') if "123".contains(c(4)) => {
                let number = c(5);
                let gender = words[..words.len() - 1]
                    .iter()
                    .rev()
                    .flat_map(|(_, tags)| tags.iter().map(AsRef::as_ref))
                    .find_map(|t: &str| match (nth(t, 0), nth(t, 1)) {
                        (Some('N'), Some('C')) if nth(t, 3) == Some(number) => nth(t, 2),
                        (Some('D'), _) if nth(t, 4) == Some(number) => nth(t, 3),
                        _ => None,
                    })
                    .unwrap_or('0');
                expectation.push(TagPattern::adjective(gender, number), 2);
            }
            // other pronouns (demonstrative, indefinite, ...): 3rd person.
            // Usually a determiner if the word can be one ("una", "esta").
            ('P', 'D') | ('P', 'I') | ('P', 'X') => {
                let weight = if is_determiner { 1 } else { 2 };
                expectation.push(TagPattern::finite_verb('3', c(4)), weight);
            }
            _ => {}
        }
    }

    let has_verb_expectation = expectation
        .patterns
        .iter()
        .any(|p| p.parts.first() == Some(&Part::Eq('V')));
    let after_si = words
        .iter()
        .rev()
        .take(3)
        .any(|(text, _)| text.as_ref().to_lowercase() == "si");

    if has_verb_expectation && after_si {
        expectation.boosts.push(TagPattern::imperfect_subjunctive());
    }

    expectation
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(input: &[(&'static str, &[&'static str])]) -> Vec<(&'static str, Vec<&'static str>)> {
        input.iter().map(|(w, t)| (*w, t.to_vec())).collect()
    }

    fn matches_any(e: &Expectation, tag: &str) -> bool {
        e.patterns.iter().any(|p| p.matches(tag))
    }

    #[test]
    fn pattern_from_prefix() {
        let p = TagPattern::from_prefix("VMIP3P");
        assert!(p.matches("VMIP3P0"));
        assert!(!p.matches("VMIP3S0"));
        assert!(!p.matches("VMI"));
        assert!(TagPattern::from_prefix("V??P3P").matches("VSIP3P0"));
    }

    #[test]
    fn eagles_filter() {
        assert!(is_eagles_tag("NCMP000"));
        assert!(is_eagles_tag("RG"));
        assert!(!is_eagles_tag("_GN_MP"));
        assert!(!is_eagles_tag("SENT_END"));
        assert!(!is_eagles_tag(""));
        assert!(!is_eagles_tag("UNKNOWN"));
    }

    #[test]
    fn noun_phrase_expects_verb_or_adjective() {
        let e = infer(&tokens(&[
            ("Los", &["DA0MP0", "_GN_MP"]),
            ("niños", &["AQ0MP0", "NCMP000", "SENT_END"]),
        ]));
        assert!(matches_any(&e, "VMIP3P0")); // juegan
        assert!(matches_any(&e, "VMSP3P0")); // jueguen
        assert!(!matches_any(&e, "VMIP3S0")); // juega
        assert!(matches_any(&e, "AQ0MP0")); // contentos
        assert!(matches_any(&e, "AQ0CP0")); // felices
        assert!(!matches_any(&e, "AQ0FP0"));
        // "niños" is a noun here, not an adjective before a noun
        assert!(!matches_any(&e, "NCCP000"));
        assert!(e.boosts.is_empty());
    }

    #[test]
    fn determiner_expects_agreeing_noun() {
        let e = infer(&tokens(&[("las", &["PP3FPA00", "DA0FP0"])]));
        assert!(matches_any(&e, "NCFP000"));
        assert!(matches_any(&e, "NCCP000"));
        assert!(!matches_any(&e, "NCMS000"));
        assert!(matches_any(&e, "AQ0FP0"));
        // clitic reading, secondary
        assert!(matches_any(&e, "VMIP3S0"));
        assert_eq!(e.weight(&["NCFP000"]), 2);
        assert_eq!(e.weight(&["VMIP3S0"]), 1);
        assert_eq!(e.weight(&["RG"]), 0);
    }

    #[test]
    fn subject_pronoun_expects_verb() {
        let e = infer(&tokens(&[("si", &["CS"]), ("nosotros", &["PP1MP000"])]));
        assert!(matches_any(&e, "VMSI1P0"));
        assert!(matches_any(&e, "VMIP1P0"));
        assert!(!matches_any(&e, "VMIP3P0"));
        assert_eq!(e.boosts.len(), 1);
        assert!(e.boosts[0].matches("VMSI1P0"));
        assert!(!e.boosts[0].matches("VMIP1P0"));
    }

    #[test]
    fn punctuation_is_skipped_and_empty_context_is_empty() {
        let e = infer(&tokens(&[("Yo", &["PP1CSN00"]), (",", &["Fc"])]));
        assert!(matches_any(&e, "VMIP1S0"));
        assert!(infer::<&str, &str>(&[]).is_empty());
    }

    #[test]
    fn copula_expects_agreeing_adjective() {
        let e = infer(&tokens(&[
            ("Las", &["DA0FP0"]),
            ("niñas", &["NCFP000"]),
            ("son", &["VSIP3P0"]),
        ]));
        assert!(matches_any(&e, "AQ0FP0"));
        assert!(matches_any(&e, "AQ0CP0"));
        assert!(!matches_any(&e, "AQ0MP0"));
        assert!(!matches_any(&e, "AQ0FS0"));
    }

    #[test]
    fn labels() {
        assert_eq!(TagPattern::noun('F', 'P').label(), "NCFP*");
        assert_eq!(TagPattern::finite_verb('3', 'P').label(), "V?[IS]?3P*");
    }
}
