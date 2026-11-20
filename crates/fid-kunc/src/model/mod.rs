//! Uncertainty Model graph types, canonical form and digest. Spec node
//! `UNC.MODEL.GRAPH.SCHEMA`.
//!
//! An Uncertainty Model `um:<name>@<semver>` is a graph of input nodes (a quantity with a
//! [`Distribution`] and a source reference), operator nodes (a pinned kernel algorithm with
//! the digest of its full parameter set and declared ports), output nodes, edges between
//! `(node, port)` endpoints and correlation groups. This module owns the types, the
//! `uncertainty-model/v1` document schema, the canonical form and the SHA-256 digest.
//! Structural validation (dangling edges, cycles, manifest membership, arity, units on
//! edges, unsourced inputs, unreachable outputs, correlation groups) is
//! `UNC.MODEL.GRAPH.CHECK`, implemented by [`validate`] (submodule `check`); storage is
//! `UNC.MODEL.GRAPH.STORE`.
//!
//! [`UncertaintyModel`] values come from two places only, [`UncertaintyModel::new`] and
//! [`UncertaintyModel::parse`], which run the same checks. Every value has one canonical
//! byte form and one digest; any permutation of nodes, edges, correlation groups or the
//! inputs inside a group gives the same digest (REQ-UNC.MODEL.GRAPH.SCHEMA-002).
//!
//! # Document (`uncertainty-model/v1`)
//!
//! One JSON object; every scalar is a JSON string (kernel convention), unknown members
//! fail, absent optional members are omitted:
//!
//! ```text
//! { "schema": "uncertainty-model/v1", "name", "semver",
//!   "inputs":    [{ "id", "quantity_kind", "unit",
//!                   "distribution": { "kind", "params": { <name>: <f64> ... } },
//!                   "source"?: { "kind", "digest", "author" } }],
//!   "operators": [{ "id", "algorithm_id", "algorithm_version", "params": { ... },
//!                   "params_digest"?,
//!                   "ports_in":  [{ "name", "quantity_kind", "unit", "arity" }],
//!                   "ports_out": [{ "name", "quantity_kind", "unit" }] }],
//!   "outputs":   [{ "id", "quantity_kind", "unit", "characteristic"? }],
//!   "edges":     [{ "from": { "node", "port" }, "to": { "node", "port" } }],
//!   "correlations": [{ "inputs": [<input id> ...], "matrix": [[<f64> ...] ...] }] }
//! ```
//!
//! - `f64`: the IEEE-754 bit pattern as exactly 16 lowercase hex digits. NaN, infinities
//!   and negative zero are refused, so every number has one spelling.
//! - `unit`: `si:m`, `si:rad` or `si:1` ([`UnitCode`]). The distribution carries no unit
//!   of its own: location and scale parameters take the node's unit, `nu` and `beta` are
//!   `si:1`. Distribution kinds and parameters: `normal {mu, sigma}`, `rect {a, b}`,
//!   `tri {a, b, c}`, `arcsine {a, b}`, `scaled_t {mu, s, nu}`, `trap {a, b, beta}`, checked
//!   by the `UNC.MODEL.DIST` constructors.
//! - `arity`: the number of edges the port takes, decimal `1..=4294967295` without leading
//!   zeros. Checked against the edges by `UNC.MODEL.GRAPH.CHECK`.
//! - `source.kind`: `certificate`, `dcc`, `qualification`, `type_a_dataset`, `declared`.
//!   `source` may be absent; `UNC.MODEL.GRAPH.CHECK` reports such an input as
//!   `unsourced_input`.
//! - `params`: the operator's full parameter set, embedded so a model document is
//!   self-contained and replayable. Any object of objects, arrays and strings within the
//!   reader rules (printable ASCII without `"` or `\`). `params_digest` is derived from it:
//!   SHA-256 of its RFC 8785 bytes (REQ-UNC.MODEL.GRAPH.SCHEMA-003). A document may omit
//!   `params_digest`; when it supplies one, it must equal the recomputed digest
//!   (`schema_invalid{operators.params_digest}`). The canonical form always writes it.
//! - Ports of input and output nodes are implicit: an input node has exactly one output port
//!   and an output node exactly one input port, both named `value`. Edges name them like
//!   any other port; `UNC.MODEL.GRAPH.CHECK` resolves endpoints.
//!
//! # Text (untrusted)
//!
//! Every string is printable ASCII, so bidirectional controls, other control characters,
//! non-NFC text and look-alike code points cannot occur; the reader refuses any other byte
//! and every escape sequence. Patterns:
//!
//! - `name`: `[a-z0-9][a-z0-9-]{0,63}` (REQ-UNC.MODEL.GRAPH.SCHEMA-001);
//! - `semver`: `MAJOR.MINOR.PATCH`, each `0` or `1..=9` digits without a leading zero (the
//!   core-version subset of Semantic Versioning 2.0.0 that `UNC.UEC.STMT` [`ModelRef`]
//!   accepts);
//! - node IDs, port names and edge endpoints: `[a-z0-9][a-z0-9_.-]{0,63}`;
//! - `quantity_kind`: `[a-z][a-z0-9_]{0,63}`;
//! - `algorithm_id` (at most [`MAX_ID_BYTES`]) and `algorithm_version`: together a kernel
//!   algorithm ID `module.feature.method@MAJOR.MINOR.PATCH` (`fid_kci::AlgorithmId`);
//! - `characteristic`: `[A-Za-z0-9][A-Za-z0-9._:-]{0,127}`;
//! - `source.author`: 1..=256 bytes of `0x20..=0x7e` except `"` and `\`, without leading,
//!   trailing or doubled spaces.
//!
//! # Canonical form and digest
//!
//! Inputs, operators and outputs each sorted by `id`; edges by (target node, target port,
//! source node, source port); inside each correlation group the inputs sorted by ID with
//! the matrix rows and columns permuted to match; groups by their first (smallest) input
//! ID. Port lists keep their declared order: it is the operator's argument order. Byte
//! order equals RFC 8785's UTF-16 order on ASCII. The bytes are written by the kernel RFC
//! 8785 writer (`fid_kci::kci::json`, drift-guarded against `fid-canon` by
//! `crates/fid-canon/tests/kci_drift.rs`); the digest is SHA-256 over them.
//!
//! # Validation order (pinned, so the first reported error is deterministic)
//!
//! [`UncertaintyModel::parse`]: input size over [`MAX_INPUT_BYTES`] fails `too_large`; the
//! reader (syntax, value types, printable ASCII, escapes, duplicate member names, trailing
//! data) fails `schema_invalid{json.*}`, and nesting deeper than [`MAX_DEPTH`] or more than
//! [`MAX_VALUES`] values fails `too_large`; then the document is mapped member by member
//! in document order (missing, unknown or wrongly typed members, bad numbers, enums,
//! digests, distribution parameters and a `params_digest` that differs from its `params`
//! fail `schema_invalid{<path>}`; an unknown member reports the path of its object, the
//! root being `model`). These parse-stage errors depend on the document's order.
//!
//! Then, as for [`UncertaintyModel::new`], on parts sorted into canonical order:
//! `duplicate_node` naming the smallest repeated ID (one ID space for all three kinds); an
//! input repeated within or across correlation groups (`schema_invalid{correlations.inputs}`);
//! `name`, `semver`; every input, operator and output field in sorted order; edges, then a
//! repeated edge (`schema_invalid{edges}`); correlation groups (fewer than two inputs, a
//! matrix that is not `n x n`, a non-finite or negative-zero entry); last, the canonical
//! document must fit the reader's own bounds (`too_large`). With repeated IDs refused
//! first, every sort is total, so the first error reported by this stage does not depend on
//! the order of nodes, edges, groups or group inputs (port lists keep their declared order).

mod check;
pub(crate) mod reader;
mod wire;

pub use check::{validate, ModelError, ValidModel, VALUE_PORT};

use std::fmt;

use fid_kci::kci::json::{to_canonical, Json};
use fid_types::Digest;

use crate::dist::{Distribution, Params, UnitCode};
use crate::statement::ModelRef;

/// Schema tag written as the `schema` member of every model document.
pub const SCHEMA: &str = "uncertainty-model/v1";
/// Largest document [`UncertaintyModel::parse`] reads: 16 MiB.
pub const MAX_INPUT_BYTES: usize = 16 * 1024 * 1024;
/// Deepest nesting of arrays and objects in a document (the root object is depth 1).
pub const MAX_DEPTH: usize = 32;
/// Longest model name, node ID, port name and quantity kind, in bytes.
pub const MAX_ID_BYTES: usize = 64;
/// Longest characteristic ID, in bytes.
pub const MAX_CHAR_ID_BYTES: usize = 128;
/// Longest source author, in bytes.
pub const MAX_AUTHOR_BYTES: usize = 256;
/// Most JSON values (strings, arrays and objects; member names not counted) in a document:
/// `2^20`.
pub const MAX_VALUES: usize = 1 << 20;
/// Deepest nesting inside an operator's `params` (the object itself is depth 1): the
/// document's [`MAX_DEPTH`] less the three levels that enclose it.
pub const MAX_PARAMS_DEPTH: usize = MAX_DEPTH - 3;

/// Errors of this module (REQ-UNC.MODEL.GRAPH.SCHEMA-004).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaError {
    /// The document or parts break the schema at `field` (a fixed path such as
    /// `inputs.distribution.params.sigma`, or `json.<detail>` for the reader).
    SchemaInvalid { field: &'static str },
    /// The document is larger than [`MAX_INPUT_BYTES`], nested deeper than [`MAX_DEPTH`] or
    /// holds more than [`MAX_VALUES`] values.
    TooLarge,
    /// Two nodes share an ID (the smallest such ID).
    DuplicateNode { id: String },
}

impl SchemaError {
    /// Stable error code.
    pub const fn code(&self) -> &'static str {
        match self {
            SchemaError::SchemaInvalid { .. } => "schema_invalid",
            SchemaError::TooLarge => "too_large",
            SchemaError::DuplicateNode { .. } => "duplicate_node",
        }
    }

    /// The field of `schema_invalid`, if any.
    pub const fn field(&self) -> Option<&'static str> {
        match self {
            SchemaError::SchemaInvalid { field } => Some(field),
            SchemaError::TooLarge | SchemaError::DuplicateNode { .. } => None,
        }
    }
}

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SchemaError::SchemaInvalid { field } => write!(f, "schema_invalid: {field}"),
            SchemaError::TooLarge => f.write_str("too_large"),
            // The ID passed the node-ID pattern, so it is plain ASCII.
            SchemaError::DuplicateNode { id } => write!(f, "duplicate_node: {id}"),
        }
    }
}

impl std::error::Error for SchemaError {}

pub(crate) fn invalid(field: &'static str) -> SchemaError {
    SchemaError::SchemaInvalid { field }
}

macro_rules! wire_enum {
    ($(#[$m:meta])* $name:ident { $($(#[$vm:meta])* $v:ident => $w:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $name { $($(#[$vm])* $v),+ }

        impl $name {
            /// Every value, in declaration order.
            pub const ALL: &'static [$name] = &[$($name::$v),+];

            /// The wire string.
            pub const fn wire(self) -> &'static str {
                match self { $($name::$v => $w),+ }
            }

            /// Parses the exact wire string (case-sensitive).
            pub fn from_wire(s: &str) -> Option<$name> {
                match s { $($w => Some($name::$v),)+ _ => None }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.wire())
            }
        }
    };
}

wire_enum! {
    /// Kind of the record an input's value and uncertainty come from.
    InputSourceKind {
        /// Calibration certificate (not machine-readable).
        Certificate => "certificate",
        /// Digital calibration certificate.
        Dcc => "dcc",
        /// Qualification record.
        Qualification => "qualification",
        /// Type-A dataset.
        TypeADataset => "type_a_dataset",
        /// Value declared by a person (the author).
        Declared => "declared",
    }
}

/// Reference to the record an input's distribution comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRef {
    pub kind: InputSourceKind,
    /// Content digest of the record.
    pub digest: Digest,
    /// Who issued or declared it (untrusted text; see the module documentation).
    pub author: String,
}

/// An input quantity.
#[derive(Debug, Clone, PartialEq)]
pub struct InputNode {
    pub id: String,
    pub quantity_kind: String,
    pub unit: UnitCode,
    /// Must be in `unit` (location and scale parameters).
    pub distribution: Distribution,
    /// `None` is representable so that `UNC.MODEL.GRAPH.CHECK` can report `unsourced_input`.
    pub source: Option<SourceRef>,
}

/// An operator input port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InPort {
    pub name: String,
    pub quantity_kind: String,
    pub unit: UnitCode,
    /// Number of edges the port takes, at least 1.
    pub arity: u32,
}

/// An operator output port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutPort {
    pub name: String,
    pub quantity_kind: String,
    pub unit: UnitCode,
}

/// A kernel operator pinned by algorithm ID, version and parameter digest
/// (REQ-UNC.MODEL.GRAPH.SCHEMA-003).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorNode {
    pub id: String,
    /// `module.feature.method`.
    pub algorithm_id: String,
    /// `MAJOR.MINOR.PATCH`.
    pub algorithm_version: String,
    /// The operator's full parameter set: an object (see the module documentation). Its
    /// digest is derived, never supplied: [`Self::params_digest`].
    pub params: Json,
    /// In the operator's argument order.
    pub ports_in: Vec<InPort>,
    /// In the operator's result order.
    pub ports_out: Vec<OutPort>,
}

impl OperatorNode {
    /// SHA-256 of the RFC 8785 bytes of [`Self::params`] (REQ-UNC.MODEL.GRAPH.SCHEMA-003).
    /// Fails `schema_invalid{operators.params}` when `params` is not an object within the
    /// text and nesting rules; never fails on an operator of an [`UncertaintyModel`].
    pub fn params_digest(&self) -> Result<Digest, SchemaError> {
        check(params_ok(&self.params), "operators.params")?;
        let bytes = to_canonical(&self.params).map_err(|_| invalid("operators.params"))?;
        Ok(fid_kci::kci::sha256_digest(&bytes))
    }

    /// The kernel algorithm ID `algorithm_id@algorithm_version`, if both parts are valid.
    pub fn algorithm(&self) -> Option<fid_kci::AlgorithmId> {
        fid_kci::AlgorithmId::parse(&format!("{}@{}", self.algorithm_id, self.algorithm_version)).ok()
    }
}

/// An output quantity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputNode {
    pub id: String,
    pub quantity_kind: String,
    pub unit: UnitCode,
    /// The characteristic this output reports, if any.
    pub characteristic: Option<String>,
}

/// One end of an edge.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Endpoint {
    pub node: String,
    pub port: String,
}

/// A directed edge from a source `(node, port)` to a target `(node, port)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edge {
    pub from: Endpoint,
    pub to: Endpoint,
}

/// Inputs whose joint distribution has the given correlation matrix (`matrix[i][j]`
/// belongs to `inputs[i]` and `inputs[j]`). Positive definiteness and the unit diagonal are
/// checked by `UNC.MODEL.GRAPH.CHECK` through `UNC.MODEL.DIST`.
#[derive(Debug, Clone, PartialEq)]
pub struct CorrelationGroup {
    pub inputs: Vec<String>,
    pub matrix: Vec<Vec<f64>>,
}

/// The parts of a model, in any order.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelParts {
    pub name: String,
    pub semver: String,
    pub inputs: Vec<InputNode>,
    pub operators: Vec<OperatorNode>,
    pub outputs: Vec<OutputNode>,
    pub edges: Vec<Edge>,
    pub correlations: Vec<CorrelationGroup>,
}

/// A schema-valid Uncertainty Model in canonical order, with its canonical bytes and digest.
#[derive(Debug, Clone, PartialEq)]
pub struct UncertaintyModel {
    parts: ModelParts,
    canonical: Vec<u8>,
    digest: Digest,
}

impl UncertaintyModel {
    /// Puts `parts` into canonical order, checks them (see the module documentation for the
    /// order) and computes the canonical bytes and digest.
    pub fn new(mut parts: ModelParts) -> Result<UncertaintyModel, SchemaError> {
        canonicalize(&mut parts)?;
        let canonical = wire::to_canonical(&parts)?;
        // Every model in memory is also a document the reader accepts (size, depth, values).
        if canonical.len() > MAX_INPUT_BYTES {
            return Err(SchemaError::TooLarge);
        }
        reader::read(&canonical)?;
        let digest = fid_kci::kci::sha256_digest(&canonical);
        Ok(UncertaintyModel { parts, canonical, digest })
    }

    /// Parses an `uncertainty-model/v1` document (any member order, any node, edge and
    /// group order, insignificant whitespace allowed) with the bounds of
    /// REQ-UNC.MODEL.GRAPH.SCHEMA-004.
    pub fn parse(input: &[u8]) -> Result<UncertaintyModel, SchemaError> {
        if input.len() > MAX_INPUT_BYTES {
            return Err(SchemaError::TooLarge);
        }
        let json = reader::read(input)?;
        UncertaintyModel::new(wire::from_json(json)?)
    }

    /// The canonical RFC 8785 bytes.
    pub fn canonical_json(&self) -> &[u8] {
        &self.canonical
    }

    /// SHA-256 of [`Self::canonical_json`].
    pub fn digest(&self) -> Digest {
        self.digest
    }

    /// `um:<name>@<semver>`.
    pub fn id(&self) -> String {
        format!("um:{}@{}", self.parts.name, self.parts.semver)
    }

    /// The reference a result statement carries (`UNC.UEC.STMT`): ID `um:<name>`, semver
    /// and digest.
    pub fn model_ref(&self) -> ModelRef {
        ModelRef { id: format!("um:{}", self.parts.name), semver: self.parts.semver.clone(), digest: self.digest }
    }

    /// The parts, in canonical order.
    pub fn parts(&self) -> &ModelParts {
        &self.parts
    }

    pub fn name(&self) -> &str {
        &self.parts.name
    }

    pub fn semver(&self) -> &str {
        &self.parts.semver
    }

    /// Sorted by ID.
    pub fn inputs(&self) -> &[InputNode] {
        &self.parts.inputs
    }

    /// Sorted by ID.
    pub fn operators(&self) -> &[OperatorNode] {
        &self.parts.operators
    }

    /// Sorted by ID.
    pub fn outputs(&self) -> &[OutputNode] {
        &self.parts.outputs
    }

    /// Sorted by (target node, target port, source node, source port).
    pub fn edges(&self) -> &[Edge] {
        &self.parts.edges
    }

    /// Each sorted inside; groups sorted by first input ID.
    pub fn correlations(&self) -> &[CorrelationGroup] {
        &self.parts.correlations
    }
}

// ------------------------------------------------------------------ text rules

fn bytes_match(s: &str, max: usize, first: fn(u8) -> bool, rest: fn(u8) -> bool) -> bool {
    let b = s.as_bytes();
    match b.split_first() {
        Some((&c, tail)) => b.len() <= max && first(c) && tail.iter().all(|&x| rest(x)),
        None => false,
    }
}

fn lower_digit(c: u8) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit()
}

/// `[a-z0-9][a-z0-9-]{0,63}`.
pub fn is_model_name(s: &str) -> bool {
    bytes_match(s, MAX_ID_BYTES, lower_digit, |c| lower_digit(c) || c == b'-')
}

/// `MAJOR.MINOR.PATCH`, each 1..=9 digits without a leading zero.
pub fn is_semver(s: &str) -> bool {
    let mut n = 0;
    for part in s.split('.') {
        n += 1;
        let ok = !part.is_empty()
            && part.len() <= 9
            && part.bytes().all(|b| b.is_ascii_digit())
            && (part.len() == 1 || !part.starts_with('0'));
        if !ok {
            return false;
        }
    }
    n == 3
}

/// Node IDs and port names: `[a-z0-9][a-z0-9_.-]{0,63}`.
pub fn is_node_id(s: &str) -> bool {
    bytes_match(s, MAX_ID_BYTES, lower_digit, |c| lower_digit(c) || matches!(c, b'_' | b'.' | b'-'))
}

/// `[a-z][a-z0-9_]{0,63}`.
pub fn is_quantity_kind(s: &str) -> bool {
    bytes_match(s, MAX_ID_BYTES, |c| c.is_ascii_lowercase(), |c| lower_digit(c) || c == b'_')
}

/// `[A-Za-z0-9][A-Za-z0-9._:-]{0,127}`.
pub fn is_characteristic(s: &str) -> bool {
    bytes_match(
        s,
        MAX_CHAR_ID_BYTES,
        |c| c.is_ascii_alphanumeric(),
        |c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b':' | b'-'),
    )
}

/// 1..=256 bytes of `0x20..=0x7e` except `"` and `\`, no leading, trailing or doubled space.
pub fn is_author(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= MAX_AUTHOR_BYTES
        && b.iter().all(|&c| (0x20..=0x7e).contains(&c) && c != b'"' && c != b'\\')
        && b.first() != Some(&b' ')
        && b.last() != Some(&b' ')
        && !b.windows(2).any(|w| w == b"  ")
}

fn check(ok: bool, field: &'static str) -> Result<(), SchemaError> {
    if ok {
        Ok(())
    } else {
        Err(invalid(field))
    }
}

const NEG_ZERO: u64 = 0x8000_0000_0000_0000;

/// Finite and not negative zero: the numbers a model may hold.
pub(crate) fn is_model_number(x: f64) -> bool {
    x.is_finite() && x.to_bits() != NEG_ZERO
}

fn params_of(d: &Distribution) -> Vec<f64> {
    match d.params() {
        Params::Normal { mu, sigma } => vec![mu, sigma],
        Params::Rect { a, b } | Params::Arcsine { a, b } => vec![a, b],
        Params::Tri { a, b, c } => vec![a, b, c],
        Params::ScaledT { mu, s, nu } => vec![mu, s, nu],
        Params::Trap { a, b, beta } => vec![a, b, beta],
    }
}

// ------------------------------------------------------------------ canonical order + checks

fn by_bytes(a: &str, b: &str) -> std::cmp::Ordering {
    a.as_bytes().cmp(b.as_bytes())
}

fn edge_key(e: &Edge) -> [&[u8]; 4] {
    [e.to.node.as_bytes(), e.to.port.as_bytes(), e.from.node.as_bytes(), e.from.port.as_bytes()]
}

fn canonicalize(p: &mut ModelParts) -> Result<(), SchemaError> {
    p.inputs.sort_by(|a, b| by_bytes(&a.id, &b.id));
    p.operators.sort_by(|a, b| by_bytes(&a.id, &b.id));
    p.outputs.sort_by(|a, b| by_bytes(&a.id, &b.id));
    p.edges.sort_by(|a, b| edge_key(a).cmp(&edge_key(b)));

    // Repeated IDs first: once they are refused, every sort above and below is total, so the
    // checks that follow see one order whatever the input order was.
    let mut ids: Vec<&str> = p.inputs.iter().map(|n| n.id.as_str()).collect();
    ids.extend(p.operators.iter().map(|n| n.id.as_str()));
    ids.extend(p.outputs.iter().map(|n| n.id.as_str()));
    ids.sort_by(|a, b| by_bytes(a, b));
    if let Some(w) = ids.windows(2).find(|w| w[0] == w[1]) {
        return Err(SchemaError::DuplicateNode { id: w[0].to_owned() });
    }
    check(unique(p.correlations.iter().flat_map(|g| g.inputs.iter().map(String::as_str))), "correlations.inputs")?;
    // Sorting by the smallest input is the canonical group order (each group's first input
    // after its own sort); no input is shared, so the order is total.
    p.correlations.sort_by(|a, b| {
        let first = |g: &CorrelationGroup| g.inputs.iter().map(|s| s.as_bytes()).min().unwrap_or_default().to_vec();
        first(a).cmp(&first(b))
    });

    check(is_model_name(&p.name), "name")?;
    check(is_semver(&p.semver), "semver")?;
    for n in &p.inputs {
        check(is_node_id(&n.id), "inputs.id")?;
        check(is_quantity_kind(&n.quantity_kind), "inputs.quantity_kind")?;
        check(n.distribution.unit() == n.unit, "inputs.unit")?;
        check(params_of(&n.distribution).into_iter().all(is_model_number), "inputs.distribution.params")?;
        if let Some(s) = &n.source {
            check(is_author(&s.author), "inputs.source.author")?;
        }
    }
    for n in &mut p.operators {
        check(is_node_id(&n.id), "operators.id")?;
        check(n.algorithm_id.len() <= MAX_ID_BYTES, "operators.algorithm_id")?;
        check(n.algorithm().is_some(), "operators.algorithm")?;
        check(params_ok(&n.params), "operators.params")?;
        // Canonical member order inside params, so equal models compare equal in memory.
        check(sort_members(&mut n.params), "operators.params")?;
        for port in &n.ports_in {
            check(is_node_id(&port.name), "operators.ports_in.name")?;
            check(is_quantity_kind(&port.quantity_kind), "operators.ports_in.quantity_kind")?;
            check(port.arity >= 1, "operators.ports_in.arity")?;
        }
        check(unique(n.ports_in.iter().map(|x| x.name.as_str())), "operators.ports_in")?;
        for port in &n.ports_out {
            check(is_node_id(&port.name), "operators.ports_out.name")?;
            check(is_quantity_kind(&port.quantity_kind), "operators.ports_out.quantity_kind")?;
        }
        check(unique(n.ports_out.iter().map(|x| x.name.as_str())), "operators.ports_out")?;
    }
    for n in &p.outputs {
        check(is_node_id(&n.id), "outputs.id")?;
        check(is_quantity_kind(&n.quantity_kind), "outputs.quantity_kind")?;
        if let Some(c) = &n.characteristic {
            check(is_characteristic(c), "outputs.characteristic")?;
        }
    }

    for e in &p.edges {
        check(is_node_id(&e.from.node), "edges.from.node")?;
        check(is_node_id(&e.from.port), "edges.from.port")?;
        check(is_node_id(&e.to.node), "edges.to.node")?;
        check(is_node_id(&e.to.port), "edges.to.port")?;
    }
    check(!p.edges.windows(2).any(|w| w[0] == w[1]), "edges")?;

    for g in &mut p.correlations {
        canonical_group(g)?;
    }
    Ok(())
}

/// Text inside `params`: any length (including empty) of `0x20..=0x7e` except `"` and `\`,
/// the strings the reader accepts.
fn is_params_text(s: &str) -> bool {
    s.bytes().all(|c| (0x20..=0x7e).contains(&c) && c != b'"' && c != b'\\')
}

/// `params` is an object, nested at most [`MAX_PARAMS_DEPTH`] deep, whose member names and
/// strings are params text. Member names are unique (checked by the canonical writer).
pub(crate) fn params_ok(j: &Json) -> bool {
    matches!(j, Json::Obj(_)) && params_value_ok(j, 1)
}

/// Sorts object members by name (byte order equals UTF-16 order on params text) at every
/// level; false on a repeated member name. Only called after [`params_ok`], which bounds the
/// depth.
fn sort_members(j: &mut Json) -> bool {
    match j {
        Json::Str(_) => true,
        Json::Arr(items) => items.iter_mut().all(sort_members),
        Json::Obj(members) => {
            members.sort_by(|a, b| by_bytes(&a.0, &b.0));
            !members.windows(2).any(|w| w[0].0 == w[1].0) && members.iter_mut().all(|(_, x)| sort_members(x))
        }
    }
}

fn params_value_ok(j: &Json, depth: usize) -> bool {
    match j {
        Json::Str(s) => is_params_text(s),
        _ if depth > MAX_PARAMS_DEPTH => false,
        Json::Arr(items) => items.iter().all(|x| params_value_ok(x, depth + 1)),
        Json::Obj(members) => members.iter().all(|(k, x)| is_params_text(k) && params_value_ok(x, depth + 1)),
    }
}

fn unique<'a>(names: impl Iterator<Item = &'a str>) -> bool {
    let mut v: Vec<&str> = names.collect();
    v.sort_by(|a, b| by_bytes(a, b));
    !v.windows(2).any(|w| w[0] == w[1])
}

/// Checks a group and sorts its inputs, permuting the matrix to match.
fn canonical_group(g: &mut CorrelationGroup) -> Result<(), SchemaError> {
    let n = g.inputs.len();
    check(n >= 2, "correlations.inputs")?;
    for id in &g.inputs {
        check(is_node_id(id), "correlations.inputs")?;
    }
    check(g.matrix.len() == n && g.matrix.iter().all(|r| r.len() == n), "correlations.matrix")?;
    check(g.matrix.iter().flatten().all(|&x| is_model_number(x)), "correlations.matrix")?;
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| by_bytes(&g.inputs[a], &g.inputs[b]));
    let inputs: Vec<String> = order.iter().map(|&i| g.inputs[i].clone()).collect();
    check(!inputs.windows(2).any(|w| w[0] == w[1]), "correlations.inputs")?;
    let matrix: Vec<Vec<f64>> = order.iter().map(|&i| order.iter().map(|&j| g.matrix[i][j]).collect()).collect();
    g.inputs = inputs;
    g.matrix = matrix;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn text_patterns() {
        assert!(is_model_name("bore-ls") && is_model_name("0") && !is_model_name("Bore") && !is_model_name("-a"));
        assert!(is_model_name(&"a".repeat(64)) && !is_model_name(&"a".repeat(65)) && !is_model_name(""));
        assert!(is_semver("1.0.0") && is_semver("0.10.3") && is_semver("999999999.0.0"));
        for bad in ["01.0.0", "1.0", "1.0.0-rc.1", "1.0.0+b", "1.0.0.0", "1..0", "1000000000.0.0", ""] {
            assert!(!is_semver(bad), "{bad}");
        }
        assert!(is_node_id("x_ring.r-1") && !is_node_id("_x") && !is_node_id("X") && !is_node_id("a b"));
        assert!(is_quantity_kind("length") && !is_quantity_kind("1length") && !is_quantity_kind("len-gth"));
        assert!(is_characteristic("DIM-1:a.b_c") && !is_characteristic(".a") && !is_characteristic("a b"));
        assert!(is_author("J. Smith, Lab 4") && !is_author(" J") && !is_author("J ") && !is_author("J  S"));
        assert!(!is_author("a\"b") && !is_author("a\\b") && !is_author("a\tb") && !is_author("\u{e9}"));
        assert!(!is_author("a\u{202e}b") && !is_author(""));
    }

    #[test]
    fn numbers_have_one_spelling() {
        assert!(is_model_number(0.0) && is_model_number(-1.0));
        assert!(!is_model_number(-0.0) && !is_model_number(f64::NAN) && !is_model_number(f64::INFINITY));
    }

    #[test]
    fn group_matrix_follows_its_inputs() {
        let mut g = CorrelationGroup {
            inputs: vec!["c".into(), "a".into(), "b".into()],
            matrix: vec![vec![1.0, 0.2, 0.3], vec![0.2, 1.0, 0.1], vec![0.3, 0.1, 1.0]],
        };
        canonical_group(&mut g).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(g.inputs, ["a", "b", "c"]);
        assert_eq!(g.matrix, vec![vec![1.0, 0.1, 0.2], vec![0.1, 1.0, 0.3], vec![0.2, 0.3, 1.0]]);
    }
}
