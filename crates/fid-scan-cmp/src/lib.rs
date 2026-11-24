//! Scan comparison of Metrology Maven (spec node `SCAN.CMP`). Pure Rust, `f64` metres, no
//! I/O, no `unsafe`, no GPU dependency; kernel layer, KNP-1.
//!
//! Implemented leaves:
//! - `SCAN.CMP.DEV.PRIM`: [`deviation`], [`deviation_field`], [`DevParams`], [`NominalRef`],
//!   [`CadNominal`], [`CadHit`], [`DevResult`], [`Method`], [`NoMatchReason`] (see [`dev`]).
//! - `SCAN.CMP.DEV.FIELD`: the [`DeviationField`] element, its crate-private builder and its
//!   canonical digest (see [`field`]).
//!
//! # Only the CPU kernel path builds a field (REQ-SCAN.CMP.DEV.FIELD-002, AC-3)
//!
//! A [`DeviationField`] is read-only from outside this crate. The compiling control block
//! uses the same imports and the read accessors:
//!
//! ```
//! use fid_scan_cmp::field::{DevStatus, DeviationField};
//! fn read(f: &DeviationField) -> (usize, Option<DevStatus>, Option<f64>) {
//!     (f.len(), f.status(0), f.value(0))
//! }
//! ```
//!
//! The builder is private to the crate, so it cannot be named, let alone constructed:
//!
//! ```compile_fail,E0603
//! use fid_scan_cmp::field::FieldBuilder;
//! ```
//!
//! ```compile_fail,E0603
//! let b = fid_scan_cmp::field::FieldBuilder::new(1, todo!());
//! ```
//!
//! A field has no public constructor and no `Default`:
//!
//! ```compile_fail,E0599
//! use fid_scan_cmp::field::DeviationField;
//! let f = DeviationField::new(Vec::new(), Vec::new());
//! ```
//!
//! ```compile_fail,E0599
//! use fid_scan_cmp::field::DeviationField;
//! let f = DeviationField::default();
//! ```
//!
//! Its fields are private, so a struct literal is refused:
//!
//! ```compile_fail,E0451
//! use fid_scan_cmp::field::DeviationField;
//! fn forge(f: &DeviationField) -> DeviationField {
//!     DeviationField { values: Vec::new(), ..f.clone() }
//! }
//! ```
//!
//! And no buffer converts into one, whether a value column or any other type (a GPU
//! preview buffer included):
//!
//! ```compile_fail,E0277
//! use fid_scan_cmp::field::DeviationField;
//! fn convert(values: Vec<f64>) -> DeviationField {
//!     values.into()
//! }
//! ```
//!
//! ```compile_fail,E0277
//! use fid_scan_cmp::field::DeviationField;
//! fn convert(values: &[f32]) -> DeviationField {
//!     DeviationField::from(values)
//! }
//! ```

pub mod dev;
pub mod error;
pub mod field;
mod number;

pub use dev::{
    deviation, deviation_field, BatchConfig, CadHit, CadNominal, DevParams, DevResult, Method, NoMatchReason,
    NominalRef,
};
pub use error::{DevError, FieldError, IncompleteKind};
pub use field::{DevStatus, DeviationField, FieldProvenance, FieldStats};
