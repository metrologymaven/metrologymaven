//! `fid-design words-check --copy <jsonl> [--words <json>] [--json]` (spec node
//! `UX.SYSTEM.PRINC`, REQ-UX.SYSTEM.PRINC-004, -005).
//!
//! `reserved-words@1` (the `--words` file) is a JSON array of
//! `{ word, meaning, allowed_prefixes: [..], case_insensitive, inflections?: [..] }`.
//! The UI-copy export (`--copy`) is JSON Lines, one `{ "id": "<message id>", "text": "<English
//! source>" }` per line; blank lines are skipped.
//!
//! Matching is whole-word: the text is split at every character that is not alphanumeric, and
//! each piece is compared with the word and its listed inflections (lowercased when
//! `case_insensitive` is true). A message whose ID does not start with one of the word's
//! `allowed_prefixes` is reported as `WORD-RESERVED <word> <message id>`, once per word and
//! message. Malformed entries of either file are `WORDS-SCHEMA` findings.
//!
//! Exit codes: 0 no findings, 1 findings, 2 unreadable input.

use crate::finding::{findings_text, findings_value, read_text, Finding};
use fid_canon::CValue;
use std::collections::BTreeMap;
use std::path::Path;

/// Report format ID.
pub const REPORT_FORMAT: &str = "fid-design-words-check@1";
/// Format ID of the reserved-word list.
pub const WORDS_FORMAT: &str = "reserved-words@1";
/// Default `--words`, relative to the working directory (the repository root).
pub const DEFAULT_WORDS: &str = "design/reserved-words.json";
/// Largest UI-copy export read (16 MiB).
pub const MAX_COPY_BYTES: u64 = 16 * 1024 * 1024;
/// Largest reserved-word list read.
pub const MAX_WORDS_BYTES: u64 = 1024 * 1024;

pub const RESERVED: &str = "WORD-RESERVED";
pub const SCHEMA: &str = "WORDS-SCHEMA";

const WORD_KEYS: [&str; 5] = ["allowed_prefixes", "case_insensitive", "inflections", "meaning", "word"];

/// One entry of `reserved-words@1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReservedWord {
    pub word: String,
    pub meaning: String,
    pub allowed_prefixes: Vec<String>,
    pub case_insensitive: bool,
    /// The word followed by its inflections, as written in the file.
    pub forms: Vec<String>,
}

impl ReservedWord {
    fn key(&self, s: &str) -> String {
        if self.case_insensitive {
            s.to_lowercase()
        } else {
            s.to_string()
        }
    }

    /// Whether a message ID is under one of the permitted prefixes.
    pub fn allows(&self, id: &str) -> bool {
        self.allowed_prefixes.iter().any(|p| id.starts_with(p.as_str()))
    }
}

/// A parsed reserved-word list with the schema findings it raised.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WordList {
    pub words: Vec<ReservedWord>,
    pub findings: Vec<Finding>,
}

fn is_word(s: &str) -> bool {
    !s.is_empty() && s.chars().all(char::is_alphanumeric)
}

fn str_list(v: Option<&CValue>) -> Option<Vec<String>> {
    match v {
        Some(CValue::Arr(a)) => a
            .iter()
            .map(|x| match x {
                CValue::Str(s) => Some(s.clone()),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

fn parse_entry(i: usize, v: &CValue, f: &mut Vec<Finding>) -> Option<ReservedWord> {
    let subject = format!("words[{i}]");
    let mut bad = |d: String| {
        f.push(Finding::new(SCHEMA, &subject, d));
        None
    };
    let CValue::Obj(m) = v else {
        return bad("entry is not an object".to_string());
    };
    if let Some(k) = m.keys().find(|k| !WORD_KEYS.contains(&k.as_str())) {
        return bad(format!("unknown key `{k}`"));
    }
    let Some(CValue::Str(word)) = m.get("word") else {
        return bad("`word` must be a string".to_string());
    };
    if !is_word(word) {
        return bad(format!("`word` {word:?} must be one non-empty run of letters or digits"));
    }
    let meaning = match m.get("meaning") {
        Some(CValue::Str(s)) if !s.trim().is_empty() => s.clone(),
        _ => return bad(format!("`{word}`: `meaning` must be a non-empty string")),
    };
    let Some(allowed_prefixes) = str_list(m.get("allowed_prefixes")) else {
        return bad(format!("`{word}`: `allowed_prefixes` must be an array of strings"));
    };
    if allowed_prefixes.iter().any(|p| p.is_empty()) {
        return bad(format!("`{word}`: an allowed prefix is empty"));
    }
    let Some(CValue::Bool(case_insensitive)) = m.get("case_insensitive") else {
        return bad(format!("`{word}`: `case_insensitive` must be a boolean"));
    };
    let inflections = match m.get("inflections") {
        None => Vec::new(),
        some => match str_list(some) {
            Some(l) => l,
            None => return bad(format!("`{word}`: `inflections` must be an array of strings")),
        },
    };
    if let Some(x) = inflections.iter().find(|x| !is_word(x)) {
        return bad(format!("`{word}`: inflection {x:?} must be one non-empty run of letters or digits"));
    }
    let mut forms = vec![word.clone()];
    forms.extend(inflections);
    Some(ReservedWord { word: word.clone(), meaning, allowed_prefixes, case_insensitive: *case_insensitive, forms })
}

/// Parses a `reserved-words@1` document. Every defect is a `WORDS-SCHEMA` finding; valid
/// entries are kept.
pub fn parse_words(text: &str) -> WordList {
    let mut f = Vec::new();
    let v = match fid_canon::parse(text.as_bytes()) {
        Ok(v) => v,
        Err(e) => {
            f.push(Finding::new(SCHEMA, "words", format!("not valid {WORDS_FORMAT} JSON: {}", e.code())));
            return WordList { words: Vec::new(), findings: f };
        }
    };
    let CValue::Arr(entries) = v else {
        f.push(Finding::new(SCHEMA, "words", format!("{WORDS_FORMAT} must be a JSON array")));
        return WordList { words: Vec::new(), findings: f };
    };
    let mut words: Vec<ReservedWord> = Vec::new();
    // A form (as matched) may belong to one word only.
    let mut owner: BTreeMap<String, String> = BTreeMap::new();
    for (i, e) in entries.iter().enumerate() {
        let Some(w) = parse_entry(i, e, &mut f) else {
            continue;
        };
        let mut clash = None;
        for form in &w.forms {
            let k = form.to_lowercase();
            match owner.get(&k) {
                Some(o) => {
                    clash = Some(format!("form {form:?} of `{}` is already listed under `{o}`", w.word));
                    break;
                }
                None => {
                    owner.insert(k, w.word.clone());
                }
            }
        }
        if let Some(d) = clash {
            f.push(Finding::new(SCHEMA, format!("words[{i}]"), d));
            continue;
        }
        words.push(w);
    }
    words.sort_by(|a, b| a.word.cmp(&b.word));
    WordList { words, findings: f }
}

/// One message of the UI-copy export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub id: String,
    pub text: String,
    /// 1-based line in the export.
    pub line: usize,
}

/// Parses the UI-copy export. Malformed lines and repeated IDs are `WORDS-SCHEMA` findings.
pub fn parse_copy(text: &str, f: &mut Vec<Finding>) -> Vec<Message> {
    let mut out = Vec::new();
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for (i, raw) in text.lines().enumerate() {
        let line = i + 1;
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        let subject = format!("copy:{line}");
        let obj = match fid_canon::parse(raw.as_bytes()) {
            Ok(CValue::Obj(m)) => m,
            Ok(_) => {
                f.push(Finding::new(SCHEMA, subject, "line is not a JSON object"));
                continue;
            }
            Err(e) => {
                f.push(Finding::new(SCHEMA, subject, format!("not valid JSON: {}", e.code())));
                continue;
            }
        };
        if let Some(k) = obj.keys().find(|k| *k != "id" && *k != "text") {
            f.push(Finding::new(SCHEMA, subject, format!("unknown key `{k}`")));
            continue;
        }
        let (Some(CValue::Str(id)), Some(CValue::Str(text))) = (obj.get("id"), obj.get("text")) else {
            f.push(Finding::new(SCHEMA, subject, "`id` and `text` must both be strings"));
            continue;
        };
        if id.is_empty() {
            f.push(Finding::new(SCHEMA, subject, "`id` is empty"));
            continue;
        }
        if let Some(first) = seen.get(id) {
            f.push(Finding::new(SCHEMA, subject, format!("message id `{id}` repeats line {first}")));
            continue;
        }
        seen.insert(id.clone(), line);
        out.push(Message { id: id.clone(), text: text.clone(), line });
    }
    out
}

/// Result of `words-check`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub words: usize,
    pub messages: usize,
    pub findings: Vec<Finding>,
}

/// Checks UI copy against a reserved-word list held in memory.
pub fn check_text(copy: &str, words_json: &str) -> Report {
    let list = parse_words(words_json);
    let mut f = list.findings;
    let messages = parse_copy(copy, &mut f);
    // Lowercased form -> indices of the words that list it.
    let mut index: BTreeMap<String, usize> = BTreeMap::new();
    for (i, w) in list.words.iter().enumerate() {
        for form in &w.forms {
            index.insert(form.to_lowercase(), i);
        }
    }
    let mut hits: BTreeMap<(String, String), Finding> = BTreeMap::new();
    for m in &messages {
        for tok in m.text.split(|c: char| !c.is_alphanumeric()).filter(|t| !t.is_empty()) {
            let Some(&i) = index.get(&tok.to_lowercase()) else {
                continue;
            };
            let Some(w) = list.words.get(i) else {
                continue;
            };
            let k = w.key(tok);
            if !w.forms.iter().any(|form| w.key(form) == k) || w.allows(&m.id) {
                continue;
            }
            hits.entry((w.word.clone(), m.id.clone())).or_insert_with(|| {
                Finding::new(
                    RESERVED,
                    format!("{} {}", w.word, m.id),
                    format!(
                        "copy line {}: {tok:?} is reserved for: {} Permitted only under: {}",
                        m.line,
                        w.meaning,
                        w.allowed_prefixes.join(", ")
                    ),
                )
            });
        }
    }
    f.extend(hits.into_values());
    f.sort();
    f.dedup();
    Report { words: list.words.len(), messages: messages.len(), findings: f }
}

/// Reads both files and checks them. `Err` is an unreadable input (exit 2).
pub fn check(copy: &Path, words: &Path) -> Result<Report, String> {
    let w = read_text(words, MAX_WORDS_BYTES)?;
    let c = read_text(copy, MAX_COPY_BYTES)?;
    Ok(check_text(&c, &w))
}

impl Report {
    pub fn exit_code(&self) -> u8 {
        u8::from(!self.findings.is_empty())
    }

    pub fn to_cvalue(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("format".to_string(), CValue::Str(REPORT_FORMAT.to_string()));
        m.insert("words".to_string(), CValue::Num(self.words as f64));
        m.insert("messages".to_string(), CValue::Num(self.messages as f64));
        m.insert("findings".to_string(), findings_value(&self.findings));
        m.insert("ok".to_string(), CValue::Bool(self.findings.is_empty()));
        CValue::Obj(m)
    }

    /// RFC 8785 canonical JSON.
    pub fn to_json(&self) -> Result<Vec<u8>, fid_canon::CanonError> {
        fid_canon::to_canonical_bytes(&self.to_cvalue())
    }

    pub fn to_text(&self) -> String {
        let mut s =
            format!("fid-design words-check: {} message(s) against {} reserved word(s)\n", self.messages, self.words);
        findings_text(&mut s, &self.findings);
        s
    }
}
