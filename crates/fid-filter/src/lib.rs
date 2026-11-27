//! Point selection and outlier policies for kernel fits. Pure Rust, `f64`, single-threaded,
//! no I/O, no `unsafe` (KNP-1, CONSTITUTION §4).
//!
//! Modules land one spec leaf at a time; see `spec/krn/out/`. This crate starts with the
//! policy model ([`policy`], spec node `KRN.OUT.POL`): the [`policy::OutlierPolicy`] types and
//! their validation, the [`policy::PolicyMethod`] trait every method implements, the
//! [`policy::ExclusionMask`] with its digest, the two guards every method obeys, and
//! [`policy::apply`], the one entry point in the kernel through which a fit's input set is
//! reduced (REQ-KRN.OUT.POL-006). Methods: [`gate`] (spec node `KRN.OUT.GATE`), the
//! distance-to-nominal gate and the tolerance band over caller-supplied signed nominal
//! distances; [`stat`] (spec node `KRN.OUT.STAT`), the n-sigma and robust-scale policies on
//! the fit's residuals. [`disclose`] (spec node `KRN.OUT.DISC`) is the full record of a
//! removal, [`disclose::ExclusionRecord`], with its impact disclosure (the fit before and
//! after the exclusion and their differences) and its canonical encoding. The other method
//! (`KRN.OUT.RANSAC`), the reject-and-refit loop (`KRN.OUT.RUN`) and point selection
//! (`KRN.OUT.SEL`) are separate leaves.

pub mod disclose;
pub mod gate;
pub mod policy;
pub mod stat;
