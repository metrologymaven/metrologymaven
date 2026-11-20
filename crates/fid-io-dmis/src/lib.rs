//! DMIS 5.3 support for Metrology Maven (spec node `IO.DMIS`).
//!
//! This crate holds the conformance-table generator ([`conf`], spec node `IO.DMIS.CONF`),
//! the bounded lexer ([`lex`], spec node `IO.DMIS.PARSE.LEX`) and the generic statement tree
//! with its descriptor registry ([`tree`] and [`registry`], spec node `IO.DMIS.PARSE.TREE`).
//! No grammar family registers descriptors yet (`IO.DMIS.PARSE.GDEF` and `.GEXEC` are not
//! built), so the built-in registry is empty and every statement parses as `Unknown`. The
//! published table (`docs/dmis/`) therefore still declares `none` for every statement; it can
//! never claim more than the support declarations, and every `partial` or `full` declaration
//! must name a test that exists.

pub mod conf;
pub mod inventory;
pub mod lex;
pub mod registry;
pub mod tree;
