//! Operator tangent contract, tangent table and coverage report. Spec node
//! `UNC.PROP.LPU.TAN`.
//!
//! LPU obtains the derivatives of kernel operators through this contract only. Each operator
//! of an Uncertainty Model is pinned by its exact algorithm ID and semver
//! (`module.feature.method@MAJOR.MINOR.PATCH`, [`AlgorithmId`]); a provider
//! ([`OperatorTangent`]) registered for exactly that ID returns the operator's Jacobian with
//! respect to its numeric inputs at the evaluation point. A missing provider is
//! `no_derivative{operator}`; there is no fallback to a nearby version and no numerical
//! differentiation (REQ-UNC.PROP.LPU.TAN-001, -003).
//!
//! # Jacobian layout (pinned)
//!
//! Rows are the operator's scalar output components in canonical output order, columns its
//! scalar input components in canonical input order. For an operator node of an
//! Uncertainty Model ([`TangentTable::operator_jacobian`]) that is: one row per output port
//! in declared `ports_out` order, and one column per incoming edge, ports in declared
//! `ports_in` order and, inside a port of arity `k`, its `k` edges in the model's canonical
//! edge order (source node, then source port), the order in which `UNC.PROP.MCM.BLOCK`
//! packs the port's input blob; [`input_sources`] lists them. Every port component is one
//! scalar (the model's units are scalar units).
//!
//! # Checks (pinned order)
//!
//! [`TangentTable::jacobian`]: exact lookup (`no_derivative`); the call shape, 1 to
//! [`MAX_DIM`] rows and columns (`tangent_bad_call`); every input finite
//! (`tangent_bad_input{non_finite}`, KNP-1: NaN is always an error); the provider call
//! (its error is passed through typed as `tangent_provider_error`); the returned Jacobian
//! shape (`tangent_shape_mismatch`, REQ-002); then every kink hint (`tangent_bad_kink`).
//! [`TangentTable::operator_jacobian`] first resolves the node's algorithm ID
//! (`tangent_bad_call{algorithm_id}`), looks it up, derives the shape from the ports
//! (`tangent_bad_call{ports_in|ports_out|too_large}`), checks the input count and each
//! input's unit against its port (`tangent_bad_input{count|unit}`) and passes the RFC 8785
//! bytes of the node's `params` (the bytes `params_digest` is taken over) to the provider.
//!
//! # Coverage report (`tangent-coverage@1`, REQ-004)
//!
//! [`coverage_report`] is the sorted set difference between the kernel manifest's
//! algorithms and the table's providers, both in the [`AlgorithmId`] total order (module,
//! feature, method, then numeric version). Its canonical JSON is
//! `{"manifest":"sha256:...","missing":[<id>...],"orphan":[<id>...],"version":"tangent-coverage@1"}`
//! written by the kernel RFC 8785 writer; `manifest` is the manifest digest
//! (`kernel_manifest_digest`) the report was computed against.
//!
//! Determinism D0: no floating-point arithmetic happens here; values are passed through.

use std::collections::BTreeMap;
use std::fmt;

use fid_kci::kci::json::{to_canonical, Json, JsonError};
use fid_kci::{AlgorithmId, KernelManifest};
use fid_num::la::{Mat, MAX_DIM};
use fid_types::Digest;

use crate::dist::UnitCode;
use crate::model::{Endpoint, OperatorNode, UncertaintyModel};

/// Version tag of the coverage report artifact.
pub const COVERAGE_VERSION: &str = "tangent-coverage@1";
/// Most kink hints one provider call may return.
pub const MAX_KINKS: usize = 4096;
/// Longest operator name kept in an error built from caller-supplied text, in bytes.
const MAX_NAME_BYTES: usize = 160;

/// One scalar input component at the evaluation point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScalarView {
    /// The value, in `unit`.
    pub value: f64,
    /// The unit of the value (the unit of the port it feeds).
    pub unit: UnitCode,
}

/// A hint from a non-smooth provider: the operator has a kink in input component
/// `input_component`, `margin` (in that component's unit, finite and not negative) away
/// from the evaluation point. The Jacobian is the one-sided derivative on the evaluation
/// point's side.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Kink {
    /// Column index of the input component.
    pub input_component: usize,
    /// Distance from the evaluation point to the kink.
    pub margin: f64,
}

/// What a provider returns: the dense Jacobian and optional kink hints.
#[derive(Debug, Clone, PartialEq)]
pub struct TangentOut {
    /// Rows = output components, columns = input components (see the module documentation).
    pub jac: Mat,
    /// Kink hints of non-smooth providers; empty for smooth ones.
    pub kinks: Vec<Kink>,
}

/// A provider's own failure (for example parameters it cannot read). Passed through typed in
/// [`TanError::Provider`]; its code never replaces the table's own codes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderError {
    /// The provider's stable code.
    pub code: &'static str,
    /// Human-readable detail.
    pub detail: String,
}

impl ProviderError {
    /// A provider error with a code and a detail.
    pub fn new(code: &'static str, detail: &str) -> ProviderError {
        ProviderError { code, detail: detail.to_string() }
    }
}

/// A point at which a provider's Jacobian is checked against the test-only central-difference
/// harness (REQ-UNC.PROP.LPU.TAN-005): the operator's parameter bytes (RFC 8785) and inputs.
#[derive(Debug, Clone, PartialEq)]
pub struct SamplePoint {
    /// RFC 8785 bytes of the operator's parameter set.
    pub params: Vec<u8>,
    /// The input components, in canonical input order.
    pub inputs: Vec<ScalarView>,
}

/// The tangent of one kernel operator, registered for exactly one algorithm ID and semver.
///
/// The contract has no way to evaluate the operator itself: a provider returns derivatives,
/// never values, so the engine has nothing to perturb (REQ-UNC.PROP.LPU.TAN-003).
pub trait OperatorTangent {
    /// The exact algorithm ID with semver this provider differentiates.
    fn alg_id(&self) -> &AlgorithmId;

    /// The Jacobian at `inputs` for the parameter set `params` (RFC 8785 bytes). Rows are the
    /// output components in canonical output order, columns the input components in
    /// canonical input order.
    fn jacobian(&self, params: &[u8], inputs: &[ScalarView]) -> Result<TangentOut, ProviderError>;

    /// The points at which the test harness checks this provider. Every provider registers
    /// at least one (REQ-UNC.PROP.LPU.TAN-005).
    fn sample_points(&self) -> Vec<SamplePoint>;
}

/// Errors of the tangent table, each with a stable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TanError {
    /// No provider is registered for exactly this `alg_id@semver` (REQ-001).
    NoDerivative { operator: String },
    /// The provider's Jacobian has the wrong shape for the call (REQ-002). Shapes are
    /// (rows, columns).
    TangentShapeMismatch { operator: String, expected: (usize, usize), actual: (usize, usize) },
    /// The call cannot be differentiated as given: `reason` is `algorithm_id`, `ports_in`,
    /// `ports_out`, `too_large` or `params`.
    BadCall { operator: String, reason: &'static str },
    /// An input is unusable: `reason` is `count`, `unit` or `non_finite`; `index` is the
    /// input component (the count found, for `count`).
    BadInput { operator: String, reason: &'static str, index: usize },
    /// A kink hint names a column outside the Jacobian, has a negative or non-finite margin,
    /// or the provider returned more than [`MAX_KINKS`] hints; `index` is the hint's
    /// position.
    BadKink { operator: String, index: usize },
    /// A second provider for the same algorithm ID.
    DuplicateProvider { operator: String },
    /// The provider itself failed.
    Provider { operator: String, error: ProviderError },
}

impl TanError {
    /// Stable code.
    pub fn code(&self) -> &'static str {
        match self {
            TanError::NoDerivative { .. } => "no_derivative",
            TanError::TangentShapeMismatch { .. } => "tangent_shape_mismatch",
            TanError::BadCall { .. } => "tangent_bad_call",
            TanError::BadInput { .. } => "tangent_bad_input",
            TanError::BadKink { .. } => "tangent_bad_kink",
            TanError::DuplicateProvider { .. } => "tangent_duplicate_provider",
            TanError::Provider { .. } => "tangent_provider_error",
        }
    }

    /// The operator the error names.
    pub fn operator(&self) -> &str {
        match self {
            TanError::NoDerivative { operator }
            | TanError::TangentShapeMismatch { operator, .. }
            | TanError::BadCall { operator, .. }
            | TanError::BadInput { operator, .. }
            | TanError::BadKink { operator, .. }
            | TanError::DuplicateProvider { operator }
            | TanError::Provider { operator, .. } => operator,
        }
    }
}

impl fmt::Display for TanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = self.code();
        match self {
            TanError::NoDerivative { operator } | TanError::DuplicateProvider { operator } => {
                write!(f, "{code}: {operator}")
            }
            TanError::TangentShapeMismatch { operator, expected, actual } => {
                write!(f, "{code}: {operator}: expected {}x{}, got {}x{}", expected.0, expected.1, actual.0, actual.1)
            }
            TanError::BadCall { operator, reason } => write!(f, "{code}: {operator}: {reason}"),
            TanError::BadInput { operator, reason, index } => write!(f, "{code}: {operator}: {reason} at {index}"),
            TanError::BadKink { operator, index } => write!(f, "{code}: {operator}: kink {index}"),
            TanError::Provider { operator, error } => {
                write!(f, "{code}: {operator}: {}: {}", error.code, sanitize(&error.detail))
            }
        }
    }
}

impl std::error::Error for TanError {}

/// Caller-supplied text in an error: printable ASCII only, at most [`MAX_NAME_BYTES`].
fn sanitize(s: &str) -> String {
    s.chars().filter(|c| (' '..='~').contains(c)).take(MAX_NAME_BYTES).collect()
}

/// The tangent providers, keyed by exact algorithm ID with semver (an ordered map, KNP-1).
#[derive(Default)]
pub struct TangentTable {
    providers: BTreeMap<AlgorithmId, Box<dyn OperatorTangent>>,
}

impl fmt::Debug for TangentTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.providers.keys().map(|k| k.to_string())).finish()
    }
}

impl TangentTable {
    /// An empty table.
    pub fn new() -> TangentTable {
        TangentTable::default()
    }

    /// Registers a provider under its [`OperatorTangent::alg_id`]. An ID that does not
    /// round-trip through [`AlgorithmId::parse`] is `tangent_bad_call{algorithm_id}`; a second
    /// provider for the same ID is `tangent_duplicate_provider`, and the first one stays.
    pub fn register(&mut self, provider: Box<dyn OperatorTangent>) -> Result<(), TanError> {
        let id = provider.alg_id().clone();
        // `AlgorithmId` has public fields, so a provider can hold an ID that no card or
        // manifest could spell. Only an ID that round-trips through its own parser is keyed.
        if AlgorithmId::parse(&id.to_string()).ok().as_ref() != Some(&id) {
            return Err(TanError::BadCall { operator: sanitize(&id.to_string()), reason: "algorithm_id" });
        }
        if self.providers.contains_key(&id) {
            return Err(TanError::DuplicateProvider { operator: id.to_string() });
        }
        self.providers.insert(id, provider);
        Ok(())
    }

    /// Registered algorithm IDs, in [`AlgorithmId`] order.
    pub fn ids(&self) -> Vec<AlgorithmId> {
        self.providers.keys().cloned().collect()
    }

    /// Number of providers.
    pub fn len(&self) -> usize {
        self.providers.len()
    }

    /// True when no provider is registered.
    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }

    /// The provider registered for exactly `id` (module, feature, method and all three
    /// version numbers), else `no_derivative{operator}` (REQ-001).
    pub fn lookup(&self, id: &AlgorithmId) -> Result<&dyn OperatorTangent, TanError> {
        match self.providers.get(id) {
            Some(p) => Ok(p.as_ref()),
            None => Err(TanError::NoDerivative { operator: id.to_string() }),
        }
    }

    /// The Jacobian of operator `id` with `outputs` output components at `inputs`, checked
    /// in the order of the module documentation.
    pub fn jacobian(
        &self,
        id: &AlgorithmId,
        params: &[u8],
        inputs: &[ScalarView],
        outputs: usize,
    ) -> Result<TangentOut, TanError> {
        let provider = self.lookup(id)?;
        let operator = id.to_string();
        let cols = inputs.len();
        if outputs == 0 || outputs > MAX_DIM {
            return Err(TanError::BadCall { operator, reason: "ports_out" });
        }
        if cols == 0 || cols > MAX_DIM {
            return Err(TanError::BadCall { operator, reason: "ports_in" });
        }
        if let Some(index) = inputs.iter().position(|s| !s.value.is_finite()) {
            return Err(TanError::BadInput { operator, reason: "non_finite", index });
        }
        let out = match provider.jacobian(params, inputs) {
            Ok(out) => out,
            Err(error) => return Err(TanError::Provider { operator, error }),
        };
        let actual = (out.jac.rows(), out.jac.cols());
        if actual != (outputs, cols) {
            return Err(TanError::TangentShapeMismatch { operator, expected: (outputs, cols), actual });
        }
        if out.kinks.len() > MAX_KINKS {
            return Err(TanError::BadKink { operator, index: MAX_KINKS });
        }
        let bad_kink = |k: &Kink| k.input_component >= cols || !k.margin.is_finite() || k.margin < 0.0;
        if let Some(index) = out.kinks.iter().position(bad_kink) {
            return Err(TanError::BadKink { operator, index });
        }
        Ok(out)
    }

    /// The Jacobian of an Uncertainty Model operator node at `inputs` (one component per
    /// incoming edge, in canonical input order). See the module documentation for the layout
    /// and the check order.
    pub fn operator_jacobian(&self, op: &OperatorNode, inputs: &[ScalarView]) -> Result<TangentOut, TanError> {
        let id = match op.algorithm() {
            Some(id) => id,
            None => {
                let operator = sanitize(&format!("{}@{}", op.algorithm_id, op.algorithm_version));
                return Err(TanError::BadCall { operator, reason: "algorithm_id" });
            }
        };
        self.lookup(&id)?;
        let operator = id.to_string();
        if op.ports_out.is_empty() {
            return Err(TanError::BadCall { operator, reason: "ports_out" });
        }
        if op.ports_out.len() > MAX_DIM {
            return Err(TanError::BadCall { operator, reason: "too_large" });
        }
        // Column count: the sum of the input ports' arities, bounded before it can overflow.
        let mut cols: usize = 0;
        for port in &op.ports_in {
            let arity = usize::try_from(port.arity).unwrap_or(usize::MAX);
            cols = match cols.checked_add(arity) {
                Some(c) if c <= MAX_DIM => c,
                _ => return Err(TanError::BadCall { operator, reason: "too_large" }),
            };
        }
        if cols == 0 {
            return Err(TanError::BadCall { operator, reason: "ports_in" });
        }
        if inputs.len() != cols {
            return Err(TanError::BadInput { operator, reason: "count", index: inputs.len() });
        }
        let mut index = 0;
        for port in &op.ports_in {
            for _ in 0..port.arity {
                match inputs.get(index) {
                    Some(s) if s.unit == port.unit => {}
                    Some(_) => return Err(TanError::BadInput { operator, reason: "unit", index }),
                    None => return Err(TanError::BadInput { operator, reason: "count", index: inputs.len() }),
                }
                index += 1;
            }
        }
        let params = match to_canonical(&op.params) {
            Ok(bytes) => bytes,
            Err(_) => return Err(TanError::BadCall { operator, reason: "params" }),
        };
        self.jacobian(&id, &params, inputs, op.ports_out.len())
    }
}

/// The source endpoint of each Jacobian column of operator node `op` in `model`: input
/// ports in declared order and, inside a port, its incoming edges in the model's canonical
/// edge order (source node, then source port). This is the order in which
/// `UNC.PROP.MCM.BLOCK` packs a port's values into its input blob, so column `j` of
/// [`TangentTable::operator_jacobian`] is the sensitivity to the value on edge `j` of this
/// list. Ports with fewer edges than their arity are a `UNC.MODEL.GRAPH.CHECK` error; this
/// function lists the edges that exist.
pub fn input_sources<'m>(model: &'m UncertaintyModel, op: &OperatorNode) -> Vec<&'m Endpoint> {
    let mut out = Vec::new();
    for port in &op.ports_in {
        for e in model.edges().iter().filter(|e| e.to.node == op.id && e.to.port == port.name) {
            out.push(&e.from);
        }
    }
    out
}

/// The `tangent-coverage@1` report (REQ-004).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageReport {
    /// Digest of the kernel manifest the report was computed against.
    pub manifest: Digest,
    /// Manifest operators with no registered provider, in [`AlgorithmId`] order.
    pub missing: Vec<AlgorithmId>,
    /// Providers whose operator the manifest does not list, in [`AlgorithmId`] order.
    pub orphan: Vec<AlgorithmId>,
}

impl CoverageReport {
    /// True when every manifest operator has a provider and no provider is an orphan.
    pub fn is_complete(&self) -> bool {
        self.missing.is_empty() && self.orphan.is_empty()
    }

    /// RFC 8785 canonical JSON of the report (see the module documentation).
    pub fn canonical_json(&self) -> Result<Vec<u8>, JsonError> {
        let ids = |v: &[AlgorithmId]| Json::Arr(v.iter().map(|i| Json::Str(i.to_string())).collect());
        to_canonical(&Json::Obj(vec![
            ("manifest".to_string(), Json::Str(self.manifest.as_str().to_string())),
            ("missing".to_string(), ids(&self.missing)),
            ("orphan".to_string(), ids(&self.orphan)),
            ("version".to_string(), Json::Str(COVERAGE_VERSION.to_string())),
        ]))
    }
}

/// Lists, in algorithm-ID order, every manifest operator without a provider and every
/// provider whose operator is absent from the manifest (REQ-004). Both lists are already
/// strictly sorted (the manifest by its own validation, the table by its ordered map), so
/// this is one merge pass.
pub fn coverage_report(manifest: &KernelManifest, table: &TangentTable) -> CoverageReport {
    let listed: Vec<&AlgorithmId> = manifest.core().algorithms.iter().map(|a| &a.id).collect();
    let have: Vec<&AlgorithmId> = table.providers.keys().collect();
    let (mut missing, mut orphan) = (Vec::new(), Vec::new());
    let (mut i, mut j) = (0, 0);
    while i < listed.len() || j < have.len() {
        match (listed.get(i), have.get(j)) {
            (Some(a), Some(b)) => match a.cmp(b) {
                std::cmp::Ordering::Less => {
                    missing.push((*a).clone());
                    i += 1;
                }
                std::cmp::Ordering::Greater => {
                    orphan.push((*b).clone());
                    j += 1;
                }
                std::cmp::Ordering::Equal => {
                    i += 1;
                    j += 1;
                }
            },
            (Some(a), None) => {
                missing.push((*a).clone());
                i += 1;
            }
            (None, Some(b)) => {
                orphan.push((*b).clone());
                j += 1;
            }
            (None, None) => break,
        }
    }
    CoverageReport { manifest: manifest.manifest_digest(), missing, orphan }
}
