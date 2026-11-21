//! DRF construction from datum labels (spec node `GDT.DRF.BUILD`): the commands
//! `gdt.build_drf@1` (W2, dry run R1; CLI `fid gdt build-drf`, MCP `gdt_build_drf`) and
//! `gdt.get_drf@1` (R0; CLI `fid gdt get-drf`, MCP `gdt_get_drf`), the canonical DRF document,
//! its journal event and exact replay.
//!
//! **P1a scope, as built.** Planar datum features at RMB. A datum reference with a material
//! boundary modifier (`A(M)`, `A(L)`) is refused (`unsupported_material_boundary{label}`, the
//! D-045 first-review subset is RMB only), never read as RMB. A cylindrical datum feature is
//! refused with `unsupported_datum_type_p1a{label}`: its simulator (`KRN.ENV.MCC.CYL` /
//! `KRN.ENV.TAN.CYL`) is not built, and no axis is ever made up. Datums with datum targets are
//! refused (`datum_targets_unsupported{label}`, owned by `GDT.DRF.COMMON`).
//!
//! **Pipeline of [`build_drf`]** (every check runs before any change; a refusal and a dry run
//! change nothing and journal nothing):
//!
//! 1. the request: a characteristic ID, one to three datum references `L`, `L(M)` or `L(L)`
//!    (`bad_params`, `no_datums`, `too_many_datums`, `bad_datum_reference`,
//!    `duplicate_datum_label`);
//! 2. every reference at RMB (`unsupported_material_boundary{label}`);
//! 3. every label declared on the principal's branch (`GDT.DRF.DATUM`; `datum_undefined{label}`),
//!    planar (`unsupported_datum_type_p1a{label}`) and without targets;
//! 4. the interpretation chain: `gdt.resolve_chain@1`'s resolver for the characteristic type
//!    [`FRAME_CHARACTERISTIC`] with the modifier `rmb`, behind the governing-spec gate (errors
//!    pass through); an outcome other than `RESOLVED` is `chain_not_resolved{outcome}`; the row
//!    must be a `datum_frame` row whose only modifier is `rmb`
//!    (`material_boundary_mismatch{row}`); its chain must hold exactly one association step,
//!    whose algorithm is the simulator (`unsupported_simulator{alg}`);
//! 5. the interpretation block (D-045, `GDT.CHAR.INTERP`): the row and the characteristic's
//!    defaults profile with recomputed digests and their statuses from the given
//!    [`RegisterView`]; a draft rule is `rule_not_admitted{rule}` and a missing rule
//!    `incomplete_result{interpretation_rules}`. There is no way to build a DRF without it;
//! 6. the measured points of each datum's bound feature ([`FeaturePoints`];
//!    `measurement_missing{feature}`), simulated in precedence order by
//!    `fid_gps::drf::plane_sim` (certified tangential planes of `fid-fit` only; its refusals
//!    and the builder's, such as `degenerate_datum_pair{A,B}`, pass through);
//! 7. the document ([`DRF_SCHEMA`]), its ID (the `sha256:` digest of its canonical bytes),
//!    the store on the principal's branch and the event.
//!
//! **Values.** Every value in a DRF comes from measured points through a certified (free
//! normal) or closed-form (constrained normal) fit; there is no input for claimed or nominal
//! datum geometry, and the document says `"values": "measured"`. The nominal outward normals
//! that pick the material side and the basic relationship are recorded as
//! `nominal_outward`, never as datum geometry.
//!
//! **Label.** The document carries `interpretation` (`provisional` unless every rule is
//! signed off) with its rules and register; every surface shows the line [`drf_label`]
//! gives, which asks [`crate::requires_provisional_label_bytes`]. That predicate reads
//! `fid.gdt-result/2` records only, so today every DRF shows `interpretation: provisional`
//! (fail closed; see the leaf's Open questions).
//!
//! **Branches.** As for datums: a person writes and reads the working branch only; an agent or
//! a script writes the agent overlay and reads it over the working branch.
//!
//! Floats in the document are written as `0x` + 16 lowercase hex digits of their bits, so the
//! bytes are exact and the same on every target (D0).

pub mod command;
pub mod event;

pub use command::{
    build_command_def, get_command_def, invoke_cli, invoke_mcp, BUILD_DRF_CLI, BUILD_DRF_MCP, GET_DRF_CLI, GET_DRF_MCP,
};
pub use event::{replay, DrfEvent, DrfReplayed};

use crate::chars::dprof::{Branch, CharId, DprofState};
use crate::chars::interp::{register_cvalue, Interpreted, RegisterView};
use crate::chars::interp::{PROVISIONAL_LABEL_TEXT, SIGNED_OFF_LINE};
use crate::chars::record::RecordError;
use crate::chars::spec::{SpecError, SpecState};
use crate::dmt_flow::status::Purpose;
use crate::drf::datum::{DatumLabel, DatumModel, FeatureType, RefId};
use crate::resolve::{resolve_chain, CharacteristicRef, CommandError, CHAR_SCHEMA};
use fid_canon::CValue;
use fid_gps::dmt::{AlgId, DmtTable, OperatorKind};
use fid_gps::drf::build::{Dof, DofSet, Precedence, SimGeometry};
use fid_gps::drf::plane_sim::{build_measured_planar_drf, MeasuredPlane, PlaneEvidence, SimError};
use fid_gps::resolve::Resolution;
use fid_io_reg::Principal;
use fid_types::Digest;
use std::collections::BTreeMap;
use std::fmt;

/// Command ID of the build command.
pub const BUILD_COMMAND: &str = "gdt.build_drf@1";
/// Command ID of the read command.
pub const GET_COMMAND: &str = "gdt.get_drf@1";
/// `schema` of a DRF document.
pub const DRF_SCHEMA: &str = "fid.gdt-drf/1";
/// The DMT characteristic type of a datum reference frame row.
pub const FRAME_CHARACTERISTIC: &str = "datum_frame";
/// The only material boundary of P1a (and of the D-045 first-review subset).
pub const RMB: &str = "rmb";
/// Most measured points of one feature.
pub const MAX_FEATURE_POINTS: usize = 1 << 20;
/// Most measured features in one [`FeaturePoints`].
pub const MAX_FEATURES: usize = 4096;
/// Most DRFs per branch of a [`DrfStore`].
pub const MAX_STORED_DRFS: usize = 10_000;
/// Longest text echoed back inside an error, in bytes.
const MAX_ECHO: usize = 160;

fn echo(s: &str) -> String {
    let mut end = s.len().min(MAX_ECHO);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_owned()
}

/// Errors of the DRF commands. [`BuildError::code`] is the stable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildError {
    /// `bad_params{reason}`: CLI arguments or MCP parameters that are not a valid call.
    BadParams {
        /// Static reason.
        reason: &'static str,
    },
    /// `no_datums`.
    NoDatums,
    /// `too_many_datums{count}`: more than three references.
    TooManyDatums {
        /// How many were given.
        count: usize,
    },
    /// `bad_datum_reference{reason}`: a reference is not `L`, `L(M)` or `L(L)` with a valid
    /// label.
    BadDatumReference {
        /// Static reason.
        reason: &'static str,
    },
    /// `duplicate_datum_label{label}`.
    DuplicateDatumLabel {
        /// The label.
        label: String,
    },
    /// `unsupported_material_boundary{label}`: an MMB or LMB reference (P1a and D-045: RMB only).
    UnsupportedMaterialBoundary {
        /// The label.
        label: String,
        /// `mmb` or `lmb`.
        boundary: &'static str,
    },
    /// `datum_undefined{label}`: no datum with this label on the principal's branch.
    DatumUndefined {
        /// The label.
        label: String,
    },
    /// `unsupported_datum_type_p1a{label}`: the datum feature is not planar (cylindrical
    /// simulators are not built).
    UnsupportedDatumType {
        /// The label.
        label: String,
        /// The feature type.
        feature_type: &'static str,
    },
    /// `datum_targets_unsupported{label}`.
    DatumTargetsUnsupported {
        /// The label.
        label: String,
    },
    /// `measurement_missing{feature}`: no measured points for the datum's feature.
    MeasurementMissing {
        /// The feature.
        feature: String,
    },
    /// `measurement_invalid{feature:reason}`: points that cannot be stored.
    MeasurementInvalid {
        /// The feature (truncated).
        feature: String,
        /// Static reason.
        reason: &'static str,
    },
    /// An error of the chain resolution (the governing-spec gate, `dmt_not_loaded`, the
    /// resolver), passed through.
    Chain(CommandError),
    /// `chain_not_resolved{outcome}`: the resolver said `UNRESOLVED` or `UNSUPPORTED`.
    ChainNotResolved {
        /// The outcome.
        outcome: &'static str,
    },
    /// `material_boundary_mismatch{row}`: the resolved row is not a `datum_frame` row at RMB.
    MaterialBoundaryMismatch {
        /// The row key.
        row: String,
    },
    /// `unsupported_simulator{alg}`: the chain's association step is missing, repeated or not
    /// a tangential plane fit.
    UnsupportedSimulator {
        /// The algorithm ID, or `none`.
        alg: String,
    },
    /// An error of the interpretation block (`rule_not_admitted{rule}`,
    /// `incomplete_result{interpretation_rules}`), passed through.
    Interpretation(RecordError),
    /// An error of the measured planar path or the builder, passed through.
    Simulation(SimError),
    /// `drf_store_full{branch}`.
    StoreFull {
        /// The branch.
        branch: Branch,
    },
    /// `drf_unknown{id}`: no DRF with this ID visible to the principal.
    DrfUnknown {
        /// The ID (truncated).
        id: String,
    },
    /// `encode_failed`: the canonical writer failed (a defect).
    Encode,
    /// `event_invalid{reason}`: a journal event could not be read.
    Event {
        /// Static reason.
        reason: &'static str,
    },
}

impl BuildError {
    /// The stable code.
    pub fn code(&self) -> &'static str {
        match self {
            BuildError::BadParams { .. } => "bad_params",
            BuildError::NoDatums => "no_datums",
            BuildError::TooManyDatums { .. } => "too_many_datums",
            BuildError::BadDatumReference { .. } => "bad_datum_reference",
            BuildError::DuplicateDatumLabel { .. } => "duplicate_datum_label",
            BuildError::UnsupportedMaterialBoundary { .. } => "unsupported_material_boundary",
            BuildError::DatumUndefined { .. } => "datum_undefined",
            BuildError::UnsupportedDatumType { .. } => "unsupported_datum_type_p1a",
            BuildError::DatumTargetsUnsupported { .. } => "datum_targets_unsupported",
            BuildError::MeasurementMissing { .. } => "measurement_missing",
            BuildError::MeasurementInvalid { .. } => "measurement_invalid",
            BuildError::Chain(e) => e.code(),
            BuildError::ChainNotResolved { .. } => "chain_not_resolved",
            BuildError::MaterialBoundaryMismatch { .. } => "material_boundary_mismatch",
            BuildError::UnsupportedSimulator { .. } => "unsupported_simulator",
            BuildError::Interpretation(e) => e.code(),
            BuildError::Simulation(e) => e.code(),
            BuildError::StoreFull { .. } => "drf_store_full",
            BuildError::DrfUnknown { .. } => "drf_unknown",
            BuildError::Encode => "encode_failed",
            BuildError::Event { .. } => "event_invalid",
        }
    }
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let c = self.code();
        match self {
            BuildError::BadParams { reason: r }
            | BuildError::BadDatumReference { reason: r }
            | BuildError::Event { reason: r } => write!(f, "{c}{{{r}}}"),
            BuildError::ChainNotResolved { outcome } => write!(f, "{c}{{{outcome}}}"),
            BuildError::TooManyDatums { count } => write!(f, "{c}{{{count}}}"),
            BuildError::DuplicateDatumLabel { label: s }
            | BuildError::UnsupportedMaterialBoundary { label: s, .. }
            | BuildError::DatumUndefined { label: s }
            | BuildError::UnsupportedDatumType { label: s, .. }
            | BuildError::DatumTargetsUnsupported { label: s }
            | BuildError::MeasurementMissing { feature: s }
            | BuildError::MaterialBoundaryMismatch { row: s }
            | BuildError::UnsupportedSimulator { alg: s }
            | BuildError::DrfUnknown { id: s } => write!(f, "{c}{{{}}}", s.escape_debug()),
            BuildError::MeasurementInvalid { feature, reason } => {
                write!(f, "{c}{{{}:{reason}}}", feature.escape_debug())
            }
            BuildError::StoreFull { branch } => write!(f, "{c}{{{}}}", branch.as_str()),
            BuildError::Chain(e) => e.fmt(f),
            BuildError::Interpretation(e) => e.fmt(f),
            BuildError::Simulation(e) => e.fmt(f),
            BuildError::NoDatums | BuildError::Encode => f.write_str(c),
        }
    }
}

impl std::error::Error for BuildError {}

/// The material boundary of a datum reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Boundary {
    /// Regardless of material boundary (no modifier).
    Rmb,
    /// Maximum material boundary (`(M)`).
    Mmb,
    /// Least material boundary (`(L)`).
    Lmb,
}

impl Boundary {
    /// `rmb`, `mmb`, `lmb`.
    pub fn as_str(self) -> &'static str {
        match self {
            Boundary::Rmb => "rmb",
            Boundary::Mmb => "mmb",
            Boundary::Lmb => "lmb",
        }
    }
}

/// One datum reference of the feature control frame: a label and its material boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatumRef {
    label: DatumLabel,
    boundary: Boundary,
}

impl DatumRef {
    /// Parses `L`, `L(M)` or `L(L)` with `L` a datum label (one to three ASCII capitals):
    /// `bad_datum_reference{empty|bad_modifier|bad_label}`. No modifier is RMB (P1a reading
    /// for both dialects [unverified for ISO 5459 editions before 2011]).
    pub fn parse(s: &str) -> Result<DatumRef, BuildError> {
        let bad = |reason| BuildError::BadDatumReference { reason };
        if s.is_empty() {
            return Err(bad("empty"));
        }
        let (label, boundary) = match s.find('(') {
            None => (s, Boundary::Rmb),
            Some(i) => match &s[i..] {
                "(M)" => (&s[..i], Boundary::Mmb),
                "(L)" => (&s[..i], Boundary::Lmb),
                _ => return Err(bad("bad_modifier")),
            },
        };
        let label = DatumLabel::parse(label).map_err(|_| bad("bad_label"))?;
        Ok(DatumRef { label, boundary })
    }

    /// The label.
    pub fn label(&self) -> &DatumLabel {
        &self.label
    }

    /// The material boundary.
    pub fn boundary(&self) -> Boundary {
        self.boundary
    }

    /// The reference as written (`A`, `A(M)`, `A(L)`).
    pub fn text(&self) -> String {
        match self.boundary {
            Boundary::Rmb => self.label.as_str().to_owned(),
            Boundary::Mmb => format!("{}(M)", self.label.as_str()),
            Boundary::Lmb => format!("{}(L)", self.label.as_str()),
        }
    }
}

/// The parameters of `gdt.build_drf@1`.
#[derive(Debug, Clone, Copy)]
pub struct BuildRequest<'a> {
    /// `char`: the characteristic whose governing spec and defaults profile apply.
    pub char: &'a str,
    /// `datums`: one to three references in precedence order.
    pub datums: &'a [&'a str],
}

#[derive(Debug, Clone, PartialEq)]
struct Measured {
    xyz: Vec<f64>,
    nominal: [f64; 3],
    digest: Digest,
}

/// The measured points of the plan's features, with the nominal outward normal of each
/// feature's datum face (from the CAD model), content-addressed by [`FeaturePoints::digest`].
/// This is the narrow input the build reads measurements from; it never holds datum geometry.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FeaturePoints {
    map: BTreeMap<RefId, Measured>,
}

/// SHA-256 of `fid.gdt-feature-points/1\n`, then the nominal and the coordinates as
/// little-endian `f64` bits.
fn points_digest(xyz: &[f64], nominal: [f64; 3]) -> Digest {
    let mut b = Vec::with_capacity(32 + 8 * (3 + xyz.len()));
    b.extend_from_slice(b"fid.gdt-feature-points/1\n");
    for x in nominal.iter().chain(xyz) {
        b.extend_from_slice(&x.to_bits().to_le_bytes());
    }
    Digest::from_bytes(fid_canon::digest_bytes(&b).0)
}

impl FeaturePoints {
    /// An empty set.
    pub fn new() -> FeaturePoints {
        FeaturePoints::default()
    }

    /// Stores (or replaces) the points of `feature` and returns their digest. Refused with
    /// `measurement_invalid{feature:reason}`: `bad_id`, `bad_length` (not whole triples),
    /// `too_many_points`, `non_finite` (a coordinate or nominal component), `too_many_features`.
    pub fn insert(&mut self, feature: &str, xyz: Vec<f64>, nominal_outward: [f64; 3]) -> Result<Digest, BuildError> {
        let bad = |reason| BuildError::MeasurementInvalid { feature: echo(feature), reason };
        let id = RefId::parse(feature).ok_or_else(|| bad("bad_id"))?;
        if xyz.len() % 3 != 0 {
            return Err(bad("bad_length"));
        }
        if xyz.len() / 3 > MAX_FEATURE_POINTS {
            return Err(bad("too_many_points"));
        }
        if !xyz.iter().chain(nominal_outward.iter()).all(|x| x.is_finite()) {
            return Err(bad("non_finite"));
        }
        if !self.map.contains_key(&id) && self.map.len() >= MAX_FEATURES {
            return Err(bad("too_many_features"));
        }
        let digest = points_digest(&xyz, nominal_outward);
        self.map.insert(id, Measured { xyz, nominal: nominal_outward, digest });
        Ok(digest)
    }

    /// The digest of a feature's points.
    pub fn digest(&self, feature: &str) -> Option<Digest> {
        RefId::parse(feature).and_then(|id| self.map.get(&id)).map(|m| m.digest)
    }
}

/// The DRFs committed on each branch, by ID, as their canonical bytes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DrfStore {
    working: BTreeMap<String, Vec<u8>>,
    agent: BTreeMap<String, Vec<u8>>,
}

impl DrfStore {
    /// An empty store.
    pub fn new() -> DrfStore {
        DrfStore::default()
    }

    /// The bytes of DRF `id` as `principal` sees it: a person the working branch only; an
    /// agent or a script the overlay first, then the working branch.
    pub fn get(&self, principal: Principal, id: &str) -> Option<&[u8]> {
        match Branch::for_principal(principal) {
            Branch::Working => self.working.get(id),
            Branch::Agent => self.agent.get(id).or_else(|| self.working.get(id)),
        }
        .map(Vec::as_slice)
    }

    /// The number of DRFs held on `branch` itself.
    pub fn len(&self, branch: Branch) -> usize {
        self.own(branch).len()
    }

    /// Whether `branch` holds no DRF.
    pub fn is_empty(&self, branch: Branch) -> bool {
        self.own(branch).is_empty()
    }

    fn own(&self, branch: Branch) -> &BTreeMap<String, Vec<u8>> {
        match branch {
            Branch::Working => &self.working,
            Branch::Agent => &self.agent,
        }
    }

    fn room(&self, branch: Branch, id: &str) -> Result<(), BuildError> {
        let own = self.own(branch);
        if !own.contains_key(id) && own.len() >= MAX_STORED_DRFS {
            return Err(BuildError::StoreFull { branch });
        }
        Ok(())
    }

    fn put(&mut self, branch: Branch, id: String, bytes: Vec<u8>) {
        match branch {
            Branch::Working => self.working.insert(id, bytes),
            Branch::Agent => self.agent.insert(id, bytes),
        };
    }
}

/// Everything `gdt.build_drf@1` reads: the characteristic's governing spec and defaults
/// profiles, the loaded DMT table, the declared datums, the measured points and the status
/// register view of the interpretation block (D-045).
#[derive(Debug, Clone, Copy)]
pub struct BuildInputs<'a> {
    /// Governing specs (`GDT.CHAR.SPEC`).
    pub spec: &'a SpecState,
    /// Defaults profiles (`GDT.CHAR.DPROF`).
    pub dprof: &'a DprofState,
    /// The loaded DMT table (`None`: `dmt_not_loaded`).
    pub table: Option<&'a DmtTable>,
    /// The declared datums (`GDT.DRF.DATUM`).
    pub datums: &'a DatumModel,
    /// The measured points.
    pub points: &'a FeaturePoints,
    /// The status register view.
    pub register: &'a RegisterView,
}

/// The result of `gdt.build_drf@1`.
#[derive(Debug, Clone, PartialEq)]
pub struct Built {
    id: Digest,
    branch: Branch,
    dry_run: bool,
    doc: CValue,
    bytes: Vec<u8>,
    event: Option<DrfEvent>,
}

impl Built {
    /// The DRF ID: the digest of its canonical bytes.
    pub fn id(&self) -> Digest {
        self.id
    }
    /// The branch written (or that would be written).
    pub fn branch(&self) -> Branch {
        self.branch
    }
    /// Whether this was a dry run.
    pub fn dry_run(&self) -> bool {
        self.dry_run
    }
    /// The DRF document's canonical bytes (what `gdt.get_drf@1` returns).
    pub fn drf_bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// The display line of the interpretation label ([`drf_label`]).
    pub fn label(&self) -> &'static str {
        drf_label(&self.bytes)
    }
    /// The journal event; `None` for a dry run.
    pub fn event(&self) -> Option<&DrfEvent> {
        self.event.as_ref()
    }
    /// The `--json` / MCP result: `{branch, drf, dry_run, id, label}`.
    pub fn to_cvalue(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("branch".to_owned(), s(self.branch.as_str()));
        m.insert("drf".to_owned(), self.doc.clone());
        m.insert("dry_run".to_owned(), CValue::Bool(self.dry_run));
        m.insert("id".to_owned(), s(self.id.as_str()));
        m.insert("label".to_owned(), s(self.label()));
        CValue::Obj(m)
    }
    /// The result as RFC 8785 canonical JSON bytes.
    pub fn to_json_bytes(&self) -> Result<Vec<u8>, BuildError> {
        fid_canon::to_canonical_bytes(&self.to_cvalue()).map_err(|_| BuildError::Encode)
    }
}

/// The display line every surface shows for a stored DRF: the catalog text of the
/// provisional label whenever [`crate::requires_provisional_label_bytes`] says so (which is
/// every DRF today: the predicate signs off `fid.gdt-result/2` records only), else the
/// signed-off line.
pub fn drf_label(bytes: &[u8]) -> &'static str {
    if crate::requires_provisional_label_bytes(bytes) {
        PROVISIONAL_LABEL_TEXT
    } else {
        SIGNED_OFF_LINE
    }
}

fn s(v: &str) -> CValue {
    CValue::Str(v.to_owned())
}

/// `0x` + 16 lowercase hex digits of the bits.
pub fn hex(x: f64) -> String {
    format!("0x{:016x}", x.to_bits())
}

fn hex3(v: [f64; 3]) -> CValue {
    CValue::Arr(v.iter().map(|x| s(&hex(*x))).collect())
}

fn dofs(d: DofSet) -> CValue {
    CValue::Arr(d.dofs().into_iter().map(|x: Dof| s(x.name())).collect())
}

fn precedence(p: Precedence) -> &'static str {
    match p {
        Precedence::Primary => "primary",
        Precedence::Secondary => "secondary",
        Precedence::Tertiary => "tertiary",
    }
}

fn obj<const N: usize>(items: [(&str, CValue); N]) -> CValue {
    CValue::Obj(items.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

fn digest_or_null(d: Option<Digest>) -> CValue {
    d.map_or(CValue::Null, |d| s(d.as_str()))
}

fn evidence(e: &PlaneEvidence) -> CValue {
    obj([
        ("candidates", CValue::Num(f64::from(e.candidates))),
        ("certificate", digest_or_null(e.certificate)),
        ("contacts", CValue::Arr(e.contacts.iter().map(|c| CValue::Num(f64::from(*c))).collect())),
        ("local_optimum_only", CValue::Bool(e.local_optimum_only)),
        ("max_violation", s(&hex(e.max_violation))),
        ("objective", s(&hex(e.objective))),
        ("optimality", s(e.optimality)),
        ("orientation", s(e.orientation.as_str())),
        ("output_digest", s(e.fit_digest.as_str())),
    ])
}

fn parse_request(req: &BuildRequest<'_>) -> Result<(CharId, Vec<DatumRef>), BuildError> {
    let char = CharId::parse(req.char).ok_or(BuildError::BadParams { reason: "bad_char_id" })?;
    if req.datums.is_empty() {
        return Err(BuildError::NoDatums);
    }
    if req.datums.len() > fid_gps::drf::build::MAX_DATUMS {
        return Err(BuildError::TooManyDatums { count: req.datums.len() });
    }
    let mut refs: Vec<DatumRef> = Vec::new();
    for d in req.datums {
        let r = DatumRef::parse(d)?;
        if refs.iter().any(|e| e.label == r.label) {
            return Err(BuildError::DuplicateDatumLabel { label: r.label.as_str().to_owned() });
        }
        refs.push(r);
    }
    Ok((char, refs))
}

fn frame_characteristic(char: &CharId) -> Result<CharacteristicRef, BuildError> {
    let doc = obj([
        ("char", s(char.as_str())),
        ("characteristic", s(FRAME_CHARACTERISTIC)),
        ("modifiers", CValue::Arr(vec![s(RMB)])),
        ("override", CValue::Null),
        ("schema", s(CHAR_SCHEMA)),
    ]);
    let bytes = fid_canon::to_canonical_bytes(&doc).map_err(|_| BuildError::Encode)?;
    CharacteristicRef::parse(&bytes).map_err(BuildError::Chain)
}

fn simulator(chain: &fid_gps::dmt::OperatorChain) -> Result<AlgId, BuildError> {
    let assoc: Vec<&AlgId> =
        chain.steps().iter().filter(|st| st.kind() == OperatorKind::Association).map(|st| st.alg()).collect();
    match assoc[..] {
        [alg] => Ok(alg.clone()),
        [] => Err(BuildError::UnsupportedSimulator { alg: "none".to_owned() }),
        [first, ..] => Err(BuildError::UnsupportedSimulator { alg: first.as_str().to_owned() }),
    }
}

/// `gdt.build_drf@1` (W2; dry run R1): the pipeline of the module documentation. Returns the
/// DRF and, unless `dry_run`, stores it on the principal's branch and returns its event.
pub fn build_drf(
    store: &mut DrfStore,
    inp: &BuildInputs<'_>,
    principal: Principal,
    req: &BuildRequest<'_>,
    dry_run: bool,
) -> Result<Built, BuildError> {
    let (char, refs) = parse_request(req)?;
    for r in &refs {
        if r.boundary != Boundary::Rmb {
            return Err(BuildError::UnsupportedMaterialBoundary {
                label: r.label.as_str().to_owned(),
                boundary: r.boundary.as_str(),
            });
        }
    }
    let mut defs = Vec::with_capacity(refs.len());
    for r in &refs {
        let label = r.label.as_str();
        let def =
            inp.datums.datum(principal, label).ok_or_else(|| BuildError::DatumUndefined { label: label.to_owned() })?;
        if def.feature_type() != FeatureType::Plane {
            return Err(BuildError::UnsupportedDatumType {
                label: label.to_owned(),
                feature_type: def.feature_type().as_str(),
            });
        }
        if !def.targets().is_empty() {
            return Err(BuildError::DatumTargetsUnsupported { label: label.to_owned() });
        }
        defs.push(def);
    }

    // The interpretation chain of the frame, behind the governing-spec gate.
    let resolution = resolve_chain(inp.spec, inp.dprof, principal, &frame_characteristic(&char)?, inp.table)
        .map_err(BuildError::Chain)?;
    let res = match resolution {
        Resolution::Resolved(r) => r,
        other => return Err(BuildError::ChainNotResolved { outcome: other.outcome() }),
    };
    let table = inp.table.ok_or(BuildError::Chain(CommandError::DmtNotLoaded))?;
    let row = table.get(res.row().as_str()).ok_or(BuildError::ChainNotResolved { outcome: "UNRESOLVED" })?;
    if row.characteristic() != FRAME_CHARACTERISTIC || row.modifiers().len() != 1 || row.modifiers()[0] != RMB {
        return Err(BuildError::MaterialBoundaryMismatch { row: row.key().as_str().to_owned() });
    }
    let alg = simulator(res.chain())?;

    // The interpretation block (D-045): no DRF without it.
    let spec_err = |e: SpecError| BuildError::Chain(CommandError::Spec(e));
    let gs = inp.spec.require_governing_spec(principal, char.as_str(), inp.dprof).map_err(spec_err)?;
    let stored =
        inp.dprof.resolve(Branch::for_principal(principal), gs.defaults_profile()).map_err(|e| spec_err(e.into()))?;
    let block = Interpreted::compute(&[row], Some(stored.doc()), inp.register, Purpose::Evaluate)
        .finish(row.key().as_str(), res.chain_digest(), gs.defaults_profile())
        .map_err(BuildError::Interpretation)?;

    // The measured points, simulated in precedence order.
    let mut measured = Vec::with_capacity(defs.len());
    for def in &defs {
        let m = inp
            .points
            .map
            .get(def.feature_ref())
            .ok_or_else(|| BuildError::MeasurementMissing { feature: def.feature_ref().as_str().to_owned() })?;
        measured.push(m);
    }
    let planes: Vec<MeasuredPlane<'_>> = refs
        .iter()
        .zip(&measured)
        .map(|(r, m)| MeasuredPlane { label: r.label.as_str(), xyz: &m.xyz, nominal_outward: m.nominal })
        .collect();
    let built = build_measured_planar_drf(&planes, &alg, row.key()).map_err(|e| match e {
        SimError::UnsupportedSimulator { alg } => BuildError::UnsupportedSimulator { alg },
        other => BuildError::Simulation(other),
    })?;

    // The document.
    let drf = built.drf();
    let mut rows = Vec::with_capacity(refs.len());
    for (((r, def), m), (dd, (sim, ev))) in
        refs.iter().zip(&defs).zip(&measured).zip(drf.datums().iter().zip(built.datums().iter().zip(built.evidence())))
    {
        let (point, normal) = match sim.geometry {
            SimGeometry::Plane { point, normal } => (point, normal),
            SimGeometry::Axis { .. } => return Err(BuildError::Encode),
        };
        rows.push(obj([
            ("boundary", s(r.boundary.as_str())),
            ("constrained", dofs(dd.constrained)),
            ("dmt_row", s(dd.dmt_row.as_str())),
            ("feature", s(def.feature_ref().as_str())),
            ("fit", evidence(ev)),
            ("kind", s(dd.kind.name())),
            ("label", s(&dd.label)),
            ("measurement", obj([("digest", s(m.digest.as_str())), ("points", CValue::Num((m.xyz.len() / 3) as f64))])),
            ("nominal_outward", hex3(m.nominal)),
            ("plane", obj([("normal", hex3(normal)), ("point", hex3(point))])),
            ("precedence", s(precedence(dd.precedence))),
            ("simulator", s(dd.alg.as_str())),
        ]));
    }
    let [x, y, z] = drf.axes();
    let doc = obj([
        ("axes", obj([("x", hex3(x)), ("y", hex3(y)), ("z", hex3(z))])),
        (
            "chain",
            obj([
                ("chain_digest", s(res.chain_digest().as_str())),
                ("dmt_digest", s(res.dmt_digest().as_str())),
                ("row", s(res.row().as_str())),
                ("variant", s(res.variant())),
            ]),
        ),
        ("char", s(char.as_str())),
        ("datums", CValue::Arr(rows)),
        ("free", dofs(drf.free())),
        ("interpretation", s(block.interpretation.as_str())),
        ("interpretation_register", register_cvalue(block.register)),
        ("interpretation_rules", CValue::Arr(block.rules.iter().map(|r| r.to_cvalue()).collect())),
        ("origin", hex3(drf.origin())),
        ("plan", s(inp.datums.plan().as_str())),
        ("schema", s(DRF_SCHEMA)),
        ("values", s("measured")),
    ]);
    let bytes = fid_canon::to_canonical_bytes(&doc).map_err(|_| BuildError::Encode)?;
    let id = Digest::from_bytes(fid_canon::digest_bytes(&bytes).0);
    let branch = Branch::for_principal(principal);
    store.room(branch, id.as_str())?;
    if dry_run {
        return Ok(Built { id, branch, dry_run, doc, bytes, event: None });
    }
    let refs_text: Vec<String> = refs.iter().map(DatumRef::text).collect();
    let digests: Vec<(String, Digest)> =
        defs.iter().zip(&measured).map(|(d, m)| (d.feature_ref().as_str().to_owned(), m.digest)).collect();
    let event = DrfEvent::new(principal, char, refs_text, digests, doc.clone(), id, block.pin);
    store.put(branch, id.as_str().to_owned(), bytes.clone());
    Ok(Built { id, branch, dry_run, doc, bytes, event: Some(event) })
}

/// `gdt.get_drf@1` (R0): the stored canonical bytes of DRF `id` as `principal` sees it,
/// unchanged (`drf_unknown{id}` otherwise).
pub fn get_drf<'s>(store: &'s DrfStore, principal: Principal, id: &str) -> Result<&'s [u8], BuildError> {
    store.get(principal, id).ok_or_else(|| BuildError::DrfUnknown { id: echo(id) })
}
