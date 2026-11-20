//! The value reads of spec node `CORE.PRJ.STAGE.SWITCH` over the project store (the read API
//! itself, `fid_project::StageContext`, is in the model crate, which cannot link the store).
//!
//! - [`CasValueStore`]: [`ValueStore`] over the content-addressed blob store
//!   (`CORE.STO.CAS`): a slot digest names a single blob of at most `CHUNK_SIZE` (8 MiB).
//!   The blob file is read once, bounded at `CHUNK_SIZE` + 1 bytes. Nothing stored is
//!   `blob_missing`; bytes that do not hash to the digest are `blob_corrupt`; more than
//!   `CHUNK_SIZE` bytes, or bytes that parse as a chunk manifest (the slot digest of a
//!   chunked object names its manifest, never its content) are `storage` ("chunked object
//!   not supported"), so a manifest is never served as a value; any other failure is
//!   `storage`.
//! - [`StoreValueCache`]: [`ValueCache`] over the evictable `fid-store` cache
//!   (`CORE.STO.CACHE`). The cache has only memo and snapshot keys, so a value is stored under
//!   the memo key of the pure read `project.stage_value@1` whose only input is the slot
//!   digest ([`value_key`]). Values above `CHUNK_SIZE` are never put, an item file above
//!   `CHUNK_SIZE` plus the item header is not read (checked before `get`), and
//!   [`open_value_cache`] opens the cache with the small limit [`VALUE_CACHE_LIMIT`], which
//!   is also the cache's own per-read bound. The cache checks its own items; `StageContext`
//!   checks again that the bytes hash to the slot digest, so a stale item is never served.
//! - [`to_bus_error`]: a value read refused, as a bus error of catalogue v3: `bad_reference`
//!   and `bad_stage` (`stage_not_found`) are `validation`, `no_value` is
//!   `not_found/unknown_entity`, and `blob_missing`, `blob_corrupt` and `storage` are
//!   `internal/storage`.

use fid_bus::error::{BusError, Detail, InternalReason, NotFoundReason, ValidationReason};
use fid_project::{ValueCache, ValueError, ValueStore};
use fid_store::cache::{Cache, CacheConfig, CacheKey, CacheMode, ITEM_HEADER_LEN};
use fid_store::cas::{BlobStore, ChunkManifest, CHUNK_SIZE};
use fid_types::{CommandName, Digest};
use std::fs::File;
use std::io::{ErrorKind, Read};
use std::path::Path;

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

/// Most bytes of one value (one blob).
pub const MAX_VALUE_BYTES: u64 = CHUNK_SIZE;
/// Size limit of the value cache opened by [`open_value_cache`]: 64 MiB.
pub const VALUE_CACHE_LIMIT: u64 = 64 * 1024 * 1024;
const CHUNKED: &str = "chunked object not supported";

/// The value cache in `dir`, limited to [`VALUE_CACHE_LIMIT`].
pub fn open_value_cache(dir: &Path) -> Cache {
    Cache::open(dir, CacheConfig { limit: VALUE_CACHE_LIMIT, mode: CacheMode::Enabled })
}

/// [`ValueStore`] over a blob store.
#[derive(Debug)]
pub struct CasValueStore<'a>(pub &'a BlobStore);

impl ValueStore for CasValueStore<'_> {
    fn load(&self, digest: &Digest) -> Result<Vec<u8>, ValueError> {
        let file = match File::open(self.0.path_of(digest)) {
            Ok(f) => f,
            Err(e) if e.kind() == ErrorKind::NotFound => return Err(ValueError::BlobMissing(*digest)),
            Err(e) => return Err(ValueError::Storage(e.to_string())),
        };
        let mut bytes = Vec::new();
        file.take(MAX_VALUE_BYTES + 1).read_to_end(&mut bytes).map_err(|e| ValueError::Storage(e.to_string()))?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_VALUE_BYTES {
            return Err(ValueError::Storage(CHUNKED.to_string()));
        }
        if Digest::from_bytes(fid_canon::digest_bytes(&bytes).0) != *digest {
            return Err(ValueError::BlobCorrupt(*digest));
        }
        if ChunkManifest::parse(&bytes).is_ok() {
            return Err(ValueError::Storage(CHUNKED.to_string()));
        }
        Ok(bytes)
    }
}

/// [`ValueCache`] over the `fid-store` cache (open it with [`open_value_cache`]).
#[derive(Debug)]
pub struct StoreValueCache<'a>(pub &'a mut Cache);

impl ValueCache for StoreValueCache<'_> {
    fn get(&mut self, digest: &Digest) -> Option<Vec<u8>> {
        let key = value_key(digest).ok()?;
        // Bound the read before it happens: an item file larger than one value plus its
        // header is not read (and is dropped).
        let path = self.0.path_of(&key)?;
        match std::fs::metadata(&path) {
            Ok(m) if m.len() > MAX_VALUE_BYTES.saturating_add(ITEM_HEADER_LEN) => {
                self.0.remove(&key);
                return None;
            }
            Ok(_) => {}
            Err(_) => return None,
        }
        self.0.get(&key)
    }

    fn put(&mut self, digest: &Digest, bytes: &[u8]) {
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_VALUE_BYTES {
            return;
        }
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
