//! Bounded JSON-lines framing (REQ-AUTO.CTRL.WIRE-003).
//!
//! [`LineReader`] never holds more than `limit + 1` unconsumed bytes before it decides
//! whether a line fits, so a peer that never sends a newline cannot grow memory. Each
//! byte is scanned for a newline once, so byte-at-a-time senders cost linear time.

use std::io::{self, ErrorKind, Read};

/// Largest single `read` call, in bytes.
const CHUNK: usize = 64 * 1024;

/// What the next line read produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    /// One complete line, without its terminating `\n`.
    Line(Vec<u8>),
    /// More than `limit` bytes arrived without a newline.
    TooLarge,
    /// The peer closed the stream between lines.
    Eof,
    /// The peer closed the stream in the middle of a line; the partial bytes are dropped.
    PartialEof,
}

/// A newline-delimited reader with a hard per-line bound.
#[derive(Debug)]
pub struct LineReader<R> {
    inner: R,
    pending: Vec<u8>,
    /// Leading bytes of `pending` already known to hold no newline.
    scanned: usize,
    /// Read buffer, allocated once.
    scratch: Vec<u8>,
}

impl<R: Read> LineReader<R> {
    /// Wraps a byte source.
    pub fn new(inner: R) -> Self {
        LineReader { inner, pending: Vec::new(), scanned: 0, scratch: Vec::new() }
    }

    /// The wrapped source.
    pub fn get_mut(&mut self) -> &mut R {
        &mut self.inner
    }

    /// True when bytes of a next line are already buffered.
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Reads the next line of at most `limit` bytes (newline excluded).
    ///
    /// Reads at most `limit + 1` unconsumed bytes before deciding; a longer line is
    /// [`Frame::TooLarge`] and the reader must not be used for further lines.
    pub fn next_line(&mut self, limit: usize) -> io::Result<Frame> {
        let window = limit.saturating_add(1);
        loop {
            let scan = self.pending.len().min(window);
            let from = self.scanned.min(scan);
            if let Some(i) = self.pending[from..scan].iter().position(|&b| b == b'\n') {
                let mut line: Vec<u8> = self.pending.drain(..=from + i).collect();
                line.pop();
                self.scanned = 0;
                return Ok(Frame::Line(line));
            }
            self.scanned = scan;
            if self.pending.len() > limit {
                return Ok(Frame::TooLarge);
            }
            let want = window - self.pending.len();
            let n = self.fill(want)?;
            if n == 0 {
                return Ok(if self.pending.is_empty() { Frame::Eof } else { Frame::PartialEof });
            }
        }
    }

    /// Discards input up to and including the next newline, or until EOF, reading at
    /// most `budget` bytes. Returns true if a newline or EOF was reached.
    pub fn discard_line(&mut self, budget: usize) -> io::Result<bool> {
        let mut seen = 0usize;
        loop {
            self.scanned = 0;
            if let Some(i) = self.pending.iter().position(|&b| b == b'\n') {
                self.pending.drain(..=i);
                return Ok(true);
            }
            seen = seen.saturating_add(self.pending.len());
            self.pending.clear();
            if seen >= budget {
                return Ok(false);
            }
            if self.fill(budget - seen)? == 0 {
                return Ok(true);
            }
        }
    }

    /// Discards everything until EOF, reading at most `budget` bytes.
    pub fn drain(&mut self, budget: usize) -> io::Result<()> {
        let mut seen = self.pending.len();
        self.pending.clear();
        self.scanned = 0;
        while seen < budget {
            let n = self.fill(budget - seen)?;
            if n == 0 {
                break;
            }
            seen = seen.saturating_add(n);
            self.pending.clear();
        }
        Ok(())
    }

    /// Reads up to `want` (at most one chunk) bytes onto the end of `pending`.
    fn fill(&mut self, want: usize) -> io::Result<usize> {
        let want = want.clamp(1, CHUNK);
        if self.scratch.len() < CHUNK {
            self.scratch.resize(CHUNK, 0);
        }
        let n = loop {
            match self.inner.read(&mut self.scratch[..want]) {
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                other => break other?,
            }
        };
        let n = n.min(want);
        self.pending.extend_from_slice(&self.scratch[..n]);
        Ok(n)
    }
}
