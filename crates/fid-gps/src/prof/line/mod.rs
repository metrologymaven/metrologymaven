//! Profile of a line (spec node `GDT.PROF.LINE`). Today it holds:
//!
//! - [`sect`] (`GDT.PROF.LINE.SECT`): the section planes a chain names, the band membership
//!   of the measured points and their line-element deviations, ready for the per-section
//!   evaluation (`GDT.PROF.LINE.EVAL`, not built yet).

pub mod sect;
