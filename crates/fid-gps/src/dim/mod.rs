//! Dimensional checks (spec subsystem `GDT.DIM`): the pure evaluation of each check from
//! measured points. The command layer (gate, resolver, ResultRecord, journal) is in `fid-gdt`.
//!
//! - [`radial`] (`GDT.DIM.RADIAL`): diameter and radius of a least-squares circle, cylinder or
//!   sphere fitted by `fid-fit`, after checking the feature's fit against the association the
//!   resolved operator chain names.
//! - [`size`] (`GDT.DIM.SIZEISO.RANK`): the ISO rank-order statistics over local sizes.
//! - [`section`] (`GDT.DIM.SECT.SPAN`): the 2D line, circle or point fitted to a named span of
//!   a section curve, in the section plane, with the section's ID and digest.

pub mod angle;
pub mod linear;
pub mod ptdev;
pub mod radial;
pub mod section;
pub mod size;
