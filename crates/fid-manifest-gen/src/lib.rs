//! Build-time kernel manifest generator for Metrology Maven (spec node `KRN.MAN.MANI`).
//!
//! This crate is a build-time tool: it is never linked into the shipped kernel artifact and
//! may read files. Modules land one spec leaf at a time:
//!
//! - [`implclosure`]: per-algorithm implementation digests over the crate closure
//!   (`KRN.MAN.MANI.IMPL`).
//!
//! The manifest schema, its canonical encoding and its digests live in the kernel crate
//! (`fid_kci::manifest`, `KRN.MAN.MANI.SCHEMA`); this crate reuses them and the kernel's
//! SHA-256 and canonical JSON writer rather than carrying its own.

pub mod implclosure;
