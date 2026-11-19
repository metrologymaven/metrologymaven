//! Where chunks live: the content-addressed blob store of CORE.STO.
//!
//! A chunk's manifest entry names it by its store address, computed here by
//! [`blob_address`] with the CORE.STO.CAS rule, so the manifest does not depend on which
//! [`ChunkStore`] holds the bytes: content of at most [`STORE_CHUNK_SIZE`] bytes is one
//! blob named by its SHA-256; longer content is cut into chunks of that size and named by
//! the SHA-256 of its canonical `fid-chunks@1` manifest. The `fid_store::BlobStore`
//! implementation (feature `store`) stores through `BlobStore::put` and checks that the
//! address it returns is this one.

use crate::error::OocError;
use fid_kci::kci::Sha256;
use std::cell::RefCell;
use std::collections::BTreeMap;

/// The CORE.STO.CAS chunk size: content up to this many bytes is one blob.
pub const STORE_CHUNK_SIZE: u64 = 8 * 1024 * 1024;

fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finish()
}

fn hex(d: &[u8; 32]) -> String {
    let mut s = String::with_capacity(64);
    for b in d {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// The CORE.STO.CAS address of `bytes`: SHA-256 of the bytes up to [`STORE_CHUNK_SIZE`],
/// else SHA-256 of the canonical (RFC 8785) `fid-chunks@1` manifest
/// `{"chunk_size":"8388608","chunks":["sha256:<hex>",...],"format":"fid-chunks@1","size":"<n>"}`.
pub fn blob_address(bytes: &[u8]) -> [u8; 32] {
    if bytes.len() as u64 <= STORE_CHUNK_SIZE {
        return sha256(bytes);
    }
    let mut h = Sha256::new();
    h.update(format!("{{\"chunk_size\":\"{STORE_CHUNK_SIZE}\",\"chunks\":[").as_bytes());
    for (i, c) in bytes.chunks(STORE_CHUNK_SIZE as usize).enumerate() {
        if i > 0 {
            h.update(b",");
        }
        h.update(format!("\"sha256:{}\"", hex(&sha256(c))).as_bytes());
    }
    h.update(format!("],\"format\":\"fid-chunks@1\",\"size\":\"{}\"}}", bytes.len()).as_bytes());
    h.finish()
}

/// A content-addressed store for chunk and manifest blobs.
pub trait ChunkStore {
    /// Stores `bytes` and returns their address, which must equal [`blob_address`].
    fn put_blob(&self, bytes: &[u8]) -> Result<[u8; 32], OocError>;
    /// The verified content stored under `address`, which is `size` bytes long.
    fn get_blob(&self, address: &[u8; 32], size: u64) -> Result<Vec<u8>, OocError>;
}

/// An in-memory [`ChunkStore`] (tests, and callers without a project). Reads are checked
/// against the address like the CORE.STO store's.
#[derive(Debug, Default)]
pub struct MemStore {
    blobs: RefCell<BTreeMap<[u8; 32], Vec<u8>>>,
}

impl MemStore {
    /// An empty store.
    pub fn new() -> MemStore {
        MemStore::default()
    }

    /// Number of stored blobs.
    pub fn len(&self) -> usize {
        self.blobs.borrow().len()
    }

    /// Whether the store holds no blob.
    pub fn is_empty(&self) -> bool {
        self.blobs.borrow().is_empty()
    }

    /// Replaces what is stored under `address` (fault injection in tests).
    pub fn replace(&self, address: [u8; 32], bytes: Vec<u8>) {
        self.blobs.borrow_mut().insert(address, bytes);
    }

    /// Total bytes stored.
    pub fn stored_bytes(&self) -> u64 {
        self.blobs.borrow().values().map(|v| v.len() as u64).sum()
    }
}

impl ChunkStore for MemStore {
    fn put_blob(&self, bytes: &[u8]) -> Result<[u8; 32], OocError> {
        let a = blob_address(bytes);
        self.blobs.borrow_mut().entry(a).or_insert_with(|| bytes.to_vec());
        Ok(a)
    }

    fn get_blob(&self, address: &[u8; 32], size: u64) -> Result<Vec<u8>, OocError> {
        let blobs = self.blobs.borrow();
        let Some(v) = blobs.get(address) else {
            return Err(OocError::Store { code: "not_found", detail: hex(address) });
        };
        if v.len() as u64 != size || blob_address(v) != *address {
            return Err(OocError::Store { code: "integrity", detail: hex(address) });
        }
        Ok(v.clone())
    }
}

#[cfg(feature = "store")]
mod blob_store {
    use super::{ChunkStore, STORE_CHUNK_SIZE};
    use crate::error::OocError;
    use fid_store::cas::{Address, BlobRef, ObjectRef};
    use fid_store::{BlobStore, CasError, PutOpts};
    use fid_types::Digest;

    fn store_err(e: CasError) -> OocError {
        OocError::Store { code: e.code(), detail: e.to_string() }
    }

    /// The store address of content of `size` bytes named `digest`.
    fn address(digest: [u8; 32], size: u64) -> Address {
        let digest = Digest::from_bytes(digest);
        if size <= STORE_CHUNK_SIZE {
            Address::Blob(BlobRef { digest, size })
        } else {
            Address::Object(ObjectRef { manifest_digest: digest, size })
        }
    }

    impl ChunkStore for BlobStore {
        fn put_blob(&self, bytes: &[u8]) -> Result<[u8; 32], OocError> {
            let a = self.put_bytes(bytes, PutOpts::default()).map_err(store_err)?;
            Ok(a.digest().to_bytes())
        }

        fn get_blob(&self, address: &[u8; 32], size: u64) -> Result<Vec<u8>, OocError> {
            let bytes = self.get(&address(*address, size)).map_err(store_err)?;
            if bytes.len() as u64 != size {
                return Err(OocError::Store {
                    code: "integrity",
                    detail: format!("{} bytes, expected {size}", bytes.len()),
                });
            }
            Ok(bytes)
        }
    }
}
