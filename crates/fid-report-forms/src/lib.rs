//! Metrology Maven FAI and PPAP forms data model and commands (`RPT.FORMS`).
//!
//! Pure integer / canonical JSON crate: zero floating-point types or arithmetic (`RPT` float
//! rule), zero `unsafe`, zero panics (`CONSTITUTION §8`), `BTreeMap`/`BTreeSet` only.

#![forbid(unsafe_code)]

pub mod hdr;
