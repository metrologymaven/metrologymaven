//! GD&T characteristics and the DMT workflow of Metrology Maven (spec domain `GDT`). This crate
//! grows one leaf at a time; today it holds:
//!
//! - [`dmt_flow::roster`] (`GDT.RES.ROWFLOW.ROSTER`): the `dmt-reviewers/v1` roster file,
//!   its loader, its digest and the reviewer role lookup;
//! - [`dmt_flow::state`] (`GDT.RES.ROWFLOW.STATE`): row revisions, the review state machine
//!   (two distinct human roster reviewers, edition-held attestations, reset on any content
//!   change, content digest bound to the reviewed content) and its journal records;
//! - [`dmt_flow::status`] (`GDT.RES.STATUS`): the interpretation status register
//!   `gdt-rule-status/v1` rebuilt from the workflow journal with conflict-of-interest rules,
//!   its verification against `gdt-published/v1`, `status_of`, the trusted-roster loader and
//!   `gdt.export_rule_status@1`;
//! - [`chars::dprof`] (`GDT.CHAR.DPROF`): defaults profiles as immutable, content-addressed,
//!   versioned objects, with the publish and re-point commands, their journal events and
//!   exact replay;
//! - [`chars::spec`] (`GDT.CHAR.SPEC`): the governing spec of a characteristic (dialect,
//!   standard designators with editions, defaults profile), `gdt.set_governing_spec@1`,
//!   creation-time inheritance from plan and tenant defaults, and the evaluation gate;
//! - [`chars::record`] (`GDT.CHAR.RECORD`): the ResultRecord builder every evaluation ends
//!   in, the binding of the UNC decision (GDT never decides conformance), one record per
//!   characteristic per run, and `gdt.get_result@1`;
//! - [`chars::interp`] (`GDT.CHAR.INTERP`): the interpretation block of every record (the
//!   rules used with recomputed digests, their statuses from a verified status register, the
//!   `provisional` / `signed_off` label, the register pin and its replay) and
//!   [`requires_provisional_label`] and [`requires_provisional_label_bytes`];
//! - [`chars::qpid`] (`GDT.CHAR.IDENT`): the stable identity of a characteristic (imported
//!   QPId kept, else one UUIDv7, both through the project ID service and checked for
//!   collisions), the edit check that rejects identity changes, and the native-QPId /
//!   `fx:qpid` mapping of QIF elements;
//! - [`resolve`] (`GDT.RES.RESOLVE`): the commands `gdt.resolve_chain@1` and
//!   `gdt.get_dmt_row@1` over the pure resolver of `fid-gps`, behind the governing-spec gate;
//! - [`drf::datum`] (`GDT.DRF.DATUM`): datum declarations (label, datum feature, nominal faces,
//!   feature type, datum targets) and the command `gdt.define_datum@1`;
//! - [`drf::build`] (`GDT.DRF.BUILD`): DRF construction from declared planar datums at RMB,
//!   simulated from measured points by certified tangential planes, with the commands
//!   `gdt.build_drf@1` and `gdt.get_drf@1`, the provisional label of D-045 and exact replay;
//! - [`dim::radial`] (`GDT.DIM.RADIAL`): the diameter and radius checks `check.eval_diameter@1`
//!   and `check.eval_radius@1` over `fid-gps`'s pure evaluation, their ResultRecord, journal
//!   event and exact replay.
//!
//! Nothing here panics on any input: every outcome is a returned `Result`.

pub mod chars;
pub use chars::interp::{requires_provisional_label, requires_provisional_label_bytes};
pub mod dim;
pub mod dmt_flow;
pub mod drf;
pub mod resolve;
