//! The budget pre-scan: one pass over the raw bytes, before any value is built, that
//! enforces the nesting depth, the string length and unique keys, and reports the RFC 6901
//! pointer of the first offence in input byte order.
//!
//! The scan is lenient about syntax: on anything it cannot follow (an unterminated string,
//! a bad escape, a stray bracket) it stops without an error and leaves the verdict to
//! `fid-canon`'s strict parser. It also answers "which element holds byte `n`" for that
//! parser's syntax errors ([`pointer_at`]).
//!
//! Memory: one frame per open container (at most the depth budget) plus the keys of each
//! open object, so at most a constant times the input size.

use super::error::{push_token, TemplateError, MAX_ERROR_POINTER_BYTES};
use std::collections::BTreeSet;

/// The scan budgets.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Budgets {
    /// Deepest nesting: the root container is depth 1, a scalar adds nothing.
    pub max_depth: usize,
    /// Longest string (keys included), in bytes of decoded UTF-8.
    pub max_string: usize,
}

/// Where the scan is inside one open container.
enum Cur {
    /// Before the first member or element, or between a `,` and the next one.
    None,
    Key(String),
    Index(usize),
}

struct Frame {
    obj: bool,
    /// Index of the next array element.
    next: usize,
    /// An object is waiting for a key (after `{` or `,`).
    want_key: bool,
    cur: Cur,
    keys: BTreeSet<String>,
}

enum Outcome {
    /// The scan reached the end (or the stop byte, or lost the syntax) with no offence.
    Done,
    Offence(&'static str),
}

struct Scanner {
    stack: Vec<Frame>,
}

impl Scanner {
    /// The pointer of the element the scan is in: one token per open container that has
    /// a current member or element.
    fn pointer(&self) -> String {
        let mut out = String::new();
        for f in &self.stack {
            if out.len() > MAX_ERROR_POINTER_BYTES {
                break;
            }
            match &f.cur {
                Cur::None => break,
                Cur::Key(k) => push_token(&mut out, k),
                Cur::Index(i) => push_token(&mut out, &i.to_string()),
            }
        }
        out
    }

    /// A value starts: in an array it becomes the current element.
    fn begin_value(&mut self) {
        if let Some(f) = self.stack.last_mut() {
            if !f.obj && matches!(f.cur, Cur::None) {
                f.cur = Cur::Index(f.next);
            }
        }
    }

    fn run(&mut self, b: &[u8], stop: usize, budgets: Budgets) -> Outcome {
        let end = stop.min(b.len());
        let mut i = 0usize;
        while i < end {
            let Some(&c) = b.get(i) else { return Outcome::Done };
            match c {
                b' ' | b'\t' | b'\n' | b'\r' | b':' => i += 1,
                b'"' => {
                    let Some(close) = string_end(b, i + 1) else { return Outcome::Done };
                    let raw = b.get(i + 1..close).unwrap_or(&[]);
                    let is_key = self.stack.last().is_some_and(|f| f.obj && f.want_key);
                    if is_key {
                        let Some(key) = decode(raw) else { return Outcome::Done };
                        // A stop inside or right after the key reports the member itself.
                        if key.len() > budgets.max_string {
                            if let Some(f) = self.stack.last_mut() {
                                f.cur = Cur::Key(key);
                            }
                            return Outcome::Offence("string_too_long");
                        }
                        let Some(f) = self.stack.last_mut() else { return Outcome::Done };
                        f.want_key = false;
                        if !f.keys.insert(key.clone()) {
                            f.cur = Cur::Key(key);
                            return Outcome::Offence("duplicate_key");
                        }
                        f.cur = Cur::Key(key);
                    } else {
                        self.begin_value();
                        if close >= end {
                            return Outcome::Done;
                        }
                        let Some(len) = decoded_len(raw) else { return Outcome::Done };
                        if len > budgets.max_string {
                            return Outcome::Offence("string_too_long");
                        }
                    }
                    i = close + 1;
                }
                b'{' | b'[' => {
                    self.begin_value();
                    if self.stack.len() + 1 > budgets.max_depth {
                        return Outcome::Offence("too_deep");
                    }
                    self.stack.push(Frame {
                        obj: c == b'{',
                        next: 0,
                        want_key: c == b'{',
                        cur: Cur::None,
                        keys: BTreeSet::new(),
                    });
                    i += 1;
                }
                b'}' | b']' => {
                    if self.stack.pop().is_none() {
                        return Outcome::Done;
                    }
                    i += 1;
                }
                b',' => {
                    let Some(f) = self.stack.last_mut() else { return Outcome::Done };
                    if f.obj {
                        f.want_key = true;
                    } else {
                        f.next = f.next.saturating_add(1);
                    }
                    f.cur = Cur::None;
                    i += 1;
                }
                _ => {
                    // A literal or number: skip to the next delimiter.
                    self.begin_value();
                    // The first byte is never a delimiter (the arms above take those), so
                    // this always advances.
                    while let Some(&d) = b.get(i) {
                        if i >= end
                            || matches!(
                                d,
                                b' ' | b'\t' | b'\n' | b'\r' | b',' | b':' | b'"' | b'{' | b'[' | b'}' | b']'
                            )
                        {
                            break;
                        }
                        i += 1;
                    }
                }
            }
        }
        Outcome::Done
    }
}

/// Checks the budgets over the whole input. `Err` names the first offence in byte order.
pub(crate) fn scan(input: &[u8], budgets: Budgets) -> Result<(), TemplateError> {
    let mut s = Scanner { stack: Vec::new() };
    match s.run(input, input.len(), budgets) {
        Outcome::Done => Ok(()),
        Outcome::Offence(reason) => Err(TemplateError::invalid(&s.pointer(), reason)),
    }
}

/// The pointer of the element that holds byte `at`, for errors that only know an offset.
pub(crate) fn pointer_at(input: &[u8], at: usize, budgets: Budgets) -> String {
    let mut s = Scanner { stack: Vec::new() };
    let _ = s.run(input, at, budgets);
    s.pointer()
}

/// Index of the closing quote of the string whose body starts at `from`.
fn string_end(b: &[u8], from: usize) -> Option<usize> {
    let mut j = from;
    loop {
        match b.get(j)? {
            b'\\' => j += 2,
            b'"' => return Some(j),
            _ => j += 1,
        }
    }
}

fn hex4(raw: &[u8], at: usize) -> Option<u32> {
    let h = raw.get(at..at + 4)?;
    let s = std::str::from_utf8(h).ok()?;
    if !s.bytes().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(s, 16).ok()
}

/// Walks the escapes of a string body, calling `out` with each decoded character or raw
/// byte run. `None` on a malformed escape or a lone surrogate.
fn walk(raw: &[u8], mut out: impl FnMut(Piece<'_>)) -> Option<()> {
    let mut j = 0usize;
    let mut run_start = 0usize;
    while let Some(&c) = raw.get(j) {
        if c != b'\\' {
            j += 1;
            continue;
        }
        out(Piece::Raw(raw.get(run_start..j)?));
        let e = *raw.get(j + 1)?;
        let (ch, used) = match e {
            b'"' => ('"', 2),
            b'\\' => ('\\', 2),
            b'/' => ('/', 2),
            b'b' => ('\u{8}', 2),
            b'f' => ('\u{c}', 2),
            b'n' => ('\n', 2),
            b'r' => ('\r', 2),
            b't' => ('\t', 2),
            b'u' => {
                let hi = hex4(raw, j + 2)?;
                if (0xD800..0xDC00).contains(&hi) {
                    if raw.get(j + 6) != Some(&b'\\') || raw.get(j + 7) != Some(&b'u') {
                        return None;
                    }
                    let lo = hex4(raw, j + 8)?;
                    if !(0xDC00..0xE000).contains(&lo) {
                        return None;
                    }
                    (char::from_u32(0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00))?, 12)
                } else {
                    (char::from_u32(hi)?, 6)
                }
            }
            _ => return None,
        };
        out(Piece::Char(ch));
        j += used;
        run_start = j;
    }
    out(Piece::Raw(raw.get(run_start..)?));
    Some(())
}

enum Piece<'a> {
    Raw(&'a [u8]),
    Char(char),
}

/// The decoded text of a string body (`None` if malformed or not UTF-8).
fn decode(raw: &[u8]) -> Option<String> {
    let mut bytes = Vec::with_capacity(raw.len());
    walk(raw, |p| match p {
        Piece::Raw(r) => bytes.extend_from_slice(r),
        Piece::Char(c) => {
            let mut buf = [0u8; 4];
            bytes.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        }
    })?;
    String::from_utf8(bytes).ok()
}

/// The decoded length in bytes of a string body, without building it.
fn decoded_len(raw: &[u8]) -> Option<usize> {
    let mut n = 0usize;
    walk(raw, |p| match p {
        Piece::Raw(r) => n += r.len(),
        Piece::Char(c) => n += c.len_utf8(),
    })?;
    Some(n)
}
