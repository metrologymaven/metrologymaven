//! Report templates (spec node `RPT.MODEL.TMPL`). Modules land one spec leaf at a time.
//!
//! - [`format`]: the exchanged template file, its bounded parser, schema check and
//!   migration (`RPT.MODEL.TMPL.FORMAT`).
//! - [`slot`]: the binding-rule, sort-key and repetition-rule language of slots and the
//!   ordering contract (`RPT.MODEL.TMPL.SLOT`).

pub mod format;
pub mod slot;
