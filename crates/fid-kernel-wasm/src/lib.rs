//! The kernel's WASM reference command (spec node `KRN.MAN.WASM`).
//!
//! Built for `wasm32-wasip1`, the binary reads one canonical [`fid_kci::KernelCall`] on stdin
//! ([`call`]), calls [`fid_kci::evaluate`] with the assembled table and writes the canonical
//! result or error to stdout ([`shim`]). The crate is outside the kernel graph and holds no
//! logic beyond reading, calling and writing; [`stand_in`] carries test algorithms that only
//! the `test-algorithms` feature registers, until the `fid-kernel` assembly crate exists.

pub mod call;
pub mod shim;
pub mod stand_in;
