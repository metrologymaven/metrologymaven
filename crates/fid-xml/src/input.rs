//! Encoding detection, decoding into a bounded UTF-8 window, and position tracking.
//!
//! The source is read in chunks of [`CHUNK`] bytes and decoded into `buf`. Bytes before
//! `start` are consumed and dropped on the next read, so the buffer holds only the token
//! being read plus one chunk. Bytes in `start..avail` are valid UTF-8 made only of XML
//! characters; the first invalid byte (if any) is remembered in `bad` and reported when
//! the reader needs input beyond it.

use crate::error::XmlError;
use fid_io_reg::{BoundedSource, Budget, IoError, Position};
use std::io::{ErrorKind, Read};

/// Bytes read from the source at a time.
pub(crate) const CHUNK: usize = 8192;

/// The detected encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Encoding {
    Utf8,
    Utf16Le,
    Utf16Be,
}

/// Position of the next unconsumed byte: source byte offset, 1-based line and column.
/// Columns count bytes of the UTF-8 text from the line start, so a UTF-16 document and
/// its UTF-8 twin report the same lines and columns; byte offsets are always offsets in
/// the source. `\r\n`, `\r` and `\n` each end one line.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Tracker {
    pub src: u64,
    line: u32,
    col: u32,
    prev_cr: bool,
    wide: bool,
}

impl Tracker {
    fn new(src: u64, wide: bool) -> Self {
        Tracker { src, line: 1, col: 1, prev_cr: false, wide }
    }

    fn advance(&mut self, bytes: &[u8]) {
        for &b in bytes {
            if self.wide {
                if b & 0xC0 != 0x80 {
                    self.src = self.src.saturating_add(if b >= 0xF0 { 4 } else { 2 });
                }
            } else {
                self.src = self.src.saturating_add(1);
            }
            match b {
                b'\n' => {
                    if !self.prev_cr {
                        self.line = self.line.saturating_add(1);
                    }
                    self.col = 1;
                    self.prev_cr = false;
                }
                b'\r' => {
                    self.line = self.line.saturating_add(1);
                    self.col = 1;
                    self.prev_cr = true;
                }
                _ => {
                    self.col = self.col.saturating_add(1);
                    self.prev_cr = false;
                }
            }
        }
    }

    pub(crate) fn position(&self) -> Position {
        Position::line_col(self.src, self.line, self.col)
    }
}

/// The decoding window over a bounded source.
pub(crate) struct Input<R> {
    source: BoundedSource<R>,
    enc: Encoding,
    buf: Vec<u8>,
    start: usize,
    avail: usize,
    bad: Option<(usize, &'static str)>,
    carry: Vec<u8>,
    eof: bool,
    chunk: usize,
    pub tracker: Tracker,
}

impl<R: Read> Input<R> {
    pub(crate) fn new(source: BoundedSource<R>, chunk: usize) -> Self {
        Input {
            source,
            enc: Encoding::Utf8,
            buf: Vec::new(),
            start: 0,
            avail: 0,
            bad: None,
            carry: Vec::new(),
            eof: false,
            chunk: chunk.clamp(1, CHUNK),
            tracker: Tracker::new(0, false),
        }
    }

    pub(crate) fn budget(&self) -> &Budget {
        self.source.budget()
    }

    pub(crate) fn budget_mut(&mut self) -> &mut Budget {
        self.source.budget_mut()
    }

    pub(crate) fn into_source(self) -> BoundedSource<R> {
        self.source
    }

    /// Bytes requested from the source per read.
    pub(crate) fn chunk(&self) -> usize {
        self.chunk
    }

    pub(crate) fn encoding(&self) -> Encoding {
        self.enc
    }

    /// The unconsumed valid bytes.
    pub(crate) fn window(&self) -> &[u8] {
        self.buf.get(self.start..self.avail).unwrap_or(&[])
    }

    /// True when every byte of the source has been read and decoded without error.
    pub(crate) fn at_end(&self) -> bool {
        self.eof && self.bad.is_none() && self.carry.is_empty() && self.avail == self.buf.len()
    }

    /// The position `rel` bytes into the window.
    pub(crate) fn pos_at(&self, rel: usize) -> Position {
        let mut t = self.tracker;
        let end = self.start.saturating_add(rel).min(self.buf.len());
        if let Some(s) = self.buf.get(self.start..end) {
            t.advance(s);
        }
        t.position()
    }

    /// Consumes `n` bytes of the window.
    pub(crate) fn consume(&mut self, n: usize) {
        let end = self.start.saturating_add(n).min(self.avail);
        if let Some(s) = self.buf.get(self.start..end) {
            self.tracker.advance(s);
        }
        self.start = end;
    }

    fn harness(&self, e: IoError) -> XmlError {
        let p = self.tracker.position();
        XmlError::Harness(e.with_line_col(p.line.unwrap_or(1), p.col.unwrap_or(1)))
    }

    fn read_raw(&mut self, out: &mut [u8]) -> Result<usize, XmlError> {
        match self.source.read_bounded(out) {
            Ok(n) => Ok(n),
            Err(e) => Err(self.harness(e)),
        }
    }

    /// Reads the first bytes, detects the encoding and skips the byte order mark.
    pub(crate) fn detect(&mut self) -> Result<(), XmlError> {
        let mut head = [0u8; 4];
        let mut got = 0;
        while let Some(window) = head.get_mut(got..) {
            if window.is_empty() {
                break;
            }
            let n = match self.source.read_bounded(window) {
                Ok(n) => n,
                Err(e) => return Err(self.harness(e)),
            };
            if n == 0 {
                self.eof = true;
                break;
            }
            got += n;
        }
        let h = head.get(..got).unwrap_or(&[]);
        let (enc, bom) = match h {
            // UCS-4 (with or without BOM), UTF-16 without BOM and EBCDIC are refused.
            [0xFF, 0xFE, 0, 0, ..]
            | [0, 0, 0xFE, 0xFF, ..]
            | [0, 0, 0, 0x3C, ..]
            | [0x3C, 0, 0, 0, ..]
            | [0x3C, 0, ..]
            | [0, 0x3C, ..]
            | [0x4C, 0x6F, 0xA7, 0x94, ..] => {
                return Err(XmlError::EncodingUnsupported { at: Position::line_col(0, 1, 1) });
            }
            [0xEF, 0xBB, 0xBF, ..] => (Encoding::Utf8, 3),
            [0xFF, 0xFE, ..] => (Encoding::Utf16Le, 2),
            [0xFE, 0xFF, ..] => (Encoding::Utf16Be, 2),
            _ => (Encoding::Utf8, 0),
        };
        self.enc = enc;
        self.tracker = Tracker::new(bom, enc != Encoding::Utf8);
        let rest = h.get(bom as usize..).unwrap_or(&[]).to_vec();
        self.append(&rest)?;
        if self.eof {
            self.finish_decode();
        }
        Ok(())
    }

    /// Makes at least `want` more valid bytes available, or as many as the input holds.
    /// Returns whether any became available. Fails with `malformed` when the next byte
    /// is invalid.
    pub(crate) fn fill(&mut self, want: usize) -> Result<bool, XmlError> {
        let before = self.avail;
        let target = self.avail.saturating_add(want.max(1));
        while self.avail < target && self.bad.is_none() && !self.eof {
            self.read_chunk()?;
        }
        if self.avail > before {
            return Ok(true);
        }
        if let Some((i, reason)) = self.bad {
            let at = self.pos_at(i.saturating_sub(self.start));
            return Err(XmlError::Malformed { reason, at });
        }
        Ok(false)
    }

    fn read_chunk(&mut self) -> Result<(), XmlError> {
        self.compact();
        let mut chunk = [0u8; CHUNK];
        let n = match chunk.get_mut(..self.chunk) {
            Some(window) => self.read_raw(window)?,
            None => self.read_raw(&mut chunk)?,
        };
        if n == 0 {
            self.eof = true;
            self.finish_decode();
            return Ok(());
        }
        self.append(chunk.get(..n).unwrap_or(&[]))
    }

    /// Drops the consumed bytes.
    fn compact(&mut self) {
        if self.start == 0 {
            return;
        }
        let start = self.start.min(self.buf.len());
        let len = self.buf.len();
        self.buf.copy_within(start.., 0);
        self.buf.truncate(len - start);
        self.avail = self.avail.saturating_sub(self.start);
        if let Some((i, _)) = self.bad.as_mut() {
            *i = i.saturating_sub(self.start);
        }
        self.start = 0;
    }

    /// Decodes `data` onto the buffer, charging any growth to the allocation budget.
    fn append(&mut self, data: &[u8]) -> Result<(), XmlError> {
        let needed = self.buf.len().saturating_add(data.len().saturating_mul(2)).saturating_add(4);
        let cap = self.buf.capacity();
        if needed > cap {
            let new_cap = needed.max(cap.saturating_mul(2));
            let at = self.tracker.position();
            self.source.budget_mut().charge_alloc((new_cap - cap) as u64, at).map_err(XmlError::Harness)?;
            self.buf
                .try_reserve_exact(new_cap - self.buf.len())
                .map_err(|_| XmlError::Harness(IoError::Io { kind: ErrorKind::OutOfMemory, at }))?;
        }
        match self.enc {
            Encoding::Utf8 => self.buf.extend_from_slice(data),
            Encoding::Utf16Le | Encoding::Utf16Be => self.decode_utf16(data),
        }
        self.validate();
        Ok(())
    }

    fn push_char(&mut self, c: char) {
        let mut tmp = [0u8; 4];
        self.buf.extend_from_slice(c.encode_utf8(&mut tmp).as_bytes());
    }

    fn decode_utf16(&mut self, data: &[u8]) {
        let le = self.enc == Encoding::Utf16Le;
        let unit = |a: u8, b: u8| if le { u16::from_le_bytes([a, b]) } else { u16::from_be_bytes([a, b]) };
        let mut units = std::mem::take(&mut self.carry);
        units.extend_from_slice(data);
        let mut i = 0;
        while self.bad.is_none() {
            let Some(&[a, b]) = units.get(i..i + 2) else {
                break;
            };
            let u = u32::from(unit(a, b));
            if (0xD800..=0xDBFF).contains(&u) {
                let Some(&[c, d]) = units.get(i + 2..i + 4) else {
                    break; // the low surrogate is in the next chunk
                };
                let lo = u32::from(unit(c, d));
                if !(0xDC00..=0xDFFF).contains(&lo) {
                    self.bad = Some((self.buf.len(), "invalid_utf16"));
                    break;
                }
                match char::from_u32(0x10000 + ((u - 0xD800) << 10) + (lo - 0xDC00)) {
                    Some(ch) => self.push_char(ch),
                    None => {
                        self.bad = Some((self.buf.len(), "invalid_utf16"));
                        break;
                    }
                }
                i += 4;
            } else {
                match char::from_u32(u) {
                    Some(ch) => self.push_char(ch),
                    None => {
                        self.bad = Some((self.buf.len(), "invalid_utf16"));
                        break;
                    }
                }
                i += 2;
            }
        }
        self.carry = units.get(i..).map(<[u8]>::to_vec).unwrap_or_default();
    }

    /// Extends `avail` over the newly decoded bytes that are valid UTF-8 and XML
    /// characters, and records the first invalid byte.
    fn validate(&mut self) {
        let from = self.avail;
        let tail = self.buf.get(from..).unwrap_or(&[]);
        let (good, utf8_err) = match std::str::from_utf8(tail) {
            Ok(_) => (tail.len(), false),
            Err(e) => (e.valid_up_to(), e.error_len().is_some()),
        };
        let mut found: Option<(usize, &'static str)> = None;
        if let Some(g) = tail.get(..good) {
            for (k, &b) in g.iter().enumerate() {
                if b < 0x20 && b != b'\t' && b != b'\n' && b != b'\r' {
                    found = Some((from + k, "invalid_char"));
                    break;
                }
                // U+FFFE and U+FFFF are not XML characters.
                if b == 0xEF && matches!(g.get(k + 1..k + 3), Some(&[0xBF, 0xBE]) | Some(&[0xBF, 0xBF])) {
                    found = Some((from + k, "invalid_char"));
                    break;
                }
            }
        }
        if found.is_none() && utf8_err {
            found = Some((from + good, "invalid_utf8"));
        }
        if let Some((i, reason)) = found {
            if self.bad.is_none_or(|(j, _)| i < j) {
                self.bad = Some((i, reason));
            }
        }
        let mut end = from + good;
        if let Some((j, _)) = self.bad {
            end = end.min(j);
        }
        self.avail = end.max(from);
    }

    /// At the end of the source: undecoded or incomplete bytes are an error.
    fn finish_decode(&mut self) {
        if self.bad.is_some() {
            return;
        }
        if !self.carry.is_empty() {
            self.bad = Some((self.buf.len(), "truncated_utf16"));
        } else if self.avail < self.buf.len() {
            self.bad = Some((self.avail, "invalid_utf8"));
        }
    }
}
