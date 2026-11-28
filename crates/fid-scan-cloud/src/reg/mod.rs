//! Scan registration (spec node `SCAN.CLOUD.REG`). Built so far: the shared building blocks
//! of `SCAN.CLOUD.REG.CTGT`, the oriented-cloud projection target ([`target`]), stride
//! sampling and the overlap fraction ([`overlap`]), the valid-normal mask ([`mask`]) and the
//! errors ([`error`]), with [`target`] re-exporting all of them as the spec's module
//! `reg::target`; and the overlap matrix and pair graph of `SCAN.CLOUD.REG.GRAPH`
//! ([`graph`]); and the registration report and registered-version writer of
//! `SCAN.CLOUD.REG.RREP` ([`report`]).

pub mod error;
pub mod graph;
pub mod mask;
pub mod overlap;
pub mod report;
pub mod target;
mod uuid;
