//! The plan's datum model and the W2 command `gdt.define_datum@1`.
//!
//! [`DatumModel`] holds, for one plan, what a datum may be bound to (the nominal CAD faces,
//! the measured features with their type and nominal faces, and the geometry datum targets
//! may refer to) and the declared datums on the working branch and the agent overlay.
//!
//! **Branches (CONSTITUTION §3 W2).** The branch is derived from the principal
//! ([`Branch::for_principal`]). A person writes and reads the working branch only, so
//! nothing an agent declares ever reaches or blocks it. An agent or a script writes the
//! agent overlay, which sees the working branch beneath it: its label check covers both, and
//! its `replace` of a working-branch datum writes a shadowing overlay entry while the
//! working branch stays unchanged.

use super::event::DatumEvent;
use super::types::{DatumDefinition, DatumLabel, DatumTargetRef, FeatureType, LabelPattern, RefId, TargetKind};
use super::DatumError;
use crate::chars::dprof::{Branch, DprofState, ProfileValue};
use crate::chars::spec::{PlanId, SpecState};
use fid_canon::CValue;
use fid_io_reg::Principal;
use std::collections::{BTreeMap, BTreeSet};

/// The defaults-profile key that holds the datum label pattern (named key of
/// `fid.dmt-profile-keys/1`, `GDT.RES.SCHEMA`).
pub const LABEL_PATTERN_KEY: &str = "gdt.datum.label_pattern";
/// Most datums per branch of one plan.
pub const MAX_DATUMS: usize = 1_000;
/// Most IDs (faces, features and geometry together) in one plan.
pub const MAX_PLAN_IDS: usize = 100_000;
/// Most nominal faces one measured feature may have.
pub const MAX_FEATURE_FACES: usize = 4_096;

/// The label pattern of the plan's active defaults profile, as `principal` sees it: the
/// plan's default governing spec, else the tenant default (the inheritance rule of
/// `GDT.CHAR.SPEC`), resolved in `dprof` on the principal's branch, then its
/// [`LABEL_PATTERN_KEY`]. Fails closed with `label_pattern_unavailable{reason}`:
/// `bad_plan_id`, `no_governing_spec`, `profile_unknown` (not selectable on that branch, for
/// example an agent-published profile seen from the working branch), `key_missing`; a value
/// that is not text is `label_pattern_invalid{not_text}`, a malformed one
/// `label_pattern_invalid{reason}`.
pub fn active_label_pattern(
    spec: &SpecState,
    dprof: &DprofState,
    principal: Principal,
    plan: &str,
) -> Result<LabelPattern, DatumError> {
    let unavailable = |reason| DatumError::LabelPatternUnavailable { reason };
    let plan = PlanId::parse(plan).ok_or_else(|| unavailable("bad_plan_id"))?;
    let gs = spec.inherit(&plan).ok_or_else(|| unavailable("no_governing_spec"))?;
    let stored = dprof
        .resolve(Branch::for_principal(principal), gs.defaults_profile())
        .map_err(|_| unavailable("profile_unknown"))?;
    match stored.doc().profile().keys().get(LABEL_PATTERN_KEY) {
        None => Err(unavailable("key_missing")),
        Some(ProfileValue::Text(t)) => LabelPattern::parse(t),
        Some(_) => Err(DatumError::LabelPatternInvalid { reason: "not_text" }),
    }
}

/// One datum target as given on the command line or over MCP.
#[derive(Debug, Clone, Copy)]
pub struct TargetRequest<'a> {
    /// `id`: the datum label then a number (`A1`).
    pub id: &'a str,
    /// `kind`: `point`, `line` or `area`.
    pub kind: &'a str,
    /// `geometry`: the plan geometry the target refers to.
    pub geometry: &'a str,
}

/// The parameters of `gdt.define_datum@1`.
#[derive(Debug, Clone, Copy)]
pub struct DefineRequest<'a> {
    /// `label`.
    pub label: &'a str,
    /// `feature`: the measured feature.
    pub feature: &'a str,
    /// `faces`: nominal CAD faces of that feature.
    pub faces: &'a [&'a str],
    /// `type`: the datum feature type.
    pub feature_type: &'a str,
    /// `targets`: optional datum targets.
    pub targets: &'a [TargetRequest<'a>],
    /// `replace`: replace an existing datum with this label.
    pub replace: bool,
}

/// The result of `gdt.define_datum@1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Defined {
    plan: PlanId,
    branch: Branch,
    dry_run: bool,
    old: Option<DatumDefinition>,
    new: DatumDefinition,
    event: Option<DatumEvent>,
}

impl Defined {
    /// The branch written.
    pub fn branch(&self) -> Branch {
        self.branch
    }
    /// Whether this was a dry run.
    pub fn dry_run(&self) -> bool {
        self.dry_run
    }
    /// The definition replaced, if any.
    pub fn old(&self) -> Option<&DatumDefinition> {
        self.old.as_ref()
    }
    /// The new definition.
    pub fn definition(&self) -> &DatumDefinition {
        &self.new
    }
    /// The journal event; `None` for a dry run.
    pub fn event(&self) -> Option<&DatumEvent> {
        self.event.as_ref()
    }
    /// The `--json` / MCP result: `{branch, datum, dry_run, plan, replaced}`.
    pub fn to_cvalue(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("branch".to_owned(), CValue::Str(self.branch.as_str().to_owned()));
        m.insert("datum".to_owned(), self.new.to_cvalue());
        m.insert("dry_run".to_owned(), CValue::Bool(self.dry_run));
        m.insert("plan".to_owned(), CValue::Str(self.plan.as_str().to_owned()));
        m.insert("replaced".to_owned(), self.old.as_ref().map_or(CValue::Null, DatumDefinition::to_cvalue));
        CValue::Obj(m)
    }
    /// The result as RFC 8785 canonical JSON bytes.
    pub fn to_json_bytes(&self) -> Result<Vec<u8>, DatumError> {
        fid_canon::to_canonical_bytes(&self.to_cvalue()).map_err(|_| DatumError::invalid("result", "encode"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PlanFeature {
    pub(super) feature_type: FeatureType,
    pub(super) faces: BTreeSet<RefId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PlanGeometry {
    pub(super) kind: TargetKind,
    pub(super) face: RefId,
}

/// One plan's datum model: what datums can be bound to, and the declared datums on each
/// branch. Built with [`DatumModel::new`] and the `add_*` methods, or loaded from a
/// project file ([`super::load_project`]); every path applies the same checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatumModel {
    pub(super) plan: PlanId,
    pub(super) faces: BTreeSet<RefId>,
    pub(super) features: BTreeMap<RefId, PlanFeature>,
    pub(super) geometry: BTreeMap<RefId, PlanGeometry>,
    /// Every face, feature and geometry ID by its ASCII-lowercased text.
    ids_ci: BTreeMap<String, RefId>,
    pub(super) working: BTreeMap<DatumLabel, DatumDefinition>,
    pub(super) agent: BTreeMap<DatumLabel, DatumDefinition>,
}

impl DatumModel {
    /// An empty model of `plan` (`invalid_field{plan:bad_id}`).
    pub fn new(plan: &str) -> Result<DatumModel, DatumError> {
        let plan = PlanId::parse(plan).ok_or_else(|| DatumError::invalid("plan", "bad_id"))?;
        Ok(DatumModel {
            plan,
            faces: BTreeSet::new(),
            features: BTreeMap::new(),
            geometry: BTreeMap::new(),
            ids_ci: BTreeMap::new(),
            working: BTreeMap::new(),
            agent: BTreeMap::new(),
        })
    }

    /// The plan.
    pub fn plan(&self) -> &PlanId {
        &self.plan
    }

    /// Registers a new ID in the shared namespace of faces, features and geometry:
    /// `invalid_field{field:bad_id|duplicate|case_duplicate|too_many}`.
    fn claim(&mut self, field: &'static str, id: &str) -> Result<RefId, DatumError> {
        let id = RefId::parse(id).ok_or_else(|| DatumError::invalid(field, "bad_id"))?;
        match self.ids_ci.get(&id.as_str().to_ascii_lowercase()) {
            Some(k) if *k == id => return Err(DatumError::invalid(field, "duplicate")),
            Some(_) => return Err(DatumError::invalid(field, "case_duplicate")),
            None => {}
        }
        if self.ids_ci.len() >= MAX_PLAN_IDS {
            return Err(DatumError::invalid(field, "too_many"));
        }
        self.ids_ci.insert(id.as_str().to_ascii_lowercase(), id.clone());
        Ok(id)
    }

    /// Adds a nominal CAD face.
    pub fn add_face(&mut self, id: &str) -> Result<(), DatumError> {
        let id = self.claim("face", id)?;
        self.faces.insert(id);
        Ok(())
    }

    /// Adds a measured feature of type `feature_type` (`invalid_field{feature_type:unknown}`)
    /// with its nominal faces, each already added (`reference_missing{face}`), at most
    /// [`MAX_FEATURE_FACES`] and none twice (`invalid_field{faces:too_many|duplicate}`).
    pub fn add_feature(&mut self, id: &str, feature_type: &str, faces: &[&str]) -> Result<(), DatumError> {
        let t = FeatureType::parse(feature_type).ok_or_else(|| DatumError::invalid("feature_type", "unknown"))?;
        if faces.len() > MAX_FEATURE_FACES {
            return Err(DatumError::invalid("faces", "too_many"));
        }
        let mut set = BTreeSet::new();
        for f in faces {
            let f = RefId::parse(f).ok_or_else(|| DatumError::invalid("faces", "bad_id"))?;
            if !self.faces.contains(&f) {
                return Err(DatumError::missing(f.as_str()));
            }
            if !set.insert(f) {
                return Err(DatumError::invalid("faces", "duplicate"));
            }
        }
        let id = self.claim("feature", id)?;
        self.features.insert(id, PlanFeature { feature_type: t, faces: set });
        Ok(())
    }

    /// Adds a geometry a datum target may refer to: its kind
    /// (`invalid_field{kind:unknown}`) and the nominal face it lies on, already added
    /// (`reference_missing{face}`).
    pub fn add_geometry(&mut self, id: &str, kind: &str, face: &str) -> Result<(), DatumError> {
        let kind = TargetKind::parse(kind).ok_or_else(|| DatumError::invalid("kind", "unknown"))?;
        let face = RefId::parse(face).ok_or_else(|| DatumError::invalid("face", "bad_id"))?;
        if !self.faces.contains(&face) {
            return Err(DatumError::missing(face.as_str()));
        }
        let id = self.claim("geometry", id)?;
        self.geometry.insert(id, PlanGeometry { kind, face });
        Ok(())
    }

    /// The datum with this label as `principal` sees it: a person the working branch only;
    /// an agent or a script the overlay first, then the working branch. This is the lookup a
    /// DRF build uses: a label that is not declared has no datum.
    pub fn datum(&self, principal: Principal, label: &str) -> Option<&DatumDefinition> {
        let label = DatumLabel::parse(label).ok()?;
        self.visible(Branch::for_principal(principal), &label)
    }

    /// The datums held on `branch` itself, ascending by label.
    pub fn datums(&self, branch: Branch) -> impl Iterator<Item = &DatumDefinition> {
        self.own(branch).values()
    }

    fn own(&self, branch: Branch) -> &BTreeMap<DatumLabel, DatumDefinition> {
        match branch {
            Branch::Working => &self.working,
            Branch::Agent => &self.agent,
        }
    }

    fn visible(&self, branch: Branch, label: &DatumLabel) -> Option<&DatumDefinition> {
        match branch {
            Branch::Working => self.working.get(label),
            Branch::Agent => self.agent.get(label).or_else(|| self.working.get(label)),
        }
    }

    /// The plan-relative checks of a definition, in this order: the feature exists
    /// (`reference_missing{feature}`) and has the declared type
    /// (`feature_type_mismatch{feature}`); each face, ascending, exists
    /// (`reference_missing{face}`) and is a nominal face of that feature
    /// (`face_not_on_feature{face}`); each target, ascending by ID, refers to an existing
    /// geometry (`reference_missing{geometry}`) of the target's kind
    /// (`datum_target_invalid{target:kind_mismatch}`) lying on one of the datum's faces
    /// (`datum_target_invalid{target:not_on_datum_faces}`).
    pub fn check_references(&self, def: &DatumDefinition) -> Result<(), DatumError> {
        let feature =
            self.features.get(def.feature_ref()).ok_or_else(|| DatumError::missing(def.feature_ref().as_str()))?;
        if feature.feature_type != def.feature_type() {
            return Err(DatumError::FeatureTypeMismatch { feature: def.feature_ref().as_str().to_owned() });
        }
        for f in def.nominal_faces() {
            if !self.faces.contains(f) {
                return Err(DatumError::missing(f.as_str()));
            }
            if !feature.faces.contains(f) {
                return Err(DatumError::FaceNotOnFeature { face: f.as_str().to_owned() });
            }
        }
        for t in def.targets() {
            check_target(self, def, t)?;
        }
        Ok(())
    }

    /// `gdt.define_datum@1` (W2; CLI `fid gdt define-datum`, MCP `gdt_define_datum`):
    /// declares a datum on the principal's branch and returns one event holding the full
    /// definition (REQ-001), and the replaced one when `replace` is given (REQ-002). Every
    /// check runs before any change, in this order:
    ///
    /// 1. the label: base rule and `pattern` (`bad_label{reason}`);
    /// 2. the type: known and datum-capable (`unsupported_datum_feature_type`, REQ-003);
    /// 3. the parameter grammar: `invalid_field{feature|faces|targets:reason}` and each target
    ///    alone (`datum_target_invalid{target:bad_id|bad_kind}`);
    /// 4. the definition's own rules ([`DatumDefinition::new`]);
    /// 5. the references ([`Self::check_references`], REQ-004);
    /// 6. the label is not visible on the principal's branch unless `replace`
    ///    (`datum_label_exists{label}`, REQ-002);
    /// 7. room on the branch (`model_full{branch}`).
    ///
    /// A refusal and a dry run change nothing and journal nothing.
    pub fn define_datum(
        &mut self,
        principal: Principal,
        pattern: &LabelPattern,
        req: &DefineRequest<'_>,
        dry_run: bool,
    ) -> Result<Defined, DatumError> {
        let label = pattern.label(req.label)?;
        let feature_type = FeatureType::parse_datum(req.feature_type)?;
        let feature = RefId::parse(req.feature).ok_or_else(|| DatumError::invalid("feature", "bad_id"))?;
        if req.faces.len() > super::types::MAX_DATUM_FACES {
            return Err(DatumError::invalid("faces", "too_many"));
        }
        let faces = req
            .faces
            .iter()
            .map(|f| RefId::parse(f).ok_or_else(|| DatumError::invalid("faces", "bad_id")))
            .collect::<Result<Vec<_>, _>>()?;
        if req.targets.len() > super::types::MAX_TARGETS {
            return Err(DatumError::invalid("targets", "too_many"));
        }
        let targets =
            req.targets.iter().map(|t| DatumTargetRef::new(t.id, t.kind, t.geometry)).collect::<Result<Vec<_>, _>>()?;
        let new = DatumDefinition::new(label.clone(), feature, faces, feature_type, targets)?;
        self.check_references(&new)?;
        let branch = Branch::for_principal(principal);
        let old = self.visible(branch, &label).cloned();
        if old.is_some() && !req.replace {
            return Err(DatumError::DatumLabelExists { label: label.as_str().to_owned() });
        }
        if !self.own(branch).contains_key(&label) && self.own(branch).len() >= MAX_DATUMS {
            return Err(DatumError::ModelFull { branch });
        }
        if dry_run {
            return Ok(Defined { plan: self.plan.clone(), branch, dry_run, old, new, event: None });
        }
        let event =
            DatumEvent::new(principal, self.plan.clone(), pattern.clone(), req.replace, old.clone(), new.clone());
        match branch {
            Branch::Working => self.working.insert(label, new.clone()),
            Branch::Agent => self.agent.insert(label, new.clone()),
        };
        Ok(Defined { plan: self.plan.clone(), branch, dry_run, old, new, event: Some(event) })
    }

    /// Inserts a loaded datum after the same checks the command applies (minus the label
    /// pattern, which belongs to the active profile, not the file).
    pub(super) fn put_loaded(&mut self, branch: Branch, def: DatumDefinition) -> Result<(), DatumError> {
        self.check_references(&def)?;
        let map = match branch {
            Branch::Working => &mut self.working,
            Branch::Agent => &mut self.agent,
        };
        if map.len() >= MAX_DATUMS {
            return Err(DatumError::ModelFull { branch });
        }
        map.insert(def.label().clone(), def);
        Ok(())
    }
}

fn check_target(m: &DatumModel, def: &DatumDefinition, t: &DatumTargetRef) -> Result<(), DatumError> {
    let g = m.geometry.get(t.geometry()).ok_or_else(|| DatumError::missing(t.geometry().as_str()))?;
    if g.kind != t.kind() {
        return Err(DatumError::target(t.id(), "kind_mismatch"));
    }
    if !def.nominal_faces().contains(&g.face) {
        return Err(DatumError::target(t.id(), "not_on_datum_faces"));
    }
    Ok(())
}
