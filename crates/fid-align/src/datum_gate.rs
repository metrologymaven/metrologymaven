//! Dialect gate and per-datum interpretation resolver calls (spec node `ALN.DATUM.SYS.GATE`).
//!
//! [`resolve_datum_chain`] is the pure `R1` / `D0` gate that refuses a datum-system alignment
//! unless dialect, standard edition and defaults profile are all explicitly stated, then calls
//! `fid-gps`'s pure interpretation resolver (`GDT.RES.RESOLVE`) once per datum feature in
//! precedence order (primary, secondary, tertiary).
//!
//! # Guarantees
//! - **Explicit context (REQ-ALN.DATUM.SYS.GATE-001):** absent or empty `dialect`, `edition`
//!   or `defaults_profile` fails with [`GateError::DialectUnspecified`] listing every missing
//!   field; no default is ever substituted.
//! - **P1a modifier scope (REQ-ALN.DATUM.SYS.GATE-002):** only the `rmb` modifier is permitted
//!   in P1a; any other modifier (such as `mmb` or `lmb`) fails with
//!   [`GateError::UnsupportedModifierP1a`].
//! - **No built-in interpretation (REQ-ALN.DATUM.SYS.GATE-003):** `UNRESOLVED` and
//!   `UNSUPPORTED` outcomes from `fid_gps::resolve::resolve` stop the gate immediately and
//!   pass through with the datum label and DMT key.
//! - **Combined chain digest (REQ-ALN.DATUM.SYS.GATE-004):** `combined_digest` is SHA-256
//!   over the RFC 8785 canonical JSON array of the ordered per-datum `chain_digest` strings.
//! - **Pure D0 (REQ-ALN.DATUM.SYS.GATE-005):** no I/O, no floating-point arithmetic, no
//!   `HashMap`/`HashSet`, no panics.
//!
//! Standards are cited by designator only (e.g. ASME Y14.5-2018, ISO 5459:2011); all
//! operator chains come strictly from the loaded [`DmtTable`].

use fid_canon::{digest_bytes, to_canonical_bytes, CValue};
use fid_gps::dmt::{Dialect, DmtKey, DmtTable, OperatorChain};
use fid_gps::resolve::{resolve, Resolution, ResolveError, ResolveInput, VariantSource};
use fid_types::Digest;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Maximum number of datum features in a datum-system alignment (primary, secondary, tertiary).
pub const MAX_DATUMS: usize = 3;
/// Maximum datum label length in bytes.
pub const MAX_DATUM_LABEL_LEN: usize = 32;

/// Precedence slot of a datum feature in a datum-system alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DatumPrecedence {
    Primary,
    Secondary,
    Tertiary,
}

impl DatumPrecedence {
    /// Wire / DMT characteristic suffix (`"primary"`, `"secondary"`, `"tertiary"`).
    pub fn as_str(self) -> &'static str {
        match self {
            DatumPrecedence::Primary => "primary",
            DatumPrecedence::Secondary => "secondary",
            DatumPrecedence::Tertiary => "tertiary",
        }
    }

    fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(DatumPrecedence::Primary),
            1 => Some(DatumPrecedence::Secondary),
            2 => Some(DatumPrecedence::Tertiary),
            _ => None,
        }
    }
}

impl fmt::Display for DatumPrecedence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Datum feature geometry type used to derive the DMT lookup characteristic name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DatumFeatureType {
    Plane,
    Cylinder,
    Width,
    Cone,
    Sphere,
    Point,
    Line,
}

impl DatumFeatureType {
    /// Wire / DMT segment name.
    pub fn as_str(self) -> &'static str {
        match self {
            DatumFeatureType::Plane => "plane",
            DatumFeatureType::Cylinder => "cylinder",
            DatumFeatureType::Width => "width",
            DatumFeatureType::Cone => "cone",
            DatumFeatureType::Sphere => "sphere",
            DatumFeatureType::Point => "point",
            DatumFeatureType::Line => "line",
        }
    }

    /// Parses a lowercase datum feature type name.
    pub fn parse(s: &str) -> Result<Self, GateError> {
        match s {
            "plane" => Ok(DatumFeatureType::Plane),
            "cylinder" => Ok(DatumFeatureType::Cylinder),
            "width" => Ok(DatumFeatureType::Width),
            "cone" => Ok(DatumFeatureType::Cone),
            "sphere" => Ok(DatumFeatureType::Sphere),
            "point" => Ok(DatumFeatureType::Point),
            "line" => Ok(DatumFeatureType::Line),
            _ => Err(GateError::BadInput { reason: "unknown_datum_feature_type" }),
        }
    }
}

impl fmt::Display for DatumFeatureType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Material boundary / datum modifier attached to a datum feature reference.
///
/// In P1a only [`DatumModifier::Rmb`] is supported; any other modifier fails the gate with
/// [`GateError::UnsupportedModifierP1a`] (REQ-ALN.DATUM.SYS.GATE-002).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DatumModifier {
    Rmb,
    Mmb,
    Lmb,
    Other(String),
}

impl DatumModifier {
    /// Wire name (`"rmb"`, `"mmb"`, `"lmb"`, or custom string).
    pub fn as_str(&self) -> &str {
        match self {
            DatumModifier::Rmb => "rmb",
            DatumModifier::Mmb => "mmb",
            DatumModifier::Lmb => "lmb",
            DatumModifier::Other(s) => s.as_str(),
        }
    }

    /// Parses a modifier wire token (case-sensitive lowercase).
    pub fn parse(s: &str) -> Result<Self, GateError> {
        if s.is_empty() {
            return Err(GateError::BadInput { reason: "empty_modifier" });
        }
        Ok(match s {
            "rmb" => DatumModifier::Rmb,
            "mmb" => DatumModifier::Mmb,
            "lmb" => DatumModifier::Lmb,
            other => DatumModifier::Other(other.to_string()),
        })
    }
}

impl fmt::Display for DatumModifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One datum feature reference passed to [`resolve_datum_chain`] in precedence order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatumRef {
    /// Datum label (e.g. `"A"`, `"B"`, `"C"`).
    pub label: String,
    /// Datum feature geometry type.
    pub feature_type: DatumFeatureType,
    /// Modifier set attached to this datum reference (RMB only in P1a).
    pub modifiers: Vec<DatumModifier>,
    /// Optional explicit variant override forwarded to `fid-gps`.
    pub variant_override: Option<String>,
}

impl DatumRef {
    /// Convenience constructor for a datum feature at RMB with no variant override.
    pub fn rmb(label: impl Into<String>, feature_type: DatumFeatureType) -> Self {
        DatumRef { label: label.into(), feature_type, modifiers: vec![DatumModifier::Rmb], variant_override: None }
    }
}

/// Explicit interpretation context required before resolving any datum-system alignment
/// (REQ-ALN.DATUM.SYS.GATE-001).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DialectContext {
    pub dialect: Option<Dialect>,
    pub edition: Option<String>,
    pub defaults_profile: Option<String>,
    /// Defaults-profile key/value choices forwarded to `fid_gps::resolve::resolve`.
    pub profile_choices: BTreeMap<String, String>,
}

/// Resolved DMT entry for one datum feature in the chain (REQ-ALN.DATUM.SYS.GATE-004).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedDatum {
    pub label: String,
    pub precedence: DatumPrecedence,
    pub row_id: DmtKey,
    pub variant: String,
    pub variant_source: VariantSource,
    pub chain: OperatorChain,
    pub chain_digest: Digest,
    pub limitations: Vec<String>,
}

impl ResolvedDatum {
    /// Canonical JSON representation.
    pub fn to_cvalue(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("chain".to_string(), self.chain.to_cvalue());
        m.insert("chain_digest".to_string(), CValue::Str(self.chain_digest.as_str().to_string()));
        m.insert("label".to_string(), CValue::Str(self.label.clone()));
        m.insert(
            "limitations".to_string(),
            CValue::Arr(self.limitations.iter().map(|l| CValue::Str(l.clone())).collect()),
        );
        m.insert("precedence".to_string(), CValue::Str(self.precedence.as_str().to_string()));
        m.insert("row_id".to_string(), CValue::Str(self.row_id.as_str().to_string()));
        m.insert("variant".to_string(), CValue::Str(self.variant.clone()));
        m.insert("variant_source".to_string(), CValue::Str(self.variant_source.as_str().to_string()));
        CValue::Obj(m)
    }
}

/// Output of [`resolve_datum_chain`] consumed by `ALN.DATUM.SYS.BUILD` and `ALN.DATUM.SYS.CMD`
/// (REQ-ALN.DATUM.SYS.GATE-004).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedDatumChain {
    pub per_datum: Vec<ResolvedDatum>,
    pub dmt_digest: Digest,
    /// SHA-256 over the RFC 8785 canonical JSON array of the ordered per-datum `chain_digest`s.
    pub combined_digest: Digest,
}

impl ResolvedDatumChain {
    /// Canonical JSON representation.
    pub fn to_cvalue(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("combined_digest".to_string(), CValue::Str(self.combined_digest.as_str().to_string()));
        m.insert("dmt_digest".to_string(), CValue::Str(self.dmt_digest.as_str().to_string()));
        m.insert("per_datum".to_string(), CValue::Arr(self.per_datum.iter().map(ResolvedDatum::to_cvalue).collect()));
        CValue::Obj(m)
    }
}

/// Typed errors returned by [`resolve_datum_chain`]. Every variant carries a stable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateError {
    /// One or more of `dialect`, `edition` or `defaults_profile` was absent or empty (REQ-001).
    DialectUnspecified { fields: Vec<&'static str> },
    /// A datum carried a modifier outside P1a's RMB-only scope (REQ-002).
    UnsupportedModifierP1a { label: String, modifier: String },
    /// The DMT resolver returned `UNRESOLVED` for a datum (REQ-003).
    Unresolved { label: String, key: String, dmt_digest: Box<Digest> },
    /// The DMT resolver returned `UNSUPPORTED` for a datum (REQ-003).
    Unsupported { label: String, row_id: Box<DmtKey>, key: String, dmt_digest: Box<Digest> },
    /// The underlying DMT resolver returned a typed error (e.g. `ambiguous_resolution`, `unknown_variant`).
    Resolver { label: String, error: Box<ResolveError> },
    /// Invalid datum list or datum label.
    BadInput { reason: &'static str },
    /// Canonical JSON encoding failure.
    Encode,
}

impl GateError {
    /// Stable machine-readable error code.
    pub fn code(&self) -> &'static str {
        match self {
            GateError::DialectUnspecified { .. } => "dialect_unspecified",
            GateError::UnsupportedModifierP1a { .. } => "unsupported_modifier_p1a",
            GateError::Unresolved { .. } => "UNRESOLVED",
            GateError::Unsupported { .. } => "UNSUPPORTED",
            GateError::Resolver { error, .. } => error.code(),
            GateError::BadInput { .. } => "bad_input",
            GateError::Encode => "encode_failed",
        }
    }
}

impl fmt::Display for GateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GateError::DialectUnspecified { fields } => {
                write!(f, "dialect_unspecified{{fields: [{}]}}", fields.join(", "))
            }
            GateError::UnsupportedModifierP1a { label, modifier } => {
                write!(f, "unsupported_modifier_p1a{{label: {label}, modifier: {modifier}}}")
            }
            GateError::Unresolved { label, key, .. } => {
                write!(f, "UNRESOLVED{{label: {label}, key: {key}}}")
            }
            GateError::Unsupported { label, row_id, key, .. } => {
                write!(f, "UNSUPPORTED{{label: {label}, row_id: {row_id}, key: {key}}}")
            }
            GateError::Resolver { label, error } => write!(f, "resolver({label}): {error}"),
            GateError::BadInput { reason } => write!(f, "bad_input{{{reason}}}"),
            GateError::Encode => f.write_str("encode_failed"),
        }
    }
}

impl std::error::Error for GateError {}

/// Derives the DMT characteristic segment `datum_<feature_type>_<precedence>`.
pub fn datum_characteristic(feature_type: DatumFeatureType, precedence: DatumPrecedence) -> String {
    format!("datum_{}_{}", feature_type.as_str(), precedence.as_str())
}

/// Computes `combined_digest` = SHA-256 over the RFC 8785 canonical JSON array of the ordered
/// per-datum chain digest strings (REQ-ALN.DATUM.SYS.GATE-004).
pub fn compute_combined_digest(chain_digests: &[Digest]) -> Result<Digest, GateError> {
    let arr = CValue::Arr(chain_digests.iter().map(|d| CValue::Str(d.as_str().to_string())).collect());
    let bytes = to_canonical_bytes(&arr).map_err(|_| GateError::Encode)?;
    Ok(Digest::from_bytes(digest_bytes(&bytes).0))
}

/// Pure `R1` / `D0` dialect gate and per-datum DMT resolver chain (spec node `ALN.DATUM.SYS.GATE`).
///
/// 1. Refuses with [`GateError::DialectUnspecified`] if any of `dialect`, `edition`, or
///    `defaults_profile` is absent or empty, listing all missing fields. Never substitutes
///    a default (REQ-001).
/// 2. Validates the datum list (1..=3 datums, non-empty unique labels).
/// 3. For each datum feature in precedence order (`primary`, `secondary`, `tertiary`):
///    - Checks that every modifier is [`DatumModifier::Rmb`]; any other modifier fails with
///      [`GateError::UnsupportedModifierP1a`] (REQ-002).
///    - Calls [`fid_gps::resolve::resolve`] with `datum_<feature_type>_<precedence>`, the
///      modifier set, dialect, edition and profile choices.
///    - Stops immediately and passes through [`GateError::Unresolved`] or
///      [`GateError::Unsupported`] with the datum label and DMT key (REQ-003).
/// 4. Returns [`ResolvedDatumChain`] carrying every per-datum record, the DMT table digest,
///    and `combined_digest` (REQ-004).
pub fn resolve_datum_chain(
    datums: &[DatumRef],
    ctx: &DialectContext,
    dmt: &DmtTable,
) -> Result<ResolvedDatumChain, GateError> {
    // REQ-001: check all three required context fields; never default any of them.
    let mut missing: Vec<&'static str> = Vec::new();
    if ctx.dialect.is_none() {
        missing.push("dialect");
    }
    let edition = ctx.edition.as_deref().map(str::trim).filter(|s| !s.is_empty());
    if edition.is_none() {
        missing.push("edition");
    }
    let defaults_profile = ctx.defaults_profile.as_deref().map(str::trim).filter(|s| !s.is_empty());
    if defaults_profile.is_none() {
        missing.push("defaults_profile");
    }
    if !missing.is_empty() {
        return Err(GateError::DialectUnspecified { fields: missing });
    }
    let Some(dialect) = ctx.dialect else {
        return Err(GateError::DialectUnspecified { fields: vec!["dialect"] });
    };
    let Some(edition) = edition else {
        return Err(GateError::DialectUnspecified { fields: vec!["edition"] });
    };

    if datums.is_empty() {
        return Err(GateError::BadInput { reason: "no_datums" });
    }
    if datums.len() > MAX_DATUMS {
        return Err(GateError::BadInput { reason: "too_many_datums" });
    }

    let mut seen_labels: BTreeSet<&str> = BTreeSet::new();
    for d in datums {
        if d.label.is_empty() || d.label.len() > MAX_DATUM_LABEL_LEN {
            return Err(GateError::BadInput { reason: "bad_datum_label" });
        }
        if !seen_labels.insert(d.label.as_str()) {
            return Err(GateError::BadInput { reason: "duplicate_datum_label" });
        }
    }

    let mut per_datum = Vec::with_capacity(datums.len());
    let mut chain_digests = Vec::with_capacity(datums.len());
    let editions = [edition];

    for (idx, datum) in datums.iter().enumerate() {
        let Some(precedence) = DatumPrecedence::from_index(idx) else {
            return Err(GateError::BadInput { reason: "too_many_datums" });
        };

        // REQ-002: P1a allows RMB only; any other modifier fails `unsupported_modifier_p1a`.
        let mut mod_strs: Vec<&str> = Vec::with_capacity(datum.modifiers.len());
        for m in &datum.modifiers {
            match m {
                DatumModifier::Rmb => mod_strs.push(m.as_str()),
                other => {
                    return Err(GateError::UnsupportedModifierP1a {
                        label: datum.label.clone(),
                        modifier: other.as_str().to_string(),
                    });
                }
            }
        }

        let characteristic = datum_characteristic(datum.feature_type, precedence);
        let input = ResolveInput::new(
            dialect,
            &characteristic,
            &mod_strs,
            &editions,
            ctx.profile_choices.clone(),
            datum.variant_override.as_deref(),
        )
        .map_err(|error| GateError::Resolver { label: datum.label.clone(), error: Box::new(error) })?;

        let derived_key = input.derived_key();
        let outcome = resolve(&input, dmt)
            .map_err(|error| GateError::Resolver { label: datum.label.clone(), error: Box::new(error) })?;

        match outcome {
            Resolution::Resolved(r) => {
                let chain_digest = r.chain_digest();
                chain_digests.push(chain_digest);
                per_datum.push(ResolvedDatum {
                    label: datum.label.clone(),
                    precedence,
                    row_id: r.row().clone(),
                    variant: r.variant().to_string(),
                    variant_source: r.variant_source(),
                    chain: r.chain().clone(),
                    chain_digest,
                    limitations: r.limitations().to_vec(),
                });
            }
            Resolution::Unresolved(u) => {
                return Err(GateError::Unresolved {
                    label: datum.label.clone(),
                    key: u.key().to_string(),
                    dmt_digest: Box::new(u.dmt_digest()),
                });
            }
            Resolution::Unsupported(u) => {
                return Err(GateError::Unsupported {
                    label: datum.label.clone(),
                    row_id: Box::new(u.row().clone()),
                    key: derived_key,
                    dmt_digest: Box::new(u.dmt_digest()),
                });
            }
        }
    }

    let combined_digest = compute_combined_digest(&chain_digests)?;
    Ok(ResolvedDatumChain { per_datum, dmt_digest: dmt.digest(), combined_digest })
}
