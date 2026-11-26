//! Report layout (`RPT.DESIGN`): the geometry the report designer edits through registry
//! commands. Modules land one spec leaf at a time.
//!
//! - [`page`]: page setup, the page size, orientation, margins, header and footer areas and
//!   the derived body rectangle of every page (`RPT.DESIGN.PAGE`).
//! - [`frame`]: block frames on the page body, move, resize, page moves and grid snapping
//!   (`RPT.DESIGN.PLACE.FRAME`).

pub mod frame;
pub mod page;
