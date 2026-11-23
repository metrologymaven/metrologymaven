//! Straightness evaluation modules (`GDT.FORM.STRAIGHT`).
//!
//! - [`line_elements`] (`GDT.FORM.STRAIGHT.LINEEL`): line-element extraction from cutting planes
//!   and per-element 2D straightness evaluation through `GDT.FORM.FCORE`.
//! - [`derived`] (`GDT.FORM.STRAIGHT.DERIVED`): derived median-line straightness with optional MMC.

pub mod derived;
pub mod line_elements;
