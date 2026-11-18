//! `LogicalReader`: the logical byte stream of an E57 file, with page checksums verified.

use crate::cache::{PageCache, DEFAULT_CACHE_BUDGET};
use crate::crc::crc32c;
use crate::error::PhysError;
use crate::header::{validate, E57Header, PageLayout, HEADER_LEN};
use fid_io_reg::{LimitKind, DEFAULT_BYTES};
use std::io::{Read, Seek, SeekFrom};

/// Options of [`LogicalReader::open`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReaderOptions {
    /// Byte budget of the verified-page cache (default 16 MiB).
    pub cache_budget_bytes: u64,
    /// Largest file accepted, in bytes (default: the IO.REG.LIMITS input byte limit).
    pub max_file_bytes: u64,
}

impl Default for ReaderOptions {
    fn default() -> Self {
        ReaderOptions { cache_budget_bytes: DEFAULT_CACHE_BUDGET, max_file_bytes: DEFAULT_BYTES }
    }
}

/// Where the bytes of the page just made available live.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    Cached,
    Scratch,
}

/// Reads logical ranges of an E57 file. Each page's checksum is verified when the page is
/// first read; verified pages are kept in a byte-bounded [`PageCache`]. A page that does not
/// fit the cache is verified again on every read.
#[derive(Debug)]
pub struct LogicalReader<R> {
    inner: R,
    header: E57Header,
    layout: PageLayout,
    cache: PageCache,
    /// One whole physical page.
    scratch: Vec<u8>,
    /// Index of the page held verified in `scratch`, if any.
    scratch_page: Option<u64>,
}

impl<R: Read + Seek> LogicalReader<R> {
    /// Opens an E57 file with default options.
    pub fn open(inner: R) -> Result<Self, PhysError> {
        Self::open_with(inner, ReaderOptions::default())
    }

    /// Opens an E57 file: measures its length, reads and validates the header against that
    /// length (`malformed{field}`) before reading any page, then verifies page 0 (which
    /// holds the header).
    pub fn open_with(mut inner: R, opts: ReaderOptions) -> Result<Self, PhysError> {
        let actual = inner.seek(SeekFrom::End(0)).map_err(|e| PhysError::from_io(&e, 0))?;
        if actual > opts.max_file_bytes {
            return Err(PhysError::LimitExceeded {
                limit: LimitKind::Bytes,
                value: opts.max_file_bytes,
                at: opts.max_file_bytes,
            });
        }
        if actual < HEADER_LEN as u64 {
            return Err(PhysError::TruncatedFile { at: actual });
        }
        inner.seek(SeekFrom::Start(0)).map_err(|e| PhysError::from_io(&e, 0))?;
        let mut raw = [0u8; HEADER_LEN];
        inner.read_exact(&mut raw).map_err(|e| PhysError::from_io(&e, 0))?;
        let header = E57Header::parse(&raw)?;
        let layout = validate(&header, actual)?;
        // page_size <= MAX_PAGE_SIZE (1 MiB), so this fits usize on every target.
        let page_len = usize::try_from(layout.page_size()).map_err(|_| PhysError::LimitExceeded {
            limit: LimitKind::Alloc,
            value: layout.page_size(),
            at: 0,
        })?;
        let mut reader = LogicalReader {
            inner,
            header,
            layout,
            cache: PageCache::new(opts.cache_budget_bytes),
            scratch: vec![0u8; page_len],
            scratch_page: None,
        };
        reader.ensure_page(0)?;
        Ok(reader)
    }

    /// The validated header.
    pub fn header(&self) -> &E57Header {
        &self.header
    }

    /// The page layout.
    pub fn layout(&self) -> &PageLayout {
        &self.layout
    }

    /// Logical length of the file in bytes, including the padding of the last page.
    pub fn logical_length(&self) -> u64 {
        self.layout.logical_length()
    }

    /// Logical offset of the XML section (validated at open).
    pub fn xml_logical_offset(&self) -> u64 {
        self.layout.logical_of(self.header.xml_offset).unwrap_or(0)
    }

    /// The verified-page cache (for its byte accounting).
    pub fn cache(&self) -> &PageCache {
        &self.cache
    }

    /// The underlying reader.
    pub fn into_inner(self) -> R {
        self.inner
    }

    /// Fills `buf` with the logical bytes starting at `offset`. Fails `truncated_file` if the
    /// range ends past the logical length, `checksum_mismatch{offset}` naming the physical
    /// offset of the first page in the range whose checksum is wrong.
    ///
    /// Every page in the range is verified before any byte is copied, so on `Err` `buf` is
    /// left unchanged; if a page fails while copying (the file changed between the passes, or
    /// an evicted page could not be read again), `buf` is zeroed before the error returns.
    pub fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<(), PhysError> {
        let file_end = self.layout.physical_length();
        let end = offset.checked_add(buf.len() as u64).ok_or(PhysError::TruncatedFile { at: file_end })?;
        if end > self.layout.logical_length() {
            return Err(PhysError::TruncatedFile { at: file_end });
        }
        if buf.is_empty() {
            return Ok(());
        }
        let payload = self.layout.payload();
        for index in offset / payload..=(end - 1) / payload {
            self.ensure_page(index)?;
        }
        let copied = self.copy_verified(offset, buf);
        if copied.is_err() {
            buf.fill(0);
        }
        copied
    }

    /// The copy pass of [`LogicalReader::read_at`]; the range is already bounds-checked.
    fn copy_verified(&mut self, offset: u64, buf: &mut [u8]) -> Result<(), PhysError> {
        let file_end = self.layout.physical_length();
        let payload = self.layout.payload();
        let mut done = 0usize;
        let mut logical = offset;
        while done < buf.len() {
            let index = logical / payload;
            // `within < payload <= 1 MiB`, so it fits usize.
            let within = usize::try_from(logical % payload).map_err(|_| PhysError::TruncatedFile { at: file_end })?;
            let slot = self.ensure_page(index)?;
            let page: &[u8] = match slot {
                Slot::Cached => self.cache.peek(index).unwrap_or(&[]),
                Slot::Scratch => self.scratch.get(..self.scratch.len().saturating_sub(4)).unwrap_or(&[]),
            };
            let n = page.len().saturating_sub(within).min(buf.len() - done);
            let src = page.get(within..within + n).filter(|s| !s.is_empty());
            let dst = buf.get_mut(done..done + n);
            match (src, dst) {
                (Some(s), Some(d)) => d.copy_from_slice(s),
                _ => return Err(PhysError::TruncatedFile { at: file_end }),
            }
            done += n;
            logical += n as u64;
        }
        Ok(())
    }

    /// Verifies every page's checksum, in page order (the first failure is reported).
    pub fn verify_all(&mut self) -> Result<(), PhysError> {
        for index in 0..self.layout.page_count() {
            self.ensure_page(index)?;
        }
        Ok(())
    }

    /// Makes page `index` available verified, in the cache or in `scratch`.
    fn ensure_page(&mut self, index: u64) -> Result<Slot, PhysError> {
        if self.cache.get(index).is_some() {
            return Ok(Slot::Cached);
        }
        if self.scratch_page == Some(index) {
            return Ok(Slot::Scratch);
        }
        let page_size = self.layout.page_size();
        let at = index
            .checked_mul(page_size)
            .filter(|_| index < self.layout.page_count())
            .ok_or(PhysError::TruncatedFile { at: self.layout.physical_length() })?;
        self.scratch_page = None;
        self.inner.seek(SeekFrom::Start(at)).map_err(|e| PhysError::from_io(&e, at))?;
        self.inner.read_exact(&mut self.scratch).map_err(|e| PhysError::from_io(&e, at))?;
        let payload_len = self.scratch.len().saturating_sub(4);
        let (payload, stored) = self.scratch.split_at(payload_len);
        let mut crc = [0u8; 4];
        if stored.len() == 4 {
            crc.copy_from_slice(stored);
        }
        // [unverified: the stored checksum's byte order; big-endian here.]
        if crc32c(payload) != u32::from_be_bytes(crc) {
            return Err(PhysError::ChecksumMismatch { offset: at });
        }
        if self.cache.insert(index, payload.to_vec()) {
            Ok(Slot::Cached)
        } else {
            self.scratch_page = Some(index);
            Ok(Slot::Scratch)
        }
    }
}
