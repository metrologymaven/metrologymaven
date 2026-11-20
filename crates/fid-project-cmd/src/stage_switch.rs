//! The value reads of spec node `CORE.PRJ.STAGE.SWITCH` over the project store (the read API
//! itself, `fid_project::StageContext`, is in the model crate, which cannot link the store).
//!
//! - [`CasValueStore`]: [`ValueStore`] over the content-addressed blob store
//!   (`CORE.STO.CAS`): a slot digest names a blob; the store verifies it on read. Nothing
//!   stored is `blob_missing`, bytes that do not hash to it `blob_corrupt`, any other failure
//!   `storage`. Values are single blobs (up to `CHUNK_SIZE`, 8 MiB); a chunked object's slot
//!   digest would be its manifest digest, which this adapter does not resolve (`storage`).
//! - [`StoreValueCache`]: [`ValueCache`] over the evictable `fid-store` cache
//!   (`CORE.STO.CACHE`). The cache has only memo and snapshot keys, so a value is stored under
//!   the memo key of the pure read `project.stage_value@1` whose only input is the slot
//!   digest ([`value_key`]). The cache checks its own items; `StageContext` checks again
//!   that the bytes hash to the slot digest, so a stale item is never served.
//! - [`to_bus_error`]: a value read refused, as a bus error of catalogue v3: `bad_reference`
//!   and `bad_stage` (`stage_not_found`) are `validation`, `no_value` is
//!   `not_found/unknown_entity`, and `blob_missing`, `blob_corrupt` and `storage` are
//!   `internal/storage`.

use fid_bus::error::{BusError, Detail, InternalReason, NotFoundReason, ValidationReason};
use fid_project::{ValueCache, ValueError, ValueStore};
use fid_store::cache::{Cache, CacheKey};
use fid_store::cas::{Address, BlobRef, BlobStore, CasError};
use fid_types::{CommandName, Digest};
use std::io::ErrorKind;

/// Command name of the memo key under which values are cached.
pub const VALUE_READ: &str = "project.stage_value";

/// The cache key of the value with slot digest `digest`.
pub fn value_key(digest: &Digest) -> Result<CacheKey, String> {
    let command = CommandName::parse(VALUE_READ).map_err(|e| e.to_string())?;
    Ok(CacheKey::Memo {
        command,
        major: 1,
        args_digest: *digest,
        input_digests: vec![*digest],
        kernel_digest: Digest::TOMBSTONE,
    })
}

/// [`ValueStore`] over a blob store.
#[derive(Debug)]
pub struct CasValueStore<'a>(pub &'a BlobStore);

impl ValueStore for CasValueStore<'_> {
    fn load(&self, digest: &Digest) -> Result<Vec<u8>, ValueError> {
        let size = match std::fs::metadata(self.0.path_of(digest)) {
            Ok(m) => m.len(),
            Err(e) if e.kind() == ErrorKind::NotFound => return Err(ValueError::BlobMissing(*digest)),
            Err(e) => return Err(ValueError::Storage(e.to_string())),
        };
        match self.0.get(&Address::Blob(BlobRef { digest: *digest, size })) {
            Ok(b) => Ok(b),
            Err(CasError::NotFound(_)) => Err(ValueError::BlobMissing(*digest)),
            Err(CasError::Integrity(_)) => Err(ValueError::BlobCorrupt(*digest)),
            Err(e) => Err(ValueError::Storage(e.code().to_string())),
        }
    }
}

/// [`ValueCache`] over the `fid-store` cache.
#[derive(Debug)]
pub struct StoreValueCache<'a>(pub &'a mut Cache);

impl ValueCache for StoreValueCache<'_> {
    fn get(&mut self, digest: &Digest) -> Option<Vec<u8>> {
        self.0.get(&value_key(digest).ok()?)
    }

    fn put(&mut self, digest: &Digest, bytes: &[u8]) {
        if let Ok(k) = value_key(digest) {
            self.0.put(&k, bytes);
        }
    }

    fn remove(&mut self, digest: &Digest) {
        if let Ok(k) = value_key(digest) {
            self.0.remove(&k);
        }
    }
}

/// The bus error of a refused value read (see the module docs).
pub fn to_bus_error(e: &ValueError) -> BusError {
    let d = Detail::new(&e.to_string());
    match e {
        ValueError::BadReference(_) => BusError::Validation(ValidationReason::BadReference, d),
        ValueError::StageNotFound(_) => BusError::Validation(ValidationReason::BadStage, d),
        ValueError::NoValue { .. } => BusError::NotFound(NotFoundReason::UnknownEntity, d),
        ValueError::BlobMissing(_) | ValueError::BlobCorrupt(_) | ValueError::Storage(_) => {
            BusError::Internal(InternalReason::Storage, d)
        }
    }
}
