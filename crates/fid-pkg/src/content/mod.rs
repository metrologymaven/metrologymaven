//! Result Package export content assembly (spec node `TRUST.PKG.CONTENT`, feature `export`).
//!
//! Today this module holds [`evid`] (`TRUST.PKG.CONTENT.EVID`): evidence-tier application,
//! streamed point-set gathering with SHA-256 verification and deduplication, and
//! interpretation register + roster gathering (D-045).

pub mod evid;
