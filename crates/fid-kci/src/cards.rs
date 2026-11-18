//! Algorithm IDs and spec cards. Spec node `KRN.MAN.CARD`.
//!
//! Every kernel algorithm has an ID `<module>.<feature>.<method>@<major>.<minor>.<patch>`
//! and a spec card that says what it does, where it applies, which references it
//! follows (designator and clause only, never standards text), its determinism class,
//! its known failure modes and its validation evidence.
//!
//! Cards are written in a strict TOML subset (`key = "string"` and
//! `key = ["string", ...]`, `#` comments). The design names a TOML crate at build time;
//! a small hand-written reader keeps this kernel crate dependency-free (KNP-1).

use std::cmp::Ordering;
use std::fmt;

/// Longest allowed free-text field, in characters (REQ-KRN.MAN.CARD-004).
pub const MAX_FIELD_CHARS: usize = 2000;
/// A quoted passage longer than this many words, in a card that cites a standard,
/// is treated as suspected standards text (REQ-KRN.MAN.CARD-004).
pub const MAX_QUOTE_WORDS: usize = 15;

/// A versioned algorithm ID. Orders by module, feature, method, then version.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AlgorithmId {
    pub module: String,
    pub feature: String,
    pub method: String,
    pub version: (u32, u32, u32),
}

impl Ord for AlgorithmId {
    fn cmp(&self, other: &Self) -> Ordering {
        (&self.module, &self.feature, &self.method, self.version).cmp(&(
            &other.module,
            &other.feature,
            &other.method,
            other.version,
        ))
    }
}

impl PartialOrd for AlgorithmId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for AlgorithmId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (a, b, c) = self.version;
        write!(f, "{}.{}.{}@{a}.{b}.{c}", self.module, self.feature, self.method)
    }
}

fn segment_ok(s: &str) -> bool {
    let mut b = s.bytes();
    matches!(b.next(), Some(c) if c.is_ascii_lowercase())
        && b.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
}

fn version_part(s: &str) -> Option<u32> {
    if s.is_empty() || !s.bytes().all(|c| c.is_ascii_digit()) || (s.len() > 1 && s.starts_with('0')) {
        return None;
    }
    s.parse().ok()
}

impl AlgorithmId {
    /// Parses an ID exactly (REQ-KRN.MAN.CARD-001).
    pub fn parse(s: &str) -> Result<Self, CardError> {
        let bad = || CardError::BadId(s.to_string());
        let (name, version) = s.split_once('@').ok_or_else(bad)?;
        let segs: Vec<&str> = name.split('.').collect();
        let vers: Vec<&str> = version.split('.').collect();
        if segs.len() != 3 || vers.len() != 3 || !segs.iter().all(|x| segment_ok(x)) {
            return Err(bad());
        }
        let v: Vec<u32> = vers.iter().map(|x| version_part(x)).collect::<Option<_>>().ok_or_else(bad)?;
        Ok(AlgorithmId {
            module: segs[0].to_string(),
            feature: segs[1].to_string(),
            method: segs[2].to_string(),
            version: (v[0], v[1], v[2]),
        })
    }
}

/// A parsed spec card (REQ-KRN.MAN.CARD-002).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecCard {
    pub id: AlgorithmId,
    pub title: String,
    pub method: String,
    pub applicability: String,
    pub references: Vec<String>,
    pub determinism: String,
    pub failure_modes: Vec<String>,
    pub validation: String,
}

/// Card and ID errors, each with a stable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CardError {
    BadId(String),
    MissingField(&'static str),
    UnknownField(String),
    BadDeterminism(String),
    Syntax { line: usize, why: &'static str },
    TooLong { field: String },
    StandardsTextSuspected { field: String },
}

impl CardError {
    /// Stable code.
    pub fn code(&self) -> &'static str {
        match self {
            CardError::BadId(_) => "bad_id",
            CardError::MissingField(_) => "missing_field",
            CardError::UnknownField(_) => "unknown_field",
            CardError::BadDeterminism(_) => "bad_determinism",
            CardError::Syntax { .. } => "syntax",
            CardError::TooLong { .. } => "too_long",
            CardError::StandardsTextSuspected { .. } => "standards_text_suspected",
        }
    }
}

impl fmt::Display for CardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CardError::BadId(s) => write!(f, "bad_id: {s:?}"),
            CardError::MissingField(k) => write!(f, "missing_field: {k}"),
            CardError::UnknownField(k) => write!(f, "unknown_field: {k}"),
            CardError::BadDeterminism(s) => write!(f, "bad_determinism: {s:?}"),
            CardError::Syntax { line, why } => write!(f, "syntax: line {line}: {why}"),
            CardError::TooLong { field } => write!(f, "too_long: {field} exceeds {MAX_FIELD_CHARS} characters"),
            CardError::StandardsTextSuspected { field } => {
                write!(f, "standards_text_suspected: {field} quotes more than {MAX_QUOTE_WORDS} words")
            }
        }
    }
}

impl std::error::Error for CardError {}

#[derive(Debug)]
enum Val {
    Str(String),
    List(Vec<String>),
}

/// Reads one basic string starting at `s[0] == '"'`; returns the value and the rest.
fn read_string(s: &str, line: usize) -> Result<(String, &str), CardError> {
    let err = |why| CardError::Syntax { line, why };
    let mut out = String::new();
    let mut chars = s.char_indices().skip(1);
    while let Some((i, c)) = chars.next() {
        match c {
            '"' => return Ok((out, &s[i + 1..])),
            '\\' => match chars.next() {
                Some((_, '"')) => out.push('"'),
                Some((_, '\\')) => out.push('\\'),
                Some((_, 'n')) => out.push('\n'),
                Some((_, 't')) => out.push('\t'),
                _ => return Err(err("unsupported escape")),
            },
            c => out.push(c),
        }
    }
    Err(err("unterminated string"))
}

fn parse_toml_subset(text: &str) -> Result<Vec<(String, Val, usize)>, CardError> {
    let mut out: Vec<(String, Val, usize)> = Vec::new();
    let mut lines = text.lines().enumerate().map(|(i, l)| (i + 1, l));
    while let Some((n, raw)) = lines.next() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, rest) = line.split_once('=').ok_or(CardError::Syntax { line: n, why: "expected key = value" })?;
        let key = key.trim();
        if key.is_empty() || !key.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') {
            return Err(CardError::Syntax { line: n, why: "bad key" });
        }
        if out.iter().any(|(k, _, _)| k == key) {
            return Err(CardError::Syntax { line: n, why: "duplicate key" });
        }
        let rest = rest.trim();
        if rest.starts_with('"') {
            let (v, tail) = read_string(rest, n)?;
            if !tail.trim().is_empty() && !tail.trim().starts_with('#') {
                return Err(CardError::Syntax { line: n, why: "text after string" });
            }
            out.push((key.to_string(), Val::Str(v), n));
        } else if let Some(first) = rest.strip_prefix('[') {
            let mut acc = first.to_string();
            let mut last_line = n;
            let (items, tail) = loop {
                if let Some(done) = scan_array(&acc, last_line)? {
                    break done;
                }
                let (m, next) = lines.next().ok_or(CardError::Syntax { line: n, why: "unterminated array" })?;
                last_line = m;
                acc.push('\n');
                acc.push_str(next);
            };
            if !tail.trim().is_empty() && !tail.trim().starts_with('#') {
                return Err(CardError::Syntax { line: last_line, why: "text after array" });
            }
            out.push((key.to_string(), Val::List(items), n));
        } else {
            return Err(CardError::Syntax { line: n, why: "value must be a string or an array of strings" });
        }
    }
    Ok(out)
}

/// Scans array items (possibly over several lines). Returns `None` when the closing `]`
/// has not been seen yet, otherwise the items and the text after `]`.
fn scan_array(text: &str, line: usize) -> Result<Option<(Vec<String>, String)>, CardError> {
    let mut items = Vec::new();
    let mut s = text;
    // After an item, only `,` or `]` may follow (comments and whitespace aside).
    let mut need_separator = false;
    loop {
        s = s.trim_start();
        if s.is_empty() {
            return Ok(None);
        }
        if s.starts_with('#') {
            match s.find('\n') {
                Some(i) => {
                    s = &s[i..];
                    continue;
                }
                None => return Ok(None),
            }
        }
        if let Some(after) = s.strip_prefix(']') {
            return Ok(Some((items, after.to_string())));
        }
        if need_separator {
            match s.strip_prefix(',') {
                Some(after) => {
                    s = after;
                    need_separator = false;
                    continue;
                }
                None => return Err(CardError::Syntax { line, why: "expected , or ] after array item" }),
            }
        }
        if !s.starts_with('"') {
            return Err(CardError::Syntax { line, why: "array items must be strings" });
        }
        let (v, tail) = read_string(s, line)?;
        items.push(v);
        s = tail;
        need_separator = true;
    }
}

/// Quotation mark pairs searched for quoted passages: ASCII double quotes, curly double
/// and single quotes, low-high double quotes and guillemets (REQ-KRN.MAN.CARD-004).
const QUOTE_PAIRS: [(char, char); 6] = [
    ('"', '"'),
    ('\u{201c}', '\u{201d}'),
    ('\u{2018}', '\u{2019}'),
    ('\u{201e}', '\u{201c}'),
    ('\u{ab}', '\u{bb}'),
    ('\u{bb}', '\u{ab}'),
];
const FIELDS: [&str; 8] =
    ["id", "title", "method", "applicability", "references", "determinism", "failure_modes", "validation"];
const STANDARD_MARKERS: [&str; 10] = ["ISO", "ASME", "VDA", "VDI", "DIN", "JCGM", "IEC", "ANSI", "BS EN", "NIST"];

/// Parses and checks one card (REQ-KRN.MAN.CARD-002, -004).
pub fn parse_card(text: &str) -> Result<SpecCard, CardError> {
    let kv = parse_toml_subset(text)?;
    for (k, _, _) in &kv {
        if !FIELDS.contains(&k.as_str()) {
            return Err(CardError::UnknownField(k.clone()));
        }
    }
    let get_str = |name: &'static str| -> Result<String, CardError> {
        match kv.iter().find(|(k, _, _)| k == name) {
            Some((_, Val::Str(s), _)) if !s.trim().is_empty() => Ok(s.clone()),
            Some((_, _, line)) => Err(CardError::Syntax { line: *line, why: "expected a non-empty string" }),
            None => Err(CardError::MissingField(name)),
        }
    };
    let get_list = |name: &'static str| -> Result<Vec<String>, CardError> {
        match kv.iter().find(|(k, _, _)| k == name) {
            Some((_, Val::List(v), _)) if !v.is_empty() => Ok(v.clone()),
            Some((_, _, line)) => Err(CardError::Syntax { line: *line, why: "expected a non-empty array of strings" }),
            None => Err(CardError::MissingField(name)),
        }
    };
    // Report missing fields in declaration order, so each invalid card names its gap.
    for f in FIELDS {
        if !kv.iter().any(|(k, _, _)| k == f) {
            return Err(CardError::MissingField(f));
        }
    }
    let card = SpecCard {
        id: AlgorithmId::parse(&get_str("id")?)?,
        title: get_str("title")?,
        method: get_str("method")?,
        applicability: get_str("applicability")?,
        references: get_list("references")?,
        determinism: get_str("determinism")?,
        failure_modes: get_list("failure_modes")?,
        validation: get_str("validation")?,
    };
    if fid_types::Determinism::parse(&card.determinism).is_err() {
        return Err(CardError::BadDeterminism(card.determinism));
    }
    check_text(&card)?;
    Ok(card)
}

fn quoted_passages(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    // ASCII single quotes are left out: apostrophes would pair up across ordinary prose.
    for (open, close) in QUOTE_PAIRS {
        let mut rest = s;
        while let Some(a) = rest.find(open) {
            let after = &rest[a + open.len_utf8()..];
            match after.find(close) {
                Some(b) => {
                    out.push(after[..b].to_string());
                    rest = &after[b + close.len_utf8()..];
                }
                None => break,
            }
        }
    }
    out
}

fn check_text(card: &SpecCard) -> Result<(), CardError> {
    let mut fields: Vec<(String, &str)> = vec![
        ("title".into(), card.title.as_str()),
        ("method".into(), card.method.as_str()),
        ("applicability".into(), card.applicability.as_str()),
        ("validation".into(), card.validation.as_str()),
    ];
    fields.extend(card.references.iter().enumerate().map(|(i, r)| (format!("references[{i}]"), r.as_str())));
    fields.extend(card.failure_modes.iter().enumerate().map(|(i, r)| (format!("failure_modes[{i}]"), r.as_str())));
    let cites_standard = card.references.iter().any(|r| STANDARD_MARKERS.iter().any(|m| r.contains(m)));
    for (name, text) in &fields {
        if text.chars().count() > MAX_FIELD_CHARS {
            return Err(CardError::TooLong { field: name.clone() });
        }
        let attributed = cites_standard || STANDARD_MARKERS.iter().any(|m| text.contains(m));
        if attributed && quoted_passages(text).iter().any(|q| q.split_whitespace().count() > MAX_QUOTE_WORDS) {
            return Err(CardError::StandardsTextSuspected { field: name.clone() });
        }
    }
    Ok(())
}

/// Result of matching the algorithm table against the card registry (REQ-KRN.MAN.CARD-003).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CheckReport {
    /// Algorithms without a card, sorted.
    pub missing_cards: Vec<AlgorithmId>,
    /// Cards without an algorithm, sorted.
    pub orphan_cards: Vec<AlgorithmId>,
}

impl CheckReport {
    /// True when both lists are empty; CI fails otherwise.
    pub fn is_clean(&self) -> bool {
        self.missing_cards.is_empty() && self.orphan_cards.is_empty()
    }
}

/// Compares the algorithm table with the card IDs.
pub fn check(table_ids: &[AlgorithmId], card_ids: &[AlgorithmId]) -> CheckReport {
    let mut missing: Vec<AlgorithmId> = table_ids.iter().filter(|a| !card_ids.contains(a)).cloned().collect();
    let mut orphans: Vec<AlgorithmId> = card_ids.iter().filter(|c| !table_ids.contains(c)).cloned().collect();
    missing.sort();
    missing.dedup();
    orphans.sort();
    orphans.dedup();
    CheckReport { missing_cards: missing, orphan_cards: orphans }
}
