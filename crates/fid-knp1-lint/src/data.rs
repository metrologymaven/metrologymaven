//! Reference-dataset denylist matcher. Spec node `KRN.NUM.LINT.DATA`.
//!
//! [`load_denylist`] reads `rdl-denylist.json` (published by `UNC.RDL.LINT`) and checks its
//! schema and digest; any failure is fatal `KNP1-DATA-DENYLIST`. [`scan_closure`] then lexes
//! every file of the kernel closure with the bounded lexer of `KRN.NUM.LINT.SYN`
//! ([`crate::syn_scan::lex`]) and walks the literal tokens, including those inside macro
//! bodies and invocations:
//!
//! | Rule | Fires for |
//! |---|---|
//! | `KNP1-DATA-ID` | a string, byte-string or C-string literal containing a dataset ID |
//! | `KNP1-DATA-FILE` | such a literal containing a blob file name |
//! | `KNP1-DATA-VALUE` | a numeric literal written with 12 or more significant digits whose exact value equals a denylisted decimal |
//! | `KNP1-DATA-HEX` | a float literal whose IEEE-754 bits, or an integer literal whose value, equals a denylisted hex pattern |
//!
//! Matching is fail closed: IDs and file names are compared ASCII-case-insensitively against
//! both the literal as written and its unescaped text, and against the concatenation of all
//! string literals of one macro invocation (`concat!("trac", "im-…")`); numeric values are
//! compared exactly (no float conversion) on magnitude, so a leading `-` does not hide a value;
//! float bits are compared with the sign bit cleared; an integer literal equal to a denylisted
//! bit pattern is reported wherever it appears, not only as a `from_bits` argument. A literal
//! that matches both `KNP1-DATA-VALUE` and `KNP1-DATA-HEX` yields the `VALUE` finding only (one
//! finding per planted constant). Values in `excluded` never produce a finding (their exact
//! value and their bits). A file the lexer cannot classify is `KNP1-PARSE`.
//!
//! Denylist schema (version 1): an object with exactly `version` (the number 1), `ids` and
//! `files` (unique non-empty strings), `values` (objects with exactly `decimal`, a non-empty list
//! of decimal strings, and `hex`, `0x` plus 16 lowercase hex digits), `excluded` (decimal
//! strings) and `digest`: `sha256:` plus the hex SHA-256 of the RFC 8785 canonical JSON of the
//! object without `digest`. Duplicate keys are rejected.
//!
//! Config (`[data]` of `knp1-lint.toml`): `denylist = "rdl-denylist.json"` (required; relative
//! to the config file).

use crate::findings::{input_digest, sort_findings, Finding, Rule};
use crate::graph::{self, Closure};
use crate::syn_scan::lex::{self, Kind, LitKind, Token};
use fid_canon::CValue;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Read as _;
use std::path::Path;

/// Largest denylist read, in bytes.
pub const MAX_DENYLIST_BYTES: u64 = 64 * 1024 * 1024;
/// Largest source file scanned, in bytes (a larger file is `KNP1-PARSE`).
pub const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
/// Fewest significant digits for `KNP1-DATA-VALUE`.
pub const MIN_SIGNIFICANT_DIGITS: usize = 12;
/// Longest decimal string normalised.
const MAX_DECIMAL_LEN: usize = 1024;
const SIGN_BIT: u64 = 1 << 63;

// ------------------------------------------------------------------ errors

/// Fatal error codes of this module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataErrorCode {
    /// The denylist is missing, malformed, or its digest does not match.
    Denylist,
    /// A listed source file cannot be read.
    Read,
    /// The `[data]` config is malformed.
    ConfigInvalid,
    /// The closure could not be computed (`KRN.NUM.LINT.GRAPH`).
    Graph(graph::GraphErrorCode),
}

impl DataErrorCode {
    /// The stable code string.
    pub fn as_str(self) -> &'static str {
        match self {
            DataErrorCode::Denylist => "KNP1-DATA-DENYLIST",
            DataErrorCode::Read => "KNP1-READ",
            DataErrorCode::ConfigInvalid => "KNP1-CONFIG-INVALID",
            DataErrorCode::Graph(g) => g.as_str(),
        }
    }
}

/// A fatal error of the denylist matcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataError {
    pub code: DataErrorCode,
    pub message: String,
}

impl fmt::Display for DataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for DataError {}

fn deny(m: impl Into<String>) -> DataError {
    DataError { code: DataErrorCode::Denylist, message: m.into() }
}

// ------------------------------------------------------------------ exact decimals

/// An exact non-negative decimal `digits × 10^exp`, `digits` without leading or trailing zeros
/// (`("", 0)` is zero).
pub type Exact = (String, i64);

/// Parses a decimal string (`[+-]digits[.digits][e[+-]digits]`, `_` allowed) exactly.
/// Returns the magnitude and the number of significant digits as written (from the first
/// non-zero digit to the last digit, trailing zeros included). `None` if it is not a decimal.
pub fn normalise_decimal(text: &str) -> Option<(Exact, usize)> {
    // Normalise first (separators, then zero padding), cap after: padding cannot hide a value.
    let s: String = text.chars().filter(|c| *c != '_').collect();
    let s = s.strip_prefix(['+', '-']).unwrap_or(&s);
    let (mant, exp) = match s.find(['e', 'E']) {
        Some(i) => (s.get(..i)?, s.get(i + 1..)?),
        None => (s, ""),
    };
    let (int_part, frac) = match mant.find('.') {
        Some(i) => (mant.get(..i)?, mant.get(i + 1..)?),
        None => (mant, ""),
    };
    if int_part.is_empty() && frac.is_empty() {
        return None;
    }
    if !int_part.bytes().all(|b| b.is_ascii_digit()) || !frac.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut e: i64 = 0;
    if s.contains(['e', 'E']) {
        let digits = exp.strip_prefix(['+', '-']).unwrap_or(exp);
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let digits = digits.trim_start_matches('0');
        if digits.len() > 9 {
            return None; // exponent beyond ±999 999 999: not representable here (the scan reports it)
        }
        e = if digits.is_empty() { 0 } else { digits.parse::<i64>().ok()? };
        if exp.starts_with('-') {
            e = -e;
        }
    }
    let all: String = format!("{int_part}{frac}");
    let e = e - i64::try_from(frac.len()).ok()?;
    let trimmed = all.trim_start_matches('0');
    let sig = trimmed.len();
    let stripped = trimmed.trim_end_matches('0');
    let e = e + i64::try_from(trimmed.len() - stripped.len()).ok()?;
    if stripped.len() > MAX_DECIMAL_LEN {
        return None;
    }
    if stripped.is_empty() {
        return Some(((String::new(), 0), 0));
    }
    Some(((stripped.to_string(), e), sig))
}

/// The `0x…` hex pattern as bits.
fn parse_hex(s: &str) -> Option<u64> {
    let h = s.strip_prefix("0x")?;
    if h.len() != 16 || !h.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        return None;
    }
    u64::from_str_radix(h, 16).ok()
}

// ------------------------------------------------------------------ the denylist

/// One denylisted reference value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DenyValue {
    pub decimal: Vec<String>,
    pub hex: u64,
}

/// A loaded, verified `rdl-denylist.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Denylist {
    pub ids: Vec<String>,
    pub files: Vec<String>,
    pub values: Vec<DenyValue>,
    pub excluded: Vec<String>,
    /// The recorded (and verified) `sha256:` digest.
    pub digest: String,
}

const DENYLIST_KEYS: [&str; 6] = ["digest", "excluded", "files", "ids", "values", "version"];

fn str_list(v: Option<&CValue>, what: &str) -> Result<Vec<String>, DataError> {
    let Some(CValue::Arr(a)) = v else {
        return Err(deny(format!("rdl-denylist.json: `{what}` must be an array of strings")));
    };
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for x in a {
        match x {
            CValue::Str(s) if !s.is_empty() && s.len() <= 4096 && !s.chars().any(char::is_control) => {
                if !seen.insert(s.clone()) {
                    return Err(deny(format!("rdl-denylist.json: `{what}` lists {s:?} twice")));
                }
                out.push(s.clone());
            }
            _ => return Err(deny(format!("rdl-denylist.json: `{what}` must hold non-empty strings"))),
        }
    }
    Ok(out)
}

fn decimal_list(v: Option<&CValue>, what: &str) -> Result<Vec<String>, DataError> {
    let list = str_list(v, what)?;
    if let Some(bad) = list.iter().find(|s| normalise_decimal(s).is_none()) {
        return Err(deny(format!("rdl-denylist.json: `{what}` entry {bad:?} is not a decimal")));
    }
    Ok(list)
}

/// Parses and verifies `rdl-denylist.json` (REQ-001): schema, duplicate keys, digest.
pub fn load_denylist(bytes: &[u8]) -> Result<Denylist, DataError> {
    if bytes.len() as u64 > MAX_DENYLIST_BYTES {
        return Err(deny(format!("rdl-denylist.json: larger than {MAX_DENYLIST_BYTES} bytes")));
    }
    let v = fid_canon::parse_with_limits(bytes, bytes.len(), 16)
        .map_err(|e| deny(format!("rdl-denylist.json: not valid JSON ({e})")))?;
    let CValue::Obj(obj) = v else {
        return Err(deny("rdl-denylist.json: not a JSON object"));
    };
    let keys: Vec<&str> = obj.keys().map(String::as_str).collect();
    if keys != DENYLIST_KEYS {
        return Err(deny(format!(
            "rdl-denylist.json: keys must be exactly {}; found {}",
            DENYLIST_KEYS.join(", "),
            keys.join(", ")
        )));
    }
    match obj.get("version") {
        Some(CValue::Num(n)) if *n == 1.0 => {}
        _ => return Err(deny("rdl-denylist.json: `version` must be 1")),
    }
    let ids = str_list(obj.get("ids"), "ids")?;
    let files = str_list(obj.get("files"), "files")?;
    let excluded = decimal_list(obj.get("excluded"), "excluded")?;
    let Some(CValue::Arr(vals)) = obj.get("values") else {
        return Err(deny("rdl-denylist.json: `values` must be an array"));
    };
    let mut values = Vec::new();
    for (i, x) in vals.iter().enumerate() {
        let CValue::Obj(o) = x else {
            return Err(deny(format!("rdl-denylist.json: values[{i}] is not an object")));
        };
        if o.keys().map(String::as_str).collect::<Vec<_>>() != ["decimal", "hex"] {
            return Err(deny(format!("rdl-denylist.json: values[{i}] must have exactly `decimal` and `hex`")));
        }
        let decimal = decimal_list(o.get("decimal"), &format!("values[{i}].decimal"))?;
        if decimal.is_empty() {
            return Err(deny(format!("rdl-denylist.json: values[{i}].decimal is empty")));
        }
        let hex = match o.get("hex") {
            Some(CValue::Str(h)) => parse_hex(h),
            _ => None,
        }
        .ok_or_else(|| deny(format!("rdl-denylist.json: values[{i}].hex must be 0x plus 16 lowercase hex digits")))?;
        values.push(DenyValue { decimal, hex });
    }
    let digest = match obj.get("digest") {
        Some(CValue::Str(d)) => d.clone(),
        _ => return Err(deny("rdl-denylist.json: `digest` must be a string")),
    };
    let mut body: BTreeMap<String, CValue> = obj.clone();
    body.remove("digest");
    let canonical = fid_canon::to_canonical_bytes(&CValue::Obj(body))
        .map_err(|e| deny(format!("rdl-denylist.json: cannot canonicalise ({e})")))?;
    let actual = input_digest(&canonical);
    if actual != digest {
        return Err(deny(format!("rdl-denylist.json: digest {digest} does not match its content ({actual})")));
    }
    Ok(Denylist { ids, files, values, excluded, digest })
}

/// Lookup sets built from a [`Denylist`].
#[derive(Debug, Clone, Default)]
pub struct Matcher {
    ids: Vec<String>,
    files: Vec<String>,
    values: BTreeSet<Exact>,
    excluded: BTreeSet<Exact>,
    hex_raw: BTreeSet<u64>,
    hex_abs: BTreeSet<u64>,
    excluded_bits: BTreeSet<u64>,
}

impl Matcher {
    /// Builds the lookup sets (IDs and file names lowercased).
    pub fn new(d: &Denylist) -> Matcher {
        let exact = |s: &String| normalise_decimal(s).map(|(e, _)| e);
        let excluded: BTreeSet<Exact> = d.excluded.iter().filter_map(exact).collect();
        let excluded_bits = d
            .excluded
            .iter()
            .filter_map(|s| s.replace('_', "").parse::<f64>().ok())
            .map(|x| x.to_bits() & !SIGN_BIT)
            .collect();
        Matcher {
            ids: d.ids.iter().map(|s| s.to_ascii_lowercase()).collect(),
            files: d.files.iter().map(|s| s.to_ascii_lowercase()).collect(),
            values: d.values.iter().flat_map(|v| v.decimal.iter()).filter_map(exact).collect(),
            excluded,
            hex_raw: d.values.iter().map(|v| v.hex).collect(),
            hex_abs: d.values.iter().map(|v| v.hex & !SIGN_BIT).collect(),
            excluded_bits,
        }
    }

    fn text_rules(&self, text: &str) -> Vec<(&'static str, String)> {
        let lc = text.to_ascii_lowercase();
        let mut out = Vec::new();
        if let Some(id) = self.ids.iter().find(|id| lc.contains(id.as_str())) {
            out.push(("ID", format!("string literal contains dataset ID {id:?}")));
        }
        if let Some(f) = self.files.iter().find(|f| lc.contains(f.as_str())) {
            out.push(("FILE", format!("string literal contains blob file name {f:?}")));
        }
        out
    }

    fn value_hit(&self, exact: &Exact, sig: usize) -> bool {
        sig >= MIN_SIGNIFICANT_DIGITS && self.values.contains(exact) && !self.excluded.contains(exact)
    }

    fn float_bits_hit(&self, bits: u64) -> bool {
        let abs = bits & !SIGN_BIT;
        self.hex_abs.contains(&abs) && !self.excluded_bits.contains(&abs)
    }

    fn int_bits_hit(&self, v: u128) -> bool {
        u64::try_from(v).is_ok_and(|b| self.hex_raw.contains(&b) && !self.excluded_bits.contains(&(b & !SIGN_BIT)))
    }
}

// ------------------------------------------------------------------ literals

/// Unescapes a (non-raw) string literal body; unknown escapes are kept as written.
/// Unescapes a (non-raw) string or character literal body as rustc does. `None` for an escape
/// rustc would reject: `\x` without two hex digits, `\u` without `{`, without `}`, with no or
/// more than six hex digits (`_` allowed and not counted), or naming a value outside
/// `0..=0x10FFFF` or a surrogate; and any other unknown escape. The scan reports such a
/// literal as `KNP1-PARSE`; it is never matched as written.
fn unescape(body: &str) -> Option<String> {
    let mut out = String::new();
    let mut it = body.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next()? {
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            '0' => out.push('\0'),
            '\\' => out.push('\\'),
            '\'' => out.push('\''),
            '"' => out.push('"'),
            'x' => {
                let h: String = it.by_ref().take(2).collect();
                if h.len() != 2 || !h.chars().all(|d| d.is_ascii_hexdigit()) {
                    return None;
                }
                out.push(char::from(u8::from_str_radix(&h, 16).ok()?));
            }
            'u' => {
                if it.next()? != '{' {
                    return None;
                }
                let mut code = String::new();
                loop {
                    match it.next()? {
                        '}' => break,
                        '_' => {}
                        d if d.is_ascii_hexdigit() && code.len() < 6 => code.push(d),
                        _ => return None,
                    }
                }
                if code.is_empty() {
                    return None;
                }
                out.push(char::from_u32(u32::from_str_radix(&code, 16).ok()?)?);
            }
            '\n' => {
                while it.peek().is_some_and(|w| w.is_whitespace()) {
                    it.next();
                }
            }
            _ => return None,
        }
    }
    Some(out)
}

/// The text of a string-like literal as written and unescaped.
fn string_texts(t: &Token) -> Option<(String, String)> {
    match t.kind {
        Kind::Lit { kind: LitKind::Str | LitKind::ByteStr | LitKind::CStr, .. } => {
            Some((t.text.clone(), unescape(&t.text).unwrap_or_default()))
        }
        Kind::Lit { kind: LitKind::RawStr | LitKind::RawByteStr | LitKind::RawCStr, .. } => {
            Some((t.text.clone(), t.text.clone()))
        }
        _ => None,
    }
}

/// An integer literal's value (`None` past `u128`) and its decimal digits as written.
fn int_value(text: &str) -> (Option<u128>, String) {
    let s: String = text.chars().filter(|c| *c != '_').collect();
    let (radix, body) = match s.get(..2) {
        Some("0x") => (16, s.get(2..).unwrap_or_default()),
        Some("0o") => (8, s.get(2..).unwrap_or_default()),
        Some("0b") => (2, s.get(2..).unwrap_or_default()),
        _ => (10, s.as_str()),
    };
    let digits: String = body.chars().take_while(|c| c.is_digit(radix)).collect();
    let v = u128::from_str_radix(&digits, radix).ok();
    let dec = if radix == 10 { digits } else { v.map(|x| x.to_string()).unwrap_or_default() };
    (v, dec)
}

/// A float literal without `_` and without its `f…` suffix, and the suffix.
fn float_text(text: &str) -> (String, String) {
    let s: String = text.chars().filter(|c| *c != '_').collect();
    for sfx in ["f128", "f16", "f32", "f64"] {
        if let Some(b) = s.strip_suffix(sfx) {
            return (b.to_string(), sfx.to_string());
        }
    }
    (s, String::new())
}

// ------------------------------------------------------------------ scanning

/// One file to scan (same shape as the syntax scan's).
pub use crate::syn_scan::FileRef;

fn finding(f: &FileRef<'_>, lines: &[&str], t: &Token, rule: Rule, message: String) -> Finding {
    let excerpt = lines.get((t.line as usize).saturating_sub(1)).copied().unwrap_or_default();
    Finding {
        rule,
        crate_name: f.crate_name.to_string(),
        crate_version: f.crate_version.to_string(),
        file: f.file.to_string(),
        line: t.line,
        column: t.col,
        excerpt_digest: input_digest(excerpt.as_bytes()),
        message,
    }
}

fn parse_finding(f: &FileRef<'_>, line: u32, col: u32, excerpt: &str, message: String) -> Finding {
    Finding {
        rule: Rule::Parse,
        crate_name: f.crate_name.to_string(),
        crate_version: f.crate_version.to_string(),
        file: f.file.to_string(),
        line: line.max(1),
        column: col.max(1),
        excerpt_digest: input_digest(excerpt.as_bytes()),
        message,
    }
}

/// Scans one file. Never fails: a file the lexer cannot classify is `KNP1-PARSE`.
pub fn scan_file(f: FileRef<'_>, bytes: &[u8], m: &Matcher) -> Vec<Finding> {
    if bytes.len() as u64 > MAX_FILE_BYTES {
        let msg = format!("file larger than {MAX_FILE_BYTES} bytes: not scanned");
        return vec![parse_finding(&f, 1, 1, &msg, msg.clone())];
    }
    let Ok(text) = std::str::from_utf8(bytes) else {
        let msg = "file is not valid UTF-8".to_string();
        return vec![parse_finding(&f, 1, 1, &msg, msg.clone())];
    };
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    // As rustc does: CRLF becomes LF before lexing, so `\` + CRLF is a string continuation.
    let normalised = text.replace("\r\n", "\n");
    let text = normalised.as_str();
    let lines: Vec<&str> = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect();
    let toks = match lex::lex(text) {
        Ok(t) => t,
        Err(e) => {
            let excerpt = lines.get((e.line as usize).saturating_sub(1)).copied().unwrap_or(&e.message).to_string();
            return vec![parse_finding(&f, e.line, e.col, &excerpt, format!("cannot lex file: {}", e.message))];
        }
    };
    let mut hits: BTreeMap<(u32, u32, String), Finding> = BTreeMap::new();
    let mut push = |t: &Token, suffix: &str, message: String| {
        let fnd = finding(&f, &lines, t, Rule::Data(suffix.to_string()), message);
        hits.entry((fnd.line, fnd.column, fnd.rule.id())).or_insert(fnd);
    };
    // Matching close delimiter of each `ident ! (` invocation, for concatenated strings.
    let mut stack: Vec<usize> = Vec::new();
    let mut groups: Vec<(usize, usize)> = Vec::new();
    for (i, t) in toks.iter().enumerate() {
        match t.kind {
            Kind::Open(_) => stack.push(i),
            Kind::Close(_) => {
                if let Some(o) = stack.pop() {
                    let is_macro = o >= 2
                        && matches!(toks.get(o - 1).map(|x| &x.kind), Some(Kind::Bang))
                        && matches!(toks.get(o - 2).map(|x| &x.kind), Some(Kind::Ident { .. }));
                    if is_macro {
                        groups.push((o, i));
                    }
                }
            }
            _ => {}
        }
    }
    let mut parse_hits: Vec<&Token> = Vec::new();
    for t in &toks {
        let escaped = matches!(
            t.kind,
            Kind::Lit { kind: LitKind::Str | LitKind::ByteStr | LitKind::CStr | LitKind::Char | LitKind::Byte, .. }
        );
        if escaped && unescape(&t.text).is_none() {
            parse_hits.push(t);
            continue;
        }
        if let Some((raw, unesc)) = string_texts(t) {
            let mut rules = m.text_rules(&raw);
            rules.extend(m.text_rules(&unesc));
            for (sfx, msg) in rules {
                push(t, sfx, msg);
            }
            continue;
        }
        match t.kind {
            Kind::Lit { kind: LitKind::Int, .. } => {
                let (v, dec) = int_value(&t.text);
                let Some((exact, sig)) = normalise_decimal(&dec) else {
                    parse_hits.push(t);
                    continue;
                };
                let value = m.value_hit(&exact, sig);
                if value {
                    push(t, "VALUE", format!("integer literal `{}` equals a reference value", t.text));
                } else if v.is_some_and(|v| m.int_bits_hit(v)) {
                    push(t, "HEX", format!("integer literal `{}` equals a reference value's bit pattern", t.text));
                }
            }
            Kind::Lit { kind: LitKind::Float, .. } => {
                let (num, sfx) = float_text(&t.text);
                let Some((exact, sig)) = normalise_decimal(&num) else {
                    parse_hits.push(t);
                    continue;
                };
                let value = m.value_hit(&exact, sig);
                let bits64 = num.parse::<f64>().ok().map(f64::to_bits);
                let bits32 = if sfx == "f32" { num.parse::<f32>().ok().map(|x| f64::from(x).to_bits()) } else { None };
                if value {
                    push(t, "VALUE", format!("float literal `{}` equals a reference value", t.text));
                } else if bits64.into_iter().chain(bits32).any(|b| m.float_bits_hit(b)) {
                    push(t, "HEX", format!("float literal `{}` has a reference value's bits", t.text));
                }
            }
            _ => {}
        }
    }
    // Literals concatenated by one macro invocation, rendered as `concat!` renders them
    // (`concat!("tracim-lsq-circle-00", 7, ".json")`).
    for (o, c) in groups {
        let parts: Vec<(&Token, String)> =
            toks.get(o + 1..c).unwrap_or_default().iter().filter_map(|t| concat_text(t).map(|u| (t, u))).collect();
        if parts.len() < 2 {
            continue;
        }
        let joined: String = parts.iter().map(|(_, u)| u.as_str()).collect();
        if let Some((first, _)) = parts.first() {
            for (sfx, msg) in m.text_rules(&joined) {
                push(first, sfx, format!("{msg} (strings of one macro invocation, concatenated)"));
            }
        }
    }
    let mut out: Vec<Finding> = hits.into_values().collect();
    for t in parse_hits {
        let excerpt = lines.get((t.line as usize).saturating_sub(1)).copied().unwrap_or_default();
        let msg = format!(
            "literal `{}` cannot be normalised or unescaped exactly",
            t.text.chars().take(80).collect::<String>()
        );
        out.push(parse_finding(&f, t.line, t.col, excerpt, msg));
    }
    sort_findings(&mut out);
    out
}

/// A literal as `concat!` renders it: strings and characters unescaped, integers as their
/// decimal value, floats as written (without `_` or suffix), `true`/`false`, a `-` sign.
fn concat_text(t: &Token) -> Option<String> {
    if let Some((_, u)) = string_texts(t) {
        return Some(u);
    }
    match &t.kind {
        Kind::Lit { kind: LitKind::Char | LitKind::Byte, .. } => Some(unescape(&t.text).unwrap_or_default()),
        Kind::Lit { kind: LitKind::Int, .. } => {
            let (v, dec) = int_value(&t.text);
            Some(v.map_or(dec, |v| v.to_string()))
        }
        Kind::Lit { kind: LitKind::Float, .. } => Some(float_text(&t.text).0),
        Kind::Ident { raw: false } if t.text == "true" || t.text == "false" => Some(t.text.clone()),
        Kind::Other if t.text == "-" => Some("-".to_string()),
        _ => None,
    }
}

fn read_bounded(path: &Path, limit: u64) -> std::io::Result<Vec<u8>> {
    let f = std::fs::File::open(path)?;
    let mut buf = Vec::new();
    f.take(limit + 1).read_to_end(&mut buf)?;
    Ok(buf)
}

/// Scans every listed file of every crate of the closure. Fails with `KNP1-READ` if a listed
/// file cannot be read.
pub fn scan_closure(closure: &Closure, denylist: &Denylist) -> Result<Vec<Finding>, DataError> {
    let m = Matcher::new(denylist);
    let mut out = Vec::new();
    for c in &closure.crates {
        for file in &c.files {
            let bytes = read_bounded(&c.root.join(file), MAX_FILE_BYTES).map_err(|e| DataError {
                code: DataErrorCode::Read,
                message: format!("{} {} {file}: {e}", c.name, c.version),
            })?;
            let fr = FileRef { crate_name: &c.name, crate_version: &c.version, file, crate_files: &c.files };
            out.extend(scan_file(fr, &bytes, &m));
        }
    }
    sort_findings(&mut out);
    Ok(out)
}

/// Reads and verifies the denylist file (a missing file is `KNP1-DATA-DENYLIST`).
pub fn load_denylist_file(path: &Path) -> Result<Denylist, DataError> {
    let bytes =
        read_bounded(path, MAX_DENYLIST_BYTES).map_err(|e| deny(format!("{}: cannot read: {e}", path.display())))?;
    load_denylist(&bytes)
}

/// The outcome of a run: sorted findings and the verified denylist digest for the report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataRun {
    pub findings: Vec<Finding>,
    pub denylist_digest: String,
}

/// Loads `knp1-lint.toml` (`[graph]` and `[data] denylist`), the denylist and the closure, and
/// scans it.
pub fn run(config_path: &Path) -> Result<DataRun, DataError> {
    let invalid = |m: String| DataError { code: DataErrorCode::ConfigInvalid, message: m };
    let text = std::fs::read(config_path)
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
        .ok_or_else(|| invalid(format!("{}: cannot read as UTF-8", config_path.display())))?;
    let table: toml::Table =
        text.parse().map_err(|e: toml::de::Error| invalid(format!("knp1-lint.toml: {}", e.to_string().trim_end())))?;
    let Some(toml::Value::Table(t)) = table.get("data") else {
        return Err(invalid("knp1-lint.toml: missing [data] table".to_string()));
    };
    if let Some(k) = t.keys().find(|k| k.as_str() != "denylist") {
        return Err(invalid(format!("knp1-lint.toml: unknown key data.{k}")));
    }
    let rel = match t.get("denylist") {
        Some(toml::Value::String(s)) if !s.is_empty() => s.clone(),
        _ => return Err(invalid("knp1-lint.toml: data.denylist must name the denylist file".to_string())),
    };
    let dir = config_path.parent().unwrap_or_else(|| Path::new("."));
    let denylist = load_denylist_file(&dir.join(rel))?;
    let g = |e: graph::GraphError| DataError { code: DataErrorCode::Graph(e.code), message: e.message };
    let gcfg = graph::load_config(config_path).map_err(g)?;
    let closure = graph::compute_closure(&gcfg).map_err(g)?;
    let findings = scan_closure(&closure, &denylist)?;
    Ok(DataRun { findings, denylist_digest: denylist.digest })
}
