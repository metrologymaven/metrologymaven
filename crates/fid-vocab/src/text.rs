//! Text preparation: NFC normalisation, word tokens, sentence ranges and the whole-word
//! phrase matcher (REQ-ECO.RULES.VOCAB.ENGINE-002).

use std::collections::BTreeMap;
use unicode_normalization::char::is_combining_mark;
use unicode_normalization::UnicodeNormalization;

/// Returns `s` in Unicode Normalization Form C.
pub fn nfc(s: &str) -> String {
    s.nfc().collect()
}

/// One word of a text: byte range in the (NFC) text and its lower-cased form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub start: usize,
    pub end: usize,
    pub folded: String,
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || is_combining_mark(c)
}

/// Splits `text` into maximal runs of letters, digits and combining marks.
pub fn tokenize(text: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for (i, c) in text.char_indices() {
        if is_word_char(c) {
            if start.is_none() {
                start = Some(i);
            }
        } else if let Some(s) = start.take() {
            out.push(Token { start: s, end: i, folded: text[s..i].to_lowercase() });
        }
    }
    if let Some(s) = start {
        out.push(Token { start: s, end: text.len(), folded: text[s..].to_lowercase() });
    }
    out
}

/// Sentence byte ranges: a sentence ends at `.`, `!` or `?` followed by whitespace and an
/// uppercase letter; the next sentence starts at that letter. Always at least one range.
pub fn sentences(text: &str) -> Vec<(usize, usize)> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut out = Vec::new();
    let mut begin = 0;
    let mut i = 0;
    while i < chars.len() {
        let (pos, c) = chars[i];
        if matches!(c, '.' | '!' | '?') {
            let mut j = i + 1;
            while j < chars.len() && chars[j].1.is_whitespace() {
                j += 1;
            }
            if j > i + 1 && j < chars.len() && chars[j].1.is_uppercase() {
                out.push((begin, pos + c.len_utf8()));
                begin = chars[j].0;
                i = j;
                continue;
            }
        }
        i += 1;
    }
    out.push((begin, text.len()));
    out
}

/// Whole-word, case-insensitive matcher for a list of words and phrases. Patterns are
/// tokenised like the text, so a phrase matches a run of consecutive tokens and never
/// part of a longer word.
#[derive(Debug, Clone, Default)]
pub struct Matcher {
    by_first: BTreeMap<String, Vec<Vec<String>>>,
}

impl Matcher {
    /// Builds a matcher; returns the offending pattern if one has no word characters.
    pub fn new(patterns: &[String]) -> Result<Matcher, String> {
        let mut by_first: BTreeMap<String, Vec<Vec<String>>> = BTreeMap::new();
        for p in patterns {
            let norm = nfc(p);
            let toks: Vec<String> = tokenize(&norm).into_iter().map(|t| t.folded).collect();
            let Some(first) = toks.first().cloned() else {
                return Err(p.clone());
            };
            let list = by_first.entry(first).or_default();
            if !list.contains(&toks) {
                list.push(toks);
            }
        }
        for list in by_first.values_mut() {
            // Longest first, then lexicographic, so the longest pattern wins at a position.
            list.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        }
        Ok(Matcher { by_first })
    }

    /// True when the matcher holds no pattern.
    pub fn is_empty(&self) -> bool {
        self.by_first.is_empty()
    }

    /// Non-overlapping matches in `toks`, left to right, as token index ranges `[i, j)`.
    pub fn find(&self, toks: &[Token]) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < toks.len() {
            let mut step = 1;
            if let Some(cands) = self.by_first.get(&toks[i].folded) {
                for cand in cands {
                    let end = i + cand.len();
                    if end <= toks.len() && toks[i..end].iter().zip(cand).all(|(t, c)| &t.folded == c) {
                        out.push((i, end));
                        step = cand.len();
                        break;
                    }
                }
            }
            i += step;
        }
        out
    }
}
