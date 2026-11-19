//! Command Programs for Metrology Maven. Modules land one spec leaf at a time; see `spec/auto/prog/`.

pub mod expr;
pub mod family;
pub mod fmt;

pub use fmt::{
    parse_cost, program_schema, step_fields, Arg, Bound, Branch, LoopMode, Opaque, Program, ProgramError, Step,
    StepKind, Subroutine, Uuid7, VarDecl, FORMAT, FORMAT_VERSION, MAX_BYTES, MAX_COST, MAX_DEPTH, MAX_MAJOR, MAX_STEPS,
    STEP_KINDS,
};

pub use expr::syntax::{parse_expr, print_expr, Expr, ExprKind, ParseError, Span};

pub use family::{
    binding_from_json, binding_from_value, binding_to_value, check_binding, check_decls, Binding, BindingError,
    BoundValue, Family, ParamDecl, ParamType, Variant,
};
