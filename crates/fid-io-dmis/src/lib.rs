//! DMIS 5.3 support for Metrology Maven (spec node `IO.DMIS`).
//!
//! So far this crate holds only the conformance-table generator ([`conf`], spec node
//! `IO.DMIS.CONF`). There is no DMIS parser, reader or writer yet, so the published table
//! (`docs/dmis/`) declares `none` for every statement; it can never claim more than the
//! support declarations, and every `partial` or `full` declaration must name a test that exists.

pub mod conf;
