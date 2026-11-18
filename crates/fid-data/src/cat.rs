//! Dataset catalogue and admission check (spec node `ECO.DATA.CAT`).
//!
//! The catalogue is a TOML file with one `[policy]` table (`redistributable = [...]`,
//! the licence IDs that allow mirroring) and one `[[dataset]]` table per dataset. The
//! check validates every entry, refuses mirrored entries whose licence expression
//! cannot be satisfied from the policy list, entries that declare standards text, and
//! entries that declare customer data without a `release_ref`, and checks each owner
//! against a supplied list of level-1 domain IDs. It reads only the two input files
//! (offline), and its findings are sorted by subject (entry ID) and code (D0).
//!
//! The reader accepts a bounded subset of TOML 1.0 (at most 1 MiB): comments, the two
//! table headers above, bare keys, basic and literal one-line strings, decimal integers,
//! booleans and one-level arrays of those, which may span lines. Anything else is a
//! `DATA-SCHEMA` finding for the whole file.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Read;
use std::path::Path;

/// Largest catalogue or domain-list file the check reads.
pub const MAX_INPUT_BYTES: u64 = 1 << 20;
/// Deepest parenthesis nesting accepted in a licence expression.
pub const MAX_LICENCE_DEPTH: usize = 32;

pub const DATA_SCHEMA: &str = "DATA-SCHEMA";
pub const DATA_DUP: &str = "DATA-DUP";
pub const DATA_REDIST: &str = "DATA-REDIST";
pub const DATA_STANDARDS: &str = "DATA-STANDARDS";
pub const DATA_CUSTOMER: &str = "DATA-CUSTOMER";
pub const DATA_OWNER: &str = "DATA-OWNER";
pub const DATA_IO: &str = "DATA-IO";

/// Subject of findings about a whole file rather than one entry.
pub const FILE_SUBJECT: &str = "-";

/// One problem found by the check. Ordered by subject (the entry ID), then code.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    /// The entry ID, `dataset#<n>` for an entry without a string ID, or `-` for the file.
    pub subject: String,
    pub code: &'static str,
    pub detail: String,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}: {}", self.code, self.subject, self.detail)
    }
}

fn file_finding(code: &'static str, detail: String) -> Vec<Finding> {
    vec![Finding { subject: FILE_SUBJECT.to_string(), code, detail }]
}

// ------------------------------------------------------------------ TOML subset

#[derive(Debug, Clone, PartialEq, Eq)]
enum Value {
    Str(String),
    Int(i64),
    Bool(bool),
    Arr(Vec<Value>),
}

#[derive(Debug, Default)]
struct Table {
    line: usize,
    keys: BTreeMap<String, Value>,
}

#[derive(Debug, Default)]
struct Doc {
    policy: Option<Table>,
    datasets: Vec<Table>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Current {
    None,
    Policy,
    Dataset,
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
    line: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn peek_at(&self, k: usize) -> Option<u8> {
        self.s.get(self.i + k).copied()
    }

    fn err<T>(&self, msg: &str) -> Result<T, String> {
        Err(format!("line {}: {msg}", self.line))
    }

    fn skip_spaces(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t')) {
            self.i += 1;
        }
    }

    fn skip_comment(&mut self) {
        if self.peek() == Some(b'#') {
            while !matches!(self.peek(), None | Some(b'\n')) {
                self.i += 1;
            }
        }
    }

    /// Consumes a line break (`\n` or `\r\n`). Returns false if there is none here.
    fn newline(&mut self) -> Result<bool, String> {
        match self.peek() {
            Some(b'\n') => {
                self.i += 1;
                self.line += 1;
                Ok(true)
            }
            Some(b'\r') if self.peek_at(1) == Some(b'\n') => {
                self.i += 2;
                self.line += 1;
                Ok(true)
            }
            Some(b'\r') => self.err("bare carriage return"),
            _ => Ok(false),
        }
    }

    /// Skips spaces, comments and line breaks.
    fn skip_blank(&mut self) -> Result<(), String> {
        loop {
            self.skip_spaces();
            self.skip_comment();
            if !self.newline()? {
                return Ok(());
            }
        }
    }

    /// After a header or key/value pair: optional spaces and comment, then end of line or file.
    fn end_of_line(&mut self) -> Result<(), String> {
        self.skip_spaces();
        self.skip_comment();
        if self.peek().is_none() || self.newline()? {
            Ok(())
        } else {
            self.err("expected end of line")
        }
    }

    fn bare_key(&mut self) -> Result<String, String> {
        let start = self.i;
        while matches!(self.peek(), Some(b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-')) {
            self.i += 1;
        }
        if start == self.i {
            return self.err("expected a bare key (quoted and dotted keys are not supported)");
        }
        Ok(String::from_utf8_lossy(&self.s[start..self.i]).into_owned())
    }

    fn value(&mut self, in_array: bool) -> Result<Value, String> {
        match self.peek() {
            Some(b'"') => self.basic_string().map(Value::Str),
            Some(b'\'') => self.literal_string().map(Value::Str),
            Some(b'[') if !in_array => self.array(),
            Some(b'[') => self.err("nested arrays are not supported"),
            Some(b't' | b'f' | b'+' | b'-' | b'0'..=b'9') => self.scalar_token(),
            _ => self.err("unsupported value (strings, integers, booleans and arrays only)"),
        }
    }

    fn basic_string(&mut self) -> Result<String, String> {
        if self.peek_at(1) == Some(b'"') && self.peek_at(2) == Some(b'"') {
            return self.err("multi-line strings are not supported");
        }
        self.i += 1;
        let mut out: Vec<u8> = Vec::new();
        loop {
            match self.peek() {
                None | Some(b'\n' | b'\r') => return self.err("unterminated string"),
                Some(b'"') => {
                    self.i += 1;
                    break;
                }
                Some(b'\\') => {
                    let esc = self.peek_at(1);
                    let c = match esc {
                        Some(b'b') => '\u{8}',
                        Some(b't') => '\t',
                        Some(b'n') => '\n',
                        Some(b'f') => '\u{c}',
                        Some(b'r') => '\r',
                        Some(b'"') => '"',
                        Some(b'\\') => '\\',
                        Some(b'u') => self.unicode_escape(4)?,
                        Some(b'U') => self.unicode_escape(8)?,
                        _ => return self.err("invalid escape"),
                    };
                    if !matches!(esc, Some(b'u' | b'U')) {
                        self.i += 2;
                    }
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                }
                Some(b) if (b < 0x20 && b != b'\t') || b == 0x7f => return self.err("control character in string"),
                Some(b) => {
                    out.push(b);
                    self.i += 1;
                }
            }
        }
        String::from_utf8(out).or_else(|_| self.err("string is not UTF-8"))
    }

    /// Reads `\uXXXX` or `\UXXXXXXXX` at the cursor and moves past it.
    fn unicode_escape(&mut self, digits: usize) -> Result<char, String> {
        let start = self.i + 2;
        let hex = self.s.get(start..start + digits).ok_or_else(|| format!("line {}: short escape", self.line))?;
        let text = std::str::from_utf8(hex).map_err(|_| format!("line {}: invalid escape", self.line))?;
        if !text.bytes().all(|b| b.is_ascii_hexdigit()) {
            return self.err("invalid escape");
        }
        let code = u32::from_str_radix(text, 16).map_err(|_| format!("line {}: invalid escape", self.line))?;
        let c = char::from_u32(code).ok_or_else(|| format!("line {}: escape is not a Unicode scalar", self.line))?;
        self.i = start + digits;
        Ok(c)
    }

    fn literal_string(&mut self) -> Result<String, String> {
        if self.peek_at(1) == Some(b'\'') && self.peek_at(2) == Some(b'\'') {
            return self.err("multi-line strings are not supported");
        }
        self.i += 1;
        let start = self.i;
        loop {
            match self.peek() {
                None | Some(b'\n' | b'\r') => return self.err("unterminated string"),
                Some(b'\'') => break,
                Some(b) if (b < 0x20 && b != b'\t') || b == 0x7f => return self.err("control character in string"),
                Some(_) => self.i += 1,
            }
        }
        let text = String::from_utf8(self.s[start..self.i].to_vec()).or_else(|_| self.err("string is not UTF-8"))?;
        self.i += 1;
        Ok(text)
    }

    /// `true`, `false` or a decimal integer, ending at a delimiter.
    fn scalar_token(&mut self) -> Result<Value, String> {
        let start = self.i;
        while !matches!(self.peek(), None | Some(b' ' | b'\t' | b'\r' | b'\n' | b',' | b']' | b'#')) {
            self.i += 1;
        }
        let tok = &self.s[start..self.i];
        match tok {
            b"true" => return Ok(Value::Bool(true)),
            b"false" => return Ok(Value::Bool(false)),
            _ => {}
        }
        let (neg, digits) = match tok.split_first() {
            Some((b'-', rest)) => (true, rest),
            Some((b'+', rest)) => (false, rest),
            _ => (false, tok),
        };
        let well_formed = !digits.is_empty()
            && digits.first().is_some_and(u8::is_ascii_digit)
            && digits.last().is_some_and(u8::is_ascii_digit)
            && digits.iter().all(|b| b.is_ascii_digit() || *b == b'_')
            && !digits.windows(2).any(|w| w == b"__")
            && !(digits.len() > 1 && digits[0] == b'0');
        if !well_formed {
            return self.err("unsupported value (only decimal integers, no floats or dates)");
        }
        let mut n: i64 = 0;
        for b in digits.iter().filter(|b| **b != b'_') {
            let d = i64::from(*b - b'0');
            n = n
                .checked_mul(10)
                .and_then(|n| if neg { n.checked_sub(d) } else { n.checked_add(d) })
                .ok_or_else(|| format!("line {}: integer out of range", self.line))?;
        }
        Ok(Value::Int(n))
    }

    fn array(&mut self) -> Result<Value, String> {
        self.i += 1;
        let mut items = Vec::new();
        loop {
            self.skip_blank()?;
            if self.peek() == Some(b']') {
                self.i += 1;
                return Ok(Value::Arr(items));
            }
            items.push(self.value(true)?);
            self.skip_blank()?;
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b']') => {
                    self.i += 1;
                    return Ok(Value::Arr(items));
                }
                _ => return self.err("expected , or ] in array"),
            }
        }
    }

    fn header(&mut self) -> Result<(Current, usize), String> {
        let line = self.line;
        let double = self.peek_at(1) == Some(b'[');
        self.i += if double { 2 } else { 1 };
        self.skip_spaces();
        let name = self.bare_key()?;
        self.skip_spaces();
        let close: &[u8] = if double { b"]]" } else { b"]" };
        if !self.s[self.i..].starts_with(close) {
            return self.err("malformed table header");
        }
        self.i += close.len();
        self.end_of_line()?;
        match (name.as_str(), double) {
            ("policy", false) => Ok((Current::Policy, line)),
            ("dataset", true) => Ok((Current::Dataset, line)),
            _ => Err(format!("line {line}: unknown table {name} (expected [policy] or [[dataset]])")),
        }
    }

    fn document(&mut self) -> Result<Doc, String> {
        let mut doc = Doc::default();
        let mut cur = Current::None;
        loop {
            self.skip_blank()?;
            if self.peek().is_none() {
                return Ok(doc);
            }
            if self.peek() == Some(b'[') {
                let (which, line) = self.header()?;
                match which {
                    Current::Policy if doc.policy.is_some() => {
                        return Err(format!("line {line}: [policy] defined twice"));
                    }
                    Current::Policy => doc.policy = Some(Table { line, keys: BTreeMap::new() }),
                    Current::Dataset => doc.datasets.push(Table { line, keys: BTreeMap::new() }),
                    Current::None => {}
                }
                cur = which;
                continue;
            }
            let line = self.line;
            let key = self.bare_key()?;
            self.skip_spaces();
            if self.peek() != Some(b'=') {
                return self.err("expected =");
            }
            self.i += 1;
            self.skip_spaces();
            let value = self.value(false)?;
            self.end_of_line()?;
            let table = match cur {
                Current::Policy => doc.policy.as_mut(),
                Current::Dataset => doc.datasets.last_mut(),
                Current::None => None,
            };
            let Some(table) = table else {
                return Err(format!("line {line}: key {key} outside [policy] or [[dataset]]"));
            };
            if table.keys.insert(key.clone(), value).is_some() {
                return Err(format!("line {line}: key {key} defined twice"));
            }
        }
    }
}

fn parse_doc(text: &str) -> Result<Doc, String> {
    Parser { s: text.as_bytes(), i: 0, line: 1 }.document()
}

// ------------------------------------------------------------------ licence expressions

/// A parsed SPDX licence expression. `A WITH B` is kept as one term named `A WITH B`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Licence {
    Term(String),
    And(Box<Licence>, Box<Licence>),
    Or(Box<Licence>, Box<Licence>),
}

impl Licence {
    /// True when the expression can be satisfied using only `allowed` terms:
    /// `OR` needs any side, `AND` needs both, a term must be listed exactly.
    pub fn satisfiable(&self, allowed: &BTreeSet<String>) -> bool {
        match self {
            Licence::Term(t) => allowed.contains(t),
            Licence::And(a, b) => a.satisfiable(allowed) && b.satisfiable(allowed),
            Licence::Or(a, b) => a.satisfiable(allowed) || b.satisfiable(allowed),
        }
    }

    /// All terms, left to right.
    pub fn terms(&self) -> Vec<&str> {
        let mut out = Vec::new();
        let mut stack = vec![self];
        while let Some(e) = stack.pop() {
            match e {
                Licence::Term(t) => out.push(t.as_str()),
                Licence::And(a, b) | Licence::Or(a, b) => {
                    stack.push(b);
                    stack.push(a);
                }
            }
        }
        out
    }
}

fn is_licence_id(t: &str) -> bool {
    !t.is_empty() && t.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'+' | b'-'))
}

const OPERATORS: [&str; 3] = ["AND", "OR", "WITH"];

struct LicParser<'a> {
    toks: Vec<&'a str>,
    pos: usize,
}

impl<'a> LicParser<'a> {
    fn peek(&self) -> Option<&'a str> {
        self.toks.get(self.pos).copied()
    }

    fn id(&mut self) -> Option<&'a str> {
        let t = self.peek()?;
        if t == "(" || t == ")" || OPERATORS.contains(&t) || !is_licence_id(t) {
            return None;
        }
        self.pos += 1;
        Some(t)
    }

    fn atom(&mut self, depth: usize) -> Option<Licence> {
        if self.peek() == Some("(") {
            if depth >= MAX_LICENCE_DEPTH {
                return None;
            }
            self.pos += 1;
            let e = self.disjunction(depth + 1)?;
            if self.peek() != Some(")") {
                return None;
            }
            self.pos += 1;
            return Some(e);
        }
        let name = self.id()?;
        if self.peek() == Some("WITH") {
            self.pos += 1;
            let ex = self.id()?;
            return Some(Licence::Term(format!("{name} WITH {ex}")));
        }
        Some(Licence::Term(name.to_string()))
    }

    fn conjunction(&mut self, depth: usize) -> Option<Licence> {
        let mut e = self.atom(depth)?;
        while self.peek() == Some("AND") {
            self.pos += 1;
            e = Licence::And(Box::new(e), Box::new(self.atom(depth)?));
        }
        Some(e)
    }

    fn disjunction(&mut self, depth: usize) -> Option<Licence> {
        let mut e = self.conjunction(depth)?;
        while self.peek() == Some("OR") {
            self.pos += 1;
            e = Licence::Or(Box::new(e), Box::new(self.conjunction(depth)?));
        }
        Some(e)
    }
}

/// Parses an SPDX licence expression (`AND` binds tighter than `OR`; parentheses; `WITH`).
/// Operators must be upper case. Returns `None` if the expression is malformed.
pub fn parse_licence(text: &str) -> Option<Licence> {
    let mut toks = Vec::new();
    let mut start: Option<usize> = None;
    for (i, c) in text.char_indices() {
        if c == '(' || c == ')' || c.is_whitespace() {
            if let Some(s) = start.take() {
                toks.push(&text[s..i]);
            }
            if c == '(' || c == ')' {
                toks.push(&text[i..i + 1]);
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        toks.push(&text[s..]);
    }
    let mut p = LicParser { toks, pos: 0 };
    let e = p.disjunction(0)?;
    if p.pos == p.toks.len() {
        Some(e)
    } else {
        None
    }
}

// ------------------------------------------------------------------ the check

const REQUIRED_STRINGS: [&str; 7] = ["id", "title", "source", "licence", "sha256", "owner", "redistribution"];
const OPTIONAL_FIELDS: [&str; 7] =
    ["terms_url", "size_bytes", "urls", "notes", "contains_standards_text", "contains_customer_data", "release_ref"];

fn is_entry_id(s: &str) -> bool {
    let b = s.as_bytes();
    (3..=64).contains(&b.len())
        && matches!(b[0], b'a'..=b'z' | b'0'..=b'9')
        && b.iter().all(|c| matches!(c, b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-'))
}

fn is_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// `http://` or `https://` followed by at least one character and no whitespace.
fn is_url(s: &str) -> bool {
    let rest = s.strip_prefix("https://").or_else(|| s.strip_prefix("http://"));
    rest.is_some_and(|r| !r.is_empty() && !r.chars().any(char::is_whitespace))
}

fn is_domain_id(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty() && b[0].is_ascii_uppercase() && b.iter().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

fn nonempty_str<'a>(t: &'a Table, k: &str) -> Option<&'a str> {
    match t.keys.get(k) {
        Some(Value::Str(s)) if !s.is_empty() => Some(s),
        _ => None,
    }
}

fn policy_list(policy: Option<&Table>) -> Result<BTreeSet<String>, String> {
    let Some(p) = policy else {
        return Err("missing [policy] table".into());
    };
    if let Some(k) = p.keys.keys().find(|k| k.as_str() != "redistributable") {
        return Err(format!("line {}: unknown policy key {k}", p.line));
    }
    let Some(Value::Arr(items)) = p.keys.get("redistributable") else {
        return Err(format!("line {}: policy needs redistributable = [...]", p.line));
    };
    let mut out = BTreeSet::new();
    for v in items {
        match v {
            Value::Str(s) if !s.is_empty() => {
                out.insert(s.clone());
            }
            _ => return Err(format!("line {}: redistributable items must be non-empty strings", p.line)),
        }
    }
    Ok(out)
}

/// Checks one entry. Returns its findings except `DATA-DUP`.
fn check_entry(t: &Table, subject: &str, allowed: &BTreeSet<String>, domains: &BTreeSet<String>) -> Vec<Finding> {
    let mut bad: Vec<&str> = Vec::new();
    for k in t.keys.keys() {
        if !REQUIRED_STRINGS.contains(&k.as_str()) && !OPTIONAL_FIELDS.contains(&k.as_str()) {
            bad.push(k);
        }
    }
    for k in REQUIRED_STRINGS {
        if nonempty_str(t, k).is_none() {
            bad.push(k);
        }
    }
    if nonempty_str(t, "id").is_some_and(|s| !is_entry_id(s)) {
        bad.push("id");
    }
    if nonempty_str(t, "source").is_some_and(|s| !is_url(s)) {
        bad.push("source");
    }
    if !matches!(t.keys.get("size_bytes"), Some(Value::Int(n)) if *n >= 0) {
        bad.push("size_bytes");
    }
    if nonempty_str(t, "sha256").is_some_and(|s| !is_sha256(s)) {
        bad.push("sha256");
    }
    let redistribution = nonempty_str(t, "redistribution");
    if redistribution.is_some_and(|r| r != "mirror" && r != "fetch-only") {
        bad.push("redistribution");
    }
    let urls_ok = match t.keys.get("urls") {
        Some(Value::Arr(u)) => !u.is_empty() && u.iter().all(|v| matches!(v, Value::Str(s) if is_url(s))),
        _ => false,
    };
    if !urls_ok {
        bad.push("urls");
    }
    for k in ["notes", "release_ref", "terms_url"] {
        if t.keys.contains_key(k) && nonempty_str(t, k).is_none() {
            bad.push(k);
        }
    }
    for k in ["contains_standards_text", "contains_customer_data"] {
        if t.keys.get(k).is_some_and(|v| !matches!(v, Value::Bool(_))) {
            bad.push(k);
        }
    }
    let licence_text = nonempty_str(t, "licence");
    let licence = licence_text.and_then(parse_licence);
    if licence_text.is_some() && licence.is_none() {
        bad.push("licence");
    }
    if let Some(l) = &licence {
        let has_ref = l.terms().iter().any(|x| x.starts_with("LicenseRef-"));
        if has_ref && !nonempty_str(t, "terms_url").is_some_and(is_url) {
            bad.push("terms_url");
        }
    }

    let at = |code: &'static str, detail: String| Finding {
        subject: subject.to_string(),
        code,
        detail: format!("line {}: {detail}", t.line),
    };
    let mut out = Vec::new();
    if !bad.is_empty() {
        bad.sort_unstable();
        bad.dedup();
        out.push(at(DATA_SCHEMA, format!("missing or invalid: {}", bad.join(", "))));
    }
    if let Some(owner) = nonempty_str(t, "owner") {
        if !domains.contains(owner) {
            out.push(at(DATA_OWNER, format!("owner {owner} is not a level-1 domain ID")));
        }
    }
    if let (Some(l), Some("mirror")) = (&licence, redistribution) {
        if !l.satisfiable(allowed) {
            out.push(at(
                DATA_REDIST,
                format!(
                    "licence {} does not allow mirroring under the policy; mark it fetch-only",
                    licence_text.unwrap_or_default()
                ),
            ));
        }
    }
    if t.keys.get("contains_standards_text") == Some(&Value::Bool(true)) {
        out.push(at(DATA_STANDARDS, "declares standards text".into()));
    }
    if t.keys.get("contains_customer_data") == Some(&Value::Bool(true)) && nonempty_str(t, "release_ref").is_none()
    {
        out.push(at(DATA_CUSTOMER, "declares customer data without a release_ref".into()));
    }
    out
}

/// Checks catalogue text against a set of level-1 domain IDs. Returns sorted findings;
/// an empty list means the catalogue is admitted.
pub fn check(catalogue: &str, domains: &BTreeSet<String>) -> Vec<Finding> {
    if catalogue.len() as u64 > MAX_INPUT_BYTES {
        return file_finding(DATA_IO, format!("catalogue is larger than {MAX_INPUT_BYTES} bytes"));
    }
    let doc = match parse_doc(catalogue) {
        Ok(d) => d,
        Err(e) => return file_finding(DATA_SCHEMA, e),
    };
    let allowed = match policy_list(doc.policy.as_ref()) {
        Ok(a) => a,
        Err(e) => return file_finding(DATA_SCHEMA, e),
    };
    let mut out: BTreeSet<Finding> = BTreeSet::new();
    let mut lines_by_id: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (n, t) in doc.datasets.iter().enumerate() {
        let subject = match t.keys.get("id") {
            Some(Value::Str(s)) => {
                lines_by_id.entry(s.as_str()).or_default().push(t.line);
                s.clone()
            }
            _ => format!("dataset#{}", n + 1),
        };
        out.extend(check_entry(t, &subject, &allowed, domains));
    }
    for (id, lines) in lines_by_id {
        if lines.len() > 1 {
            let at: Vec<String> = lines.iter().map(usize::to_string).collect();
            out.insert(Finding {
                subject: id.to_string(),
                code: DATA_DUP,
                detail: format!("id used by the entries at lines {}", at.join(", ")),
            });
        }
    }
    out.into_iter().collect()
}

/// Parses a domain list: one level-1 domain ID per line (`KRN`, `I18N`); blank lines and
/// lines starting with `#` are ignored.
pub fn parse_domains(text: &str) -> Result<BTreeSet<String>, String> {
    let mut out = BTreeSet::new();
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if !is_domain_id(line) {
            return Err(format!("domain list line {}: {line:?} is not a level-1 domain ID", i + 1));
        }
        out.insert(line.to_string());
    }
    Ok(out)
}

/// Reads a UTF-8 text file of at most [`MAX_INPUT_BYTES`].
pub fn read_bounded(path: &Path) -> Result<String, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(MAX_INPUT_BYTES + 1).read_to_end(&mut bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    if bytes.len() as u64 > MAX_INPUT_BYTES {
        return Err(format!("{}: larger than {MAX_INPUT_BYTES} bytes", path.display()));
    }
    String::from_utf8(bytes).map_err(|_| format!("{}: not UTF-8", path.display()))
}

/// Reads both files and checks the catalogue. Unreadable or oversized inputs and a bad
/// domain list are `DATA-IO` findings.
pub fn check_files(catalogue: &Path, domains: &Path) -> Vec<Finding> {
    let domains = match read_bounded(domains).and_then(|t| parse_domains(&t)) {
        Ok(d) => d,
        Err(e) => return file_finding(DATA_IO, e),
    };
    match read_bounded(catalogue) {
        Ok(text) => check(&text, &domains),
        Err(e) => file_finding(DATA_IO, e),
    }
}
