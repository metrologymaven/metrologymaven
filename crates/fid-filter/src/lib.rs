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
//! distances. The other methods (`KRN.OUT.STAT`, `KRN.OUT.RANSAC`), the full record with its
//! impact disclosure (`KRN.OUT.DISC`), the reject-and-refit loop (`KRN.OUT.RUN`) and point
//! selection (`KRN.OUT.SEL`) are separate leaves.

pub mod gate;
pub mod policy;
