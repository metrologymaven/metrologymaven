//! The command registry of Metrology Maven. Every command is declared once as a
//! [`def::CommandDef`]; the CLI, MCP tools, Python bindings and docs are generated from it
//! (CONSTITUTION C3). Modules land one spec leaf at a time; see `spec/core/reg/`.

pub mod catalogue;
pub mod compat;
pub mod decl;
pub mod def;
pub mod guide;
pub mod lifecycle;
pub mod manifest;
pub mod pred;
pub mod schema;
pub mod validate;

pub use catalogue::{
    Catalogue, CatalogueError, Factor, KindInfo, QuantityCatalogue, Rational, UnitInfo, CATALOGUE_FORMAT,
};
pub use compat::{
    classify, diff, Change, CommandRef, Kind, LifecycleChange, Manifest, ManifestDiff, ManifestError, Reason,
    MAX_SCHEMA_DEPTH,
};
pub use decl::{FidSchema, HandlerRef};
pub use def::{
    command_def_schema, CommandDef, DefError, Determinism, FsEffect, Idempotency, Network, Precondition, Projection,
    SideEffect, Surface, Undo, Web, MAX_PRECONDITION_BOUND,
};
/// The declaration macros (CORE.REG.DEF.MACRO); `FidSchema` is both the derive and the trait.
pub use fid_registry_macros::{fid_command, fid_commands, FidSchema};
pub use guide::{
    check_guides, guides_from_value, guides_to_value, task_guide_schema, GuideError, GuideStep, TaskGuide,
};
pub use lifecycle::{
    check_migrators, check_transition, retained, Date, LifeError, Lifecycle, LifecycleDecl, MigratorRecord, State,
    DEPRECATION_MIN_MONTHS,
};
pub use manifest::{
    check_manifest, export, export_with, manifest_schema, Export, ManifestInForce, MANIFEST_FORMAT, MAX_COMMANDS,
    MAX_MANIFEST_BYTES,
};
pub use pred::{
    compile_command, evaluate, BuildError, ContextTypes, Evaluation, FieldPath, ParamSchema, ParseError, PredContext,
    PredError, Predicate, Type, Value, Verdict,
};
pub use schema::{
    check_schema, check_schema_json, registry_keywords_schema, EntityType, Keyword, ProjectKeyword, SchemaCode,
    SchemaIssue, SelectorForm, SCHEMA_MAX_BYTES, SCHEMA_MAX_DEPTH,
};
pub use validate::{
    errors_to_value, is_valid_id, max_projection, tool_name, unit_kind, validate, validate_json, validate_with,
    CommandDecl, GeoFrameUnits, InputError, Limits, QuantityKinds, RuleCode, ValidationError,
};
