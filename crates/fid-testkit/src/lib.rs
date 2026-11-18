//! Test-support helpers for Metrology Maven crates.
//!
//! This crate is in the `test-support` layer of `layers.toml`: any crate may use it as a
//! dev-dependency, and none may use it as a normal dependency (ECO.REPO.LAYERS). It lets
//! dependency-free kernel crates check golden digests in their tests.

use sha2::{Digest, Sha256};

/// Incremental SHA-256 that prints as `sha256:<64 lowercase hex>`.
#[derive(Default, Clone)]
pub struct Sha256Hasher(Sha256);

impl Sha256Hasher {
    /// A new, empty hasher.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds bytes.
    pub fn update(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }

    /// The digest as `sha256:<hex>`.
    pub fn finish(self) -> String {
        let mut out = String::from("sha256:");
        for b in self.0.finalize() {
            out.push_str(&format!("{b:02x}"));
        }
        out
    }
}

/// `sha256:<hex>` of a byte string.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256Hasher::new();
    h.update(bytes);
    h.finish()
}
