//! Scan registration (spec node `SCAN.CLOUD.REG`). Only the shared building blocks of
//! `SCAN.CLOUD.REG.CTGT` are built so far: the oriented-cloud projection target ([`target`]),
//! stride sampling and the overlap fraction ([`overlap`]), the valid-normal mask ([`mask`]) and
//! the errors ([`error`]). [`target`] re-exports all of them, as the spec's module
//! `reg::target`.

pub mod error;
pub mod mask;
pub mod overlap;
pub mod target;
