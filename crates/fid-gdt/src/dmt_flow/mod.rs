//! DMT row proposal, review and sign-off workflow (spec node `GDT.RES.ROWFLOW`) and the
//! interpretation status register built from its journal (`GDT.RES.STATUS`).
//!
//! Built so far: [`roster`] (`GDT.RES.ROWFLOW.ROSTER`), [`state`]
//! (`GDT.RES.ROWFLOW.STATE`), the pure revision and review state machine, which reads the
//! project-published rosters through [`status::TrustedRosters`], and [`status`]
//! (`GDT.RES.STATUS`), the status register, its verification and the status lookup.

pub mod roster;
pub mod state;
pub mod status;
