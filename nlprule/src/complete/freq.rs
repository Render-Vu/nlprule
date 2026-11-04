//! Word frequencies used to rank completions.

use crate::Error;
use fst::{Automaton, IntoStreamer, Map, Streamer};
use std::{collections::BTreeMap, io::BufRead, path::Path};

/// Word frequencies stored as a [fst::Map] from lowercase word to count.
///
/// Read from a plain text list with one `word count` pair per line, separated by whitespace
/// (the format of e.g. the [FrequencyWords](https://github.com/hermitdave/FrequencyWords) lists built from
/// OpenSubtitles). Lines which do not parse are skipped, duplicate words (after lowercasing) are summed.
pub struct Frequencies {
    map: Map<Vec<u8>>,
}

impl Frequencies {
    /// Reads frequencies from a file.
    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
        let file = fs_err::File::open(path.as_ref())?;
        Self::from_reader(std::io::BufReader::new(file))
    }

    /// Reads frequencies from a reader (see the type docs for the format).
    pub fn from_reader<R: BufRead>(reader: R) -> Result<Self, Error> {
        let mut counts: BTreeMap<String, u64> = BTreeMap::new();

        for line in reader.lines() {
            let line = line?;
            let mut parts = line.split_whitespace();
            let (word, count) = match (parts.next(), parts.next(), parts.next()) {
                (Some(word), Some(count), None) => (word, count),
                _ => continue,
            };
            if let Ok(count) = count.parse::<u64>() {
                *counts.entry(word.to_lowercase()).or_insert(0) += count;
            }
        }

        Ok(Self::from_sorted(counts))
    }

    /// Builds frequencies from `(word, count)` pairs.
    pub fn from_pairs<I: IntoIterator<Item = (S, u64)>, S: AsRef<str>>(pairs: I) -> Self {
        let mut counts: BTreeMap<String, u64> = BTreeMap::new();
        for (word, count) in pairs {
            *counts.entry(word.as_ref().to_lowercase()).or_insert(0) += count;
        }
        Self::from_sorted(counts)
    }

    fn from_sorted(counts: BTreeMap<String, u64>) -> Self {
        let map = Map::from_iter(counts).expect("BTreeMap keys are sorted and unique");
        Frequencies { map }
    }

    /// The number of words.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Whether there are no words.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// The count of a (lowercase) word, 0 if unknown.
    pub fn get(&self, word: &str) -> u64 {
        self.map.get(word).unwrap_or(0)
    }

    /// Up to `max` words starting with `prefix`, in lexicographic order.
    pub fn with_prefix(&self, prefix: &str, max: usize) -> Vec<(String, u64)> {
        let automaton = fst::automaton::Str::new(prefix).starts_with();
        let mut stream = self.map.search(automaton).into_stream();
        let mut out = Vec::new();

        while let Some((key, value)) = stream.next() {
            if out.len() >= max {
                break;
            }
            if let Ok(word) = std::str::from_utf8(key) {
                out.push((word.to_owned(), value));
            }
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_merges() {
        let text = "de 100\nJuega 5\njuega 3\nmalformed\nx y z\nbad -1\njuegan 2\n";
        let freq = Frequencies::from_reader(text.as_bytes()).unwrap();
        assert_eq!(freq.len(), 3);
        assert_eq!(freq.get("juega"), 8);
        assert_eq!(freq.get("nada"), 0);
        assert_eq!(
            freq.with_prefix("jue", 10),
            vec![("juega".to_string(), 8), ("juegan".to_string(), 2)]
        );
        assert_eq!(freq.with_prefix("jue", 1).len(), 1);
    }
}
