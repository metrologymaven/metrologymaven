//! The project file of this leaf, `fid.gdt-datum-plan/1` (extension `.fidproj`): one plan's
//! nominal faces, measured features, target geometry and declared datums.
//!
//! ```json
//! { "schema": "fid.gdt-datum-plan/1", "plan": "block-1",
//!   "faces": ["face:12", "face:13"],
//!   "features": { "f1": { "type": "plane", "faces": ["face:12"] } },
//!   "geometry": { "pt:1": { "kind": "point", "face": "face:12" } },
//!   "datums": { "working": { "A": { ..definition.. } }, "agent": { } } }
//! ```
//!
//! The file holds no numbers. Loaded datums pass every check of the define command except
//! the label pattern, which belongs to the active defaults profile and not to the file.

use super::model::DatumModel;
use super::types::DatumDefinition;
use super::DatumError;
use crate::chars::dprof::Branch;
use fid_canon::{CValue, CanonError};
use std::collections::BTreeMap;

/// `schema` value of a project file.
pub const PROJECT_SCHEMA: &str = "fid.gdt-datum-plan/1";
/// Largest project file accepted, in bytes.
pub const MAX_PROJECT_BYTES: usize = fid_canon::MAX_INPUT_BYTES;
/// Deepest nesting: document, `datums`, branch, definition, `targets`, target.
const MAX_PROJECT_DEPTH: usize = 6;
const TOP: [&str; 6] = ["datums", "faces", "features", "geometry", "plan", "schema"];

type Obj = BTreeMap<String, CValue>;

fn bad(reason: &'static str) -> DatumError {
    DatumError::ProjectInvalid { reason }
}

fn obj<'a>(v: Option<&'a CValue>, want: Option<&[&str]>) -> Result<&'a Obj, DatumError> {
    match (v, want) {
        (Some(CValue::Obj(o)), None) => Ok(o),
        (Some(CValue::Obj(o)), Some(w)) if o.len() == w.len() && w.iter().all(|k| o.contains_key(*k)) => Ok(o),
        (Some(CValue::Obj(_)), Some(_)) => Err(bad("bad_keys")),
        _ => Err(bad("wrong_type")),
    }
}

fn str_of(v: Option<&CValue>) -> Result<&str, DatumError> {
    match v {
        Some(CValue::Str(s)) => Ok(s),
        _ => Err(bad("wrong_type")),
    }
}

fn strs(v: Option<&CValue>) -> Result<Vec<&str>, DatumError> {
    match v {
        Some(CValue::Arr(a)) => a.iter().map(|x| str_of(Some(x))).collect(),
        _ => Err(bad("wrong_type")),
    }
}

fn has_number(v: &CValue) -> bool {
    match v {
        CValue::Num(_) => true,
        CValue::Arr(a) => a.iter().any(has_number),
        CValue::Obj(o) => o.values().any(has_number),
        _ => false,
    }
}

/// Loads a project file strictly. Checks, in order: size (`project_invalid{too_large}`),
/// JSON with no duplicate key (`project_invalid{duplicate_key}`, else `{json}`; nesting at
/// most 6), no number anywhere (`bad_number`), the six fields (`bad_keys`, `wrong_type`),
/// `bad_schema`, the plan ID, the faces, the features, the geometry (each with the checks of
/// the `DatumModel::add_*` methods, so IDs are unique across all three, ignoring ASCII case),
/// then each branch's datums: the key equals the definition's label (`label_key_mismatch`),
/// the definition reads strictly and canonically, and passes
/// [`DatumModel::check_references`].
pub fn load_project(bytes: &[u8]) -> Result<DatumModel, DatumError> {
    if bytes.len() > MAX_PROJECT_BYTES {
        return Err(bad("too_large"));
    }
    let v = fid_canon::parse_with_limits(bytes, MAX_PROJECT_BYTES, MAX_PROJECT_DEPTH).map_err(|e| match e {
        CanonError::DuplicateKey(_) => bad("duplicate_key"),
        _ => bad("json"),
    })?;
    if has_number(&v) {
        return Err(bad("bad_number"));
    }
    let top = obj(Some(&v), Some(&TOP))?;
    if str_of(top.get("schema"))? != PROJECT_SCHEMA {
        return Err(bad("bad_schema"));
    }
    let mut m = DatumModel::new(str_of(top.get("plan"))?)?;
    for f in strs(top.get("faces"))? {
        m.add_face(f)?;
    }
    for (id, f) in obj(top.get("features"), None)? {
        let f = obj(Some(f), Some(&["faces", "type"]))?;
        m.add_feature(id, str_of(f.get("type"))?, &strs(f.get("faces"))?)?;
    }
    for (id, g) in obj(top.get("geometry"), None)? {
        let g = obj(Some(g), Some(&["face", "kind"]))?;
        m.add_geometry(id, str_of(g.get("kind"))?, str_of(g.get("face"))?)?;
    }
    let datums = obj(top.get("datums"), Some(&["agent", "working"]))?;
    for branch in [Branch::Working, Branch::Agent] {
        for (label, d) in obj(datums.get(branch.as_str()), None)? {
            let def = DatumDefinition::from_cvalue(d, bad)?;
            if def.label().as_str() != label {
                return Err(bad("label_key_mismatch"));
            }
            m.put_loaded(branch, def)?;
        }
    }
    Ok(m)
}

/// Writes the project file as RFC 8785 canonical JSON; [`load_project`] reads it back to an
/// equal model.
pub fn project_bytes(m: &DatumModel) -> Result<Vec<u8>, DatumError> {
    let text = |s: &str| CValue::Str(s.to_owned());
    let features: Obj = m
        .features
        .iter()
        .map(|(id, f)| {
            let mut o = BTreeMap::new();
            o.insert("type".to_owned(), text(f.feature_type.as_str()));
            o.insert("faces".to_owned(), CValue::Arr(f.faces.iter().map(|x| text(x.as_str())).collect()));
            (id.as_str().to_owned(), CValue::Obj(o))
        })
        .collect();
    let geometry: Obj = m
        .geometry
        .iter()
        .map(|(id, g)| {
            let mut o = BTreeMap::new();
            o.insert("kind".to_owned(), text(g.kind.as_str()));
            o.insert("face".to_owned(), text(g.face.as_str()));
            (id.as_str().to_owned(), CValue::Obj(o))
        })
        .collect();
    let side = |map: &BTreeMap<_, DatumDefinition>| {
        CValue::Obj(map.values().map(|d| (d.label().as_str().to_owned(), d.to_cvalue())).collect())
    };
    let mut datums = BTreeMap::new();
    datums.insert("working".to_owned(), side(&m.working));
    datums.insert("agent".to_owned(), side(&m.agent));
    let mut top = BTreeMap::new();
    top.insert("schema".to_owned(), text(PROJECT_SCHEMA));
    top.insert("plan".to_owned(), text(m.plan.as_str()));
    top.insert("faces".to_owned(), CValue::Arr(m.faces.iter().map(|f| text(f.as_str())).collect()));
    top.insert("features".to_owned(), CValue::Obj(features));
    top.insert("geometry".to_owned(), CValue::Obj(geometry));
    top.insert("datums".to_owned(), CValue::Obj(datums));
    let out = fid_canon::to_canonical_bytes(&CValue::Obj(top)).map_err(|_| bad("encode"))?;
    if out.len() > MAX_PROJECT_BYTES {
        return Err(bad("too_large"));
    }
    Ok(out)
}
