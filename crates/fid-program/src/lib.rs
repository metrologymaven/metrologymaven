//! Command Programs for Metrology Maven. Modules land one spec leaf at a time; see `spec/auto/prog/`.

pub mod fmt;

pub use fmt::{
    program_schema, step_fields, Arg, Bound, Branch, LoopMode, Opaque, Program, ProgramError, Step, StepKind,
    Subroutine, Uuid7, VarDecl, FORMAT, FORMAT_VERSION, MAX_BYTES, MAX_DEPTH, MAX_OBJECTS, MAX_STEPS, MAX_TOKENS,
    STEP_KINDS,
};
