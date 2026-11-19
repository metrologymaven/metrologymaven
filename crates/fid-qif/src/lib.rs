//! QIF support of Metrology Maven. This crate currently holds spec node `IO.QIF.NORM`:
//!
//! - [`norm`]: the `qif-norm@1` normaliser ([`norm::normalise`]), the comparator that
//!   reports the first difference ([`norm::compare`]), the versioned rule file and its
//!   version check ([`norm::check_version`], error `norm_version_not_bumped`);
//! - [`roundtrip`]: the QIF-XML → QIF-JSON → QIF-XML round-trip harness over a corpus
//!   manifest, with a JSON report and a gate exit code (binary `qif-roundtrip`).
//!
//! XML is read only through the hardened reader of `fid-xml` (no DTD, no entities,
//! bounded). Side effects: none in the library except reading corpus files in
//! [`roundtrip`] (R1). Determinism: D0; no floating point and no hash maps are used.

pub mod norm;
pub mod roundtrip;

pub use norm::{
    check_version, compare, compare_with, normalise, normalise_with, Diff, Equality, NormError, NormRules, LOCK_FILE,
    RULE_FILE,
};
