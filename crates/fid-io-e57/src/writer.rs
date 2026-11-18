//! `LogicalWriter`: appends logical bytes and emits checksummed pages.

use crate::crc::crc32c;
use crate::error::PhysError;
use crate::header::{validate, E57Header, PageLayout, HEADER_LEN, MAJOR_VERSION, MINOR_VERSION};
use fid_io_reg::LimitKind;
use std::io::{Seek, SeekFrom, Write};

/// Writes an E57 file's paged physical layer. The first [`HEADER_LEN`] logical bytes are
/// reserved for the header; [`LogicalWriter::append`] continues after them. Full pages are
/// written with their checksum as soon as they fill. [`LogicalWriter::finish`] zero-pads the
/// last page, then writes the header into page 0 and rewrites page 0 with its new checksum.
///
/// The writer starts at physical offset 0 of `inner`.
#[derive(Debug)]
pub struct LogicalWriter<W> {
    inner: W,
    layout: PageLayout,
    /// Payload bytes per page.
    payload: usize,
    /// Payload of the page being filled (always shorter than the payload size between calls).
    current: Vec<u8>,
    /// Payload of page 0, kept to patch the header at finish.
    first: Option<Vec<u8>>,
    /// Pages written so far.
    pages_written: u64,
}

impl<W: Write + Seek> LogicalWriter<W> {
    /// A writer with the given page size (normally [`crate::DEFAULT_PAGE_SIZE`]). Fails
    /// `malformed{page_size}` or `limit_exceeded{alloc}` for page sizes the reader rejects.
    pub fn new(mut inner: W, page_size: u64) -> Result<Self, PhysError> {
        let layout = PageLayout::new(page_size, 0)?;
        let payload = usize::try_from(layout.payload()).map_err(|_| PhysError::LimitExceeded {
            limit: LimitKind::Alloc,
            value: page_size,
            at: 0,
        })?;
        inner.seek(SeekFrom::Start(0)).map_err(|e| PhysError::from_io(&e, 0))?;
        let mut current = Vec::with_capacity(payload);
        current.resize(HEADER_LEN, 0);
        Ok(LogicalWriter { inner, layout, payload, current, first: None, pages_written: 0 })
    }

    /// The page size.
    pub fn page_size(&self) -> u64 {
        self.layout.page_size()
    }

    /// The logical offset the next appended byte will have.
    pub fn logical_position(&self) -> u64 {
        self.pages_written.saturating_mul(self.layout.payload()).saturating_add(self.current.len() as u64)
    }

    /// The physical offset the next appended byte will have (`None` on overflow). Use it to
    /// record where the XML section starts.
    pub fn physical_position(&self) -> Option<u64> {
        self.pages_written.checked_mul(self.layout.page_size())?.checked_add(self.current.len() as u64)
    }

    /// Appends logical bytes.
    pub fn append(&mut self, mut data: &[u8]) -> Result<(), PhysError> {
        let payload = self.payload;
        while !data.is_empty() {
            let room = payload.saturating_sub(self.current.len());
            let (head, tail) = data.split_at(room.min(data.len()));
            self.current.extend_from_slice(head);
            data = tail;
            if self.current.len() >= payload {
                self.flush_page()?;
            }
        }
        Ok(())
    }

    fn flush_page(&mut self) -> Result<(), PhysError> {
        let at = self.pages_written.checked_mul(self.layout.page_size()).ok_or(PhysError::LimitExceeded {
            limit: LimitKind::Bytes,
            value: u64::MAX,
            at: u64::MAX,
        })?;
        let crc = crc32c(&self.current).to_be_bytes();
        self.inner.write_all(&self.current).map_err(|e| PhysError::from_io(&e, at))?;
        self.inner.write_all(&crc).map_err(|e| PhysError::from_io(&e, at))?;
        if self.pages_written == 0 {
            self.first = Some(self.current.clone());
        }
        self.pages_written = self.pages_written.saturating_add(1);
        self.current.clear();
        Ok(())
    }

    /// Pads and writes the last page, writes the header (declaring the XML section at
    /// physical `xml_offset` with logical length `xml_length`) and rewrites page 0. Fails
    /// `malformed{field}` if the header would not pass the reader's checks. Returns the
    /// inner writer, positioned at the end of the file, and the header written.
    pub fn finish(mut self, xml_offset: u64, xml_length: u64) -> Result<(W, E57Header), PhysError> {
        if !self.current.is_empty() {
            self.current.resize(self.payload, 0);
            self.flush_page()?;
        }
        let file_length = self.pages_written.checked_mul(self.layout.page_size()).ok_or(PhysError::LimitExceeded {
            limit: LimitKind::Bytes,
            value: u64::MAX,
            at: u64::MAX,
        })?;
        let header = E57Header {
            major_version: MAJOR_VERSION,
            minor_version: MINOR_VERSION,
            file_length,
            xml_offset,
            xml_length,
            page_size: self.layout.page_size(),
        };
        validate(&header, file_length)?;
        let mut first = self.first.take().unwrap_or_default();
        match first.get_mut(..HEADER_LEN) {
            Some(dst) => dst.copy_from_slice(&header.encode()),
            None => return Err(PhysError::TruncatedFile { at: 0 }),
        }
        let crc = crc32c(&first).to_be_bytes();
        self.inner.seek(SeekFrom::Start(0)).map_err(|e| PhysError::from_io(&e, 0))?;
        self.inner.write_all(&first).map_err(|e| PhysError::from_io(&e, 0))?;
        self.inner.write_all(&crc).map_err(|e| PhysError::from_io(&e, 0))?;
        self.inner.seek(SeekFrom::Start(file_length)).map_err(|e| PhysError::from_io(&e, file_length))?;
        self.inner.flush().map_err(|e| PhysError::from_io(&e, file_length))?;
        Ok((self.inner, header))
    }
}
