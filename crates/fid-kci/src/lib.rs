//! Kernel call interface for Metrology Maven. Modules land one spec leaf at a time;
//! see `spec/krn/man/`.

pub mod cards;

pub use cards::{check, parse_card, AlgorithmId, CardError, CheckReport, SpecCard};
