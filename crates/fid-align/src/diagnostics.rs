//! `align.get_diagnostics@1` (R0, D0; spec node `ALN.QA.COND` REQ-005): the read of an
//! alignment's conditioning diagnostics, warnings and convergence history.
//!
//! The diagnostics are computed by `fid_align_solve::diagnose` when the solver finishes and are
//! stored with the residual report ([`ResidualStore::insert_diagnostics`]). This module only
//! resolves `alignment -> report reference -> stored report and diagnostics`, checks the stored
//! report against the digest the alignment recorded (so the diagnostics read are those of the
//! run that produced the alignment's report), and writes canonical JSON. No floating-point
//! arithmetic happens here: values are copied bit for bit.
//!
//! # Parameters
//!
//! `alignment` (required, canonical ID). CLI: `fid align get-diagnostics <alignment> [--json]`.
//!
//! # Result
//!
//! `{"alignment", "conditioning", "history", "report", "warnings"}`:
//!
//! - `report`: `{"digest", "id"}` of the residual report the diagnostics belong to.
//! - `conditioning`: `{"centroid", "condition_number", "length_scale", "n_points", "n_rows",
//!   "n_unobservable", "params", "pivots", "singular_values", "threshold", "unobservable_ratio",
//!   "weakest_dof"}`. Lengths are `{"unit": "m", "value": ..}` and angles
//!   `{"unit": "rad", "value": ..}`; dimensionless values (singular values, the condition
//!   number, the threshold, the ratio and the singular vector) are bit-exact `"0x"` + 16 hex
//!   digits, as residual weights are, because the registry's quantity catalogue has no
//!   dimensionless kind. `condition_number` is `null` when infinite. `pivots` is
//!   `{"rx", "ry", "rz"}`, each `"centroid"`, `"frame_origin"` or `null` (rotation not free).
//!   `threshold` is
//!   `{"basis": "default_unverified" | "caller", "value"}`: the default threshold is a claimed,
//!   uncalibrated value and says so (D-045);
//!   `unobservable_ratio` is `{"basis": "default_unverified", "value"}` for the same reason (its
//!   guard constant 8 is not calibrated). `weakest_dof` is `{"angular", "axis", "kind",
//!   "linear", "name", "vector"}`.
//! - `warnings`: at most one, `{"code": "unobservable_dof", "count", "weakest_dof"}` or
//!   `{"code": "ill_conditioned", "condition_number", "threshold", "weakest_dof"}`, with
//!   `weakest_dof` the name.
//! - `history`: `null` for a closed-form method, else `{"digest", "n_rows", "rows",
//!   "stopped_by"}` with columnar `rows` `{"d_rotation", "d_translation", "iteration", "n_used",
//!   "objective"}` (`objective` bit-exact, its unit being the method's) and `stopped_by`
//!   `{"criterion", "outcome"}`.

use crate::element::{id_value, obj, parse_id};
use crate::error::AlignError;
use crate::model::AlignmentModel;
use crate::residuals::ResidualStore;
use fid_align_solve::{AlignmentDiagnostics, Axis, CondWarning, ConvergenceHistory, Threshold};
use fid_canon::CValue;
use fid_uuid::Uuid;
use std::collections::BTreeMap;

/// Command ID of the diagnostics read.
pub const GET_DIAGNOSTICS: &str = "align.get_diagnostics@1";
/// CLI subcommand of `fid align` for the diagnostics read.
pub const CLI_DIAGNOSTICS: &str = "get-diagnostics";
/// MCP tool name of the diagnostics read.
pub const MCP_DIAGNOSTICS: &str = "align_get_diagnostics";

/// Parameters of `align.get_diagnostics@1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiagnosticsParams {
    pub alignment: Uuid,
}

impl DiagnosticsParams {
    /// Reads the parameter object; refuses an unknown key or a malformed value (`bad_params`).
    pub fn from_value(v: &CValue) -> Result<DiagnosticsParams, AlignError> {
        let CValue::Obj(o) = v else { return Err(AlignError::BadParams("params is not an object")) };
        if o.keys().any(|k| k != "alignment") {
            return Err(AlignError::BadParams("unknown parameter"));
        }
        let alignment = match o.get("alignment") {
            Some(CValue::Str(s)) => parse_id(s).ok_or(AlignError::BadParams("alignment is not a canonical id"))?,
            Some(_) => return Err(AlignError::BadParams("alignment is not a canonical id")),
            None => return Err(AlignError::BadParams("missing alignment")),
        };
        Ok(DiagnosticsParams { alignment })
    }
}

/// Reads `get-diagnostics` CLI arguments (after the subcommand) into the MCP parameter object:
/// `<alignment> [--json]`. Unknown or repeated options and a second positional argument are
/// refused (`bad_params`).
pub(crate) fn cli_params(rest: &[&str]) -> Result<CValue, AlignError> {
    let mut params = BTreeMap::new();
    let mut json = false;
    for arg in rest {
        match *arg {
            "--json" if !json => json = true,
            "--json" => return Err(AlignError::BadParams("repeated option")),
            s if s.starts_with("--") => return Err(AlignError::BadParams("unknown option")),
            s => {
                if params.insert("alignment".to_string(), CValue::Str(s.to_string())).is_some() {
                    return Err(AlignError::BadParams("unexpected argument"));
                }
            }
        }
    }
    Ok(CValue::Obj(params))
}

// ------------------------------------------------------------------ result

fn num(n: u64) -> CValue {
    // Counts here are below 2^53, so the conversion is exact.
    CValue::Num(n as f64)
}

fn text(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

fn quantity(unit: &str, value: CValue) -> CValue {
    obj([("unit", text(unit)), ("value", value)])
}

fn numbers(xs: &[f64]) -> CValue {
    CValue::Arr(xs.iter().map(|x| CValue::Num(*x)).collect())
}

fn bit_list(xs: &[f64]) -> CValue {
    CValue::Arr(xs.iter().map(|x| CValue::F64Bits(*x)).collect())
}

fn opt_bits(x: Option<f64>) -> CValue {
    x.map_or(CValue::Null, CValue::F64Bits)
}

fn threshold_value(t: &Threshold) -> CValue {
    obj([("basis", text(t.basis().as_str())), ("value", CValue::F64Bits(t.value()))])
}

/// The `conditioning` object of stored diagnostics.
pub fn conditioning_value(d: &AlignmentDiagnostics) -> CValue {
    let pivot = |a: Axis| d.pivot(a).map_or(CValue::Null, |p| text(p.as_str()));
    let pivots = obj([("rx", pivot(Axis::X)), ("ry", pivot(Axis::Y)), ("rz", pivot(Axis::Z))]);
    let w = d.weakest_dof();
    let weakest = obj([
        ("angular", quantity("rad", numbers(&w.angular()))),
        ("axis", w.kind().axis().map_or(CValue::Null, |a| text(a.as_str()))),
        ("kind", text(w.kind().kind_str())),
        ("linear", quantity("m", numbers(&w.linear()))),
        ("name", CValue::Str(w.name())),
        ("vector", bit_list(w.vector())),
    ]);
    obj([
        ("centroid", quantity("m", numbers(&d.centroid()))),
        ("condition_number", opt_bits(d.condition_number())),
        ("length_scale", quantity("m", CValue::Num(d.length_scale()))),
        ("n_points", num(d.n_points() as u64)),
        ("n_rows", num(d.n_rows() as u64)),
        ("n_unobservable", num(d.n_unobservable() as u64)),
        ("params", CValue::Arr(d.params().iter().map(|p| text(p.name())).collect())),
        ("pivots", pivots),
        ("singular_values", bit_list(d.singular_values())),
        ("threshold", threshold_value(&d.threshold())),
        (
            "unobservable_ratio",
            obj([("basis", text("default_unverified")), ("value", CValue::F64Bits(d.unobservable_ratio()))]),
        ),
        ("weakest_dof", weakest),
    ])
}

/// The `warnings` array of stored diagnostics.
pub fn warnings_value(d: &AlignmentDiagnostics) -> CValue {
    let name = d.weakest_dof().name();
    CValue::Arr(
        d.warnings()
            .iter()
            .map(|w| match w {
                CondWarning::UnobservableDof { count } => obj([
                    ("code", text(w.code())),
                    ("count", num(*count as u64)),
                    ("weakest_dof", CValue::Str(name.clone())),
                ]),
                CondWarning::IllConditioned { condition_number, threshold } => obj([
                    ("code", text(w.code())),
                    ("condition_number", opt_bits(*condition_number)),
                    ("threshold", threshold_value(threshold)),
                    ("weakest_dof", CValue::Str(name.clone())),
                ]),
            })
            .collect(),
    )
}

/// The `history` object, or `null` for a closed-form method.
pub fn history_value(h: Option<&ConvergenceHistory>) -> CValue {
    let Some(h) = h else { return CValue::Null };
    let ints = |xs: &[u64]| CValue::Arr(xs.iter().map(|n| num(*n)).collect());
    obj([
        ("digest", CValue::Str(h.digest().as_str().to_string())),
        ("n_rows", num(h.len() as u64)),
        (
            "rows",
            obj([
                ("d_rotation", quantity("rad", numbers(h.d_rotations()))),
                ("d_translation", quantity("m", numbers(h.d_translations()))),
                ("iteration", ints(h.iterations())),
                ("n_used", ints(h.n_used())),
                ("objective", bit_list(h.objectives())),
            ]),
        ),
        (
            "stopped_by",
            obj([
                ("criterion", text(h.stopped_by().criterion())),
                ("outcome", text(h.stopped_by().outcome().as_str())),
            ]),
        ),
    ])
}

/// The diagnostics read. Fails `not_found` for an unknown alignment, a report missing from the
/// store or a report stored without diagnostics, and `digest_mismatch` if the stored report is
/// not the one the alignment recorded.
pub fn get_diagnostics(
    model: &AlignmentModel,
    store: &ResidualStore,
    params: &DiagnosticsParams,
) -> Result<CValue, AlignError> {
    let alignment = model.get(&params.alignment).ok_or(AlignError::NotFound(params.alignment))?;
    let rref = alignment.report();
    let report = store.get(&rref.id).ok_or(AlignError::NotFound(rref.id))?;
    if report.digest() != rref.digest {
        return Err(AlignError::DigestMismatch);
    }
    let d = store.diagnostics(&rref.id).ok_or(AlignError::NotFound(rref.id))?;
    Ok(obj([
        ("alignment", id_value(&params.alignment)),
        ("conditioning", conditioning_value(d)),
        ("history", history_value(d.history())),
        ("report", rref.to_value()),
        ("warnings", warnings_value(d)),
    ]))
}

// ------------------------------------------------------------------ result schema

fn texts(items: &[&str]) -> CValue {
    CValue::Arr(items.iter().map(|s| text(s)).collect())
}

fn int(nullable: bool) -> CValue {
    let ty = if nullable { texts(&["integer", "null"]) } else { text("integer") };
    obj([("type", ty), ("minimum", num(0))])
}

/// A closed object with every listed property required.
fn closed(ty: CValue, props: Vec<(&str, CValue)>) -> CValue {
    let names: Vec<&str> = props.iter().map(|(k, _)| *k).collect();
    obj([
        ("type", ty),
        ("additionalProperties", CValue::Bool(false)),
        ("required", texts(&names)),
        ("properties", CValue::Obj(props.into_iter().map(|(k, v)| (k.to_string(), v)).collect())),
    ])
}

fn object(props: Vec<(&str, CValue)>) -> CValue {
    closed(text("object"), props)
}

fn number(kind: &str, unit: &str) -> CValue {
    obj([("type", text("number")), ("x-quantity", text(kind)), ("x-unit", text(unit))])
}

/// `{"unit", "value"}` with a number (`len` None) or an array of `len` numbers.
fn quantity_schema(kind: &str, unit: &str, len: Option<u64>) -> CValue {
    let value = match len {
        None => number(kind, unit),
        Some(n) => {
            obj([("type", text("array")), ("minItems", num(n)), ("maxItems", num(n)), ("items", number(kind, unit))])
        }
    };
    object(vec![("unit", obj([("const", text(unit))])), ("value", value)])
}

fn bits_schema(nullable: bool) -> CValue {
    let ty = if nullable { texts(&["string", "null"]) } else { text("string") };
    obj([("type", ty), ("pattern", text(crate::residuals::BITS_PATTERN))])
}

fn bits_array(min: u64) -> CValue {
    obj([("type", text("array")), ("minItems", num(min)), ("maxItems", num(6)), ("items", bits_schema(false))])
}

fn enum_of(items: &[&str]) -> CValue {
    obj([("enum", texts(items))])
}

fn pivot_schema() -> CValue {
    obj([("enum", CValue::Arr(vec![text("centroid"), text("frame_origin"), CValue::Null]))])
}

fn threshold_schema() -> CValue {
    object(vec![("basis", enum_of(&["caller", "default_unverified"])), ("value", bits_schema(false))])
}

/// The result schema of `align.get_diagnostics@1`.
pub fn result_schema() -> CValue {
    let id = obj([("type", text("string")), ("pattern", text(crate::command::ID_PATTERN))]);
    let digest = obj([("type", text("string")), ("pattern", text("^sha256:[0-9a-f]{64}$"))]);
    let name = obj([("type", text("string")), ("minLength", num(1))]);
    let weakest = object(vec![
        ("angular", quantity_schema("angle", "rad", Some(3))),
        ("axis", obj([("enum", CValue::Arr(vec![text("x"), text("y"), text("z"), CValue::Null]))])),
        ("kind", enum_of(&["rotation", "translation", "twist"])),
        ("linear", quantity_schema("length", "m", Some(3))),
        ("name", name.clone()),
        ("vector", bits_array(1)),
    ]);
    let conditioning = object(vec![
        ("centroid", quantity_schema("length", "m", Some(3))),
        ("condition_number", bits_schema(true)),
        ("length_scale", quantity_schema("length", "m", None)),
        ("n_points", int(false)),
        ("n_rows", int(false)),
        ("n_unobservable", int(false)),
        (
            "params",
            obj([
                ("type", text("array")),
                ("minItems", num(1)),
                ("maxItems", num(6)),
                ("items", enum_of(&["rx", "ry", "rz", "tx", "ty", "tz"])),
            ]),
        ),
        ("pivots", object(vec![("rx", pivot_schema()), ("ry", pivot_schema()), ("rz", pivot_schema())])),
        ("singular_values", bits_array(1)),
        ("threshold", threshold_schema()),
        (
            "unobservable_ratio",
            object(vec![("basis", enum_of(&["default_unverified"])), ("value", bits_schema(false))]),
        ),
        ("weakest_dof", weakest),
    ]);
    let unobservable = object(vec![
        ("code", obj([("const", text("unobservable_dof"))])),
        ("count", obj([("type", text("integer")), ("minimum", num(1)), ("maximum", num(6))])),
        ("weakest_dof", name.clone()),
    ]);
    let ill = object(vec![
        ("code", obj([("const", text("ill_conditioned"))])),
        ("condition_number", bits_schema(true)),
        ("threshold", threshold_schema()),
        ("weakest_dof", name),
    ]);
    let warnings = obj([
        ("type", text("array")),
        ("maxItems", num(1)),
        ("items", obj([("oneOf", CValue::Arr(vec![ill, unobservable]))])),
    ]);
    let ints = obj([("type", text("array")), ("items", int(false))]);
    let numbers_of = |kind: &str, unit: &str| {
        object(vec![
            ("unit", obj([("const", text(unit))])),
            ("value", obj([("type", text("array")), ("items", number(kind, unit))])),
        ])
    };
    let history = closed(
        texts(&["object", "null"]),
        vec![
            ("digest", digest.clone()),
            ("n_rows", int(false)),
            (
                "rows",
                object(vec![
                    ("d_rotation", numbers_of("angle", "rad")),
                    ("d_translation", numbers_of("length", "m")),
                    ("iteration", ints.clone()),
                    ("n_used", ints),
                    ("objective", obj([("type", text("array")), ("items", bits_schema(false))])),
                ]),
            ),
            (
                "stopped_by",
                object(vec![
                    ("criterion", obj([("type", text("string")), ("pattern", text("^[a-z0-9_]{1,64}$"))])),
                    ("outcome", enum_of(&["converged", "diverged", "limit"])),
                ]),
            ),
        ],
    );
    let report = object(vec![("digest", digest), ("id", id.clone())]);
    object(vec![
        ("alignment", id),
        ("conditioning", conditioning),
        ("history", history),
        ("report", report),
        ("warnings", warnings),
    ])
}
