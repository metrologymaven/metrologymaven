//! Command Programs for Metrology Maven. Modules land one spec leaf at a time; see `spec/auto/prog/`.

pub mod expr;
pub mod family;
pub mod flow;
pub mod fmt;
pub mod oper;

pub use fmt::{
    parse_cost, program_schema, step_fields, Arg, Bound, Branch, LoopMode, Opaque, Program, ProgramError, Step,
    StepKind, Subroutine, Uuid7, VarDecl, FORMAT, FORMAT_VERSION, MAX_BYTES, MAX_COST, MAX_DEPTH, MAX_MAJOR, MAX_STEPS,
    STEP_KINDS,
};

pub use expr::syntax::{parse_expr, print_expr, Expr, ExprKind, ParseError, Span};
pub use expr::types::{check_types, type_of, Diag, DiagCode, Dim, Kind, RecordType, Site, Type, TypeReport, Typed};

pub use family::{
    binding_from_json, binding_from_value, binding_to_value, check_binding, check_decls, Binding, BindingError,
    BoundValue, Family, ParamDecl, ParamType, Variant,
};

pub use flow::{
    analyse_flow, iteration_limits, Cfg, EdgeKind, FlowCode, FlowDiag, FlowReport, RunStop, Severity, SubId, Target,
};

pub use oper::{
    answer_from_json, operator_schema, validate, AnswerValue, AuthoredText, Decimal, ErrorCode, Field, FieldError,
    OperatorDescriptor, ResponseType, Rules, UntrustedText, ValidAnswer, OPERATOR_FORMAT,
};
