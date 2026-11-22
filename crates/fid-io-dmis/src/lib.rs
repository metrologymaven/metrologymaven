//! DMIS 5.3 support for Metrology Maven (spec node `IO.DMIS`).
//!
//! This crate holds the conformance-table generator ([`conf`], spec node `IO.DMIS.CONF`),
//! the bounded lexer ([`lex`], spec node `IO.DMIS.PARSE.LEX`) and the generic statement tree
//! with its descriptor registry ([`tree`] and [`registry`], spec node `IO.DMIS.PARSE.TREE`).
//! The grammar family for definition and result statements ([`gdef`], spec node
//! `IO.DMIS.PARSE.GDEF`) fills the built-in registry; program-only statements
//! (`IO.DMIS.PARSE.GEXEC`, not built) still parse as `Unknown`. The canonical printer and
//! parse-print-parse identity harness live in [`print`] (spec node `IO.DMIS.PARSE.PRINT`).
//! The published table (`docs/dmis/`) declares `partial` import for the GDEF keywords; it can
//! never claim more than the support declarations, and every `partial` or `full` declaration
//! must name a test that exists.

pub mod conf;
pub mod gdef;
pub mod inventory;
pub mod lex;
pub mod print;
pub mod registry;
pub mod tree;
