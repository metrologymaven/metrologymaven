//! DMT row proposal, review and sign-off workflow (spec node `GDT.RES.ROWFLOW`).
//!
//! Built so far: [`roster`] (`GDT.RES.ROWFLOW.ROSTER`) and [`state`]
//! (`GDT.RES.ROWFLOW.STATE`), the pure revision and review state machine, which reads the
//! project-published [`roster::Roster`].

pub mod roster;
pub mod state;
