//! Content-addressed blob storage and in-memory index for drawing revisions and sheets
//! (`RPT.BALLOON.SHEET.IMPORT`).

use super::error::SheetError;
use super::event::DrawingEvent;
use super::model::{DrawingRevId, DrawingRevision, DrawingSheet, RevisionId, SheetId, SheetInfo, SheetStatus};
use fid_canon::digest_bytes as canon_sha256;
use fid_store::cas::{Address, BlobRef, BlobStore, PutOpts};
use fid_types::Digest;
use std::collections::BTreeMap;

/// Maximum object size accepted when reading a drawing blob back from CAS (500 MiB + safety margin).
pub const MAX_CAS_READ_BYTES: u64 = 512 * 1024 * 1024;

/// Computes the canonical `sha256:` [`Digest`] of raw bytes.
pub fn digest_bytes(bytes: &[u8]) -> Digest {
    Digest::from_bytes(canon_sha256(bytes).0)
}

/// Content-addressed blob backend for storing imported drawing sources and raster pixel buffers.
pub trait DrawingBlobBackend {
    /// Stores `bytes` under its content digest, returning the [`BlobRef`].
    fn put_blob(&mut self, bytes: &[u8]) -> Result<BlobRef, SheetError>;

    /// Reads bytes stored under `digest`, verifying content integrity on every read.
    fn get_blob(&self, digest: &Digest) -> Result<Option<Vec<u8>>, SheetError>;
}

/// Real on-disk `CORE.STO.CAS` adapter over [`fid_store::cas::BlobStore`].
pub struct CasDrawingBlobs<'a>(pub &'a BlobStore);

impl DrawingBlobBackend for CasDrawingBlobs<'_> {
    fn put_blob(&mut self, bytes: &[u8]) -> Result<BlobRef, SheetError> {
        let addr = self.0.put_bytes(bytes, PutOpts::default()).map_err(|e| SheetError::Store(e.to_string()))?;
        let digest = match addr {
            Address::Blob(b) => b.digest,
            Address::Object(o) => o.manifest_digest,
        };
        Ok(BlobRef { digest, size: bytes.len() as u64 })
    }

    fn get_blob(&self, digest: &Digest) -> Result<Option<Vec<u8>>, SheetError> {
        match self.0.read_object(digest, MAX_CAS_READ_BYTES) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(fid_store::cas::CasError::NotFound(_)) => Ok(None),
            Err(e) => Err(SheetError::Store(e.to_string())),
        }
    }
}

/// In-memory content-addressed blob backend verifying SHA-256 on every read (for tests and replay).
#[derive(Debug, Default, Clone)]
pub struct MemoryDrawingBlobs {
    blobs: BTreeMap<Digest, Vec<u8>>,
}

impl MemoryDrawingBlobs {
    pub fn new() -> Self {
        Self::default()
    }

    /// Test helper to overwrite raw bytes under `digest` (simulating storage corruption for replay checks).
    pub fn tamper(&mut self, digest: Digest, bytes: Vec<u8>) {
        self.blobs.insert(digest, bytes);
    }

    pub fn len(&self) -> usize {
        self.blobs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.blobs.is_empty()
    }
}

impl DrawingBlobBackend for MemoryDrawingBlobs {
    fn put_blob(&mut self, bytes: &[u8]) -> Result<BlobRef, SheetError> {
        let digest = digest_bytes(bytes);
        self.blobs.entry(digest).or_insert_with(|| bytes.to_vec());
        Ok(BlobRef { digest, size: bytes.len() as u64 })
    }

    fn get_blob(&self, digest: &Digest) -> Result<Option<Vec<u8>>, SheetError> {
        let Some(bytes) = self.blobs.get(digest) else {
            return Ok(None);
        };
        let actual = digest_bytes(bytes);
        if actual != *digest {
            return Err(SheetError::Store(format!("blob digest mismatch: expected {digest}, got {actual}")));
        }
        Ok(Some(bytes.clone()))
    }
}

/// Project-scoped store holding imported drawing revisions, their sheets, and the append-only journal.
#[derive(Debug, Default, Clone)]
pub struct DrawingStore {
    revisions: BTreeMap<DrawingRevId, DrawingRevision>,
    sheets: BTreeMap<SheetId, DrawingSheet>,
    /// Uniqueness index `(part_rev, drawing_number, revision_label) -> DrawingRevId` (`REQ-008`).
    unique_key: BTreeMap<(RevisionId, String, String), DrawingRevId>,
    journal: Vec<DrawingEvent>,
}

impl DrawingStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_revision(&self, id: DrawingRevId) -> Option<&DrawingRevision> {
        self.revisions.get(&id)
    }

    pub fn sheet(&self, id: SheetId) -> Option<&DrawingSheet> {
        self.sheets.get(&id)
    }

    pub fn has_revision_key(&self, part_rev: RevisionId, drawing_number: &str, revision_label: &str) -> bool {
        self.unique_key.contains_key(&(part_rev, drawing_number.to_string(), revision_label.to_string()))
    }

    pub fn journal(&self) -> &[DrawingEvent] {
        &self.journal
    }

    pub fn all_revisions(&self) -> impl Iterator<Item = &DrawingRevision> {
        self.revisions.values()
    }

    pub fn all_sheets(&self) -> impl Iterator<Item = &DrawingSheet> {
        self.sheets.values()
    }

    pub(crate) fn insert_revision_and_sheets(
        &mut self,
        rev: DrawingRevision,
        sheets: Vec<DrawingSheet>,
        event: DrawingEvent,
    ) {
        let key = (
            rev.part_rev,
            rev.drawing_number.untrusted_value().as_str().to_string(),
            rev.revision_label.untrusted_value().as_str().to_string(),
        );
        self.unique_key.insert(key, rev.id);
        for s in sheets {
            self.sheets.insert(s.id, s);
        }
        self.revisions.insert(rev.id, rev);
        self.journal.push(event);
    }

    pub(crate) fn update_sheet_bounds(
        &mut self,
        sheet_id: SheetId,
        width_cmm: i64,
        height_cmm: i64,
        event: DrawingEvent,
    ) -> Result<DrawingSheet, SheetError> {
        let sheet = self.sheets.get_mut(&sheet_id).ok_or(SheetError::NotFound("sheet"))?;
        sheet.width_cmm = width_cmm;
        sheet.height_cmm = height_cmm;
        sheet.status = SheetStatus::Ready;
        let out = sheet.clone();
        self.journal.push(event);
        Ok(out)
    }
}

impl SheetInfo for DrawingStore {
    fn bounds(&self, s: SheetId) -> Option<(i64, i64)> {
        let sheet = self.sheets.get(&s)?;
        (sheet.status == SheetStatus::Ready).then_some((sheet.width_cmm, sheet.height_cmm))
    }

    fn revision(&self, s: SheetId) -> Option<DrawingRevId> {
        self.sheets.get(&s).map(|sheet| sheet.drawing_rev)
    }

    fn image(&self, s: SheetId) -> Option<BlobRef> {
        self.sheets.get(&s).and_then(|sheet| sheet.image)
    }

    fn ready(&self, s: SheetId) -> bool {
        self.sheets.get(&s).is_some_and(|sheet| sheet.status == SheetStatus::Ready)
    }
}
