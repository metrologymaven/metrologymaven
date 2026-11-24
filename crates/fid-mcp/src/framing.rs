//! Bounded line-delimited JSON-RPC 2.0 framing over stdio (`AUTO.MCP.SESS`).
//!
//! Enforces the 4 MiB incoming message bound (`REQ-AUTO.MCP.SESS-006`), strips CRLF on
//! Windows pipes, discards oversized lines without aborting the session, and writes RFC 8785
//! canonical JSON lines flushed immediately after every message.

use fid_canon::{to_canonical_bytes, CValue};
use std::io::{self, BufRead, Write};

/// Maximum incoming JSON-RPC line length in bytes (4 MiB, `REQ-AUTO.MCP.SESS-006`).
pub const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;
/// Maximum bytes discarded after an oversized line before giving up on the stream.
pub const DISCARD_BUDGET_BYTES: usize = 32 * 1024 * 1024;

/// One framed input line read by [`LineReader`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameLine {
    /// Complete `\n`-terminated line (without `\n` or trailing `\r`), `<= MAX_MESSAGE_BYTES`.
    Line(Vec<u8>),
    /// Line exceeded `MAX_MESSAGE_BYTES`; remainder up to `\n` was discarded.
    TooLarge,
    /// Clean EOF on a line boundary.
    Eof,
    /// EOF reached mid-line without trailing `\n`.
    PartialEof(Vec<u8>),
}

/// Bounded newline-delimited reader over `R: BufRead`.
pub struct LineReader<R> {
    inner: R,
}

impl<R: BufRead> LineReader<R> {
    pub fn new(inner: R) -> Self {
        Self { inner }
    }

    /// Reads the next line up to `max_bytes`. If a line exceeds `max_bytes`, drains until `\n`
    /// (within [`DISCARD_BUDGET_BYTES`]) and returns [`FrameLine::TooLarge`] so the session
    /// stays alive and synchronized on the next line.
    pub fn next_line(&mut self, max_bytes: usize) -> io::Result<FrameLine> {
        let mut buf = Vec::new();
        loop {
            let available = match self.inner.fill_buf() {
                Ok(b) => b,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            };
            if available.is_empty() {
                return if buf.is_empty() {
                    Ok(FrameLine::Eof)
                } else {
                    if buf.last() == Some(&b'\r') {
                        buf.pop();
                    }
                    Ok(FrameLine::PartialEof(buf))
                };
            }
            if let Some(pos) = available.iter().position(|&b| b == b'\n') {
                let take = pos;
                if buf.len().saturating_add(take) > max_bytes {
                    self.inner.consume(pos + 1);
                    return Ok(FrameLine::TooLarge);
                }
                buf.extend_from_slice(&available[..take]);
                self.inner.consume(pos + 1);
                if buf.last() == Some(&b'\r') {
                    buf.pop();
                }
                return Ok(FrameLine::Line(buf));
            }
            let chunk_len = available.len();
            if buf.len().saturating_add(chunk_len) > max_bytes {
                self.inner.consume(chunk_len);
                self.discard_to_newline(DISCARD_BUDGET_BYTES)?;
                return Ok(FrameLine::TooLarge);
            }
            buf.extend_from_slice(available);
            self.inner.consume(chunk_len);
        }
    }

    fn discard_to_newline(&mut self, budget: usize) -> io::Result<()> {
        let mut discarded = 0usize;
        while discarded < budget {
            let available = match self.inner.fill_buf() {
                Ok(b) => b,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            };
            if available.is_empty() {
                return Ok(());
            }
            if let Some(pos) = available.iter().position(|&b| b == b'\n') {
                self.inner.consume(pos + 1);
                return Ok(());
            }
            let n = available.len();
            discarded = discarded.saturating_add(n);
            self.inner.consume(n);
        }
        Ok(())
    }
}

/// Encodes `val` as RFC 8785 canonical JSON, appends `\n`, and flushes `writer`.
pub fn write_canonical_line(writer: &mut impl Write, val: &CValue) -> io::Result<()> {
    let bytes = to_canonical_bytes(val)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("canonical json: {e}")))?;
    writer.write_all(&bytes)?;
    writer.write_all(b"\n")?;
    writer.flush()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn reads_lf_and_crlf_and_recovers_after_too_large() {
        let mut input = Vec::new();
        input.extend_from_slice(b"{\"a\":1}\r\n");
        input.extend(std::iter::repeat_n(b'x', 200));
        input.push(b'\n');
        input.extend_from_slice(b"{\"b\":2}\n");

        let mut reader = LineReader::new(Cursor::new(input));
        assert_eq!(reader.next_line(64).unwrap(), FrameLine::Line(b"{\"a\":1}".to_vec()));
        assert_eq!(reader.next_line(64).unwrap(), FrameLine::TooLarge);
        assert_eq!(reader.next_line(64).unwrap(), FrameLine::Line(b"{\"b\":2}".to_vec()));
        assert_eq!(reader.next_line(64).unwrap(), FrameLine::Eof);
    }

    #[test]
    fn handles_partial_eof() {
        let mut reader = LineReader::new(Cursor::new(b"{\"partial\":true}"));
        assert_eq!(reader.next_line(MAX_MESSAGE_BYTES).unwrap(), FrameLine::PartialEof(b"{\"partial\":true}".to_vec()));
        assert_eq!(reader.next_line(MAX_MESSAGE_BYTES).unwrap(), FrameLine::Eof);
    }
}
