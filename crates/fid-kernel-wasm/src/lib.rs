//! The kernel's WASM reference command (spec node `KRN.MAN.WASM`).
//!
//! Built for `wasm32-wasip1`, the binary reads one canonical [`fid_kci::KernelCall`] on stdin
//! ([`call`]), calls [`fid_kci::evaluate`] with the assembled table and writes the canonical
//! result or error to stdout ([`shim`]). The crate is outside the kernel graph and holds no
//! logic beyond reading, calling and writing. `stand_in` carries test algorithms; it is compiled
//! only with the `test-algorithms` feature (or in unit tests), so the reference artifact does
//! not contain them. It stays until the `fid-kernel` assembly crate exists.

pub mod call;
pub mod shim;
#[cfg(any(test, feature = "test-algorithms"))]
pub mod stand_in;
