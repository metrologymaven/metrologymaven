//! Report layout (`RPT.DESIGN`): the geometry the report designer edits through registry
//! commands. Modules land one spec leaf at a time.
//!
//! - [`page`]: page setup, the page size, orientation, margins, header and footer areas and
//!   the derived body rectangle of every page (`RPT.DESIGN.PAGE`).

pub mod page;
