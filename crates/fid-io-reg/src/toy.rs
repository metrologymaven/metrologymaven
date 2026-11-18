//! A toy recursive-descent reader built on the harness. It exists for the tests (AC-1)
//! and the `limits_toy_reader` fuzz target (AC-5), and as a pattern for real readers.
//!
//! Grammar (a JSON-like subset, ASCII whitespace allowed between tokens):
//!
//! ```text
//! value  := array | number | string
//! array  := '[' ( value ( ',' value )* )? ']'
//! number := [0-9]+
//! string := '"' (any byte except '"', '\' and ASCII control)* '"'
//! ```
//!
//! Every value is one element; every array enters one nesting level; every string is
//! returned as [`UntrustedText`], and its converted length plus one `strings` slot is
//! charged as tracked allocation.

use crate::budget::Budget;
use crate::error::{IoError, Position};
use crate::limits::{LimitKind, LimitRequest, Principal};
use crate::source::BoundedSource;
use crate::untrusted::UntrustedText;
use std::io::Read;

/// Hard cap on the toy reader's recursion, independent of the depth limit, so a human
/// who raises the depth limit still cannot overflow the stack. Reported as
/// `limit_exceeded{depth}` with this value.
pub const TOY_MAX_RECURSION: u32 = 256;

/// What the toy reader found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ToyDoc {
    /// Values read.
    pub elements: u64,
    /// Deepest array nesting.
    pub max_depth: u32,
    /// Every string, in input order, as untrusted text.
    pub strings: Vec<UntrustedText>,
}

/// Reads one toy document from `source` to its end.
pub fn read<R: Read>(source: &mut BoundedSource<R>) -> Result<ToyDoc, IoError> {
    let mut bytes = Vec::new();
    source.read_to_end_bounded(&mut bytes)?;
    let mut p = Parser { s: &bytes, i: 0, doc: ToyDoc::default() };
    p.value(source.budget_mut(), 0)?;
    p.ws();
    if p.i < p.s.len() {
        return Err(p.malformed("trailing_data"));
    }
    Ok(p.doc)
}

/// Reads `data` under the budget `budget`.
pub fn read_slice(data: &[u8], budget: Budget) -> Result<ToyDoc, IoError> {
    read(&mut BoundedSource::new(data, budget))
}

/// The limits the fuzz target uses: lowered from the defaults by a script principal so
/// one input stays fast (1 MiB, 65,536 elements, 4 MiB tracked allocation).
pub fn fuzz_budget() -> Result<Budget, IoError> {
    Budget::new(Principal::Script, &LimitRequest::default().bytes(1 << 20).elements(1 << 16).alloc(1 << 22))
}

struct Parser<'s> {
    s: &'s [u8],
    i: usize,
    doc: ToyDoc,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.i += 1;
        }
    }

    /// Byte position with 1-based line and column (columns count bytes).
    fn locate(&self, byte: usize) -> Position {
        let head = self.s.get(..byte).unwrap_or(self.s);
        let newlines = head.iter().filter(|&&b| b == b'\n').count();
        let line_start = head.iter().rposition(|&b| b == b'\n').map_or(0, |p| p + 1);
        let line = u32::try_from(newlines.saturating_add(1)).unwrap_or(u32::MAX);
        let col = u32::try_from(byte.saturating_sub(line_start).saturating_add(1)).unwrap_or(u32::MAX);
        Position::line_col(byte as u64, line, col)
    }

    /// Adds line and column to an error raised at a byte position.
    fn place(&self, e: IoError) -> IoError {
        match e.position() {
            Some(at) if at.line.is_none() => {
                let p = self.locate(usize::try_from(at.byte).unwrap_or(usize::MAX));
                match (p.line, p.col) {
                    (Some(l), Some(c)) => e.with_line_col(l, c),
                    _ => e,
                }
            }
            _ => e,
        }
    }

    fn malformed(&self, reason: &'static str) -> IoError {
        IoError::Malformed { at: self.locate(self.i), reason }
    }

    fn truncated(&self) -> IoError {
        IoError::TruncatedFile { at: self.locate(self.i) }
    }

    fn value(&mut self, budget: &mut Budget, recursion: u32) -> Result<(), IoError> {
        self.ws();
        let at = Position::byte(self.i as u64);
        budget.charge_elements(1, at).map_err(|e| self.place(e))?;
        self.doc.elements = self.doc.elements.saturating_add(1);
        match self.peek() {
            Some(b'[') => self.array(budget, recursion, at),
            Some(b'0'..=b'9') => {
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.i += 1;
                }
                Ok(())
            }
            Some(b'"') => self.string(budget, at),
            Some(_) => Err(self.malformed("unexpected_byte")),
            None => Err(self.truncated()),
        }
    }

    fn array(&mut self, budget: &mut Budget, recursion: u32, at: Position) -> Result<(), IoError> {
        if recursion >= TOY_MAX_RECURSION {
            let e = IoError::LimitExceeded { limit: LimitKind::Depth, value: u64::from(TOY_MAX_RECURSION), at };
            return Err(self.place(e));
        }
        let mut guard = budget.enter(at).map_err(|e| self.place(e))?;
        if guard.depth() > self.doc.max_depth {
            self.doc.max_depth = guard.depth();
        }
        self.i += 1;
        self.ws();
        if self.peek() == Some(b']') {
            self.i += 1;
            return Ok(());
        }
        loop {
            self.value(&mut guard, recursion.saturating_add(1))?;
            self.ws();
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b']') => {
                    self.i += 1;
                    return Ok(());
                }
                Some(_) => return Err(self.malformed("expected_comma_or_close")),
                None => return Err(self.truncated()),
            }
        }
    }

    fn string(&mut self, budget: &mut Budget, at: Position) -> Result<(), IoError> {
        self.i += 1;
        let start = self.i;
        loop {
            match self.peek() {
                Some(b'"') => break,
                Some(b'\\') => return Err(self.malformed("escape_unsupported")),
                Some(b) if b < 0x20 || b == 0x7f => return Err(self.malformed("control_byte_in_string")),
                Some(_) => self.i += 1,
                None => return Err(self.truncated()),
            }
        }
        let raw = self.s.get(start..self.i).unwrap_or(&[]);
        // Charge what is actually kept: the converted text (an invalid byte becomes the
        // 3-byte U+FFFD) plus one slot of `strings`.
        let text = UntrustedText::from_utf8_lossy(raw);
        let kept = text.len().saturating_add(std::mem::size_of::<UntrustedText>());
        budget.charge_alloc(kept as u64, at).map_err(|e| self.place(e))?;
        self.doc.strings.push(text);
        self.i += 1;
        Ok(())
    }
}
