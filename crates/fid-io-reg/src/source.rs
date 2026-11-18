//! The bounded byte source (REQ-IO.REG.LIMITS-001, -002).

use crate::budget::Budget;
use crate::error::{IoError, Position};
use crate::limits::LimitKind;
use std::io::{self, Read};

/// Interrupted reads are retried at most this many times in a row.
const MAX_INTERRUPTED_RETRIES: u32 = 64;
/// Chunk size of [`BoundedSource::read_to_end_bounded`].
const CHUNK: usize = 8192;

/// A reader wrapper that charges every byte it delivers to its [`Budget`]. It hands
/// out at most the byte limit; if the input holds more, the next read fails with
/// `limit_exceeded{bytes}` at the offset equal to the limit. An input of exactly the
/// limit reads to a clean end.
#[derive(Debug)]
pub struct BoundedSource<R> {
    inner: R,
    budget: Budget,
}

impl<R: Read> BoundedSource<R> {
    /// Wraps `inner`; every byte read is charged to `budget`.
    pub fn new(inner: R, budget: Budget) -> Self {
        BoundedSource { inner, budget }
    }

    /// The budget.
    pub fn budget(&self) -> &Budget {
        &self.budget
    }

    /// The budget, for element, depth and allocation charges by the reader.
    pub fn budget_mut(&mut self) -> &mut Budget {
        &mut self.budget
    }

    /// The byte offset reached.
    pub fn offset(&self) -> u64 {
        self.budget.bytes_used()
    }

    /// Splits the source into the wrapped reader and its budget.
    pub fn into_parts(self) -> (R, Budget) {
        (self.inner, self.budget)
    }

    fn read_inner(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        let mut retries = 0;
        loop {
            match self.inner.read(buf) {
                Ok(n) => return Ok(n.min(buf.len())),
                Err(e) if e.kind() == io::ErrorKind::Interrupted && retries < MAX_INTERRUPTED_RETRIES => {
                    retries += 1;
                }
                Err(e) => return Err(IoError::from_io(e, self.offset())),
            }
        }
    }

    /// Reads into `buf` like [`Read::read`], charging the bytes delivered. `Ok(0)`
    /// means the end of input.
    pub fn read_bounded(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        if buf.is_empty() {
            return Ok(0);
        }
        let limit = self.budget.limits().bytes();
        let remaining = limit.saturating_sub(self.offset());
        if remaining == 0 {
            // At the limit: the input may end here, or it holds more than allowed.
            let mut probe = [0u8; 1];
            return match self.read_inner(&mut probe)? {
                0 => Ok(0),
                _ => Err(IoError::LimitExceeded {
                    limit: LimitKind::Bytes,
                    value: limit,
                    at: Position::byte(self.offset()),
                }),
            };
        }
        let want = usize::try_from(remaining).map_or(buf.len(), |r| r.min(buf.len()));
        let n = match buf.get_mut(..want) {
            Some(window) => self.read_inner(window)?,
            None => 0,
        };
        self.budget.charge_bytes(n as u64)?;
        Ok(n)
    }

    /// Reads the rest of the input into `out`, charging the bytes read and every growth
    /// of `out`'s capacity (as tracked allocation, charged before the growth happens).
    /// Capacity at least doubles on each growth, so the total charged stays below twice
    /// the bytes kept. Returns the number of bytes appended.
    pub fn read_to_end_bounded(&mut self, out: &mut Vec<u8>) -> Result<usize, IoError> {
        let mut chunk = [0u8; CHUNK];
        let mut total = 0usize;
        loop {
            let n = self.read_bounded(&mut chunk)?;
            if n == 0 {
                return Ok(total);
            }
            let at = Position::byte(self.offset());
            let needed = out.len().saturating_add(n);
            let cap = out.capacity();
            if needed > cap {
                let new_cap = needed.max(cap.saturating_mul(2));
                self.budget.charge_alloc((new_cap - cap) as u64, at)?;
                out.try_reserve_exact(new_cap - out.len())
                    .map_err(|_| IoError::Io { kind: io::ErrorKind::OutOfMemory, at })?;
            }
            out.extend_from_slice(chunk.get(..n).unwrap_or(&[]));
            total = total.saturating_add(n);
        }
    }
}

impl<R: Read> Read for BoundedSource<R> {
    /// Errors are [`IoError`]s wrapped by [`IoError::into_io`]; recover them with
    /// [`IoError::from_io`].
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.read_bounded(buf).map_err(IoError::into_io)
    }
}
