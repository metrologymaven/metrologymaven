//! DMT row proposal, review and sign-off workflow (spec node `GDT.RES.ROWFLOW`).
//!
//! Built so far: [`roster`] (`GDT.RES.ROWFLOW.ROSTER`). The state machine (`GDT.RES.ROWFLOW.STATE`)
//! consumes the roster through [`roster::RosterView`].

pub mod roster;
