//! The profile family (spec node `GDT.PROF`). Today it holds:
//!
//! - [`dev`] (`GDT.PROF.DEV`): the per-point signed deviations from the nominal CAD faces,
//!   with each face's nominal approximation error, that the profile and runout zones consume.
//! - [`zdef`] (`GDT.PROF.ZDEF.UNIF`): the constant-width zone definitions (bilateral,
//!   unilateral, unequally disposed) as one allowed interval on those deviations, with the
//!   disposition shift of the offset zone shape, the `zone_definition` record fragment and
//!   the `UNSUPPORTED` dispatch.
//! - [`line`] (`GDT.PROF.LINE`): profile of a line; today its section bands and line-element
//!   deviations ([`line::sect`], `GDT.PROF.LINE.SECT`).

pub mod dev;
pub mod line;
pub mod zdef;
