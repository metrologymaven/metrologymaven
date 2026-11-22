//! Format-independent uncertainty contributor contract (`contributor/v1`),
//! sub-graph expansion, library loading, and read commands (spec node
//! `UNC.MODEL.CONTRIB.CONTRACT`).
//!
//! Every contributor file declares:
//! - `schema`: `"contributor/v1"`;
//! - `id`: `uc:<name>@<semver>` (`name`: `[a-z0-9][a-z0-9-]{0,63}`, `semver`: `MAJOR.MINOR.PATCH`);
//! - `semver`: matching `MAJOR.MINOR.PATCH`;
//! - `params`: parameter specifications `{ name, unit, min, max, source_kinds }` where
//!   `min` and `max` are canonical 16-hex-digit IEEE-754 big-endian bit patterns (matching
//!   `fid-kunc` kernel convention; `-0.0`, NaN and infinities are refused);
//! - `template`: an [`UncertaintyModel`] (`uncertainty-model/v1`) whose operators are pinned
//!   kernel algorithm IDs;
//! - `applicability`: sorted, deduplicated lowercase applicability tags;
//! - `references`: standards references by designator and clause only (never standards prose);
//! - `digest` (optional on input, always verified when present, always emitted in full canonical
//!   form): SHA-256 over the RFC 8785 canonical JSON of the contributor body (all fields above
//!   except `digest`).
//!
//! [`expand`] instantiates a contributor for a caller-supplied `instance_id`, parameter values,
//! and source references: checks parameter bounds (`out_of_range`, never extrapolating), checks
//! source presence and kind (`source_kind_not_accepted`), substitutes parameters into template
//! inputs and `$param:<name>` operator parameters, prefixes every node ID with
//! `<instance_id>.`, and validates the resulting [`UncertaintyModel`] through
//! `fid_kunc::model::validate` (`contributor_invalid { findings }`).

pub mod command;

pub use command::{
    cli_params, declarations, invoke_cli, invoke_mcp, list_contributor, mcp_params, show_contributor, ContribCall,
    ContribCtx, ContributorSummary, ListContributorParams, ListContributorResult, ShowContributorParams,
    ShowContributorResult, LIST_CONTRIBUTOR, SHOW_CONTRIBUTOR,
};

use fid_canon::{parse_with_limits, to_canonical_bytes, CValue};
use fid_kci::kci::json::Json;
use fid_kci::kci::sha256_digest;
use fid_kci::manifest::KernelManifest;
use fid_kunc::dist::{Distribution, Params as DistParams, Quantity, UnitCode};
use fid_kunc::model::{
    is_model_name, is_semver, validate, InputSourceKind, ModelError, SourceRef, UncertaintyModel, ValidModel,
    MAX_ID_BYTES, MAX_PARAMS_DEPTH,
};
use fid_types::Digest;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::Path;

/// Schema identifier of every contributor document.
pub const SCHEMA: &str = "contributor/v1";
/// Largest contributor document [`ContributorModel::parse`] accepts, in bytes (2 MiB).
pub const MAX_CONTRIBUTOR_BYTES: usize = 2 * 1024 * 1024;
/// Deepest JSON nesting [`ContributorModel::parse`] accepts.
pub const MAX_JSON_DEPTH: usize = 40;
/// Most parameters one contributor may declare.
pub const MAX_PARAMS: usize = 64;
/// Most applicability tags one contributor may declare.
pub const MAX_APPLICABILITY: usize = 32;
/// Most standard references one contributor may declare.
pub const MAX_REFERENCES: usize = 32;
/// Longest designator or clause string in a [`StandardRef`], in bytes.
pub const MAX_REF_FIELD_BYTES: usize = 128;
/// Longest applicability tag, in bytes.
pub const MAX_TAG_BYTES: usize = 64;
/// Longest instance ID prefix accepted by [`expand`], in bytes.
pub const MAX_INSTANCE_ID_BYTES: usize = 32;
/// Most contributor JSON files [`ContributorLibrary::load_dir`] reads from one directory.
pub const MAX_LIBRARY_FILES: usize = 256;

/// Errors returned by the contributor contract, expansion, and read commands.
#[derive(Debug, Clone, PartialEq)]
pub enum ContribError {
    /// A parameter value lies outside its declared `[min, max]` valid range (REQ-003).
    OutOfRange { param: String, value: f64, min: f64, max: f64 },
    /// A parameter's source reference is missing or of a kind not accepted by the contributor
    /// (REQ-004).
    SourceKindNotAccepted { param: String, got: Option<InputSourceKind>, accepted: Vec<InputSourceKind> },
    /// The expanded sub-graph failed `UNC.MODEL.GRAPH.CHECK` validation (REQ-002).
    ContributorInvalid { findings: Vec<ModelError> },
    /// No contributor with the given ID exists in the library (REQ-005).
    ContributorNotFound { id: String },
    /// A contributor document or parts failed `contributor/v1` schema checks.
    SchemaInvalid { field: String, detail: String },
    /// Invalid command or expansion call parameters.
    BadParams { reason: &'static str },
    /// Bus policy class check refused the principal.
    PolicyRefused { reason: &'static str },
    /// Internal invariant failure.
    Internal { reason: &'static str },
}

impl ContribError {
    /// Stable machine-readable error code.
    pub const fn code(&self) -> &'static str {
        match self {
            ContribError::OutOfRange { .. } => "out_of_range",
            ContribError::SourceKindNotAccepted { .. } => "source_kind_not_accepted",
            ContribError::ContributorInvalid { .. } => "contributor_invalid",
            ContribError::ContributorNotFound { .. } => "contributor_not_found",
            ContribError::SchemaInvalid { .. } => "schema_invalid",
            ContribError::BadParams { .. } => "bad_params",
            ContribError::PolicyRefused { .. } => "policy_refused",
            ContribError::Internal { .. } => "internal",
        }
    }

    pub(crate) fn bad(reason: &'static str) -> Self {
        ContribError::BadParams { reason }
    }
}

impl fmt::Display for ContribError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ContribError::OutOfRange { param, value, min, max } => {
                write!(f, "out_of_range: parameter '{param}' value {value} is outside valid range [{min}, {max}]")
            }
            ContribError::SourceKindNotAccepted { param, got, accepted } => {
                let got_s = got.map_or("missing", InputSourceKind::wire);
                let acc: Vec<&str> = accepted.iter().map(|k| k.wire()).collect();
                write!(f, "source_kind_not_accepted: parameter '{param}' got '{got_s}', accepted [{}]", acc.join(", "))
            }
            ContribError::ContributorInvalid { findings } => {
                let list: Vec<String> = findings.iter().map(ToString::to_string).collect();
                write!(f, "contributor_invalid: {}", list.join("; "))
            }
            ContribError::ContributorNotFound { id } => write!(f, "contributor_not_found: {id}"),
            ContribError::SchemaInvalid { field, detail } => write!(f, "schema_invalid ({field}): {detail}"),
            ContribError::BadParams { reason } => write!(f, "bad_params: {reason}"),
            ContribError::PolicyRefused { reason } => write!(f, "policy_refused: {reason}"),
            ContribError::Internal { reason } => write!(f, "internal: {reason}"),
        }
    }
}

impl std::error::Error for ContribError {}

/// A reference to a standard by designator and clause number only (never standards prose).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct StandardRef {
    pub designator: String,
    pub clause: String,
}

/// Declaration of one contributor parameter: unit, valid `[min, max]` range, and accepted
/// source kinds (REQ-UNC.MODEL.CONTRIB.CONTRACT-001).
#[derive(Debug, Clone, PartialEq)]
pub struct ParamSpec {
    pub name: String,
    pub unit: UnitCode,
    pub min: f64,
    pub max: f64,
    pub source_kinds: Vec<InputSourceKind>,
}

/// Parts needed to construct a [`ContributorModel`].
#[derive(Debug, Clone, PartialEq)]
pub struct ContributorParts {
    pub name: String,
    pub semver: String,
    pub params: Vec<ParamSpec>,
    pub template: UncertaintyModel,
    pub applicability: Vec<String>,
    pub references: Vec<StandardRef>,
}

/// A validated, canonical `contributor/v1` definition with its RFC 8785 digest.
#[derive(Debug, Clone, PartialEq)]
pub struct ContributorModel {
    id: String,
    name: String,
    semver: String,
    params: Vec<ParamSpec>,
    template: UncertaintyModel,
    applicability: Vec<String>,
    references: Vec<StandardRef>,
    canonical_body: Vec<u8>,
    canonical_full: Vec<u8>,
    digest: Digest,
}

impl ContributorModel {
    /// Canonicalizes, checks, and hashes `parts` into a [`ContributorModel`].
    pub fn new(mut parts: ContributorParts) -> Result<Self, ContribError> {
        if !is_model_name(&parts.name) {
            return Err(schema_err("id", "contributor name must match [a-z0-9][a-z0-9-]{0,63}"));
        }
        if !is_semver(&parts.semver) {
            return Err(schema_err("semver", "semver must match MAJOR.MINOR.PATCH"));
        }
        if parts.params.len() > MAX_PARAMS {
            return Err(schema_err("params", "exceeds MAX_PARAMS"));
        }
        if parts.applicability.len() > MAX_APPLICABILITY {
            return Err(schema_err("applicability", "exceeds MAX_APPLICABILITY"));
        }
        if parts.references.len() > MAX_REFERENCES {
            return Err(schema_err("references", "exceeds MAX_REFERENCES"));
        }

        // Validate and sort params by name.
        parts.params.sort_by(|a, b| a.name.as_bytes().cmp(b.name.as_bytes()));
        let mut seen_params = BTreeSet::new();
        for p in &mut parts.params {
            if !is_param_name(&p.name) {
                return Err(schema_err("params.name", "parameter name must match [a-z][a-z0-9_]{0,63}"));
            }
            if !seen_params.insert(p.name.clone()) {
                return Err(schema_err("params.name", "duplicate parameter name"));
            }
            if !is_canonical_f64(p.min) {
                return Err(schema_err("params.min", "min must be finite and not -0.0"));
            }
            if !is_canonical_f64(p.max) {
                return Err(schema_err("params.max", "max must be finite and not -0.0"));
            }
            if p.min > p.max {
                return Err(schema_err("params.range", "min must be <= max"));
            }
            if p.source_kinds.is_empty() {
                return Err(schema_err("params.source_kinds", "must accept at least one source kind"));
            }
            p.source_kinds.sort_by(|a, b| a.wire().as_bytes().cmp(b.wire().as_bytes()));
            p.source_kinds.dedup();
        }

        // Validate and sort applicability tags.
        let mut app_set = BTreeSet::new();
        for tag in &parts.applicability {
            if !is_tag(tag) {
                return Err(schema_err("applicability", "tag must match [a-z0-9][a-z0-9_-]{0,63}"));
            }
            app_set.insert(tag.clone());
        }
        let applicability: Vec<String> = app_set.into_iter().collect();

        // Validate and sort standard references.
        for r in &parts.references {
            if !is_plain_ascii(&r.designator, MAX_REF_FIELD_BYTES) {
                return Err(schema_err("references.designator", "invalid designator string"));
            }
            if !is_plain_ascii(&r.clause, MAX_REF_FIELD_BYTES) {
                return Err(schema_err("references.clause", "invalid clause string"));
            }
        }
        parts.references.sort();
        parts.references.dedup();

        let id = format!("uc:{}@{}", parts.name, parts.semver);
        let template_val = parse_with_limits(parts.template.canonical_json(), MAX_CONTRIBUTOR_BYTES, MAX_JSON_DEPTH)
            .map_err(|_| ContribError::Internal { reason: "template canonical JSON failed to parse" })?;

        let mut body_map = BTreeMap::new();
        body_map.insert(
            "applicability".into(),
            CValue::Arr(applicability.iter().map(|t| CValue::Str(t.clone())).collect()),
        );
        body_map.insert("id".into(), CValue::Str(id.clone()));
        body_map.insert("params".into(), CValue::Arr(parts.params.iter().map(param_spec_to_cvalue).collect()));
        body_map.insert("references".into(), CValue::Arr(parts.references.iter().map(std_ref_to_cvalue).collect()));
        body_map.insert("schema".into(), CValue::Str(SCHEMA.into()));
        body_map.insert("semver".into(), CValue::Str(parts.semver.clone()));
        body_map.insert("template".into(), template_val);

        let canonical_body = to_canonical_bytes(&CValue::Obj(body_map.clone()))
            .map_err(|_| ContribError::Internal { reason: "contributor body has no canonical form" })?;
        if canonical_body.len() > MAX_CONTRIBUTOR_BYTES {
            return Err(schema_err("contributor", "exceeds MAX_CONTRIBUTOR_BYTES"));
        }
        let digest = sha256_digest(&canonical_body);

        body_map.insert("digest".into(), CValue::Str(digest.as_str().to_owned()));
        let canonical_full = to_canonical_bytes(&CValue::Obj(body_map))
            .map_err(|_| ContribError::Internal { reason: "contributor document has no canonical form" })?;

        Ok(Self {
            id,
            name: parts.name,
            semver: parts.semver,
            params: parts.params,
            template: parts.template,
            applicability,
            references: parts.references,
            canonical_body,
            canonical_full,
            digest,
        })
    }

    /// Parses and validates a `contributor/v1` JSON document. If a `"digest"` field is
    /// present, verifies it equals the SHA-256 of the canonical body.
    pub fn parse(bytes: &[u8]) -> Result<Self, ContribError> {
        if bytes.len() > MAX_CONTRIBUTOR_BYTES {
            return Err(schema_err("contributor", "exceeds MAX_CONTRIBUTOR_BYTES"));
        }
        let val = parse_with_limits(bytes, MAX_CONTRIBUTOR_BYTES, MAX_JSON_DEPTH)
            .map_err(|e| schema_err("json", format!("invalid JSON: {e}")))?;
        let obj = as_obj(&val, "contributor")?;

        let allowed: &[&str] =
            &["applicability", "digest", "id", "params", "references", "schema", "semver", "template"];
        for k in obj.keys() {
            if !allowed.contains(&k.as_str()) {
                return Err(schema_err("contributor", format!("unknown field '{k}'")));
            }
        }

        let schema = req_str(obj, "schema")?;
        if schema != SCHEMA {
            return Err(schema_err("schema", format!("expected '{SCHEMA}', got '{schema}'")));
        }

        let id = req_str(obj, "id")?;
        let semver = req_str(obj, "semver")?;
        let (name, id_semver) = parse_contributor_id(id)?;
        if id_semver != semver {
            return Err(schema_err("semver", "semver does not match id suffix"));
        }

        let declared_digest = match obj.get("digest") {
            None => None,
            Some(CValue::Str(s)) => {
                let d = Digest::parse(s).map_err(|_| schema_err("digest", "invalid sha256 digest"))?;
                Some(d)
            }
            Some(_) => return Err(schema_err("digest", "expected string")),
        };

        let params_arr = as_arr(req_field(obj, "params")?, "params")?;
        if params_arr.len() > MAX_PARAMS {
            return Err(schema_err("params", "exceeds MAX_PARAMS"));
        }
        let mut params = Vec::with_capacity(params_arr.len());
        for (idx, item) in params_arr.iter().enumerate() {
            let ppath = format!("params[{idx}]");
            let pobj = as_obj(item, &ppath)?;
            let p_allowed: &[&str] = &["max", "min", "name", "source_kinds", "unit"];
            for k in pobj.keys() {
                if !p_allowed.contains(&k.as_str()) {
                    return Err(schema_err(&ppath, format!("unknown field '{k}'")));
                }
            }
            let pname = req_str(pobj, "name")?.to_owned();
            let unit_s = req_str(pobj, "unit")?;
            let unit = UnitCode::parse(unit_s).ok_or_else(|| schema_err(format!("{ppath}.unit"), "unknown unit"))?;
            let min = parse_f64_hex(req_str(pobj, "min")?, &format!("{ppath}.min"))?;
            let max = parse_f64_hex(req_str(pobj, "max")?, &format!("{ppath}.max"))?;
            let sk_arr = as_arr(req_field(pobj, "source_kinds")?, &format!("{ppath}.source_kinds"))?;
            let mut source_kinds = Vec::with_capacity(sk_arr.len());
            for sk in sk_arr {
                let CValue::Str(s) = sk else {
                    return Err(schema_err(format!("{ppath}.source_kinds"), "expected string"));
                };
                let kind = InputSourceKind::from_wire(s)
                    .ok_or_else(|| schema_err(format!("{ppath}.source_kinds"), format!("unknown source kind '{s}'")))?;
                source_kinds.push(kind);
            }
            params.push(ParamSpec { name: pname, unit, min, max, source_kinds });
        }

        let app_arr = as_arr(req_field(obj, "applicability")?, "applicability")?;
        let mut applicability = Vec::with_capacity(app_arr.len());
        for a in app_arr {
            let CValue::Str(s) = a else {
                return Err(schema_err("applicability", "expected string"));
            };
            applicability.push(s.clone());
        }

        let refs_arr = as_arr(req_field(obj, "references")?, "references")?;
        let mut references = Vec::with_capacity(refs_arr.len());
        for (idx, r) in refs_arr.iter().enumerate() {
            let rpath = format!("references[{idx}]");
            let robj = as_obj(r, &rpath)?;
            for k in robj.keys() {
                if k != "designator" && k != "clause" {
                    return Err(schema_err(&rpath, format!("unknown field '{k}'")));
                }
            }
            references.push(StandardRef {
                designator: req_str(robj, "designator")?.to_owned(),
                clause: req_str(robj, "clause")?.to_owned(),
            });
        }

        let template_val = req_field(obj, "template")?;
        let template_bytes = to_canonical_bytes(template_val)
            .map_err(|_| schema_err("template", "template is not canonicalizable JSON"))?;
        let template = UncertaintyModel::parse(&template_bytes)
            .map_err(|e| schema_err("template", format!("invalid template model: {e}")))?;

        let model = Self::new(ContributorParts {
            name: name.to_owned(),
            semver: semver.to_owned(),
            params,
            template,
            applicability,
            references,
        })?;

        if let Some(d) = declared_digest {
            if d != model.digest {
                return Err(schema_err("digest", "declared digest does not match canonical body SHA-256"));
            }
        }

        Ok(model)
    }

    /// Contributor ID: `uc:<name>@<semver>`.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Contributor name (between `uc:` and `@`).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Semantic version `MAJOR.MINOR.PATCH`.
    pub fn semver(&self) -> &str {
        &self.semver
    }

    /// Parameter specifications, sorted by name.
    pub fn params(&self) -> &[ParamSpec] {
        &self.params
    }

    /// Parameter names, sorted ascending.
    pub fn param_names(&self) -> Vec<String> {
        self.params.iter().map(|p| p.name.clone()).collect()
    }

    /// Sub-graph template (`uncertainty-model/v1`).
    pub fn template(&self) -> &UncertaintyModel {
        &self.template
    }

    /// Applicability tags, sorted and unique.
    pub fn applicability(&self) -> &[String] {
        &self.applicability
    }

    /// Standards references (designator and clause only), sorted and unique.
    pub fn references(&self) -> &[StandardRef] {
        &self.references
    }

    /// Canonical RFC 8785 bytes of the contributor body (hashed to produce [`Self::digest`]).
    pub fn canonical_body_json(&self) -> &[u8] {
        &self.canonical_body
    }

    /// Full canonical RFC 8785 JSON document including the `"digest"` field.
    pub fn canonical_json(&self) -> &[u8] {
        &self.canonical_full
    }

    /// SHA-256 digest of [`Self::canonical_body_json`].
    pub fn digest(&self) -> Digest {
        self.digest
    }
}

/// Caller-supplied value for one contributor parameter at expansion time.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamInput {
    /// Physical value in the parameter's declared unit, checked against `[min, max]`.
    pub value: f64,
    /// Optional distribution override for the parameter's input node. When `None`, the
    /// template input node's distribution is recentered at `value` preserving its spread.
    pub distribution: Option<Distribution>,
}

impl ParamInput {
    /// A parameter value that recenters the template input node's distribution.
    pub const fn value(value: f64) -> Self {
        Self { value, distribution: None }
    }

    /// A parameter value paired with an explicit validated [`Distribution`].
    pub const fn with_distribution(value: f64, distribution: Distribution) -> Self {
        Self { value, distribution: Some(distribution) }
    }
}

/// A validated sub-graph produced by [`expand`].
#[derive(Debug, Clone, PartialEq)]
pub struct SubGraph {
    contributor_id: String,
    contributor_digest: Digest,
    instance_id: String,
    valid_model: ValidModel,
}

impl SubGraph {
    /// Contributor ID (`uc:<name>@<semver>`).
    pub fn contributor_id(&self) -> &str {
        &self.contributor_id
    }

    /// Digest of the contributor definition that produced this sub-graph.
    pub fn contributor_digest(&self) -> Digest {
        self.contributor_digest
    }

    /// Instance ID prefix carried by every node ID (`<instance_id>.<node>`).
    pub fn instance_id(&self) -> &str {
        &self.instance_id
    }

    /// The `UNC.MODEL.GRAPH.CHECK`-validated model.
    pub fn valid_model(&self) -> &ValidModel {
        &self.valid_model
    }

    /// The expanded canonical [`UncertaintyModel`].
    pub fn model(&self) -> &UncertaintyModel {
        self.valid_model.model()
    }

    /// SHA-256 digest of the expanded canonical sub-graph.
    pub fn digest(&self) -> Digest {
        self.valid_model.model().digest()
    }

    /// Canonical RFC 8785 JSON bytes of the expanded sub-graph.
    pub fn canonical_json(&self) -> &[u8] {
        self.valid_model.model().canonical_json()
    }
}

/// Instantiates `contributor` into a validated [`SubGraph`] for `instance_id`
/// (REQ-UNC.MODEL.CONTRIB.CONTRACT-002, -003, -004).
///
/// Steps in deterministic order:
/// 1. Validate `instance_id` (`[a-z0-9][a-z0-9_-]{0,31}`) and reject undeclared parameter/source keys.
/// 2. For every declared parameter (sorted by name):
///    - Verify parameter presence, finite value, and `min <= value <= max` (`out_of_range`,
///      never extrapolating).
///    - Verify source reference presence and that its `kind` is in `spec.source_kinds`
///      (`source_kind_not_accepted`).
/// 3. Substitute parameters into matching template `InputNode`s (attaching `SourceRef` and
///    applying the parameter value/distribution) and into any operator `"$param:<name>"` slots.
/// 4. Prefix every node ID (inputs, operators, outputs, edge endpoints, correlation inputs)
///    with `<instance_id>.`.
/// 5. Canonicalize as an [`UncertaintyModel`] and validate through `fid_kunc::model::validate`
///    against `manifest`, returning `contributor_invalid { findings }` on any finding.
pub fn expand(
    contributor: &ContributorModel,
    instance_id: &str,
    params: &BTreeMap<String, ParamInput>,
    sources: &BTreeMap<String, SourceRef>,
    manifest: &KernelManifest,
) -> Result<SubGraph, ContribError> {
    if !is_instance_id(instance_id) {
        return Err(ContribError::bad("instance_id must match [a-z0-9][a-z0-9_-]{0,31}"));
    }

    let spec_map: BTreeMap<&str, &ParamSpec> = contributor.params.iter().map(|p| (p.name.as_str(), p)).collect();
    for k in params.keys() {
        if !spec_map.contains_key(k.as_str()) {
            return Err(ContribError::bad("unknown parameter supplied"));
        }
    }
    for k in sources.keys() {
        if !spec_map.contains_key(k.as_str()) {
            return Err(ContribError::bad("unknown source key supplied"));
        }
    }

    // Check ranges (REQ-003) and source kinds (REQ-004) in deterministic parameter order.
    for spec in &contributor.params {
        let Some(input) = params.get(&spec.name) else {
            return Err(ContribError::bad("missing required parameter"));
        };
        if !input.value.is_finite() || input.value < spec.min || input.value > spec.max {
            return Err(ContribError::OutOfRange {
                param: spec.name.clone(),
                value: input.value,
                min: spec.min,
                max: spec.max,
            });
        }
        if let Some(dist) = input.distribution {
            if dist.unit() != spec.unit {
                return Err(ContribError::bad("parameter distribution unit does not match spec unit"));
            }
        }
        match sources.get(&spec.name) {
            None => {
                return Err(ContribError::SourceKindNotAccepted {
                    param: spec.name.clone(),
                    got: None,
                    accepted: spec.source_kinds.clone(),
                });
            }
            Some(src) if !spec.source_kinds.contains(&src.kind) => {
                return Err(ContribError::SourceKindNotAccepted {
                    param: spec.name.clone(),
                    got: Some(src.kind),
                    accepted: spec.source_kinds.clone(),
                });
            }
            Some(_) => {}
        }
    }

    let mut parts = contributor.template.parts().clone();

    for input_node in &mut parts.inputs {
        if let Some(spec) = spec_map.get(input_node.id.as_str()) {
            if input_node.unit != spec.unit {
                return Err(schema_err("template.inputs", "template input unit differs from parameter spec unit"));
            }
            let p_in =
                params.get(&spec.name).ok_or(ContribError::Internal { reason: "validated parameter disappeared" })?;
            let src =
                sources.get(&spec.name).ok_or(ContribError::Internal { reason: "validated source disappeared" })?;
            input_node.source = Some(src.clone());
            input_node.distribution = match p_in.distribution {
                Some(d) => d,
                None => recenter_distribution(input_node.distribution, p_in.value)?,
            };
        }
        input_node.id = prefix_id(instance_id, &input_node.id)?;
    }

    for op in &mut parts.operators {
        op.params = substitute_json_params(&op.params, params, 0)?;
        op.id = prefix_id(instance_id, &op.id)?;
    }

    for out in &mut parts.outputs {
        out.id = prefix_id(instance_id, &out.id)?;
    }

    for edge in &mut parts.edges {
        edge.from.node = prefix_id(instance_id, &edge.from.node)?;
        edge.to.node = prefix_id(instance_id, &edge.to.node)?;
    }

    for group in &mut parts.correlations {
        for id in &mut group.inputs {
            *id = prefix_id(instance_id, id)?;
        }
    }

    let expanded = UncertaintyModel::new(parts)
        .map_err(|e| schema_err("expanded", format!("expanded sub-graph failed schema check: {e}")))?;

    let valid_model =
        validate(&expanded, manifest).map_err(|findings| ContribError::ContributorInvalid { findings })?;

    Ok(SubGraph {
        contributor_id: contributor.id.clone(),
        contributor_digest: contributor.digest,
        instance_id: instance_id.to_owned(),
        valid_model,
    })
}

/// An in-memory library of validated contributors, keyed and sorted by `uc:<name>@<semver>`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ContributorLibrary {
    entries: BTreeMap<String, ContributorModel>,
}

impl ContributorLibrary {
    /// An empty library.
    pub fn new() -> Self {
        Self { entries: BTreeMap::new() }
    }

    /// Inserts one contributor, refusing a duplicate ID with a different digest.
    pub fn insert(&mut self, model: ContributorModel) -> Result<(), ContribError> {
        if let Some(existing) = self.entries.get(model.id()) {
            if existing.digest() != model.digest() {
                return Err(schema_err(
                    "library",
                    format!("duplicate contributor id '{}' with differing digest", model.id()),
                ));
            }
            return Ok(());
        }
        self.entries.insert(model.id().to_owned(), model);
        Ok(())
    }

    /// Builds a library from an iterator of contributors.
    pub fn from_models(models: impl IntoIterator<Item = ContributorModel>) -> Result<Self, ContribError> {
        let mut lib = Self::new();
        for m in models {
            lib.insert(m)?;
        }
        Ok(lib)
    }

    /// Loads every `*.json` contributor file from `dir` in deterministic filename order.
    pub fn load_dir(dir: &Path) -> Result<Self, ContribError> {
        let rd = fs::read_dir(dir).map_err(|_| schema_err("library", "failed to read contributor directory"))?;
        let mut files = Vec::new();
        for entry in rd {
            let entry = entry.map_err(|_| schema_err("library", "failed to read directory entry"))?;
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("json") {
                files.push(path);
            }
        }
        files.sort();
        if files.len() > MAX_LIBRARY_FILES {
            return Err(schema_err("library", "exceeds MAX_LIBRARY_FILES"));
        }
        let mut lib = Self::new();
        for path in files {
            let bytes = fs::read(&path).map_err(|_| schema_err("library", "failed to read contributor file"))?;
            let model = ContributorModel::parse(&bytes)?;
            lib.insert(model)?;
        }
        Ok(lib)
    }

    /// Looks up one contributor by exact ID (`uc:<name>@<semver>`).
    pub fn get(&self, id: &str) -> Option<&ContributorModel> {
        self.entries.get(id)
    }

    /// Lists contributors sorted by ID, optionally filtered by an `applicability` tag.
    pub fn list(&self, applicability: Option<&str>) -> Vec<&ContributorModel> {
        self.entries
            .values()
            .filter(|c| applicability.is_none_or(|tag| c.applicability.iter().any(|a| a == tag)))
            .collect()
    }

    /// Number of contributors in the library.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True if the library holds no contributors.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Internal helpers

fn schema_err(field: impl Into<String>, detail: impl Into<String>) -> ContribError {
    ContribError::SchemaInvalid { field: field.into(), detail: detail.into() }
}

fn is_canonical_f64(v: f64) -> bool {
    v.is_finite() && !(v == 0.0 && v.is_sign_negative())
}

/// Encodes a finite non-negative-zero `f64` as 16 lowercase IEEE-754 big-endian hex digits.
pub fn f64_to_hex(v: f64) -> Result<String, ContribError> {
    if !is_canonical_f64(v) {
        return Err(schema_err("f64", "expected finite f64 (not -0.0)"));
    }
    Ok(format!("{:016x}", v.to_bits()))
}

fn parse_f64_hex(s: &str, field: &str) -> Result<f64, ContribError> {
    if s.len() != 16 || !s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return Err(schema_err(field, "expected 16 lowercase hex digits for IEEE-754 f64"));
    }
    let bits = u64::from_str_radix(s, 16).map_err(|_| schema_err(field, "invalid hex f64"))?;
    let v = f64::from_bits(bits);
    if !is_canonical_f64(v) {
        return Err(schema_err(field, "non-finite or -0.0 f64 refused"));
    }
    Ok(v)
}

fn canonical_zero(v: f64) -> f64 {
    if v == 0.0 {
        0.0
    } else {
        v
    }
}

fn recenter_distribution(dist: Distribution, new_center: f64) -> Result<Distribution, ContribError> {
    let u = dist.unit();
    let q = |v: f64| Quantity::new(canonical_zero(v), u);
    let q1 = |v: f64| Quantity::new(canonical_zero(v), UnitCode::One);
    let res = match dist.params() {
        DistParams::Normal { sigma, .. } => Distribution::normal(q(new_center), q(sigma)),
        DistParams::Rect { a, b } => {
            let half = 0.5 * (b - a);
            Distribution::rect(q(new_center - half), q(new_center + half))
        }
        DistParams::Tri { a, b, c } => {
            let mid = 0.5 * (a + b);
            let delta = new_center - mid;
            Distribution::tri(q(a + delta), q(b + delta), q(c + delta))
        }
        DistParams::Arcsine { a, b } => {
            let half = 0.5 * (b - a);
            Distribution::arcsine(q(new_center - half), q(new_center + half))
        }
        DistParams::ScaledT { s, nu, .. } => Distribution::scaled_t(q(new_center), q(s), q1(nu)),
        DistParams::Trap { a, b, beta } => {
            let half = 0.5 * (b - a);
            Distribution::trap(q(new_center - half), q(new_center + half), q1(beta))
        }
    };
    res.map_err(|e| schema_err("template.inputs.distribution", format!("recentering failed: {e}")))
}

fn substitute_json_params(
    val: &Json,
    params: &BTreeMap<String, ParamInput>,
    depth: usize,
) -> Result<Json, ContribError> {
    if depth > MAX_PARAMS_DEPTH {
        return Err(schema_err("template.operators.params", "exceeds MAX_PARAMS_DEPTH"));
    }
    match val {
        Json::Str(s) => {
            if let Some(pname) = s.strip_prefix("$param:") {
                let p = params.get(pname).ok_or_else(|| {
                    schema_err("template.operators.params", format!("template references undeclared param '{pname}'"))
                })?;
                Ok(Json::Str(f64_to_hex(canonical_zero(p.value))?))
            } else {
                Ok(Json::Str(s.clone()))
            }
        }
        Json::Arr(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(substitute_json_params(item, params, depth + 1)?);
            }
            Ok(Json::Arr(out))
        }
        Json::Obj(entries) => {
            let mut out = Vec::with_capacity(entries.len());
            for (k, v) in entries {
                out.push((k.clone(), substitute_json_params(v, params, depth + 1)?));
            }
            Ok(Json::Obj(out))
        }
    }
}

fn prefix_id(instance_id: &str, node_id: &str) -> Result<String, ContribError> {
    let prefixed = format!("{instance_id}.{node_id}");
    if prefixed.len() > MAX_ID_BYTES {
        return Err(ContribError::bad("prefixed node ID exceeds MAX_ID_BYTES"));
    }
    Ok(prefixed)
}

fn parse_contributor_id(id: &str) -> Result<(&str, &str), ContribError> {
    let rest = id.strip_prefix("uc:").ok_or_else(|| schema_err("id", "contributor id must start with 'uc:'"))?;
    let (name, semver) =
        rest.split_once('@').ok_or_else(|| schema_err("id", "contributor id must be uc:<name>@<semver>"))?;
    if !is_model_name(name) || !is_semver(semver) {
        return Err(schema_err("id", "invalid contributor id name or semver"));
    }
    Ok((name, semver))
}

fn is_param_name(s: &str) -> bool {
    let b = s.as_bytes();
    match b.split_first() {
        Some((&first, tail)) => {
            b.len() <= MAX_ID_BYTES
                && first.is_ascii_lowercase()
                && tail.iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
        }
        None => false,
    }
}

fn is_tag(s: &str) -> bool {
    let b = s.as_bytes();
    match b.split_first() {
        Some((&first, tail)) => {
            b.len() <= MAX_TAG_BYTES
                && (first.is_ascii_lowercase() || first.is_ascii_digit())
                && tail.iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_' || c == b'-')
        }
        None => false,
    }
}

fn is_instance_id(s: &str) -> bool {
    let b = s.as_bytes();
    match b.split_first() {
        Some((&first, tail)) => {
            b.len() <= MAX_INSTANCE_ID_BYTES
                && (first.is_ascii_lowercase() || first.is_ascii_digit())
                && tail.iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_' || c == b'-')
        }
        None => false,
    }
}

fn is_plain_ascii(s: &str, max: usize) -> bool {
    !s.is_empty()
        && s.len() <= max
        && !s.starts_with(' ')
        && !s.ends_with(' ')
        && !s.contains("  ")
        && s.bytes().all(|b| (0x20..=0x7e).contains(&b) && b != b'"' && b != b'\\')
}

fn param_spec_to_cvalue(p: &ParamSpec) -> CValue {
    let mut m = BTreeMap::new();
    m.insert("max".into(), CValue::Str(format!("{:016x}", p.max.to_bits())));
    m.insert("min".into(), CValue::Str(format!("{:016x}", p.min.to_bits())));
    m.insert("name".into(), CValue::Str(p.name.clone()));
    m.insert(
        "source_kinds".into(),
        CValue::Arr(p.source_kinds.iter().map(|k| CValue::Str(k.wire().to_owned())).collect()),
    );
    m.insert("unit".into(), CValue::Str(p.unit.as_str().to_owned()));
    CValue::Obj(m)
}

fn std_ref_to_cvalue(r: &StandardRef) -> CValue {
    let mut m = BTreeMap::new();
    m.insert("clause".into(), CValue::Str(r.clause.clone()));
    m.insert("designator".into(), CValue::Str(r.designator.clone()));
    CValue::Obj(m)
}

fn as_obj<'a>(v: &'a CValue, field: &str) -> Result<&'a BTreeMap<String, CValue>, ContribError> {
    match v {
        CValue::Obj(m) => Ok(m),
        _ => Err(schema_err(field, "expected JSON object")),
    }
}

fn as_arr<'a>(v: &'a CValue, field: &str) -> Result<&'a [CValue], ContribError> {
    match v {
        CValue::Arr(a) => Ok(a),
        _ => Err(schema_err(field, "expected JSON array")),
    }
}

fn req_field<'a>(m: &'a BTreeMap<String, CValue>, key: &str) -> Result<&'a CValue, ContribError> {
    m.get(key).ok_or_else(|| schema_err(key, "missing required field"))
}

fn req_str<'a>(m: &'a BTreeMap<String, CValue>, key: &str) -> Result<&'a str, ContribError> {
    match req_field(m, key)? {
        CValue::Str(s) => Ok(s.as_str()),
        _ => Err(schema_err(key, "expected string")),
    }
}
