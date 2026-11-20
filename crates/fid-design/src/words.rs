//! `fid-design words-check --copy <jsonl> [--words <json>] [--json]` (spec node
//! `UX.SYSTEM.PRINC`, REQ-UX.SYSTEM.PRINC-004, -005).
//!
//! `reserved-words@1` (the `--words` file) is a JSON array of
//! `{ word, meaning, allowed_prefixes: [..], case_insensitive: true, inflections?: [..] }`.
//! Every prefix ends in `.`; `case_insensitive` must be `true`. The list must name at least the
//! five words of REQ-004 (approve, conforms, release, sign, verified).
//!
//! The UI-copy export (`--copy`) is JSON Lines, one `{ "id": "<message id>", "text": "<English
//! source>" }` per line; blank lines are skipped, message IDs use only `[a-z0-9._]`, and an
//! export with no message is a finding.
//!
//! Matching is whole-word and case-insensitive: Turkish `İ` and `ı` are mapped to `i`, the text
//! is split at every character that is not alphanumeric, and each lowercased piece is compared
//! with the word and its listed inflections. A message whose ID does not start with one of the
//! word's `allowed_prefixes` is reported as `WORD-RESERVED <word> <message id>`, once per word
//! and message.
//!
//! The check fails closed on text that could hide a reserved word from that comparison:
//! `WORDS-OBFUSCATED <message id>` for any invisible or formatting character (U+00AD, U+034F,
//! U+200B..U+200F, U+202A..U+202E, U+2060..U+206F, U+FEFF), any compatibility form (U+FB00..
//! U+FB4F, U+FF00..U+FFEF, U+1D400..U+1D7FF), or a word mixing Latin letters with Cyrillic or
//! Greek. Ordinary accented Latin letters are not flagged. Malformed entries of either file are
//! `WORDS-SCHEMA` findings.
//!
//! Exit codes: 0 no findings, 1 findings, 2 unreadable input.

use crate::finding::{findings_text, findings_value, read_text, Finding};
use fid_canon::CValue;
use std::collections::{BTreeMap, BTreeSet};
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
/// Words every list must contain (REQ-UX.SYSTEM.PRINC-004).
pub const REQUIRED_WORDS: [&str; 5] = ["approve", "conforms", "release", "sign", "verified"];

pub const RESERVED: &str = "WORD-RESERVED";
pub const OBFUSCATED: &str = "WORDS-OBFUSCATED";
pub const SCHEMA: &str = "WORDS-SCHEMA";

const WORD_KEYS: [&str; 5] = ["allowed_prefixes", "case_insensitive", "inflections", "meaning", "word"];

/// One entry of `reserved-words@1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReservedWord {
    pub word: String,
    pub meaning: String,
    pub allowed_prefixes: Vec<String>,
    /// The word followed by its inflections, as written in the file.
    pub forms: Vec<String>,
}

impl ReservedWord {
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

/// The comparison key of a piece of text: Turkish dotted capital I and dotless i become `i`,
/// then the text is lowercased.
pub fn fold(s: &str) -> String {
    s.chars().map(|c| if c == '\u{0130}' || c == '\u{0131}' { 'i' } else { c }).collect::<String>().to_lowercase()
}

fn is_word(s: &str) -> bool {
    !s.is_empty() && s.chars().all(char::is_alphanumeric)
}

fn valid_id(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'_')
}

/// Invisible or formatting characters that can split or hide a word.
fn is_invisible(c: char) -> bool {
    matches!(c, '\u{00AD}' | '\u{034F}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{206F}' | '\u{FEFF}')
}

/// Compatibility forms (ligatures, fullwidth, mathematical alphanumerics).
fn is_compat(c: char) -> bool {
    matches!(c, '\u{FB00}'..='\u{FB4F}' | '\u{FF00}'..='\u{FFEF}' | '\u{1D400}'..='\u{1D7FF}')
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Script {
    Latin,
    Greek,
    Cyrillic,
}

fn script(c: char) -> Option<Script> {
    match c {
        'a'..='z' | 'A'..='Z' | '\u{00C0}'..='\u{024F}' | '\u{1E00}'..='\u{1EFF}' => Some(Script::Latin),
        '\u{0370}'..='\u{03FF}' | '\u{1F00}'..='\u{1FFF}' => Some(Script::Greek),
        '\u{0400}'..='\u{052F}' | '\u{1C80}'..='\u{1C8F}' | '\u{2DE0}'..='\u{2DFF}' | '\u{A640}'..='\u{A69F}' => {
            Some(Script::Cyrillic)
        }
        _ => None,
    }
}

/// Whether a word mixes Latin letters with Cyrillic or Greek ones.
fn mixed_script(tok: &str) -> bool {
    let scripts: Vec<Script> = tok.chars().filter_map(script).collect();
    scripts.contains(&Script::Latin) && scripts.iter().any(|s| *s != Script::Latin)
}

/// The reasons a message text could hide a reserved word, sorted; empty when clean.
pub fn obfuscation(text: &str) -> Vec<String> {
    let mut why = BTreeSet::new();
    for c in text.chars() {
        if is_invisible(c) {
            why.insert(format!("invisible or formatting character U+{:04X}", u32::from(c)));
        } else if is_compat(c) {
            why.insert(format!("compatibility form U+{:04X}", u32::from(c)));
        }
    }
    for tok in text.split(|c: char| !c.is_alphanumeric()).filter(|t| mixed_script(t)) {
        why.insert(format!("{tok:?} mixes Latin with Cyrillic or Greek letters"));
    }
    why.into_iter().collect()
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
    if let Some(p) = allowed_prefixes.iter().find(|p| p.len() < 2 || !p.ends_with('.') || !valid_id(p)) {
        return bad(format!("`{word}`: prefix {p:?} must use `[a-z0-9._]` and end in `.`"));
    }
    match m.get("case_insensitive") {
        Some(CValue::Bool(true)) => {}
        _ => return bad(format!("`{word}`: `case_insensitive` must be true")),
    }
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
    Some(ReservedWord { word: word.clone(), meaning, allowed_prefixes, forms })
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
    if entries.is_empty() {
        f.push(Finding::new(SCHEMA, "words", "the reserved-word list is empty"));
        return WordList { words: Vec::new(), findings: f };
    }
    let mut words: Vec<ReservedWord> = Vec::new();
    // A form (as matched) may belong to one word only.
    let mut owner: BTreeMap<String, String> = BTreeMap::new();
    for (i, e) in entries.iter().enumerate() {
        let Some(w) = parse_entry(i, e, &mut f) else {
            continue;
        };
        let mut clash = None;
        let mut mine = Vec::new();
        for form in &w.forms {
            let k = fold(form);
            if let Some(o) = owner.get(&k) {
                clash = Some(format!("form {form:?} of `{}` is already listed under `{o}`", w.word));
                break;
            }
            if mine.contains(&k) {
                clash = Some(format!("form {form:?} of `{}` is listed twice", w.word));
                break;
            }
            mine.push(k);
        }
        if let Some(d) = clash {
            f.push(Finding::new(SCHEMA, format!("words[{i}]"), d));
            continue;
        }
        for k in mine {
            owner.insert(k, w.word.clone());
        }
        words.push(w);
    }
    let missing: Vec<&str> =
        REQUIRED_WORDS.iter().copied().filter(|r| !words.iter().any(|w| fold(&w.word) == *r)).collect();
    if !missing.is_empty() {
        f.push(Finding::new(SCHEMA, "words", format!("required word(s) missing or invalid: {}", missing.join(", "))));
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

/// Parses the UI-copy export. Malformed lines, repeated IDs and an export without any valid
/// message are `WORDS-SCHEMA` findings.
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
        if !valid_id(id) {
            f.push(Finding::new(SCHEMA, subject, format!("message id {id:?} must be non-empty `[a-z0-9._]`")));
            continue;
        }
        if let Some(first) = seen.get(id) {
            f.push(Finding::new(SCHEMA, subject, format!("message id `{id}` repeats line {first}")));
            continue;
        }
        seen.insert(id.clone(), line);
        out.push(Message { id: id.clone(), text: text.clone(), line });
    }
    if out.is_empty() {
        f.push(Finding::new(SCHEMA, "copy", "the UI-copy export has no messages"));
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
    // Folded form -> index of the word that lists it.
    let mut index: BTreeMap<String, usize> = BTreeMap::new();
    for (i, w) in list.words.iter().enumerate() {
        for form in &w.forms {
            index.insert(fold(form), i);
        }
    }
    let mut hits: BTreeMap<(String, String), Finding> = BTreeMap::new();
    for m in &messages {
        let why = obfuscation(&m.text);
        if !why.is_empty() {
            f.push(Finding::new(OBFUSCATED, &m.id, format!("copy line {}: {}", m.line, why.join("; "))));
        }
        let folded = fold(&m.text);
        for tok in folded.split(|c: char| !c.is_alphanumeric()).filter(|t| !t.is_empty()) {
            let Some(w) = index.get(tok).and_then(|&i| list.words.get(i)) else {
                continue;
            };
            if w.allows(&m.id) {
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
