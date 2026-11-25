//! The report model of Metrology Maven (spec node `RPT.MODEL` and its children). Modules land
//! one spec leaf at a time; see `spec/rpt/`.
//!
//! - [`bind`]: block binding and resolution of stable IDs against an explicit journal head
//!   (`RPT.MODEL.BIND.RESOLVE`).
//! - [`blocks`]: shipped report block kinds, payload schemas, resolvers and commands
//!   (`RPT.MODEL.BLOCKS`).
//! - [`doc`]: the report document model and its storage and commands (`RPT.MODEL.DOC`).
//! - [`fmt`]: value formatting, decision display and source labels (`RPT.MODEL.FMT`); the
//!   only place where a value of record becomes text.
//! - [`tmpl`]: report templates; the template file format (`RPT.MODEL.TMPL.FORMAT`) and
//!   the slot language (`RPT.MODEL.TMPL.SLOT`), and template versions and commands
//! - [`balloon`]: 2D drawing revision import, sheet coordinates and ballooning
//!   (`RPT.BALLOON`).

pub mod balloon;
pub mod bind;
pub mod blocks;
pub mod doc;
pub mod fmt;
mod json_num;
pub mod tmpl;
