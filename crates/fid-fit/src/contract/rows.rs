//! The command data-row format that declares each `fit.<method>_<primitive>@1` registry
//! command (REQ-KRN.LSQ.TRAIT-009).
//!
//! The kernel may not depend on `CORE`, so fit commands are declared as data: a
//! [`CommandRow`] renders to one canonical JSON object (schema
//! `schemas/fit/command-row-1.schema.json`) that `CORE.REG` reads. A row holds the command
//! ID, the algorithm ID, the side-effect class (`W2`, committing the fitted feature) with
//! its dry-run class (`R1`, the scratch result), the determinism class, and the parameter and
//! result fields. Every numeric field (`number`, `integer`, `number_array`, `integer_array`,
//! `points`) declares a quantity kind (`x-quantity`) and a unit (`x-unit`) from [`QUANTITIES`];
//! non-numeric fields declare neither.
//!
//! # Pinned JSON form
//!
//! ```text
//! {"row":"fit.command_row@1","command":"<name>@<major>","algorithm":"<algorithm id>",
//!  "side_effect":"W2","dry_run":"R1","determinism":"<class>",
//!  "params":[<field>...],"result":[<field>...]}
//! <field> = {"name":"<name>","type":"<type>"[,"x-quantity":"<kind>","x-unit":"<unit>"],
//!            "required":true|false}
//! ```
//!
//! (shown wrapped; the bytes are compact, keys in this order).

use fid_kci::AlgorithmId;
use fid_types::{CommandId, CommandName, Determinism, SideEffect};

use super::error::{alloc_failed, FitError};
use super::primitive::{algorithm_id, Method, Primitive};

/// The row-format identifier written in every row.
pub const ROW_FORMAT: &str = "fit.command_row@1";

/// The quantity kinds and units a numeric field may declare: `(kind, unit)`. Length and
/// angle are the `GEO.FRAME.UNITS` catalogue kinds in their SI units; `dimensionless` and
/// `count` (unit `1`) are not yet in that catalogue (see `CORE.REG.DEF` open questions).
pub const QUANTITIES: [(&str, &str); 4] = [("length", "m"), ("angle", "rad"), ("dimensionless", "1"), ("count", "1")];

/// A field's value type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueType {
    Number,
    Integer,
    NumberArray,
    IntegerArray,
    /// A flat array of xyz triples.
    Points,
    String,
    Boolean,
}

impl ValueType {
    /// The wire string.
    pub const fn as_str(self) -> &'static str {
        match self {
            ValueType::Number => "number",
            ValueType::Integer => "integer",
            ValueType::NumberArray => "number_array",
            ValueType::IntegerArray => "integer_array",
            ValueType::Points => "points",
            ValueType::String => "string",
            ValueType::Boolean => "boolean",
        }
    }

    /// Whether values of this type are numbers (and so need a quantity and unit).
    pub const fn is_numeric(self) -> bool {
        !matches!(self, ValueType::String | ValueType::Boolean)
    }
}

/// One parameter or result field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Field {
    pub name: &'static str,
    pub ty: ValueType,
    /// `(quantity kind, unit)`; `Some` exactly for numeric types.
    pub quantity: Option<(&'static str, &'static str)>,
    pub required: bool,
}

/// One command declaration row.
#[derive(Debug, Clone, PartialEq)]
pub struct CommandRow {
    pub command: CommandId,
    pub algorithm: AlgorithmId,
    pub side_effect: SideEffect,
    pub dry_run: SideEffect,
    pub determinism: Determinism,
    pub params: Vec<Field>,
    pub result: Vec<Field>,
}

/// Why a row is not valid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowError {
    /// A numeric field without a quantity kind and unit.
    MissingQuantity { field: &'static str },
    /// A quantity kind and unit pair that is not in [`QUANTITIES`].
    UnknownQuantity { field: &'static str },
    /// A non-numeric field with a quantity kind.
    UnexpectedQuantity { field: &'static str },
    /// Two fields of one list share a name.
    DuplicateField { field: &'static str },
    /// The side-effect class is not `W2` or the dry-run class not `R1`.
    SideEffect,
    /// The determinism class is `N` (observed); a kernel fit must be reproducible.
    Determinism,
    /// The command name is not `fit.<method>_<primitive>` for the algorithm's method and
    /// primitive, or its major is not 1.
    CommandMismatch,
}

const D: Option<(&str, &str)> = Some(("dimensionless", "1"));
const L: Option<(&str, &str)> = Some(("length", "m"));
const A: Option<(&str, &str)> = Some(("angle", "rad"));
const C: Option<(&str, &str)> = Some(("count", "1"));

const fn fld(
    name: &'static str,
    ty: ValueType,
    quantity: Option<(&'static str, &'static str)>,
    required: bool,
) -> Field {
    Field { name, ty, quantity, required }
}

/// Parameters every fit command takes, in row order.
const PARAMS: [Field; 18] = [
    fld("points", ValueType::Points, L, true),
    fld("normals", ValueType::Points, D, false),
    fld("weights", ValueType::NumberArray, D, false),
    fld("seed", ValueType::Integer, D, false),
    fld("constraints.fixed_radius", ValueType::Number, L, false),
    fld("constraints.fixed_axis_direction", ValueType::NumberArray, D, false),
    fld("constraints.projection_plane.p", ValueType::NumberArray, L, false),
    fld("constraints.projection_plane.n", ValueType::NumberArray, D, false),
    fld("thresholds.rank_tol", ValueType::Number, D, true),
    fld("thresholds.min_coverage", ValueType::Number, A, true),
    fld("nlls.method", ValueType::String, None, true),
    fld("nlls.max_iter", ValueType::Integer, C, true),
    fld("nlls.tol_grad", ValueType::Number, D, true),
    fld("nlls.tol_step", ValueType::Number, D, true),
    fld("nlls.tol_cost", ValueType::Number, D, true),
    fld("nlls.cond_limit", ValueType::Number, D, true),
    fld("nlls.rank_tol", ValueType::Number, D, true),
    fld("nlls.lm_tau", ValueType::Number, D, true),
];

/// Result fields every fit command returns after the feature fields, in row order.
const RESULT_TAIL: [Field; 14] = [
    fld("residuals", ValueType::NumberArray, L, true),
    fld("stats.n", ValueType::Integer, C, true),
    fld("stats.rms", ValueType::Number, L, true),
    fld("stats.max", ValueType::Number, L, true),
    fld("stats.min", ValueType::Number, L, true),
    fld("stats.mean", ValueType::Number, L, true),
    fld("stats.std_dev", ValueType::Number, L, true),
    fld("diagnostics.grad_norm", ValueType::Number, D, true),
    fld("diagnostics.rank", ValueType::Integer, C, true),
    fld("diagnostics.cond", ValueType::Number, D, true),
    fld("diagnostics.iterations", ValueType::Integer, C, true),
    fld("diagnostics.stop", ValueType::String, None, true),
    fld("certificate.support", ValueType::IntegerArray, C, false),
    fld("certificate.gap", ValueType::Number, L, false),
];

/// The feature fields of a primitive, in [`super::FittedFeature`] declaration order.
pub fn feature_fields(p: Primitive) -> &'static [Field] {
    use ValueType::{Integer as I, Number as N, NumberArray as V};
    const POINT: [Field; 1] = [fld("feature.c", V, L, true)];
    const LINE: [Field; 2] = [fld("feature.p", V, L, true), fld("feature.d", V, D, true)];
    const PLANE: [Field; 2] = [fld("feature.p", V, L, true), fld("feature.n", V, D, true)];
    const CIRCLE2: [Field; 2] = [fld("feature.c", V, L, true), fld("feature.r", N, L, true)];
    const CIRCLE3: [Field; 3] =
        [fld("feature.c", V, L, true), fld("feature.n", V, D, true), fld("feature.r", N, L, true)];
    const SPHERE: [Field; 2] = [fld("feature.c", V, L, true), fld("feature.r", N, L, true)];
    const CYLINDER: [Field; 3] =
        [fld("feature.p", V, L, true), fld("feature.d", V, D, true), fld("feature.r", N, L, true)];
    const CONE: [Field; 3] =
        [fld("feature.apex", V, L, true), fld("feature.d", V, D, true), fld("feature.half_angle", N, A, true)];
    const TORUS: [Field; 4] = [
        fld("feature.c", V, L, true),
        fld("feature.n", V, D, true),
        fld("feature.major_r", N, L, true),
        fld("feature.minor_r", N, L, true),
    ];
    const SLOT: [Field; 5] = [
        fld("feature.c", V, L, true),
        fld("feature.n", V, D, true),
        fld("feature.d", V, D, true),
        fld("feature.length", N, L, true),
        fld("feature.width", N, L, true),
    ];
    const ELLIPSE: [Field; 5] = [
        fld("feature.c", V, L, true),
        fld("feature.n", V, D, true),
        fld("feature.d", V, D, true),
        fld("feature.a", N, L, true),
        fld("feature.b", N, L, true),
    ];
    const PARABOLOID: [Field; 3] =
        [fld("feature.apex", V, L, true), fld("feature.d", V, D, true), fld("feature.focal", N, L, true)];
    const POLYLINE: [Field; 1] = [fld("feature.vertices", ValueType::Points, L, true)];
    const SPLINE: [Field; 3] = [
        fld("feature.degree", I, C, true),
        fld("feature.knots", V, D, true),
        fld("feature.control", ValueType::Points, L, true),
    ];
    match p {
        Primitive::Point => &POINT,
        Primitive::Line2 | Primitive::Line3 => &LINE,
        Primitive::Plane => &PLANE,
        Primitive::Circle2 => &CIRCLE2,
        Primitive::Circle3 => &CIRCLE3,
        Primitive::Sphere => &SPHERE,
        Primitive::Cylinder => &CYLINDER,
        Primitive::Cone => &CONE,
        Primitive::Torus => &TORUS,
        Primitive::Slot | Primitive::Rectangle => &SLOT,
        Primitive::Ellipse => &ELLIPSE,
        Primitive::Paraboloid => &PARABOLOID,
        Primitive::Polyline => &POLYLINE,
        Primitive::Spline => &SPLINE,
    }
}

fn copy_fields(parts: &[&[Field]]) -> Result<Vec<Field>, FitError> {
    let n = parts.iter().map(|p| p.len()).sum();
    let mut v = Vec::new();
    v.try_reserve_exact(n).map_err(|_| alloc_failed("fit.rows"))?;
    for p in parts {
        v.extend_from_slice(p);
    }
    Ok(v)
}

/// The command name `fit.<method>_<primitive>`.
pub fn command_name(primitive: Primitive, method: Method) -> String {
    format!("fit.{}_{}", method.slug(), primitive.slug())
}

/// The row of command `fit.<method>_<primitive>@1` for algorithm version `version`:
/// side effect `W2`, dry run `R1`, the common parameters, the primitive's feature fields
/// (prefixed `feature.`) and the common result fields. [`FitError::MethodUndefined`] when the
/// method is not defined for the primitive.
pub fn command_row(
    primitive: Primitive,
    method: Method,
    version: (u32, u32, u32),
    determinism: Determinism,
) -> Result<CommandRow, FitError> {
    let algorithm = algorithm_id(primitive, method, version)?;
    let name = CommandName::parse(&command_name(primitive, method))
        .map_err(|_| FitError::InvalidParams { field: "command" })?;
    let command = CommandId::new(&name, 1).map_err(|_| FitError::InvalidParams { field: "command" })?;
    let alg = [Field { name: "algorithm", ty: ValueType::String, quantity: None, required: true }];
    Ok(CommandRow {
        command,
        algorithm,
        side_effect: SideEffect::W2,
        dry_run: SideEffect::R1,
        determinism,
        params: copy_fields(&[&PARAMS])?,
        result: copy_fields(&[&alg, feature_fields(primitive), &RESULT_TAIL])?,
    })
}

fn check_fields(fields: &[Field]) -> Result<(), RowError> {
    for (i, f) in fields.iter().enumerate() {
        if fields[..i].iter().any(|g| g.name == f.name) {
            return Err(RowError::DuplicateField { field: f.name });
        }
        match (f.ty.is_numeric(), f.quantity) {
            (true, None) => return Err(RowError::MissingQuantity { field: f.name }),
            (true, Some(q)) if !QUANTITIES.contains(&q) => return Err(RowError::UnknownQuantity { field: f.name }),
            (false, Some(_)) => return Err(RowError::UnexpectedQuantity { field: f.name }),
            _ => {}
        }
    }
    Ok(())
}

impl CommandRow {
    /// Checks the row against the row schema: classes `W2` / `R1`, a reproducible
    /// determinism class, the command name matching the algorithm ID with major 1, unique
    /// field names per list, and a quantity kind and unit from [`QUANTITIES`] on exactly the
    /// numeric fields (params first, then result fields, each in order).
    pub fn validate(&self) -> Result<(), RowError> {
        if self.side_effect != SideEffect::W2 || self.dry_run != SideEffect::R1 {
            return Err(RowError::SideEffect);
        }
        if self.determinism == Determinism::N {
            return Err(RowError::Determinism);
        }
        let expected = format!("fit.{}_{}", self.algorithm.method, self.algorithm.feature);
        if self.algorithm.module != "fit" || self.command.name() != expected || self.command.major() != 1 {
            return Err(RowError::CommandMismatch);
        }
        check_fields(&self.params)?;
        check_fields(&self.result)
    }

    /// The canonical JSON bytes of the row (module documentation).
    pub fn to_json(&self) -> Result<Vec<u8>, FitError> {
        let mut s = String::new();
        let mut put = |t: &str| -> Result<(), FitError> {
            s.try_reserve(t.len()).map_err(|_| alloc_failed("fit.rows"))?;
            s.push_str(t);
            Ok(())
        };
        put("{\"row\":\"")?;
        put(ROW_FORMAT)?;
        put("\",\"command\":\"")?;
        put(self.command.as_str())?;
        put("\",\"algorithm\":\"")?;
        put(&self.algorithm.to_string())?;
        put("\",\"side_effect\":\"")?;
        put(self.side_effect.as_str())?;
        put("\",\"dry_run\":\"")?;
        put(self.dry_run.as_str())?;
        put("\",\"determinism\":\"")?;
        put(self.determinism.as_str())?;
        put("\"")?;
        for (key, list) in [("params", &self.params), ("result", &self.result)] {
            put(",\"")?;
            put(key)?;
            put("\":[")?;
            for (i, f) in list.iter().enumerate() {
                if i > 0 {
                    put(",")?;
                }
                put("{\"name\":\"")?;
                put(f.name)?;
                put("\",\"type\":\"")?;
                put(f.ty.as_str())?;
                put("\"")?;
                if let Some((q, u)) = f.quantity {
                    put(",\"x-quantity\":\"")?;
                    put(q)?;
                    put("\",\"x-unit\":\"")?;
                    put(u)?;
                    put("\"")?;
                }
                put(if f.required { ",\"required\":true}" } else { ",\"required\":false}" })?;
            }
            put("]")?;
        }
        put("}")?;
        Ok(s.into_bytes())
    }
}
