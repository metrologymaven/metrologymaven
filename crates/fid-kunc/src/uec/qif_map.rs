//! Pure QIF 3.0 mapping of [`UncertaintyStatement`] (algorithm `unc.uec.qif_map@1.0.0`,
//! mapping table `qif-unc-map@1`, spec node `UNC.UEC.QIF`).
//!
//! Pure L0 kernel module: no I/O, no third-party dependencies, no `unsafe`, no floating-point
//! arithmetic (1-ulp cross-checks compare exact IEEE-754 total-order bit patterns).
//!
//! - [`to_qif`] serialises every statement field into the versioned `fx:UncertaintyStatement`
//!   (`unc-qif-stmt@1`) extension element (IEEE-754 16-hex-digit scalars so round-trips are
//!   bit-exact) and fills [`NativeUncFields`] for UEC-1..3 only. UEC-0 statements carry the
//!   not-evaluated class and reason in the extension and no native numeric fields (REQ-002).
//! - [`from_qif`] is extension-authoritative: when `ext_xml` is present it parses it under
//!   strict bounds (`MAX_EXT_BYTES = 1 MiB`, `MAX_XML_DEPTH = 16`), reconstructs and validates
//!   the statement through `UNC.UEC.STMT`, and verifies that every present native value matches
//!   the extension within 1 ulp (`qif_unc_conflict` otherwise, REQ-005). When `ext_xml` is
//!   empty and native fields are present, it assigns UEC-1 `DECLARED` with source `qif_import`
//!   through `UNC.UEC.CLASS` (`uec::assign`), never UEC-2 or UEC-3 (REQ-004).

use std::fmt;

use fid_kci::kci::json::{to_canonical as json_to_canonical, Json};
use fid_kci::kci::sha256_digest;

use crate::statement::{
    CoverageInterval, EvidenceClass, Expanded, IntervalKind, Method, ModelRef, PrincipalKind, Provenance, SourceKind,
    SourceRef, StatementParts, StmtError, UncertaintyStatement, SCHEMA,
};
use crate::uec::{
    assign, AssignError, AssignInput, Basis, ContributorSource, RecordFacts, RecordTable, SourceClaim, SourceRecords,
    RULE_SET,
};

/// Algorithm ID of this mapping.
pub const ALGORITHM_ID: &str = "unc.uec.qif_map@1.0.0";
/// Mapping table identifier (`docs/unc/qif-unc-map-1.md`).
pub const MAP_ID: &str = "qif-unc-map@1";
/// Supported extension version tag on `fx:UncertaintyStatement/@version`.
pub const EXTENSION_VERSION: &str = "unc-qif-stmt@1";
/// Namespace URI for the `fx` QIF 3.0 extension elements (`schemas/unc/fx-uncertainty-statement-v1.xsd`).
pub const EXTENSION_NS: &str = "https://metrologymaven.org/ns/qif-ext/v1";
/// Maximum accepted size of `ext_xml` in bytes (1 MiB, REQ-UNC.UEC.QIF-006).
pub const MAX_EXT_BYTES: usize = 1 << 20;
/// Maximum XML element nesting depth (Design: depth 16).
pub const MAX_XML_DEPTH: usize = 16;
/// Maximum XML elements parsed in one extension fragment.
pub const MAX_XML_ELEMENTS: usize = 100_000;
/// Source ID assigned to native-only QIF imports (REQ-UNC.UEC.QIF-004).
pub const QIF_IMPORT_SOURCE_ID: &str = "qif_import";
/// Synthetic model ID used for native-only QIF imports.
pub const QIF_IMPORT_MODEL_ID: &str = "um:qif-import";

/// One row of the `qif-unc-map@1` mapping table.
///
/// Cites QIF 3.0 element names and ISO 23952 clause numbers only (never standards text).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QifMapRow {
    /// Field path in `UncertaintyStatement`.
    pub statement_field: &'static str,
    /// QIF 3.0 native element name, if one exists (`None` when extension-only).
    pub qif_native_element: Option<&'static str>,
    /// Extension element path under `UserDataXML/fx:UncertaintyStatement`.
    pub extension_element: &'static str,
    /// ISO 23952:2020 (QIF 3.0) clause reference only (unverified against full XSD text).
    pub iso_23952_clause: &'static str,
}

/// Published `qif-unc-map@1` table mapping `UncertaintyStatement` fields to QIF 3.0 elements.
pub const QIF_UNC_MAP: &[QifMapRow] = &[
    QifMapRow {
        statement_field: "class",
        qif_native_element: None,
        extension_element: "fx:UncertaintyStatement/fx:Class",
        iso_23952_clause: "ISO 23952:2020 cl. 6.3 (Attributes/UserDataXML)",
    },
    QifMapRow {
        statement_field: "uec0_reason",
        qif_native_element: None,
        extension_element: "fx:UncertaintyStatement/fx:Uec0Reason",
        iso_23952_clause: "ISO 23952:2020 cl. 6.3 (Attributes/UserDataXML)",
    },
    QifMapRow {
        statement_field: "method",
        qif_native_element: None,
        extension_element: "fx:UncertaintyStatement/fx:Method",
        iso_23952_clause: "ISO 23952:2020 cl. 6.3 (Attributes/UserDataXML)",
    },
    QifMapRow {
        statement_field: "unit",
        qif_native_element: Some("LinearUnit / AngularUnit (QIF FileUnits / attribute)"),
        extension_element: "fx:UncertaintyStatement/fx:Unit",
        iso_23952_clause: "ISO 23952:2020 cl. 6.5 (Units)",
    },
    QifMapRow {
        statement_field: "u_std",
        qif_native_element: Some("CombinedUncertainty"),
        extension_element: "fx:UncertaintyStatement/fx:UStd",
        iso_23952_clause: "ISO 23952:2020 cl. 6.8 (Results / MeasurementUncertainty)",
    },
    QifMapRow {
        statement_field: "expanded.U",
        qif_native_element: Some("ExpandedUncertainty"),
        extension_element: "fx:UncertaintyStatement/fx:Expanded/fx:U",
        iso_23952_clause: "ISO 23952:2020 cl. 6.8 (Results / MeasurementUncertainty)",
    },
    QifMapRow {
        statement_field: "expanded.k",
        qif_native_element: Some("CoverageFactor"),
        extension_element: "fx:UncertaintyStatement/fx:Expanded/fx:K",
        iso_23952_clause: "ISO 23952:2020 cl. 6.8 (Results / MeasurementUncertainty)",
    },
    QifMapRow {
        statement_field: "interval.low",
        qif_native_element: Some("LowerBound"),
        extension_element: "fx:UncertaintyStatement/fx:Interval/fx:Low",
        iso_23952_clause: "ISO 23952:2020 cl. 6.8 (Results / MeasurementUncertainty)",
    },
    QifMapRow {
        statement_field: "interval.high",
        qif_native_element: Some("UpperBound"),
        extension_element: "fx:UncertaintyStatement/fx:Interval/fx:High",
        iso_23952_clause: "ISO 23952:2020 cl. 6.8 (Results / MeasurementUncertainty)",
    },
    QifMapRow {
        statement_field: "interval.p",
        qif_native_element: Some("CoverageProbability"),
        extension_element: "fx:UncertaintyStatement/fx:Interval/fx:P",
        iso_23952_clause: "ISO 23952:2020 cl. 6.8 (Results / MeasurementUncertainty)",
    },
    QifMapRow {
        statement_field: "interval.kind",
        qif_native_element: None,
        extension_element: "fx:UncertaintyStatement/fx:Interval/fx:Kind",
        iso_23952_clause: "ISO 23952:2020 cl. 6.3 (Attributes/UserDataXML)",
    },
    QifMapRow {
        statement_field: "model",
        qif_native_element: None,
        extension_element: "fx:UncertaintyStatement/fx:Model",
        iso_23952_clause: "ISO 23952:2020 cl. 6.3 (Attributes/UserDataXML)",
    },
    QifMapRow {
        statement_field: "sources",
        qif_native_element: None,
        extension_element: "fx:UncertaintyStatement/fx:Sources/fx:Source",
        iso_23952_clause: "ISO 23952:2020 cl. 6.3 (Attributes/UserDataXML)",
    },
    QifMapRow {
        statement_field: "contributors",
        qif_native_element: None,
        extension_element: "fx:UncertaintyStatement/fx:Contributors/fx:Contributor",
        iso_23952_clause: "ISO 23952:2020 cl. 6.3 (Attributes/UserDataXML)",
    },
    QifMapRow {
        statement_field: "lpu_validated_by_mc",
        qif_native_element: None,
        extension_element: "fx:UncertaintyStatement/fx:LpuValidatedByMc",
        iso_23952_clause: "ISO 23952:2020 cl. 6.3 (Attributes/UserDataXML)",
    },
    QifMapRow {
        statement_field: "nonlinearity",
        qif_native_element: None,
        extension_element: "fx:UncertaintyStatement/fx:Nonlinearity",
        iso_23952_clause: "ISO 23952:2020 cl. 6.3 (Attributes/UserDataXML)",
    },
    QifMapRow {
        statement_field: "selection_rule",
        qif_native_element: None,
        extension_element: "fx:UncertaintyStatement/fx:SelectionRule",
        iso_23952_clause: "ISO 23952:2020 cl. 6.3 (Attributes/UserDataXML)",
    },
    QifMapRow {
        statement_field: "estimate_convention",
        qif_native_element: None,
        extension_element: "fx:UncertaintyStatement/fx:EstimateConvention",
        iso_23952_clause: "ISO 23952:2020 cl. 6.3 (Attributes/UserDataXML)",
    },
    QifMapRow {
        statement_field: "provenance",
        qif_native_element: None,
        extension_element: "fx:UncertaintyStatement/fx:Provenance",
        iso_23952_clause: "ISO 23952:2020 cl. 6.3 (Attributes/UserDataXML)",
    },
];

/// Lower and upper bounds carried in native QIF interval fields.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NativeInterval {
    pub low: f64,
    pub high: f64,
}

/// Numeric and unit values carried in QIF 3.0 native elements for readers that ignore
/// `fx:UncertaintyStatement`. Always all-`None` for UEC-0 (REQ-UNC.UEC.QIF-002).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NativeUncFields {
    pub u_std: Option<f64>,
    pub expanded_u: Option<f64>,
    pub k: Option<f64>,
    pub interval: Option<NativeInterval>,
    pub p: Option<f64>,
    pub unit: Option<String>,
}

impl NativeUncFields {
    /// True if any numeric uncertainty field (`u_std`, `expanded_u`, `k`, `interval`, `p`) is set.
    pub fn has_numeric(&self) -> bool {
        self.u_std.is_some()
            || self.expanded_u.is_some()
            || self.k.is_some()
            || self.interval.is_some()
            || self.p.is_some()
    }
}

/// Neutral QIF 3.0 uncertainty fragment exchanged between L0 (`fid-kunc`) and L2 IO crates.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct UncQifFragment {
    /// Native QIF uncertainty values (populated for UEC-1..3 only; all `None` for UEC-0).
    pub native: NativeUncFields,
    /// Canonical UTF-8 XML bytes of `<fx:UncertaintyStatement version="unc-qif-stmt@1">...`.
    /// Empty when no extension element is present (native-only import).
    pub ext_xml: Vec<u8>,
}

impl UncQifFragment {
    /// True when the `fx:UncertaintyStatement` extension XML is present.
    pub fn has_extension(&self) -> bool {
        !self.ext_xml.is_empty()
    }
}

/// Errors returned by [`from_qif`] and [`from_qif_verified`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QifMapError {
    /// A native QIF value disagrees with the authoritative extension by more than 1 ulp (or
    /// appears on a UEC-0 statement).
    Conflict { field: &'static str },
    /// The extension element declares an unknown version string (REQ-UNC.UEC.QIF-006).
    UnsupportedExtension { version: String },
    /// The extension exceeds [`MAX_EXT_BYTES`], [`MAX_XML_DEPTH`] or [`MAX_XML_ELEMENTS`].
    LimitExceeded,
    /// The extension XML is syntactically invalid or breaks the `unc-qif-stmt@1` element schema.
    MalformedXml(&'static str),
    /// Reconstructed statement failed `UNC.UEC.STMT` validation.
    Statement(StmtError),
    /// Native-only import failed `UNC.UEC.CLASS` assignment.
    Assign(AssignError),
}

impl QifMapError {
    /// Stable machine-readable error code.
    pub const fn code(&self) -> &'static str {
        match self {
            QifMapError::Conflict { .. } => "qif_unc_conflict",
            QifMapError::UnsupportedExtension { .. } => "unsupported_extension",
            QifMapError::LimitExceeded => "limit_exceeded",
            QifMapError::MalformedXml(_) => "malformed_xml",
            QifMapError::Statement(e) => e.code(),
            QifMapError::Assign(e) => e.code(),
        }
    }
}

impl fmt::Display for QifMapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QifMapError::Conflict { field } => write!(f, "qif_unc_conflict{{field: \"{field}\"}}"),
            QifMapError::UnsupportedExtension { version } => {
                write!(f, "unsupported_extension: {version}")
            }
            QifMapError::LimitExceeded => f.write_str("limit_exceeded"),
            QifMapError::MalformedXml(msg) => write!(f, "malformed_xml: {msg}"),
            QifMapError::Statement(e) => e.fmt(f),
            QifMapError::Assign(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for QifMapError {}

impl From<StmtError> for QifMapError {
    fn from(e: StmtError) -> Self {
        QifMapError::Statement(e)
    }
}

impl From<AssignError> for QifMapError {
    fn from(e: AssignError) -> Self {
        QifMapError::Assign(e)
    }
}

// ------------------------------------------------------------------ IEEE-754 hex & 1-ulp compare

const HEX: &[u8; 16] = b"0123456789abcdef";

fn f64_to_hex(x: f64) -> String {
    let bits = x.to_bits();
    let mut out = String::with_capacity(16);
    for i in (0..16).rev() {
        let nibble = ((bits >> (4 * i)) & 15) as usize;
        out.push(char::from(HEX[nibble]));
    }
    out
}

/// Maps any finite `f64` to a strictly monotonic `u64` with step 1 between adjacent representable
/// values (including across zero, where `+0.0` and `-0.0` share `0x8000_0000_0000_0000`).
/// Pure integer bit operations: zero floating-point arithmetic (KNP-1, D0).
fn ordered_bits(x: f64) -> u64 {
    let b = if x == 0.0 { 0u64 } else { x.to_bits() };
    if (b >> 63) != 0 {
        (!b).wrapping_add(1)
    } else {
        b ^ (1u64 << 63)
    }
}

/// True iff both `a` and `b` are finite and differ by at most 1 ulp (REQ-UNC.UEC.QIF-005).
pub fn within_one_ulp(a: f64, b: f64) -> bool {
    if !a.is_finite() || !b.is_finite() {
        return false;
    }
    ordered_bits(a).abs_diff(ordered_bits(b)) <= 1
}

// ------------------------------------------------------------------ to_qif

fn push_xml_escaped(out: &mut String, text: &str) {
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
}

fn push_elem(out: &mut String, tag: &str, text: &str) {
    out.push('<');
    out.push_str(tag);
    out.push('>');
    push_xml_escaped(out, text);
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
}

fn bool_str(b: bool) -> &'static str {
    if b {
        "true"
    } else {
        "false"
    }
}

/// Maps an [`UncertaintyStatement`] to a neutral QIF 3.0 fragment (REQ-UNC.UEC.QIF-001, -002).
///
/// The extension element `fx:UncertaintyStatement` (`unc-qif-stmt@1`) always carries every field
/// of the statement with IEEE-754 hex scalars. For UEC-0 statements, `native` carries no numeric
/// uncertainty fields; for UEC-1..3, `native` carries `u_std`, `expanded_u`, `k`, `interval`,
/// `p` and `unit` where present.
pub fn to_qif(statement: &UncertaintyStatement) -> UncQifFragment {
    let native = if statement.class() == EvidenceClass::Uec0 {
        NativeUncFields::default()
    } else {
        NativeUncFields {
            u_std: statement.u_std(),
            expanded_u: statement.expanded().map(|e| e.value),
            k: statement.expanded().map(|e| e.k),
            interval: statement.interval().map(|i| NativeInterval { low: i.low, high: i.high }),
            p: statement.interval().map(|i| i.p),
            unit: statement.unit().map(str::to_owned),
        }
    };

    let mut xml = String::new();
    xml.push_str("<fx:UncertaintyStatement xmlns:fx=\"");
    xml.push_str(EXTENSION_NS);
    xml.push_str("\" version=\"");
    xml.push_str(EXTENSION_VERSION);
    xml.push_str("\">");

    push_elem(&mut xml, "fx:Class", statement.class().wire());
    if let Some(reason) = statement.uec0_reason() {
        push_elem(&mut xml, "fx:Uec0Reason", reason.wire());
    }
    if let Some(method) = statement.method() {
        push_elem(&mut xml, "fx:Method", method.wire());
    }
    if let Some(unit) = statement.unit() {
        push_elem(&mut xml, "fx:Unit", unit);
    }
    if let Some(u_std) = statement.u_std() {
        push_elem(&mut xml, "fx:UStd", &f64_to_hex(u_std));
    }
    if let Some(exp) = statement.expanded() {
        xml.push_str("<fx:Expanded>");
        push_elem(&mut xml, "fx:U", &f64_to_hex(exp.value));
        push_elem(&mut xml, "fx:K", &f64_to_hex(exp.k));
        xml.push_str("</fx:Expanded>");
    }
    if let Some(intvl) = statement.interval() {
        xml.push_str("<fx:Interval>");
        push_elem(&mut xml, "fx:Low", &f64_to_hex(intvl.low));
        push_elem(&mut xml, "fx:High", &f64_to_hex(intvl.high));
        push_elem(&mut xml, "fx:P", &f64_to_hex(intvl.p));
        push_elem(&mut xml, "fx:Kind", intvl.kind.wire());
        xml.push_str("</fx:Interval>");
    }
    if let Some(model) = statement.model() {
        xml.push_str("<fx:Model>");
        push_elem(&mut xml, "fx:Id", &model.id);
        push_elem(&mut xml, "fx:Semver", &model.semver);
        push_elem(&mut xml, "fx:Digest", model.digest.as_str());
        xml.push_str("</fx:Model>");
    }
    if !statement.sources().is_empty() {
        xml.push_str("<fx:Sources>");
        for src in statement.sources() {
            xml.push_str("<fx:Source>");
            push_elem(&mut xml, "fx:Kind", src.kind.wire());
            push_elem(&mut xml, "fx:Id", &src.id);
            push_elem(&mut xml, "fx:Digest", src.digest.as_str());
            if src.unverified {
                push_elem(&mut xml, "fx:Unverified", "true");
            }
            xml.push_str("</fx:Source>");
        }
        xml.push_str("</fx:Sources>");
    }
    if !statement.contributors().is_empty() {
        xml.push_str("<fx:Contributors>");
        for c in statement.contributors() {
            xml.push_str("<fx:Contributor>");
            push_elem(&mut xml, "fx:InputId", &c.input_id);
            if let Some(sens) = c.sensitivity {
                push_elem(&mut xml, "fx:Sensitivity", &f64_to_hex(sens));
            }
            push_elem(&mut xml, "fx:UContrib", &f64_to_hex(c.u_contrib));
            push_elem(&mut xml, "fx:VarianceShare", &f64_to_hex(c.variance_share));
            xml.push_str("</fx:Contributor>");
        }
        xml.push_str("</fx:Contributors>");
    }
    if let Some(val) = statement.lpu_validated_by_mc() {
        push_elem(&mut xml, "fx:LpuValidatedByMc", bool_str(val));
    }
    if let Some(nlin) = statement.nonlinearity() {
        xml.push_str("<fx:Nonlinearity>");
        push_elem(&mut xml, "fx:Version", &nlin.version);
        push_elem(&mut xml, "fx:Outcome", nlin.verdict.wire());
        push_elem(&mut xml, "fx:V1", &f64_to_hex(nlin.v1));
        push_elem(&mut xml, "fx:V2", &f64_to_hex(nlin.v2));
        if let Some(ratio) = nlin.ratio {
            push_elem(&mut xml, "fx:Ratio", &f64_to_hex(ratio));
        }
        push_elem(&mut xml, "fx:ProbedInputs", &nlin.probed_inputs.to_string());
        push_elem(&mut xml, "fx:ProbedPairs", &nlin.probed_pairs.to_string());
        push_elem(&mut xml, "fx:CrossTermsTruncated", bool_str(nlin.cross_terms_truncated));
        if let Some(reason) = &nlin.reason {
            push_elem(&mut xml, "fx:Reason", reason);
        }
        xml.push_str("</fx:Nonlinearity>");
    }
    if let Some(rule) = statement.selection_rule() {
        push_elem(&mut xml, "fx:SelectionRule", rule);
    }
    if let Some(conv) = statement.estimate_convention() {
        push_elem(&mut xml, "fx:EstimateConvention", conv);
    }
    let prov = statement.provenance();
    xml.push_str("<fx:Provenance>");
    if !prov.rule_sets.is_empty() {
        xml.push_str("<fx:RuleSets>");
        for rs in &prov.rule_sets {
            push_elem(&mut xml, "fx:RuleSet", rs);
        }
        xml.push_str("</fx:RuleSets>");
    }
    if let Some(p) = &prov.principal {
        push_elem(&mut xml, "fx:Principal", p);
    }
    if let Some(pk) = prov.principal_kind {
        push_elem(&mut xml, "fx:PrincipalKind", pk.wire());
    }
    push_elem(&mut xml, "fx:Proposal", bool_str(prov.proposal));
    if let Some(ab) = &prov.adopted_by {
        push_elem(&mut xml, "fx:AdoptedBy", ab);
    }
    if let Some(sup) = &prov.supersedes {
        push_elem(&mut xml, "fx:Supersedes", sup.as_str());
    }
    xml.push_str("</fx:Provenance>");

    xml.push_str("</fx:UncertaintyStatement>");
    UncQifFragment { native, ext_xml: xml.into_bytes() }
}

// ------------------------------------------------------------------ bounded XML parser (no deps)

#[derive(Debug, Clone)]
struct XmlElem {
    name: String,
    attrs: Vec<(String, String)>,
    text: String,
    children: Vec<XmlElem>,
}

impl XmlElem {
    fn attr(&self, key: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
}

fn unescape_xml(raw: &str) -> Result<String, QifMapError> {
    let mut out = String::with_capacity(raw.len());
    let bytes = raw.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes.get(i) == Some(&b'&') {
            let rest = raw.get(i..).unwrap_or("");
            let Some(semi) = rest.find(';') else {
                return Err(QifMapError::MalformedXml("unterminated entity reference"));
            };
            let ent = rest.get(1..semi).unwrap_or("");
            match ent {
                "amp" => out.push('&'),
                "lt" => out.push('<'),
                "gt" => out.push('>'),
                "quot" => out.push('"'),
                "apos" => out.push('\''),
                _ => return Err(QifMapError::MalformedXml("unsupported entity reference")),
            }
            i += semi + 1;
        } else {
            let rest = raw.get(i..).unwrap_or("");
            let Some(ch) = rest.chars().next() else {
                break;
            };
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    Ok(out)
}

fn skip_ws(s: &[u8], i: &mut usize) {
    while let Some(&b) = s.get(*i) {
        if matches!(b, b' ' | b'\t' | b'\r' | b'\n') {
            *i += 1;
        } else {
            break;
        }
    }
}

fn is_name_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b':' | b'.')
}

fn read_name<'a>(src: &'a str, bytes: &[u8], i: &mut usize) -> Result<&'a str, QifMapError> {
    let start = *i;
    while let Some(&b) = bytes.get(*i) {
        if is_name_char(b) {
            *i += 1;
        } else {
            break;
        }
    }
    if *i == start {
        return Err(QifMapError::MalformedXml("expected XML name"));
    }
    src.get(start..*i).ok_or(QifMapError::MalformedXml("invalid UTF-8 slice"))
}

/// Iterative, strictly bounded XML element parser (depth <= `MAX_XML_DEPTH`, bytes <=
/// `MAX_EXT_BYTES`, elements <= `MAX_XML_ELEMENTS`). Refuses DTDs, comments/PIs inside the
/// fragment except an optional leading `<?xml ...?>`, and mismatched or unclosed tags.
fn parse_xml_root(input: &[u8]) -> Result<XmlElem, QifMapError> {
    if input.len() > MAX_EXT_BYTES {
        return Err(QifMapError::LimitExceeded);
    }
    let src = std::str::from_utf8(input).map_err(|_| QifMapError::MalformedXml("non-UTF-8 XML"))?;
    let bytes = src.as_bytes();
    let mut i = 0usize;
    skip_ws(bytes, &mut i);
    // Optional XML declaration.
    if bytes.get(i..i + 5) == Some(b"<?xml") {
        let rest = src.get(i..).unwrap_or("");
        let Some(end_rel) = rest.find("?>") else {
            return Err(QifMapError::MalformedXml("unterminated xml declaration"));
        };
        i += end_rel + 2;
        skip_ws(bytes, &mut i);
    }

    let mut stack: Vec<XmlElem> = Vec::new();
    let mut root: Option<XmlElem> = None;
    let mut count = 0usize;

    while i < bytes.len() {
        if bytes.get(i) == Some(&b'<') {
            if bytes.get(i + 1) == Some(&b'!') || bytes.get(i + 1) == Some(&b'?') {
                return Err(QifMapError::MalformedXml("disallowed DTD, comment or PI"));
            }
            if bytes.get(i + 1) == Some(&b'/') {
                // Closing tag.
                i += 2;
                let close_name = read_name(src, bytes, &mut i)?;
                skip_ws(bytes, &mut i);
                if bytes.get(i) != Some(&b'>') {
                    return Err(QifMapError::MalformedXml("expected `>` on closing tag"));
                }
                i += 1;
                let Some(done) = stack.pop() else {
                    return Err(QifMapError::MalformedXml("unexpected closing tag"));
                };
                if done.name != close_name {
                    return Err(QifMapError::MalformedXml("mismatched closing tag"));
                }
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(done);
                } else {
                    if root.is_some() {
                        return Err(QifMapError::MalformedXml("multiple root elements"));
                    }
                    root = Some(done);
                }
            } else {
                // Opening or self-closing tag.
                if root.is_some() {
                    return Err(QifMapError::MalformedXml("trailing element after root"));
                }
                i += 1;
                let name = read_name(src, bytes, &mut i)?.to_owned();
                count = count.saturating_add(1);
                if count > MAX_XML_ELEMENTS {
                    return Err(QifMapError::LimitExceeded);
                }
                if stack.len() >= MAX_XML_DEPTH {
                    return Err(QifMapError::LimitExceeded);
                }
                let mut attrs = Vec::new();
                loop {
                    skip_ws(bytes, &mut i);
                    match bytes.get(i) {
                        Some(&b'>') | Some(&b'/') => break,
                        None => return Err(QifMapError::MalformedXml("unterminated start tag")),
                        _ => {
                            let ak = read_name(src, bytes, &mut i)?.to_owned();
                            skip_ws(bytes, &mut i);
                            if bytes.get(i) != Some(&b'=') {
                                return Err(QifMapError::MalformedXml("expected `=` in attribute"));
                            }
                            i += 1;
                            skip_ws(bytes, &mut i);
                            let Some(&q) = bytes.get(i) else {
                                return Err(QifMapError::MalformedXml("missing attribute quote"));
                            };
                            if q != b'"' && q != b'\'' {
                                return Err(QifMapError::MalformedXml("expected attribute quote"));
                            }
                            i += 1;
                            let v_start = i;
                            while let Some(&c) = bytes.get(i) {
                                if c == q {
                                    break;
                                }
                                if c == b'<' {
                                    return Err(QifMapError::MalformedXml("`<` inside attribute"));
                                }
                                i += 1;
                            }
                            if bytes.get(i) != Some(&q) {
                                return Err(QifMapError::MalformedXml("unterminated attribute"));
                            }
                            let raw_val =
                                src.get(v_start..i).ok_or(QifMapError::MalformedXml("invalid UTF-8 attribute"))?;
                            let av = unescape_xml(raw_val)?;
                            i += 1;
                            if attrs.iter().any(|(k, _)| k == &ak) {
                                return Err(QifMapError::MalformedXml("duplicate attribute"));
                            }
                            attrs.push((ak, av));
                        }
                    }
                }
                let self_closing = if bytes.get(i) == Some(&b'/') {
                    if bytes.get(i + 1) != Some(&b'>') {
                        return Err(QifMapError::MalformedXml("expected `/>`"));
                    }
                    i += 2;
                    true
                } else if bytes.get(i) == Some(&b'>') {
                    i += 1;
                    false
                } else {
                    return Err(QifMapError::MalformedXml("expected `>`"));
                };
                let elem = XmlElem { name, attrs, text: String::new(), children: Vec::new() };
                if self_closing {
                    if let Some(parent) = stack.last_mut() {
                        parent.children.push(elem);
                    } else {
                        root = Some(elem);
                    }
                } else {
                    stack.push(elem);
                }
            }
        } else {
            let t_start = i;
            while let Some(&b) = bytes.get(i) {
                if b == b'<' {
                    break;
                }
                i += 1;
            }
            let chunk = src.get(t_start..i).ok_or(QifMapError::MalformedXml("invalid UTF-8 text"))?;
            if let Some(top) = stack.last_mut() {
                if !chunk.trim().is_empty() || top.children.is_empty() {
                    let decoded = unescape_xml(chunk)?;
                    top.text.push_str(&decoded);
                }
            } else if !chunk.trim().is_empty() {
                return Err(QifMapError::MalformedXml("text outside root element"));
            }
        }
    }

    if !stack.is_empty() {
        return Err(QifMapError::MalformedXml("unclosed XML element"));
    }
    root.ok_or(QifMapError::MalformedXml("empty XML document"))
}

// ------------------------------------------------------------------ XML -> canonical JSON

fn local_name(name: &str) -> &str {
    match name.split_once(':') {
        Some((_, local)) => local,
        None => name,
    }
}

fn leaf_text(elem: &XmlElem) -> Result<&str, QifMapError> {
    if !elem.children.is_empty() || !elem.attrs.is_empty() {
        return Err(QifMapError::MalformedXml("leaf element must not have children or attributes"));
    }
    Ok(elem.text.as_str())
}

fn container_only(elem: &XmlElem) -> Result<(), QifMapError> {
    if !elem.text.trim().is_empty() || !elem.attrs.is_empty() {
        return Err(QifMapError::MalformedXml("container element must not have text or attributes"));
    }
    Ok(())
}

fn set_once<'a>(slot: &mut Option<&'a str>, val: &'a str) -> Result<(), QifMapError> {
    if slot.is_some() {
        return Err(QifMapError::MalformedXml("duplicate child element"));
    }
    *slot = Some(val);
    Ok(())
}

fn xml_to_canonical_json(root: &XmlElem) -> Result<Vec<u8>, QifMapError> {
    if local_name(&root.name) != "UncertaintyStatement" {
        return Err(QifMapError::MalformedXml("root element must be fx:UncertaintyStatement"));
    }
    for (k, v) in &root.attrs {
        match k.as_str() {
            "version" => {}
            "xmlns" | "xmlns:fx" => {
                if v != EXTENSION_NS {
                    return Err(QifMapError::MalformedXml("unexpected fx namespace URI"));
                }
            }
            _ => return Err(QifMapError::MalformedXml("unknown attribute on root element")),
        }
    }
    let version = root.attr("version").ok_or(QifMapError::MalformedXml("missing version attribute"))?;
    if version != EXTENSION_VERSION {
        return Err(QifMapError::UnsupportedExtension { version: version.to_owned() });
    }
    if !root.text.trim().is_empty() {
        return Err(QifMapError::MalformedXml("unexpected text directly inside root"));
    }

    let mut members: Vec<(String, Json)> = Vec::new();
    members.push(("schema".to_owned(), Json::Str(SCHEMA.to_owned())));

    let mut seen_class = false;
    let mut seen_uec0 = false;
    let mut seen_method = false;
    let mut seen_unit = false;
    let mut seen_u_std = false;
    let mut seen_expanded = false;
    let mut seen_interval = false;
    let mut seen_model = false;
    let mut seen_sources = false;
    let mut seen_contributors = false;
    let mut seen_lpu_mc = false;
    let mut seen_nlin = false;
    let mut seen_sel = false;
    let mut seen_est = false;
    let mut seen_prov = false;

    let mut sources_json: Vec<Json> = Vec::new();
    let mut contributors_json: Vec<Json> = Vec::new();

    for child in &root.children {
        match local_name(&child.name) {
            "Class" => {
                if seen_class {
                    return Err(QifMapError::MalformedXml("duplicate fx:Class"));
                }
                seen_class = true;
                members.push(("class".to_owned(), Json::Str(leaf_text(child)?.to_owned())));
            }
            "Uec0Reason" => {
                if seen_uec0 {
                    return Err(QifMapError::MalformedXml("duplicate fx:Uec0Reason"));
                }
                seen_uec0 = true;
                members.push(("uec0_reason".to_owned(), Json::Str(leaf_text(child)?.to_owned())));
            }
            "Method" => {
                if seen_method {
                    return Err(QifMapError::MalformedXml("duplicate fx:Method"));
                }
                seen_method = true;
                members.push(("method".to_owned(), Json::Str(leaf_text(child)?.to_owned())));
            }
            "Unit" => {
                if seen_unit {
                    return Err(QifMapError::MalformedXml("duplicate fx:Unit"));
                }
                seen_unit = true;
                members.push(("unit".to_owned(), Json::Str(leaf_text(child)?.to_owned())));
            }
            "UStd" => {
                if seen_u_std {
                    return Err(QifMapError::MalformedXml("duplicate fx:UStd"));
                }
                seen_u_std = true;
                members.push(("u_std".to_owned(), Json::Str(leaf_text(child)?.to_owned())));
            }
            "Expanded" => {
                if seen_expanded {
                    return Err(QifMapError::MalformedXml("duplicate fx:Expanded"));
                }
                seen_expanded = true;
                container_only(child)?;
                let mut u = None;
                let mut k = None;
                for c in &child.children {
                    match local_name(&c.name) {
                        "U" => set_once(&mut u, leaf_text(c)?)?,
                        "K" => set_once(&mut k, leaf_text(c)?)?,
                        _ => return Err(QifMapError::MalformedXml("unknown element in fx:Expanded")),
                    }
                }
                let mut o = Vec::new();
                if let Some(v) = u {
                    o.push(("U".to_owned(), Json::Str(v.to_owned())));
                }
                if let Some(v) = k {
                    o.push(("k".to_owned(), Json::Str(v.to_owned())));
                }
                members.push(("expanded".to_owned(), Json::Obj(o)));
            }
            "Interval" => {
                if seen_interval {
                    return Err(QifMapError::MalformedXml("duplicate fx:Interval"));
                }
                seen_interval = true;
                container_only(child)?;
                let mut low = None;
                let mut high = None;
                let mut p = None;
                let mut kind = None;
                for c in &child.children {
                    match local_name(&c.name) {
                        "Low" => set_once(&mut low, leaf_text(c)?)?,
                        "High" => set_once(&mut high, leaf_text(c)?)?,
                        "P" => set_once(&mut p, leaf_text(c)?)?,
                        "Kind" => set_once(&mut kind, leaf_text(c)?)?,
                        _ => return Err(QifMapError::MalformedXml("unknown element in fx:Interval")),
                    }
                }
                let mut o = Vec::new();
                if let Some(v) = low {
                    o.push(("low".to_owned(), Json::Str(v.to_owned())));
                }
                if let Some(v) = high {
                    o.push(("high".to_owned(), Json::Str(v.to_owned())));
                }
                if let Some(v) = p {
                    o.push(("p".to_owned(), Json::Str(v.to_owned())));
                }
                if let Some(v) = kind {
                    o.push(("kind".to_owned(), Json::Str(v.to_owned())));
                }
                members.push(("interval".to_owned(), Json::Obj(o)));
            }
            "Model" => {
                if seen_model {
                    return Err(QifMapError::MalformedXml("duplicate fx:Model"));
                }
                seen_model = true;
                container_only(child)?;
                let mut id = None;
                let mut semver = None;
                let mut digest = None;
                for c in &child.children {
                    match local_name(&c.name) {
                        "Id" => set_once(&mut id, leaf_text(c)?)?,
                        "Semver" => set_once(&mut semver, leaf_text(c)?)?,
                        "Digest" => set_once(&mut digest, leaf_text(c)?)?,
                        _ => return Err(QifMapError::MalformedXml("unknown element in fx:Model")),
                    }
                }
                let mut o = Vec::new();
                if let Some(v) = id {
                    o.push(("id".to_owned(), Json::Str(v.to_owned())));
                }
                if let Some(v) = semver {
                    o.push(("semver".to_owned(), Json::Str(v.to_owned())));
                }
                if let Some(v) = digest {
                    o.push(("digest".to_owned(), Json::Str(v.to_owned())));
                }
                members.push(("model".to_owned(), Json::Obj(o)));
            }
            "Sources" => {
                if seen_sources {
                    return Err(QifMapError::MalformedXml("duplicate fx:Sources"));
                }
                seen_sources = true;
                container_only(child)?;
                for s_elem in &child.children {
                    if local_name(&s_elem.name) != "Source" {
                        return Err(QifMapError::MalformedXml("expected fx:Source"));
                    }
                    container_only(s_elem)?;
                    let mut kind = None;
                    let mut id = None;
                    let mut digest = None;
                    let mut unverified = None;
                    for c in &s_elem.children {
                        match local_name(&c.name) {
                            "Kind" => set_once(&mut kind, leaf_text(c)?)?,
                            "Id" => set_once(&mut id, leaf_text(c)?)?,
                            "Digest" => set_once(&mut digest, leaf_text(c)?)?,
                            "Unverified" => set_once(&mut unverified, leaf_text(c)?)?,
                            _ => return Err(QifMapError::MalformedXml("unknown element in fx:Source")),
                        }
                    }
                    let mut o = Vec::new();
                    if let Some(v) = kind {
                        o.push(("kind".to_owned(), Json::Str(v.to_owned())));
                    }
                    if let Some(v) = id {
                        o.push(("id".to_owned(), Json::Str(v.to_owned())));
                    }
                    if let Some(v) = digest {
                        o.push(("digest".to_owned(), Json::Str(v.to_owned())));
                    }
                    if let Some(v) = unverified {
                        o.push(("unverified".to_owned(), Json::Str(v.to_owned())));
                    }
                    sources_json.push(Json::Obj(o));
                }
            }
            "Contributors" => {
                if seen_contributors {
                    return Err(QifMapError::MalformedXml("duplicate fx:Contributors"));
                }
                seen_contributors = true;
                container_only(child)?;
                for c_elem in &child.children {
                    if local_name(&c_elem.name) != "Contributor" {
                        return Err(QifMapError::MalformedXml("expected fx:Contributor"));
                    }
                    container_only(c_elem)?;
                    let mut input_id = None;
                    let mut sensitivity = None;
                    let mut u_contrib = None;
                    let mut variance_share = None;
                    for c in &c_elem.children {
                        match local_name(&c.name) {
                            "InputId" => set_once(&mut input_id, leaf_text(c)?)?,
                            "Sensitivity" => set_once(&mut sensitivity, leaf_text(c)?)?,
                            "UContrib" => set_once(&mut u_contrib, leaf_text(c)?)?,
                            "VarianceShare" => set_once(&mut variance_share, leaf_text(c)?)?,
                            _ => {
                                return Err(QifMapError::MalformedXml("unknown element in fx:Contributor"));
                            }
                        }
                    }
                    let mut o = Vec::new();
                    if let Some(v) = input_id {
                        o.push(("input_id".to_owned(), Json::Str(v.to_owned())));
                    }
                    if let Some(v) = sensitivity {
                        o.push(("sensitivity".to_owned(), Json::Str(v.to_owned())));
                    }
                    if let Some(v) = u_contrib {
                        o.push(("u_contrib".to_owned(), Json::Str(v.to_owned())));
                    }
                    if let Some(v) = variance_share {
                        o.push(("variance_share".to_owned(), Json::Str(v.to_owned())));
                    }
                    contributors_json.push(Json::Obj(o));
                }
            }
            "LpuValidatedByMc" => {
                if seen_lpu_mc {
                    return Err(QifMapError::MalformedXml("duplicate fx:LpuValidatedByMc"));
                }
                seen_lpu_mc = true;
                members.push(("lpu_validated_by_mc".to_owned(), Json::Str(leaf_text(child)?.to_owned())));
            }
            "Nonlinearity" => {
                if seen_nlin {
                    return Err(QifMapError::MalformedXml("duplicate fx:Nonlinearity"));
                }
                seen_nlin = true;
                container_only(child)?;
                let mut version = None;
                let mut outcome = None;
                let mut v1 = None;
                let mut v2 = None;
                let mut ratio = None;
                let mut probed_inputs = None;
                let mut probed_pairs = None;
                let mut cross_terms = None;
                let mut reason = None;
                for c in &child.children {
                    match local_name(&c.name) {
                        "Version" => set_once(&mut version, leaf_text(c)?)?,
                        "Outcome" => set_once(&mut outcome, leaf_text(c)?)?,
                        "V1" => set_once(&mut v1, leaf_text(c)?)?,
                        "V2" => set_once(&mut v2, leaf_text(c)?)?,
                        "Ratio" => set_once(&mut ratio, leaf_text(c)?)?,
                        "ProbedInputs" => set_once(&mut probed_inputs, leaf_text(c)?)?,
                        "ProbedPairs" => set_once(&mut probed_pairs, leaf_text(c)?)?,
                        "CrossTermsTruncated" => set_once(&mut cross_terms, leaf_text(c)?)?,
                        "Reason" => set_once(&mut reason, leaf_text(c)?)?,
                        _ => {
                            return Err(QifMapError::MalformedXml("unknown element in fx:Nonlinearity"));
                        }
                    }
                }
                let mut o = Vec::new();
                if let Some(v) = version {
                    o.push(("version".to_owned(), Json::Str(v.to_owned())));
                }
                if let Some(v) = outcome {
                    o.push(("outcome".to_owned(), Json::Str(v.to_owned())));
                }
                if let Some(v) = v1 {
                    o.push(("v1".to_owned(), Json::Str(v.to_owned())));
                }
                if let Some(v) = v2 {
                    o.push(("v2".to_owned(), Json::Str(v.to_owned())));
                }
                if let Some(v) = ratio {
                    o.push(("ratio".to_owned(), Json::Str(v.to_owned())));
                }
                if let Some(v) = probed_inputs {
                    o.push(("probed_inputs".to_owned(), Json::Str(v.to_owned())));
                }
                if let Some(v) = probed_pairs {
                    o.push(("probed_pairs".to_owned(), Json::Str(v.to_owned())));
                }
                if let Some(v) = cross_terms {
                    o.push(("cross_terms_truncated".to_owned(), Json::Str(v.to_owned())));
                }
                if let Some(v) = reason {
                    o.push(("reason".to_owned(), Json::Str(v.to_owned())));
                }
                members.push(("nonlinearity".to_owned(), Json::Obj(o)));
            }
            "SelectionRule" => {
                if seen_sel {
                    return Err(QifMapError::MalformedXml("duplicate fx:SelectionRule"));
                }
                seen_sel = true;
                members.push(("selection_rule".to_owned(), Json::Str(leaf_text(child)?.to_owned())));
            }
            "EstimateConvention" => {
                if seen_est {
                    return Err(QifMapError::MalformedXml("duplicate fx:EstimateConvention"));
                }
                seen_est = true;
                members.push(("estimate_convention".to_owned(), Json::Str(leaf_text(child)?.to_owned())));
            }
            "Provenance" => {
                if seen_prov {
                    return Err(QifMapError::MalformedXml("duplicate fx:Provenance"));
                }
                seen_prov = true;
                container_only(child)?;
                let mut rule_sets: Vec<Json> = Vec::new();
                let mut seen_rs = false;
                let mut principal = None;
                let mut principal_kind = None;
                let mut proposal = None;
                let mut adopted_by = None;
                let mut supersedes = None;
                for c in &child.children {
                    match local_name(&c.name) {
                        "RuleSets" => {
                            if seen_rs {
                                return Err(QifMapError::MalformedXml("duplicate fx:RuleSets"));
                            }
                            seen_rs = true;
                            container_only(c)?;
                            for rs in &c.children {
                                if local_name(&rs.name) != "RuleSet" {
                                    return Err(QifMapError::MalformedXml("expected fx:RuleSet"));
                                }
                                rule_sets.push(Json::Str(leaf_text(rs)?.to_owned()));
                            }
                        }
                        "Principal" => set_once(&mut principal, leaf_text(c)?)?,
                        "PrincipalKind" => set_once(&mut principal_kind, leaf_text(c)?)?,
                        "Proposal" => set_once(&mut proposal, leaf_text(c)?)?,
                        "AdoptedBy" => set_once(&mut adopted_by, leaf_text(c)?)?,
                        "Supersedes" => set_once(&mut supersedes, leaf_text(c)?)?,
                        _ => {
                            return Err(QifMapError::MalformedXml("unknown element in fx:Provenance"));
                        }
                    }
                }
                let mut o = Vec::new();
                o.push(("rule_sets".to_owned(), Json::Arr(rule_sets)));
                if let Some(v) = principal {
                    o.push(("principal".to_owned(), Json::Str(v.to_owned())));
                }
                if let Some(v) = principal_kind {
                    o.push(("principal_kind".to_owned(), Json::Str(v.to_owned())));
                }
                if let Some(v) = proposal {
                    o.push(("proposal".to_owned(), Json::Str(v.to_owned())));
                }
                if let Some(v) = adopted_by {
                    o.push(("adopted_by".to_owned(), Json::Str(v.to_owned())));
                }
                if let Some(v) = supersedes {
                    o.push(("supersedes".to_owned(), Json::Str(v.to_owned())));
                }
                members.push(("provenance".to_owned(), Json::Obj(o)));
            }
            _ => return Err(QifMapError::MalformedXml("unknown child element in root")),
        }
    }

    members.push(("sources".to_owned(), Json::Arr(sources_json)));
    members.push(("contributors".to_owned(), Json::Arr(contributors_json)));

    json_to_canonical(&Json::Obj(members)).map_err(|_| QifMapError::MalformedXml("canonical JSON error"))
}

// ------------------------------------------------------------------ cross-check & native-only

fn cross_check_native(stmt: &UncertaintyStatement, native: &NativeUncFields) -> Result<(), QifMapError> {
    if stmt.class() == EvidenceClass::Uec0 {
        if native.u_std.is_some() {
            return Err(QifMapError::Conflict { field: "u_std" });
        }
        if native.expanded_u.is_some() {
            return Err(QifMapError::Conflict { field: "U" });
        }
        if native.k.is_some() {
            return Err(QifMapError::Conflict { field: "k" });
        }
        if native.interval.is_some() {
            return Err(QifMapError::Conflict { field: "interval" });
        }
        if native.p.is_some() {
            return Err(QifMapError::Conflict { field: "p" });
        }
        return Ok(());
    }

    if let Some(n_u_std) = native.u_std {
        match stmt.u_std() {
            Some(e_u_std) if within_one_ulp(n_u_std, e_u_std) => {}
            _ => return Err(QifMapError::Conflict { field: "u_std" }),
        }
    }
    if let Some(n_u) = native.expanded_u {
        match stmt.expanded() {
            Some(exp) if within_one_ulp(n_u, exp.value) => {}
            _ => return Err(QifMapError::Conflict { field: "U" }),
        }
    }
    if let Some(n_k) = native.k {
        match stmt.expanded() {
            Some(exp) if within_one_ulp(n_k, exp.k) => {}
            _ => return Err(QifMapError::Conflict { field: "k" }),
        }
    }
    if let Some(n_int) = native.interval {
        match stmt.interval() {
            Some(e_int) => {
                if !within_one_ulp(n_int.low, e_int.low) {
                    return Err(QifMapError::Conflict { field: "interval.low" });
                }
                if !within_one_ulp(n_int.high, e_int.high) {
                    return Err(QifMapError::Conflict { field: "interval.high" });
                }
            }
            None => return Err(QifMapError::Conflict { field: "interval" }),
        }
    }
    if let Some(n_p) = native.p {
        match stmt.interval() {
            Some(e_int) if within_one_ulp(n_p, e_int.p) => {}
            _ => return Err(QifMapError::Conflict { field: "p" }),
        }
    }
    if let Some(n_unit) = &native.unit {
        match stmt.unit() {
            Some(e_unit) if e_unit == n_unit => {}
            _ => return Err(QifMapError::Conflict { field: "unit" }),
        }
    }
    Ok(())
}

fn native_import_digest(native: &NativeUncFields) -> fid_types::Digest {
    let mut m: Vec<(String, Json)> = Vec::new();
    m.push(("source".to_owned(), Json::Str(QIF_IMPORT_SOURCE_ID.to_owned())));
    if let Some(v) = native.u_std {
        m.push(("u_std".to_owned(), Json::Str(f64_to_hex(v))));
    }
    if let Some(v) = native.expanded_u {
        m.push(("U".to_owned(), Json::Str(f64_to_hex(v))));
    }
    if let Some(v) = native.k {
        m.push(("k".to_owned(), Json::Str(f64_to_hex(v))));
    }
    if let Some(iv) = native.interval {
        m.push(("low".to_owned(), Json::Str(f64_to_hex(iv.low))));
        m.push(("high".to_owned(), Json::Str(f64_to_hex(iv.high))));
    }
    if let Some(v) = native.p {
        m.push(("p".to_owned(), Json::Str(f64_to_hex(v))));
    }
    if let Some(u) = &native.unit {
        m.push(("unit".to_owned(), Json::Str(u.clone())));
    }
    let bytes = json_to_canonical(&Json::Obj(m)).unwrap_or_default();
    sha256_digest(&bytes)
}

fn from_native_only(native: &NativeUncFields) -> Result<UncertaintyStatement, QifMapError> {
    let src_digest = native_import_digest(native);
    let input = AssignInput {
        basis: Basis::Declared,
        model_bound: false,
        instruments_bound: false,
        contributor_sources: vec![ContributorSource {
            input_id: QIF_IMPORT_SOURCE_ID.to_owned(),
            source: Some(SourceClaim { kind: SourceKind::Declared, digest: src_digest, origin: None }),
        }],
        validation_record: None,
    };
    let records =
        RecordTable::new(vec![(src_digest, RecordFacts::Source { kind: SourceKind::Declared, origin: None })])?;
    let assigned = assign(&input, &records)?;

    let expanded = match (native.expanded_u, native.k) {
        (Some(value), Some(k)) => Some(Expanded { value, k }),
        _ => None,
    };
    let interval = match (native.interval, native.p) {
        (Some(iv), Some(p)) => {
            Some(CoverageInterval { low: iv.low, high: iv.high, p, kind: IntervalKind::ProbSymmetric })
        }
        _ => None,
    };

    let mut parts = StatementParts::new(assigned.class);
    parts.method = Some(Method::Declared);
    parts.unit = native.unit.clone();
    parts.u_std = native.u_std;
    parts.expanded = expanded;
    parts.interval = interval;
    parts.model = Some(ModelRef {
        id: QIF_IMPORT_MODEL_ID.to_owned(),
        semver: "1.0.0".to_owned(),
        digest: sha256_digest(b"um:qif-import@1.0.0"),
    });
    parts.sources = vec![SourceRef {
        kind: SourceKind::Declared,
        id: QIF_IMPORT_SOURCE_ID.to_owned(),
        digest: src_digest,
        unverified: false,
    }];
    parts.provenance = Provenance {
        rule_sets: vec![RULE_SET.to_owned()],
        principal: Some(QIF_IMPORT_SOURCE_ID.to_owned()),
        principal_kind: Some(PrincipalKind::Human),
        proposal: false,
        adopted_by: None,
        supersedes: None,
    };

    let stmt = assigned.statement(parts)?;
    Ok(stmt)
}

/// Maps a QIF 3.0 fragment back to a validated [`UncertaintyStatement`].
///
/// - When `fragment.ext_xml` is non-empty, the extension is authoritative: parsed under strict
///   bounds, validated via [`UncertaintyStatement::parse_canonical`], and cross-checked against
///   `fragment.native` within 1 ulp (REQ-UNC.UEC.QIF-001, -005, -006).
/// - When `fragment.ext_xml` is empty, native fields are imported as UEC-1 `DECLARED` with
///   source `qif_import` through `UNC.UEC.CLASS` (REQ-UNC.UEC.QIF-004).
pub fn from_qif(fragment: &UncQifFragment) -> Result<UncertaintyStatement, QifMapError> {
    if !fragment.ext_xml.is_empty() {
        let root = parse_xml_root(&fragment.ext_xml)?;
        let canonical = xml_to_canonical_json(&root)?;
        let stmt = UncertaintyStatement::parse_canonical(&canonical)?;
        cross_check_native(&stmt, &fragment.native)?;
        Ok(stmt)
    } else {
        from_native_only(&fragment.native)
    }
}

/// Same as [`from_qif`], additionally verifying every UEC-2/3 source against `records` via
/// [`UncertaintyStatement::parse_canonical_verified`] when an extension element is present.
pub fn from_qif_verified(
    fragment: &UncQifFragment,
    records: &dyn SourceRecords,
) -> Result<UncertaintyStatement, QifMapError> {
    if !fragment.ext_xml.is_empty() {
        let root = parse_xml_root(&fragment.ext_xml)?;
        let canonical = xml_to_canonical_json(&root)?;
        let stmt = UncertaintyStatement::parse_canonical_verified(&canonical, records)?;
        cross_check_native(&stmt, &fragment.native)?;
        Ok(stmt)
    } else {
        from_native_only(&fragment.native)
    }
}
