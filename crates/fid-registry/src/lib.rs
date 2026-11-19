//! The command registry of Metrology Maven. Every command is declared once as a
//! [`def::CommandDef`]; the CLI, MCP tools, Python bindings and docs are generated from it
//! (CONSTITUTION C3). Modules land one spec leaf at a time; see `spec/core/reg/`.

pub mod def;
pub mod guide;
pub mod lifecycle;
pub mod pred;

pub use def::{
    command_def_schema, CommandDef, DefError, Determinism, FsEffect, Idempotency, Network, Precondition, Projection,
    SideEffect, Surface, Undo, Web, MAX_PRECONDITION_BOUND,
};
pub use guide::{
    check_guides, guides_from_value, guides_to_value, task_guide_schema, GuideError, GuideStep, TaskGuide,
};
pub use lifecycle::{
    check_migrators, check_transition, retained, Date, LifeError, Lifecycle, LifecycleDecl, MigratorRecord, State,
    DEPRECATION_MIN_MONTHS,
};
pub use pred::{
    compile_command, evaluate, BuildError, ContextTypes, Evaluation, FieldPath, ParamSchema, ParseError, PredContext,
    PredError, Predicate, Type, Value, Verdict,
};
