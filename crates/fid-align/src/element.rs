//! The `Alignment` element: fields, canonical form, strict decoding and content digest
//! (REQ-001, REQ-003).
//!
//! # Element form (RFC 8785 canonical JSON, every key always present)
//!
//! ```text
//! { "algorithm":  "module.feature.method@1.2.3",
//!   "digest":     "sha256:<64 hex>",
//!   "failure":    null | "<code>",            (a code exactly when status is "failed")
//!   "id":         "<uuid>",
//!   "inputs":     [ {"digest": "sha256:..", "id": "<uuid>"}, ... ],   (command order)
//!   "params":     { "<name>": {"si": "0x<16 hex>", "text": "<decimal>", "unit": "<symbol>"}, ... },
//!   "parent":     null | {"digest": "sha256:..", "id": "<uuid>"},
//!   "provenance": {"kind": "human"|"agent", "principal": "<id>", "state": "human"|"proposed"|"adopted"},
//!   "report":     {"digest": "sha256:..", "id": "<uuid>"},
//!   "status":     "ok"|"not_converged"|"ambiguous"|"failed"|"outdated",
//!   "transform":  {"kind": "rigid", "r": [[r00, r01, r02], [..], [..]], "t": [t0, t1, t2]}
//!               | {"kind": "similarity", "r": .., "s": s, "t": ..} }
//! ```
//!
//! Every float is `"0x"` plus the 16 lowercase hex digits of its IEEE 754 bits
//! ([`fid_canon::CValue::F64Bits`]), so values are copied, never re-rounded. Transforms are
//! stored canonically: `-0.0` in the rotation or translation becomes `+0.0` before digesting,
//! and a similarity scale must be a positive normal float.
//!
//! # Content digest (REQ-003)
//!
//! SHA-256 over the canonical bytes of
//! `{"algorithm", "inputs": [input digests in command order], "params": {name: si bits},
//! "parent": null | {"digest", "id"}, "report": report digest, "transform"}`.
//! Status, failure, provenance, the element's own ID and input IDs are not hashed.

use crate::error::{kept, AlignError};
use crate::solver::code_ok;
use fid_canon::{parse_with_limits, CValue, CanonError};
use fid_geo_frame::{AuthoredQuantity, Rigid, Similarity, Transform, Unit, UnitsError, XformError};
use fid_kci::AlgorithmId;
use fid_types::Digest;
use fid_uuid::Uuid;
use std::collections::{BTreeMap, BTreeSet};

/// Most inputs of one alignment.
pub const MAX_INPUTS: usize = 1024;
/// Most parameters of one alignment.
pub const MAX_PARAMS: usize = 64;
/// Longest parameter name, in bytes.
pub const MAX_PARAM_NAME: usize = 64;
/// Longest principal ID, in bytes.
pub const MAX_PRINCIPAL_BYTES: usize = 128;
/// Largest serialised alignment [`Alignment::from_json`] reads, in bytes.
pub const MAX_ELEMENT_BYTES: usize = 1024 * 1024;
/// Deepest nesting [`Alignment::from_json`] reads (the element form needs 4).
const MAX_ELEMENT_DEPTH: usize = 8;
/// Longest algorithm ID text, in bytes.
const MAX_ALGORITHM_BYTES: usize = 256;

// ------------------------------------------------------------------ small values

/// Parses a canonical entity ID: lowercase hyphenated, neither nil nor max.
pub(crate) fn parse_id(text: &str) -> Option<Uuid> {
    let id = Uuid::parse(text).ok()?;
    (id.to_string() == text && id != Uuid::NIL && id != Uuid::MAX).then_some(id)
}

pub(crate) fn id_value(id: &Uuid) -> CValue {
    CValue::Str(id.to_string())
}

fn digest_value(d: &Digest) -> CValue {
    CValue::Str(d.as_str().to_string())
}

pub(crate) fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

/// Reads an object that must have exactly `keys`.
fn exact<'a>(v: &'a CValue, keys: &[&str], what: &'static str) -> Result<&'a BTreeMap<String, CValue>, AlignError> {
    let CValue::Obj(o) = v else { return Err(AlignError::BadElement(what)) };
    if o.len() != keys.len() || !keys.iter().all(|k| o.contains_key(*k)) {
        return Err(AlignError::BadElement(what));
    }
    Ok(o)
}

fn field<'a>(o: &'a BTreeMap<String, CValue>, key: &str) -> Result<&'a CValue, AlignError> {
    o.get(key).ok_or(AlignError::Internal("checked key missing"))
}

fn string<'a>(v: &'a CValue, what: &'static str) -> Result<&'a str, AlignError> {
    match v {
        CValue::Str(s) => Ok(s),
        _ => Err(AlignError::BadElement(what)),
    }
}

fn id_from(v: &CValue, what: &'static str) -> Result<Uuid, AlignError> {
    parse_id(string(v, what)?).ok_or(AlignError::BadElement(what))
}

fn digest_from(v: &CValue, what: &'static str) -> Result<Digest, AlignError> {
    Digest::parse(string(v, what)?).map_err(|_| AlignError::BadElement(what))
}

/// Decodes `"0x"` plus 16 lowercase hex digits; NaN and infinities are refused.
fn f64_from(v: &CValue, what: &'static str) -> Result<f64, AlignError> {
    let s = string(v, what)?;
    let hex = s.strip_prefix("0x").ok_or(AlignError::BadElement(what))?;
    if hex.len() != 16 || !hex.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)) {
        return Err(AlignError::BadElement(what));
    }
    let bits = u64::from_str_radix(hex, 16).map_err(|_| AlignError::BadElement(what))?;
    let x = f64::from_bits(bits);
    if !x.is_finite() {
        return Err(AlignError::NonFinite(what));
    }
    Ok(x)
}

fn f64_array<const N: usize>(v: &CValue, what: &'static str) -> Result<[f64; N], AlignError> {
    let CValue::Arr(a) = v else { return Err(AlignError::BadElement(what)) };
    if a.len() != N {
        return Err(AlignError::BadElement(what));
    }
    let mut out = [0.0; N];
    for (o, x) in out.iter_mut().zip(a) {
        *o = f64_from(x, what)?;
    }
    Ok(out)
}

macro_rules! wire_enum {
    ($(#[$m:meta])* $name:ident { $($(#[$vm:meta])* $variant:ident => $s:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $name {
            $($(#[$vm])* $variant,)+
        }

        impl $name {
            /// Every value, in declaration order.
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            /// The wire string.
            pub fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $s,)+
                }
            }

            /// Parses a wire string; anything else is `unknown_kind`.
            pub fn parse(s: &str) -> Result<$name, AlignError> {
                match s {
                    $($s => Ok($name::$variant),)+
                    other => Err(AlignError::UnknownKind(kept(other))),
                }
            }
        }
    };
}

wire_enum!(
    /// The status of an alignment (REQ-001).
    AlignmentStatus {
        Ok => "ok",
        NotConverged => "not_converged",
        Ambiguous => "ambiguous",
        Failed => "failed",
        Outdated => "outdated",
    }
);

wire_enum!(
    /// The kind of principal that created an alignment.
    PrincipalKind {
        Human => "human",
        Agent => "agent",
    }
);

wire_enum!(
    /// The provenance state (transitions belong to `ALN.MODEL.ADOPT`; this leaf stores it).
    ProvenanceState {
        Human => "human",
        Proposed => "proposed",
        Adopted => "adopted",
    }
);

// ------------------------------------------------------------------ references

/// An element ID plus that element's content digest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ElementRef {
    pub id: Uuid,
    pub digest: Digest,
}

/// An input element and its content digest when the alignment was solved.
pub type InputRef = ElementRef;
/// The parent alignment and its content digest when the alignment was solved.
pub type ParentRef = ElementRef;
/// The residual report (owned by `ALN.QA.REPORT`) and its digest.
pub type ReportRef = ElementRef;

impl ElementRef {
    /// `{"digest": .., "id": ..}`.
    pub fn to_value(&self) -> CValue {
        obj([("digest", digest_value(&self.digest)), ("id", id_value(&self.id))])
    }

    fn from_value(v: &CValue, what: &'static str) -> Result<ElementRef, AlignError> {
        let o = exact(v, &["digest", "id"], what)?;
        Ok(ElementRef { id: id_from(field(o, "id")?, what)?, digest: digest_from(field(o, "digest")?, what)? })
    }
}

// ------------------------------------------------------------------ parameters

/// The parameters of an alignment: a map sorted by name. Each keeps its authored text and
/// unit and its SI value from exactly one rounding (GEO.FRAME.UNITS).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ParamSet(BTreeMap<String, AuthoredQuantity>);

fn quantity(text: &str, unit: &str) -> Result<AuthoredQuantity, AlignError> {
    let unit = Unit::from_symbol(unit).ok_or_else(|| AlignError::UnknownKind(kept(unit)))?;
    AuthoredQuantity::parse(text, unit).map_err(|e| match e {
        UnitsError::NonFinite => AlignError::NonFinite("param"),
        other => AlignError::BadQuantity(other),
    })
}

impl ParamSet {
    pub fn new() -> ParamSet {
        ParamSet::default()
    }

    /// Adds parameter `name` (`^[a-z][a-z0-9_]*$`, at most [`MAX_PARAM_NAME`] bytes) authored
    /// as `text` in the unit with symbol `unit`. Refuses a repeated name, an unknown unit
    /// (`unknown_kind`), a text that is not a decimal (`bad_quantity`), NaN or infinity
    /// (`non_finite`), and more than [`MAX_PARAMS`] parameters.
    pub fn insert(&mut self, name: &str, text: &str, unit: &str) -> Result<(), AlignError> {
        if !code_ok(name) || name.len() > MAX_PARAM_NAME {
            return Err(AlignError::BadParams("parameter name must match ^[a-z][a-z0-9_]*$, at most 64 bytes"));
        }
        if self.0.contains_key(name) {
            return Err(AlignError::BadParams("duplicate parameter"));
        }
        if self.0.len() >= MAX_PARAMS {
            return Err(AlignError::InputLimit("params"));
        }
        self.0.insert(name.to_string(), quantity(text, unit)?);
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&AuthoredQuantity> {
        self.0.get(name)
    }

    /// Parameters in ascending name order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &AuthoredQuantity)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v))
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// `{name: si bits}`: the hashed form.
    fn si_value(&self) -> CValue {
        CValue::Obj(self.0.iter().map(|(k, q)| (k.clone(), CValue::F64Bits(q.si()))).collect())
    }

    /// `{name: {"si", "text", "unit"}}`: the stored form.
    pub fn to_value(&self) -> CValue {
        CValue::Obj(
            self.0
                .iter()
                .map(|(k, q)| {
                    let v = obj([
                        ("si", CValue::F64Bits(q.si())),
                        ("text", CValue::Str(q.text().to_string())),
                        ("unit", CValue::Str(q.unit().symbol().to_string())),
                    ]);
                    (k.clone(), v)
                })
                .collect(),
        )
    }

    /// Reads the stored form. The text is converted again and must give the stored SI bits.
    pub fn from_value(v: &CValue) -> Result<ParamSet, AlignError> {
        let CValue::Obj(o) = v else { return Err(AlignError::BadElement("params")) };
        if o.len() > MAX_PARAMS {
            return Err(AlignError::InputLimit("params"));
        }
        let mut set = ParamSet::new();
        for (name, p) in o {
            if !code_ok(name) || name.len() > MAX_PARAM_NAME {
                return Err(AlignError::BadElement("param name"));
            }
            let po = exact(p, &["si", "text", "unit"], "param")?;
            let si = f64_from(field(po, "si")?, "param")?;
            set.insert(name, string(field(po, "text")?, "param")?, string(field(po, "unit")?, "param")?)?;
            match set.get(name) {
                Some(q) if q.si().to_bits() == si.to_bits() => {}
                _ => return Err(AlignError::BadElement("param si differs from its text")),
            }
        }
        Ok(set)
    }
}

// ------------------------------------------------------------------ transform

/// The stored form of a transform (see the module docs).
pub fn transform_value(t: &Transform) -> CValue {
    let r = t.rotation();
    let rows = r.iter().map(|row| CValue::Arr(row.iter().map(|x| CValue::F64Bits(*x)).collect())).collect();
    let tv = CValue::Arr(t.translation().iter().map(|x| CValue::F64Bits(*x)).collect());
    match t {
        Transform::Rigid(_) => obj([("kind", CValue::Str("rigid".into())), ("r", CValue::Arr(rows)), ("t", tv)]),
        Transform::Similarity(s) => obj([
            ("kind", CValue::Str("similarity".into())),
            ("r", CValue::Arr(rows)),
            ("s", CValue::F64Bits(s.s())),
            ("t", tv),
        ]),
    }
}

fn degenerate(e: XformError) -> AlignError {
    match e {
        XformError::NonFinite => AlignError::NonFinite("transform"),
        other => AlignError::Degenerate(other),
    }
}

/// `0.0` for either zero, `x` otherwise (no arithmetic).
fn plus_zero(x: f64) -> f64 {
    if x == 0.0 {
        0.0
    } else {
        x
    }
}

/// The canonical copy of a transform: every `-0.0` in the rotation and translation becomes
/// `+0.0`, so two transforms that act identically digest identically; a similarity scale that
/// is not a normal float (subnormal) is refused as `degenerate_transform`.
pub(crate) fn canonical_transform(t: &Transform) -> Result<Transform, AlignError> {
    let r = t.rotation().map(|row| row.map(plus_zero));
    let rigid = Rigid::new(r, t.translation().map(plus_zero)).map_err(degenerate)?;
    match t {
        Transform::Rigid(_) => Ok(Transform::Rigid(rigid)),
        Transform::Similarity(sim) => {
            if !sim.s().is_normal() {
                return Err(AlignError::Degenerate(XformError::BadScale));
            }
            Ok(Transform::Similarity(Similarity::new(rigid, sim.s()).map_err(degenerate)?))
        }
    }
}

/// Reads a stored transform and validates it through its GEO.FRAME constructor: a rotation
/// that is not orthonormal or not proper, or a scale that is not a positive normal float, is
/// refused as `degenerate_transform`. A `-0.0` entry is refused as `bad_element`, because the
/// stored form is canonical and holds only `+0.0`.
pub fn transform_from(v: &CValue) -> Result<Transform, AlignError> {
    let CValue::Obj(o) = v else { return Err(AlignError::BadElement("transform")) };
    let kind = string(o.get("kind").ok_or(AlignError::BadElement("transform"))?, "transform")?;
    let keys: &[&str] = match kind {
        "rigid" => &["kind", "r", "t"],
        "similarity" => &["kind", "r", "s", "t"],
        other => return Err(AlignError::UnknownKind(kept(other))),
    };
    let o = exact(v, keys, "transform")?;
    let CValue::Arr(rows) = field(o, "r")? else { return Err(AlignError::BadElement("transform")) };
    if rows.len() != 3 {
        return Err(AlignError::BadElement("transform"));
    }
    let mut r = [[0.0; 3]; 3];
    for (out, row) in r.iter_mut().zip(rows) {
        *out = f64_array::<3>(row, "transform")?;
    }
    let t = f64_array::<3>(field(o, "t")?, "transform")?;
    if r.iter().flatten().chain(t.iter()).any(|x| x.to_bits() == 1 << 63) {
        return Err(AlignError::BadElement("negative zero in transform"));
    }
    let rigid = Rigid::new(r, t).map_err(degenerate)?;
    if kind == "rigid" {
        return Ok(Transform::Rigid(rigid));
    }
    let s = f64_from(field(o, "s")?, "transform")?;
    let t = Transform::Similarity(Similarity::new(rigid, s).map_err(degenerate)?);
    canonical_transform(&t)
}

// ------------------------------------------------------------------ provenance

/// Who created the alignment (REQ-001). `ALN.MODEL.ADOPT` owns the state transitions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    principal: String,
    kind: PrincipalKind,
    state: ProvenanceState,
}

impl Provenance {
    /// Validated construction. The principal ID is 1..=[`MAX_PRINCIPAL_BYTES`] printable
    /// ASCII characters, no space, no upper case (the bus's principal ID rule). An agent
    /// principal can never carry state `human`.
    pub fn new(principal: &str, kind: PrincipalKind, state: ProvenanceState) -> Result<Provenance, AlignError> {
        let ok = !principal.is_empty()
            && principal.len() <= MAX_PRINCIPAL_BYTES
            && principal.bytes().all(|b| b.is_ascii_graphic() && !b.is_ascii_uppercase());
        if !ok {
            return Err(AlignError::BadParams("principal must be 1..=128 printable ASCII, no space, no upper case"));
        }
        if kind == PrincipalKind::Agent && state == ProvenanceState::Human {
            return Err(AlignError::BadParams("an agent principal cannot have provenance state human"));
        }
        Ok(Provenance { principal: principal.to_string(), kind, state })
    }

    /// The provenance of a new alignment: state `human` for a human, `proposed` for an agent
    /// (the creation rule of ALN.MODEL.ADOPT REQ-001 and REQ-002).
    pub fn for_creation(principal: &str, kind: PrincipalKind) -> Result<Provenance, AlignError> {
        let state = match kind {
            PrincipalKind::Human => ProvenanceState::Human,
            PrincipalKind::Agent => ProvenanceState::Proposed,
        };
        Provenance::new(principal, kind, state)
    }

    pub fn principal(&self) -> &str {
        &self.principal
    }

    pub fn kind(&self) -> PrincipalKind {
        self.kind
    }

    pub fn state(&self) -> ProvenanceState {
        self.state
    }

    pub fn to_value(&self) -> CValue {
        obj([
            ("kind", CValue::Str(self.kind.as_str().into())),
            ("principal", CValue::Str(self.principal.clone())),
            ("state", CValue::Str(self.state.as_str().into())),
        ])
    }

    fn from_value(v: &CValue) -> Result<Provenance, AlignError> {
        let o = exact(v, &["kind", "principal", "state"], "provenance")?;
        let kind = PrincipalKind::parse(string(field(o, "kind")?, "provenance")?)?;
        let state = ProvenanceState::parse(string(field(o, "state")?, "provenance")?)?;
        Provenance::new(string(field(o, "principal")?, "provenance")?, kind, state)
            .map_err(|_| AlignError::BadElement("provenance"))
    }
}

// ------------------------------------------------------------------ element

/// An alignment element (REQ-001). Built only by [`crate::AlignmentModel`] or decoded by
/// [`Alignment::from_value`], so its digest always matches its content.
#[derive(Debug, Clone, PartialEq)]
pub struct Alignment {
    id: Uuid,
    algorithm: AlgorithmId,
    inputs: Vec<InputRef>,
    params: ParamSet,
    transform: Transform,
    parent: Option<ParentRef>,
    report: ReportRef,
    provenance: Provenance,
    status: AlignmentStatus,
    failure: Option<String>,
    digest: Digest,
}

/// The fields a solve determines; hashed into the content digest.
pub(crate) struct Content<'a> {
    pub algorithm: &'a AlgorithmId,
    pub inputs: &'a [InputRef],
    pub params: &'a ParamSet,
    pub transform: &'a Transform,
    pub parent: Option<&'a ParentRef>,
    pub report: &'a ReportRef,
}

impl Content<'_> {
    fn value(&self) -> CValue {
        obj([
            ("algorithm", CValue::Str(self.algorithm.to_string())),
            ("inputs", CValue::Arr(self.inputs.iter().map(|i| digest_value(&i.digest)).collect())),
            ("params", self.params.si_value()),
            ("parent", self.parent.map_or(CValue::Null, ElementRef::to_value)),
            ("report", digest_value(&self.report.digest)),
            ("transform", transform_value(self.transform)),
        ])
    }

    /// SHA-256 of the canonical bytes of the hashed form (REQ-003).
    pub fn digest(&self) -> Result<Digest, AlignError> {
        let d = fid_canon::digest(&self.value()).map_err(|_| AlignError::Internal("canonical json"))?;
        Ok(Digest::from_bytes(d.0))
    }
}

/// Refuses a repeated input ID.
pub(crate) fn check_inputs(inputs: &[InputRef]) -> Result<(), AlignError> {
    if inputs.len() > MAX_INPUTS {
        return Err(AlignError::InputLimit("inputs"));
    }
    let mut seen = BTreeSet::new();
    for i in inputs {
        if !seen.insert(i.id) {
            return Err(AlignError::DuplicateInput(i.id));
        }
    }
    Ok(())
}

fn check_failure(status: AlignmentStatus, failure: Option<&str>) -> Result<(), AlignError> {
    match (status, failure) {
        (AlignmentStatus::Failed, Some(code)) if code_ok(code) => Ok(()),
        (AlignmentStatus::Failed, _) => Err(AlignError::BadElement("a failed alignment needs a failure code")),
        (_, Some(_)) => Err(AlignError::BadElement("only a failed alignment has a failure code")),
        (_, None) => Ok(()),
    }
}

impl Alignment {
    /// Assembles an element and computes its digest. Crate-internal: the model validates
    /// every part before calling it.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn assemble(
        id: Uuid,
        algorithm: AlgorithmId,
        inputs: Vec<InputRef>,
        params: ParamSet,
        transform: Transform,
        parent: Option<ParentRef>,
        report: ReportRef,
        provenance: Provenance,
        status: AlignmentStatus,
        failure: Option<String>,
    ) -> Result<Alignment, AlignError> {
        check_inputs(&inputs)?;
        check_failure(status, failure.as_deref())?;
        let transform = canonical_transform(&transform)?;
        let digest = Content {
            algorithm: &algorithm,
            inputs: &inputs,
            params: &params,
            transform: &transform,
            parent: parent.as_ref(),
            report: &report,
        }
        .digest()?;
        Ok(Alignment { id, algorithm, inputs, params, transform, parent, report, provenance, status, failure, digest })
    }

    /// The same element with another status and failure; content and digest unchanged.
    pub(crate) fn with_status(
        &self,
        status: AlignmentStatus,
        failure: Option<String>,
    ) -> Result<Alignment, AlignError> {
        check_failure(status, failure.as_deref())?;
        Ok(Alignment { status, failure, ..self.clone() })
    }

    /// The same element with a new parent reference and status, recomputing its content digest.
    pub(crate) fn with_parent(
        &self,
        parent: Option<ParentRef>,
        status: AlignmentStatus,
    ) -> Result<Alignment, AlignError> {
        Alignment::assemble(
            self.id,
            self.algorithm.clone(),
            self.inputs.clone(),
            self.params.clone(),
            self.transform,
            parent,
            self.report,
            self.provenance.clone(),
            status,
            None,
        )
    }

    /// The same element with a new transform, recomputing its canonical form and content digest.
    pub(crate) fn with_transform(&self, transform: Transform) -> Result<Alignment, AlignError> {
        Alignment::assemble(
            self.id,
            self.algorithm.clone(),
            self.inputs.clone(),
            self.params.clone(),
            transform,
            self.parent,
            self.report,
            self.provenance.clone(),
            self.status,
            self.failure.clone(),
        )
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn algorithm(&self) -> &AlgorithmId {
        &self.algorithm
    }

    /// Inputs in command order.
    pub fn inputs(&self) -> &[InputRef] {
        &self.inputs
    }

    pub fn params(&self) -> &ParamSet {
        &self.params
    }

    pub fn transform(&self) -> &Transform {
        &self.transform
    }

    pub fn parent(&self) -> Option<&ParentRef> {
        self.parent.as_ref()
    }

    pub fn report(&self) -> &ReportRef {
        &self.report
    }

    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    pub fn status(&self) -> AlignmentStatus {
        self.status
    }

    /// The error code of a `failed` alignment.
    pub fn failure(&self) -> Option<&str> {
        self.failure.as_deref()
    }

    /// The content digest (REQ-003).
    pub fn digest(&self) -> Digest {
        self.digest
    }

    /// The element form (see the module docs).
    pub fn to_value(&self) -> CValue {
        obj([
            ("algorithm", CValue::Str(self.algorithm.to_string())),
            ("digest", digest_value(&self.digest)),
            ("failure", self.failure.as_ref().map_or(CValue::Null, |f| CValue::Str(f.clone()))),
            ("id", id_value(&self.id)),
            ("inputs", CValue::Arr(self.inputs.iter().map(ElementRef::to_value).collect())),
            ("params", self.params.to_value()),
            ("parent", self.parent.as_ref().map_or(CValue::Null, ElementRef::to_value)),
            ("provenance", self.provenance.to_value()),
            ("report", self.report.to_value()),
            ("status", CValue::Str(self.status.as_str().into())),
            ("transform", transform_value(&self.transform)),
        ])
    }

    /// The summary `align.list_alignments@1` returns:
    /// `{"algorithm", "digest", "id", "parent": null | "<uuid>", "status"}`.
    pub fn summary_value(&self) -> CValue {
        obj([
            ("algorithm", CValue::Str(self.algorithm.to_string())),
            ("digest", digest_value(&self.digest)),
            ("id", id_value(&self.id)),
            ("parent", self.parent.as_ref().map_or(CValue::Null, |p| id_value(&p.id))),
            ("status", CValue::Str(self.status.as_str().into())),
        ])
    }

    /// RFC 8785 canonical bytes of [`Alignment::to_value`].
    pub fn to_json(&self) -> Result<Vec<u8>, AlignError> {
        fid_canon::to_canonical_bytes(&self.to_value()).map_err(|_| AlignError::Internal("canonical json"))
    }

    /// Reads the element form strictly: exactly the pinned keys, canonical IDs and digests,
    /// known enumerations, finite floats, a valid transform, parameters whose text converts
    /// to the stored SI bits, and a digest equal to the digest of the content.
    pub fn from_value(v: &CValue) -> Result<Alignment, AlignError> {
        const KEYS: [&str; 11] = [
            "algorithm",
            "digest",
            "failure",
            "id",
            "inputs",
            "params",
            "parent",
            "provenance",
            "report",
            "status",
            "transform",
        ];
        let o = exact(v, &KEYS, "element keys")?;
        let id = id_from(field(o, "id")?, "id")?;
        let text = string(field(o, "algorithm")?, "algorithm")?;
        if text.len() > MAX_ALGORITHM_BYTES {
            return Err(AlignError::BadElement("algorithm"));
        }
        let algorithm = AlgorithmId::parse(text).map_err(|_| AlignError::BadElement("algorithm"))?;
        let CValue::Arr(list) = field(o, "inputs")? else { return Err(AlignError::BadElement("inputs")) };
        if list.len() > MAX_INPUTS {
            return Err(AlignError::InputLimit("inputs"));
        }
        let inputs = list.iter().map(|i| ElementRef::from_value(i, "input")).collect::<Result<Vec<_>, _>>()?;
        let params = ParamSet::from_value(field(o, "params")?)?;
        let transform = transform_from(field(o, "transform")?)?;
        let parent = match field(o, "parent")? {
            CValue::Null => None,
            p => Some(ElementRef::from_value(p, "parent")?),
        };
        let report = ElementRef::from_value(field(o, "report")?, "report")?;
        let provenance = Provenance::from_value(field(o, "provenance")?)?;
        let status = AlignmentStatus::parse(string(field(o, "status")?, "status")?)?;
        let failure = match field(o, "failure")? {
            CValue::Null => None,
            f => Some(string(f, "failure")?.to_string()),
        };
        let stored = digest_from(field(o, "digest")?, "digest")?;
        let a =
            Alignment::assemble(id, algorithm, inputs, params, transform, parent, report, provenance, status, failure)?;
        if a.digest != stored {
            return Err(AlignError::DigestMismatch);
        }
        Ok(a)
    }

    /// Parses canonical JSON bytes (at most [`MAX_ELEMENT_BYTES`], duplicate keys refused)
    /// and reads them with [`Alignment::from_value`].
    pub fn from_json(bytes: &[u8]) -> Result<Alignment, AlignError> {
        let v = parse_with_limits(bytes, MAX_ELEMENT_BYTES, MAX_ELEMENT_DEPTH).map_err(|e| match e {
            CanonError::InputLimit => AlignError::InputLimit("element"),
            CanonError::DuplicateKey(_) => AlignError::BadElement("duplicate key"),
            CanonError::NonFiniteNumber => AlignError::NonFinite("element"),
            _ => AlignError::BadElement("not valid json"),
        })?;
        Alignment::from_value(&v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_ids_only() {
        assert!(parse_id("00000000-0000-7000-8000-00000000000a").is_some());
        assert!(parse_id("00000000-0000-7000-8000-00000000000A").is_none());
        assert!(parse_id("00000000-0000-0000-0000-000000000000").is_none());
        assert!(parse_id("ffffffff-ffff-ffff-ffff-ffffffffffff").is_none());
        assert!(parse_id("").is_none());
    }

    #[test]
    fn floats_decode_from_lowercase_bits_only() {
        let s = |x: &str| CValue::Str(x.to_string());
        assert_eq!(f64_from(&s("0x3ff0000000000000"), "x"), Ok(1.0));
        assert_eq!(f64_from(&s("0x8000000000000000"), "x").map(f64::to_bits), Ok(1 << 63));
        for bad in
            ["0x3FF0000000000000", "3ff0000000000000", "0x3ff000000000000", "0x3ff00000000000000", "0x+ff0000000000000"]
        {
            assert_eq!(f64_from(&s(bad), "x"), Err(AlignError::BadElement("x")), "{bad}");
        }
        for nonfinite in ["0x7ff8000000000000", "0x7ff0000000000000", "0xfff0000000000000"] {
            assert_eq!(f64_from(&s(nonfinite), "x"), Err(AlignError::NonFinite("x")), "{nonfinite}");
        }
    }

    #[test]
    fn wire_enums_round_trip_and_refuse_unknown_values() {
        for s in AlignmentStatus::ALL {
            assert_eq!(AlignmentStatus::parse(s.as_str()), Ok(*s));
        }
        assert_eq!(AlignmentStatus::parse("OK"), Err(AlignError::UnknownKind("OK".into())));
        assert!(PrincipalKind::parse("script").is_err());
        assert!(ProvenanceState::parse("").is_err());
    }
}

#[cfg(test)]
mod canonical_tests {
    use super::*;

    #[test]
    fn negative_zero_becomes_positive_and_subnormal_scales_are_refused() {
        let r = [[1.0, -0.0, 0.0], [0.0, 1.0, -0.0], [-0.0, 0.0, 1.0]];
        let t = Transform::Rigid(Rigid::new(r, [-0.0, 1.0, -0.0]).unwrap_or(Rigid::IDENTITY));
        let c = canonical_transform(&t).unwrap_or(Transform::IDENTITY);
        assert!(c.rotation().iter().flatten().chain(c.translation().iter()).all(|x| x.to_bits() != 1 << 63));
        assert_eq!(c.translation()[1].to_bits(), 1.0f64.to_bits());
        let rigid = Rigid::new(r, [0.0; 3]).unwrap_or(Rigid::IDENTITY);
        for (s, ok) in [(f64::from_bits(1), false), (f64::MIN_POSITIVE, true), (1.5, true)] {
            let sim = Similarity::new(rigid, s).map(Transform::Similarity);
            let got = sim.map_err(|_| ()).and_then(|t| canonical_transform(&t).map_err(|_| ()));
            assert_eq!(got.is_ok(), ok, "{s:e}");
        }
    }
}
