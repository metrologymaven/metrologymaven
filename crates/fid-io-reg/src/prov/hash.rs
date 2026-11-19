//! Streaming SHA-256 of the source while the reader consumes it (REQ-IO.REG.PROV-001).

use crate::budget::Budget;
use crate::error::IoError;
use crate::prov::ProvError;
use crate::source::BoundedSource;
use fid_types::Digest;
use sha2::{Digest as _, Sha256};
use std::io::{self, Read};

/// Chunk size of the drain in [`HashingSource::finish`].
const DRAIN_CHUNK: usize = 8192;

/// The digest and size of one whole source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceDigest {
    sha256: Digest,
    bytes: u64,
}

impl SourceDigest {
    /// Crate-private: a digest only ever comes from in-process hashing
    /// ([`HashingSource::finish`]), never from a caller's claim.
    pub(crate) fn new(sha256: Digest, bytes: u64) -> SourceDigest {
        SourceDigest { sha256, bytes }
    }

    /// The SHA-256 of every byte of the source.
    pub fn sha256(&self) -> Digest {
        self.sha256
    }

    /// The number of bytes of the source.
    pub fn bytes(&self) -> u64 {
        self.bytes
    }
}

/// A [`BoundedSource`] that hashes every byte it hands to the reader. The bytes are charged
/// to the budget exactly as by the wrapped source, so hashing adds no way around the byte
/// limit.
#[derive(Debug)]
pub struct HashingSource<R> {
    inner: BoundedSource<R>,
    hasher: Sha256,
    bytes: u64,
}

impl<R: Read> HashingSource<R> {
    /// Wraps a bounded source that has not been read yet. A source that has already
    /// delivered bytes is refused (`source_already_read`): the digest must cover the file
    /// from its first byte.
    pub fn new(inner: BoundedSource<R>) -> Result<Self, ProvError> {
        let offset = inner.offset();
        if offset != 0 {
            return Err(ProvError::SourceAlreadyRead { offset });
        }
        Ok(HashingSource { inner, hasher: Sha256::new(), bytes: 0 })
    }

    /// The budget.
    pub fn budget(&self) -> &Budget {
        self.inner.budget()
    }

    /// The budget, for element, depth and allocation charges by the reader.
    pub fn budget_mut(&mut self) -> &mut Budget {
        self.inner.budget_mut()
    }

    /// Bytes delivered (and hashed) so far.
    pub fn bytes_hashed(&self) -> u64 {
        self.bytes
    }

    /// Reads like [`BoundedSource::read_bounded`] and hashes the bytes delivered.
    pub fn read_bounded(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        let n = self.inner.read_bounded(buf)?;
        if let Some(got) = buf.get(..n) {
            self.hasher.update(got);
        }
        self.bytes = self.bytes.saturating_add(n as u64);
        Ok(n)
    }

    /// Ends the read. Whatever the reader left unread is read to the end through the same
    /// bounded source (still charged to the byte limit), so the digest and size always
    /// describe the whole file and never only the part a reader chose to consume. Returns
    /// the digest and the budget.
    pub fn finish(mut self) -> Result<(SourceDigest, Budget), IoError> {
        let mut chunk = [0u8; DRAIN_CHUNK];
        while self.read_bounded(&mut chunk)? != 0 {}
        let sha256 = Digest::from_bytes(self.hasher.finalize().into());
        let (_, budget) = self.inner.into_parts();
        Ok((SourceDigest::new(sha256, self.bytes), budget))
    }
}

impl<R: Read> Read for HashingSource<R> {
    /// Errors are [`IoError`]s wrapped by [`IoError::into_io`]; recover them with
    /// [`IoError::from_io`].
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.read_bounded(buf).map_err(IoError::into_io)
    }
}
