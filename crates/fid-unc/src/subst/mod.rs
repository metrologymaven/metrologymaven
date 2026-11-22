//! ISO 15530-3 calibrated-workpiece (substitution) uncertainty evaluation (`UNC.MODEL.SUBST`).
//!
//! - [`input`]: `SubstitutionInput`, `CalibratedReference`, `RunSet`, `Conditions`,
//!   `ProcedureProfile`, `CheckedInput`, `SubstError`, and [`input::check`]
//!   (spec node `UNC.MODEL.SUBST.INPUT`).

pub mod input;

pub use input::{
    check, CalibratedReference, CharacteristicKind, CheckedInput, CheckedReference, CheckedRunSet, Conditions,
    InsufficientScope, ProcedureProfile, ReferenceSourceKind, Run, RunSet, SubstError, SubstitutionInput,
    DEFAULT_MIN_REPETITIONS_PER_RUN, DEFAULT_MIN_RUNS, DEFAULT_PROFILE_ID, SUBST_INPUT_SCHEMA,
};
