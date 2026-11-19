//! The report model of Metrology Maven (spec node `RPT.MODEL` and its children). Modules land
//! one spec leaf at a time; see `spec/rpt/`.
//!
//! - [`doc`]: the report document model and its storage and commands (`RPT.MODEL.DOC`).
//! - [`tmpl`]: report templates; so far the template file format (`RPT.MODEL.TMPL.FORMAT`).

pub mod doc;
pub mod tmpl;
