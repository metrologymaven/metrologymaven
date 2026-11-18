//! Evaluator configuration (REQ-ECO.COMM.TIMELINE-005): thresholds, author, maintainers
//! and the bot list, read from JSON or from a bounded TOML subset.
//!
//! Keys: `remind_days`, `close_days` (integers 1..=36500), `author` (string),
//! `maintainers`, `bots` (arrays of strings). Unknown and duplicate keys are rejected.
//! A file whose first non-blank character is `{` is JSON; anything else is read as TOML:
//! comments, bare keys, one-line basic or literal strings, decimal integers and arrays of
//! strings (which may span lines). Tables and other TOML types are rejected. Every error is
//! `TIMELINE-CONFIG`.

use crate::TimelineError;
use fid_canon::CValue;

/// Default reminder threshold in days (REQ-ECO.COMM-005).
pub const DEFAULT_REMIND_DAYS: u32 = 14;
/// Default closure-proposal threshold in days (REQ-ECO.COMM-005).
pub const DEFAULT_CLOSE_DAYS: u32 = 30;
/// Largest threshold accepted, in days (100 years).
pub const MAX_DAYS: u32 = 36_500;
/// Largest config file accepted, in bytes.
pub const MAX_CONFIG_BYTES: usize = 1 << 20;

/// Evaluator configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub remind_days: u32,
    pub close_days: u32,
    /// The PR author's login. When `None`, the actor of the first `opened` event is used.
    pub author: Option<String>,
    /// Maintainer logins. When `None` (key absent), every actor who is neither the author
    /// nor a bot counts as a maintainer; `Some(vec![])` means nobody does.
    pub maintainers: Option<Vec<String>>,
    /// Bot logins: their events never change who the blocker is (REQ-002).
    pub bots: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            remind_days: DEFAULT_REMIND_DAYS,
            close_days: DEFAULT_CLOSE_DAYS,
            author: None,
            maintainers: None,
            bots: Vec::new(),
        }
    }
}

/// Logins compare ASCII case-insensitively, as forge logins do.
pub(crate) fn same_login(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

impl Config {
    /// Whether `login` is on the bot list.
    pub fn is_bot(&self, login: &str) -> bool {
        self.bots.iter().any(|b| same_login(b, login))
    }

    /// Whether `login` counts as a maintainer (see [`Config::maintainers`]). The caller
    /// excludes the author and bots first.
    pub fn is_maintainer(&self, login: &str) -> bool {
        match &self.maintainers {
            None => true,
            Some(list) => list.iter().any(|m| same_login(m, login)),
        }
    }
}

enum Val {
    Str(String),
    Int(i64),
    List(Vec<String>),
}

fn err(msg: impl Into<String>) -> TimelineError {
    TimelineError::config(msg)
}

/// Parses a config file (JSON or the TOML subset, see the module docs).
pub fn parse_config(text: &str) -> Result<Config, TimelineError> {
    if text.len() > MAX_CONFIG_BYTES {
        return Err(err(format!("config is larger than {MAX_CONFIG_BYTES} bytes")));
    }
    let entries = if text.trim_start().starts_with('{') { json_entries(text)? } else { toml_entries(text)? };
    let mut cfg = Config::default();
    let mut seen: Vec<String> = Vec::new();
    for (key, val) in entries {
        if seen.contains(&key) {
            return Err(err(format!("duplicate key {key:?}")));
        }
        seen.push(key.clone());
        match (key.as_str(), val) {
            ("remind_days", Val::Int(n)) => cfg.remind_days = days(&key, n)?,
            ("close_days", Val::Int(n)) => cfg.close_days = days(&key, n)?,
            ("author", Val::Str(s)) => cfg.author = Some(login(&key, s)?),
            ("maintainers", Val::List(l)) => cfg.maintainers = Some(logins(&key, l)?),
            ("bots", Val::List(l)) => cfg.bots = logins(&key, l)?,
            ("remind_days" | "close_days", _) => return Err(err(format!("{key} must be an integer"))),
            ("author", _) => return Err(err("author must be a string")),
            ("maintainers" | "bots", _) => return Err(err(format!("{key} must be an array of strings"))),
            _ => return Err(err(format!("unknown key {key:?}"))),
        }
    }
    Ok(cfg)
}

fn days(key: &str, n: i64) -> Result<u32, TimelineError> {
    match u32::try_from(n) {
        Ok(d) if (1..=MAX_DAYS).contains(&d) => Ok(d),
        _ => Err(err(format!("{key} must be between 1 and {MAX_DAYS}"))),
    }
}

fn login(key: &str, s: String) -> Result<String, TimelineError> {
    if s.trim().is_empty() {
        return Err(err(format!("{key} contains an empty login")));
    }
    Ok(s)
}

fn logins(key: &str, l: Vec<String>) -> Result<Vec<String>, TimelineError> {
    l.into_iter().map(|s| login(key, s)).collect()
}

// ------------------------------------------------------------------ JSON

fn json_entries(text: &str) -> Result<Vec<(String, Val)>, TimelineError> {
    let v = fid_canon::parse(text.as_bytes()).map_err(|e| err(format!("config is not valid JSON ({})", e.code())))?;
    let CValue::Obj(map) = v else {
        return Err(err("JSON config must be an object"));
    };
    let mut out = Vec::new();
    for (k, v) in map {
        let val = match v {
            CValue::Str(s) => Val::Str(s),
            CValue::Num(f) if f.fract() == 0.0 && f.abs() <= 1e15 => {
                // Integral and well inside i64: the conversion is exact.
                Val::Int(f as i64)
            }
            CValue::Arr(items) => {
                let mut l = Vec::new();
                for it in items {
                    let CValue::Str(s) = it else {
                        return Err(err(format!("{k} must be an array of strings")));
                    };
                    l.push(s);
                }
                Val::List(l)
            }
            _ => return Err(err(format!("unsupported value for key {k:?}"))),
        };
        out.push((k, val));
    }
    Ok(out)
}

// ------------------------------------------------------------------ TOML subset

struct Toml<'a> {
    b: &'a [u8],
    text: &'a str,
    pos: usize,
}

impl Toml<'_> {
    fn peek(&self) -> Option<u8> {
        self.b.get(self.pos).copied()
    }

    /// Skips spaces and tabs.
    fn blanks(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t')) {
            self.pos += 1;
        }
    }

    /// Skips a `#` comment up to (not including) the line end.
    fn comment(&mut self) {
        if self.peek() == Some(b'#') {
            while !matches!(self.peek(), None | Some(b'\n')) {
                self.pos += 1;
            }
        }
    }

    /// Skips blanks, comments and line ends.
    fn space(&mut self) {
        loop {
            self.blanks();
            self.comment();
            match self.peek() {
                Some(b'\n') => self.pos += 1,
                Some(b'\r') if self.b.get(self.pos + 1) == Some(&b'\n') => self.pos += 2,
                _ => return,
            }
        }
    }

    fn fail(&self, what: &str) -> TimelineError {
        let line = self.text.get(..self.pos).map_or(0, |s| s.matches('\n').count()) + 1;
        err(format!("TOML line {line}: {what}"))
    }

    fn key(&mut self) -> Result<String, TimelineError> {
        let start = self.pos;
        while matches!(self.peek(), Some(c) if c.is_ascii_alphanumeric() || c == b'_' || c == b'-') {
            self.pos += 1;
        }
        if self.pos == start {
            return Err(self.fail("expected a bare key (tables and quoted keys are not supported)"));
        }
        Ok(self.text.get(start..self.pos).unwrap_or_default().to_string())
    }

    fn string(&mut self) -> Result<String, TimelineError> {
        let quote = self.peek();
        self.pos += 1;
        let mut out = String::new();
        loop {
            let rest = self.text.get(self.pos..).unwrap_or_default();
            let Some(c) = rest.chars().next() else {
                return Err(self.fail("unterminated string"));
            };
            self.pos += c.len_utf8();
            match c {
                '\n' | '\r' => return Err(self.fail("unterminated string")),
                '"' if quote == Some(b'"') => return Ok(out),
                '\'' if quote == Some(b'\'') => return Ok(out),
                '\\' if quote == Some(b'"') => {
                    let e = self.peek();
                    self.pos += 1;
                    out.push(match e {
                        Some(b'"') => '"',
                        Some(b'\\') => '\\',
                        Some(b'n') => '\n',
                        Some(b't') => '\t',
                        _ => return Err(self.fail("unsupported escape in string")),
                    });
                }
                c if c.is_control() && c != '\t' => return Err(self.fail("control character in string")),
                c => out.push(c),
            }
        }
    }

    fn integer(&mut self) -> Result<i64, TimelineError> {
        let start = self.pos;
        if matches!(self.peek(), Some(b'+' | b'-')) {
            self.pos += 1;
        }
        let digits_start = self.pos;
        while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
            self.pos += 1;
        }
        let n = self.pos - digits_start;
        if n == 0 || n > 15 {
            return Err(self.fail("expected a decimal integer of at most 15 digits"));
        }
        self.text.get(start..self.pos).unwrap_or_default().parse::<i64>().map_err(|_| self.fail("bad integer"))
    }

    fn value(&mut self) -> Result<Val, TimelineError> {
        match self.peek() {
            Some(b'"' | b'\'') => Ok(Val::Str(self.string()?)),
            Some(b'[') => {
                self.pos += 1;
                let mut l = Vec::new();
                loop {
                    self.space();
                    match self.peek() {
                        Some(b']') => {
                            self.pos += 1;
                            return Ok(Val::List(l));
                        }
                        Some(b'"' | b'\'') => l.push(self.string()?),
                        _ => return Err(self.fail("arrays may hold only strings")),
                    }
                    self.space();
                    match self.peek() {
                        Some(b',') => self.pos += 1,
                        Some(b']') => {}
                        _ => return Err(self.fail("expected ',' or ']' in array")),
                    }
                }
            }
            Some(b'+' | b'-' | b'0'..=b'9') => Ok(Val::Int(self.integer()?)),
            _ => Err(self.fail("unsupported value (strings, integers and string arrays only)")),
        }
    }
}

fn toml_entries(text: &str) -> Result<Vec<(String, Val)>, TimelineError> {
    let mut p = Toml { b: text.as_bytes(), text, pos: 0 };
    let mut out = Vec::new();
    loop {
        p.space();
        if p.peek().is_none() {
            return Ok(out);
        }
        let key = p.key()?;
        p.blanks();
        if p.peek() != Some(b'=') {
            return Err(p.fail("expected '='"));
        }
        p.pos += 1;
        p.blanks();
        let val = p.value()?;
        p.blanks();
        p.comment();
        match p.peek() {
            None | Some(b'\n') => {}
            Some(b'\r') if p.b.get(p.pos + 1) == Some(&b'\n') => {}
            _ => return Err(p.fail("unexpected text after value")),
        }
        out.push((key, val));
    }
}
