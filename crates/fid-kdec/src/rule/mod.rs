//! Decision-rule value type, rule-ID grammar, outcomes and the built-in catalog. Spec node
//! `UNC.DEC.TYPE`.
//!
//! [`DecisionRule`] values come from two places only: [`DecisionRule::customer`] (and its
//! JSON form [`DecisionRule::customer_from_json`]) for customer rules, and the catalog
//! parser ([`Catalog::parse_canonical`], [`Catalog::builtin`]) for built-in rules. Both run
//! the checks below, so every rule in memory is valid and reviewable as data.
//!
//! # Validation order (pinned, so the first reported error is deterministic)
//!
//! 1. **id**: in the grammar (`invalid_rule_id`, or `invalid_rule{r}` for a negative
//!    guard-band factor); a customer rule has a `dr:customer/...` ID and a built-in rule
//!    has one of the four built-in template IDs (`invalid_rule{id}`).
//! 2. **version**: 1 ..= 999 999 999 (`invalid_rule{version}`).
//! 3. **kind**: known; never `label_only` for a customer rule; equal to the kind the ID
//!    implies for a built-in rule (`invalid_rule{kind}`).
//! 4. **params**: only the kind's own parameter (`invalid_rule{params}`); `p_min` present,
//!    canonical and strictly inside (0.5, 1); `r` canonical and >= 0, present in a customer
//!    rule and absent in the built-in template (`invalid_rule{p_min}`, `invalid_rule{r}`).
//! 5. **standards_ref**: `none` or `{designator, edition, clause}` in the closed grammars of
//!    [`StandardsRef::new`], so no standards text fits (`invalid_rule{standards_ref}`).
//! 6. **pdf_source**: known; `none` exactly for `label_only` (`invalid_rule{pdf_source}`).
//! 7. **risk_reporting**: known; `none` for `label_only` (`invalid_rule{risk_reporting}`).
//!
//! In the JSON forms, an unknown or missing member is `malformed{unknown_member}` or
//! `malformed{missing_member}` and comes first; a member of the wrong JSON type fails the
//! check of that member.
//!
//! Parameters are exact decimals ([`ParamDecimal`]); nothing here converts them to binary64
//! or compares any value against a limit.

mod catalog;
mod id;
pub mod json;
mod wire;

use std::fmt;

use fid_kci::kci::json::Json;
use fid_num::kdec::{cmp_dec, Decimal};

pub use catalog::{Catalog, BUILTIN_CATALOG_DIGEST, BUILTIN_CATALOG_JSON, CATALOG_TAG, MAX_CATALOG_BYTES, MAX_RULES};
pub use id::{ParamDecimal, RuleId, RuleRef, MAX_DECIMAL_DIGITS, MAX_REF_BYTES, MAX_SEGMENT_BYTES, MAX_VERSION};

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
    /// Kind of a decision rule (REQ-UNC.DEC.TYPE-003).
    RuleKind {
        /// Conforms when the conformance probability reaches `p_min`.
        ConformanceProbability => "conformance_probability",
        /// Acceptance zone equal to the tolerance zone.
        SimpleAcceptance => "simple_acceptance",
        /// Acceptance zone reduced by the guard band `w = r * U`.
        GuardBand => "guard_band",
        /// Carries a decision reported by a source; never a kernel decision.
        LabelOnly => "label_only",
    }
}

wire_enum! {
    /// Where the probability density for the conformance probability comes from
    /// (REQ-UNC.DEC-006).
    PdfSource {
        /// Monte Carlo samples when available, otherwise the normal approximation if
        /// `lpu_validated_by_mc` is set or the method selection permits it.
        McOrValidatedNormal => "mc_or_validated_normal",
        /// Monte Carlo samples only; otherwise `NOT_DECIDABLE(pdf_unavailable)`.
        McOnly => "mc_only",
        /// No density is used (`label_only` only).
        NotUsed => "none",
    }
}

wire_enum! {
    /// Which conformity risks a rule reports (`UNC.DEC.RISK`).
    RiskReporting {
        /// None (`label_only` only, or a customer choice).
        NotReported => "none",
        /// Specific consumer and producer risk at the result.
        Specific => "specific",
        /// Specific risk plus global risk under a declared prior.
        SpecificAndGlobal => "specific_and_global",
    }
}

wire_enum! {
    /// Why an evaluation returned `NOT_DECIDABLE` (REQ-UNC.DEC.TYPE-002).
    NotDecidableReason {
        /// The rule needs an uncertainty and the result is UEC-0.
        UMissing => "u_missing",
        /// No rule is bound.
        RuleMissing => "rule_missing",
        /// The rule needs a probability density that is not available.
        PdfUnavailable => "pdf_unavailable",
        /// The rule is label-only (`dr:as-reported-by-source`).
        LabelOnly => "label_only",
    }
}

/// The outcome of a rule evaluation (REQ-UNC.DEC.TYPE-002). `NOT_DECIDABLE` carries its
/// reason in the type, so it cannot be built without one:
///
/// ```compile_fail
/// let outcome: fid_kdec::Outcome = fid_kdec::Outcome::NotDecidable;
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Outcome {
    /// `CONFORMS`.
    Conforms,
    /// `NONCONFORMS`.
    Nonconforms,
    /// `INDETERMINATE`: the rule cannot prove conformity or nonconformity.
    Indeterminate,
    /// `NOT_DECIDABLE` with its reason.
    NotDecidable(NotDecidableReason),
}

impl Outcome {
    /// The four wire names, in declaration order.
    pub const WIRE: [&'static str; 4] = ["CONFORMS", "NONCONFORMS", "INDETERMINATE", "NOT_DECIDABLE"];

    /// The wire name (without the reason).
    pub const fn wire(self) -> &'static str {
        match self {
            Outcome::Conforms => "CONFORMS",
            Outcome::Nonconforms => "NONCONFORMS",
            Outcome::Indeterminate => "INDETERMINATE",
            Outcome::NotDecidable(_) => "NOT_DECIDABLE",
        }
    }

    /// The reason of a `NOT_DECIDABLE` outcome.
    pub const fn reason(self) -> Option<NotDecidableReason> {
        match self {
            Outcome::NotDecidable(r) => Some(r),
            _ => None,
        }
    }

    /// True for `CONFORMS` and `NONCONFORMS`, the only outcomes that state a side.
    pub const fn is_decided(self) -> bool {
        matches!(self, Outcome::Conforms | Outcome::Nonconforms)
    }

    /// The parse boundary: an outcome name and an optional reason. Order: unknown name
    /// (`unknown_outcome`), `NOT_DECIDABLE` without a reason (`reason_required`), unknown
    /// reason (`unknown_reason`), a reason on any other outcome (`unexpected_reason`).
    pub fn from_wire(outcome: &str, reason: Option<&str>) -> Result<Outcome, DecError> {
        let base = match outcome {
            "CONFORMS" => Outcome::Conforms,
            "NONCONFORMS" => Outcome::Nonconforms,
            "INDETERMINATE" => Outcome::Indeterminate,
            "NOT_DECIDABLE" => {
                let reason = reason.ok_or(DecError::ReasonRequired)?;
                let reason = NotDecidableReason::from_wire(reason).ok_or(DecError::UnknownReason)?;
                return Ok(Outcome::NotDecidable(reason));
            }
            _ => return Err(DecError::UnknownOutcome),
        };
        match reason {
            Some(_) => Err(DecError::UnexpectedReason),
            None => Ok(base),
        }
    }
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Outcome::NotDecidable(r) => write!(f, "NOT_DECIDABLE({r})"),
            other => f.write_str(other.wire()),
        }
    }
}

/// Why a rule, catalog, outcome or decision was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecError {
    /// The string is not in the rule-ID grammar.
    InvalidRuleId,
    /// A rule field or parameter is invalid; `param` names it.
    InvalidRule { param: &'static str },
    /// Two catalog rules share an (ID, version) pair.
    DuplicateRule,
    /// `NOT_DECIDABLE` without a reason.
    ReasonRequired,
    /// A reason on an outcome other than `NOT_DECIDABLE`.
    UnexpectedReason,
    /// An outcome name that is not one of the four.
    UnknownOutcome,
    /// A reason that is not one of the four.
    UnknownReason,
    /// A number is NaN, infinite or subnormal, or does not convert exactly enough.
    InvalidNumber { field: &'static str },
    /// The outcome does not fit the rule or the evidence basis; `field` names the check.
    OutcomeMismatch { field: &'static str },
    /// The reference names no rule of the catalog.
    NotInCatalog,
    /// The input is not a well-formed document (detail is a fixed code).
    Malformed { detail: &'static str },
    /// A valid catalog, but not its canonical bytes.
    NonCanonical,
}

impl DecError {
    /// Stable error code.
    pub const fn code(self) -> &'static str {
        match self {
            DecError::InvalidRuleId => "invalid_rule_id",
            DecError::InvalidRule { .. } => "invalid_rule",
            DecError::DuplicateRule => "duplicate_rule",
            DecError::ReasonRequired => "reason_required",
            DecError::UnexpectedReason => "unexpected_reason",
            DecError::UnknownOutcome => "unknown_outcome",
            DecError::UnknownReason => "unknown_reason",
            DecError::InvalidNumber { .. } => "invalid_number",
            DecError::OutcomeMismatch { .. } => "outcome_mismatch",
            DecError::NotInCatalog => "not_in_catalog",
            DecError::Malformed { .. } => "malformed",
            DecError::NonCanonical => "non_canonical",
        }
    }

    /// The named parameter, field or detail code, if any.
    pub const fn detail(self) -> Option<&'static str> {
        match self {
            DecError::InvalidRule { param: x }
            | DecError::InvalidNumber { field: x }
            | DecError::OutcomeMismatch { field: x }
            | DecError::Malformed { detail: x } => Some(x),
            _ => None,
        }
    }
}

impl fmt::Display for DecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.detail() {
            Some(x) => write!(f, "{}: {x}", self.code()),
            None => f.write_str(self.code()),
        }
    }
}

impl std::error::Error for DecError {}

/// A rule's parameters. The kind is the variant, so a kind never carries another kind's
/// parameters.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RuleParams {
    /// Minimum conformance probability, strictly inside (0.5, 1).
    ConformanceProbability { p_min: ParamDecimal },
    /// No parameters.
    SimpleAcceptance,
    /// Guard-band factor `r >= 0` (`w = r * U`); `None` only in the catalog template.
    GuardBand { r: Option<ParamDecimal> },
    /// No parameters.
    LabelOnly,
}

impl RuleParams {
    /// The kind.
    pub fn kind(&self) -> RuleKind {
        match self {
            RuleParams::ConformanceProbability { .. } => RuleKind::ConformanceProbability,
            RuleParams::SimpleAcceptance => RuleKind::SimpleAcceptance,
            RuleParams::GuardBand { .. } => RuleKind::GuardBand,
            RuleParams::LabelOnly => RuleKind::LabelOnly,
        }
    }
}

/// Longest designator, in bytes.
pub const MAX_DESIGNATOR_BYTES: usize = 32;

/// A standards reference: designator, edition and clause only (REQ-UNC.DEC.TYPE-005).
/// The grammars admit no prose: designators are upper-case, editions are years, clauses
/// are dotted numbers.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StandardsRef {
    designator: String,
    edition: String,
    clause: String,
}

impl StandardsRef {
    /// Checks and builds a reference. `designator`: 2 ..= 32 bytes of `A-Z 0-9 . / -` and
    /// single inner spaces, starting with a letter (`ISO 14253-1`, `JCGM 106`);
    /// `edition`: four digits; `clause`: 1 ..= 5 dot-separated parts, the first `1`..`999`
    /// or one capital letter (an annex), the others `0`..`999` without leading zeros.
    /// Fails with `invalid_rule{standards_ref}`.
    pub fn new(designator: &str, edition: &str, clause: &str) -> Result<StandardsRef, DecError> {
        if is_designator(designator) && is_edition(edition) && is_clause(clause) {
            Ok(StandardsRef {
                designator: designator.to_owned(),
                edition: edition.to_owned(),
                clause: clause.to_owned(),
            })
        } else {
            Err(DecError::InvalidRule { param: "standards_ref" })
        }
    }
    /// The designator, for example `ISO 14253-1`.
    pub fn designator(&self) -> &str {
        &self.designator
    }
    /// The edition year.
    pub fn edition(&self) -> &str {
        &self.edition
    }
    /// The clause number.
    pub fn clause(&self) -> &str {
        &self.clause
    }
}

fn is_designator(s: &str) -> bool {
    let b = s.as_bytes();
    (2..=MAX_DESIGNATOR_BYTES).contains(&b.len())
        && b.first().is_some_and(u8::is_ascii_uppercase)
        && b.last() != Some(&b' ')
        && !s.contains("  ")
        && b.iter().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || b" ./-".contains(c))
}

fn is_edition(s: &str) -> bool {
    s.len() == 4 && s.bytes().all(|c| c.is_ascii_digit())
}

fn is_number(p: &str, zero_ok: bool) -> bool {
    let b = p.as_bytes();
    (1..=3).contains(&b.len())
        && b.iter().all(u8::is_ascii_digit)
        && (b.first() != Some(&b'0') || (zero_ok && b.len() == 1))
}

fn is_clause(s: &str) -> bool {
    if s.len() > 32 {
        return false;
    }
    let mut parts = s.split('.');
    let first_ok = match parts.next() {
        Some(p) => is_number(p, false) || (p.len() == 1 && p.bytes().all(|c| c.is_ascii_uppercase())),
        None => false,
    };
    let mut count = 1;
    for p in parts {
        count += 1;
        if count > 5 || !is_number(p, true) {
            return false;
        }
    }
    first_ok
}

/// Every field of a rule, as given to [`DecisionRule::customer`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RuleParts {
    pub id: RuleId,
    pub version: u32,
    /// `None` for contractual rules (wire `"none"`).
    pub standards_ref: Option<StandardsRef>,
    /// Kind and parameters.
    pub params: RuleParams,
    pub pdf_source: PdfSource,
    pub risk_reporting: RiskReporting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Origin {
    Builtin,
    Customer,
}

/// A validated decision rule: a reviewable data object with an ID and version
/// (REQ-UNC.DEC-001, REQ-UNC.DEC.TYPE-001).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DecisionRule {
    parts: RuleParts,
}

impl DecisionRule {
    /// Validates a customer rule (see the module documentation for the order).
    pub fn customer(parts: RuleParts) -> Result<DecisionRule, DecError> {
        validate(&parts, Origin::Customer)?;
        Ok(DecisionRule { parts })
    }

    /// Reads and validates a customer rule from its JSON object.
    pub fn customer_from_json(v: &Json) -> Result<DecisionRule, DecError> {
        wire::rule_from_json(v, Origin::Customer)
    }

    pub(crate) fn validated(parts: RuleParts, origin: Origin) -> Result<DecisionRule, DecError> {
        validate(&parts, origin)?;
        Ok(DecisionRule { parts })
    }

    /// A guard-band instance made by [`Catalog::resolve`] from a validated template and a
    /// grammar-checked, non-negative `r`.
    pub(crate) fn from_validated_instance(parts: RuleParts) -> DecisionRule {
        DecisionRule { parts }
    }

    /// The rule's canonical RFC 8785 JSON object (members `id`, `version`, `kind`, `params`,
    /// `standards_ref`, `pdf_source`, `risk_reporting`; every scalar a string).
    pub fn canonical_json(&self) -> Result<Vec<u8>, DecError> {
        fid_kci::kci::json::to_canonical(&wire::rule_to_json(&self.parts))
            .map_err(|_| DecError::Malformed { detail: "writer" })
    }

    /// The ID.
    pub fn id(&self) -> &RuleId {
        &self.parts.id
    }
    /// The version.
    pub fn version(&self) -> u32 {
        self.parts.version
    }
    /// `id@version`.
    pub fn rule_ref(&self) -> RuleRef {
        RuleRef { id: self.parts.id.clone(), version: self.parts.version }
    }
    /// The standards reference; `None` for contractual rules.
    pub fn standards_ref(&self) -> Option<&StandardsRef> {
        self.parts.standards_ref.as_ref()
    }
    /// The kind.
    pub fn kind(&self) -> RuleKind {
        self.parts.params.kind()
    }
    /// The parameters.
    pub fn params(&self) -> &RuleParams {
        &self.parts.params
    }
    /// The probability-density source.
    pub fn pdf_source(&self) -> PdfSource {
        self.parts.pdf_source
    }
    /// The risk reporting.
    pub fn risk_reporting(&self) -> RiskReporting {
        self.parts.risk_reporting
    }
    /// A copy of every field.
    pub fn to_parts(&self) -> RuleParts {
        self.parts.clone()
    }
}

fn invalid(param: &'static str) -> DecError {
    DecError::InvalidRule { param }
}

pub(crate) fn check_id(id: &RuleId, origin: Origin) -> Result<(), DecError> {
    id.check()?;
    let ok = match origin {
        Origin::Customer => id.is_customer(),
        Origin::Builtin => !id.is_customer() && !matches!(id, RuleId::GuardBand { r: Some(_) }),
    };
    if ok {
        Ok(())
    } else {
        Err(invalid("id"))
    }
}

pub(crate) fn check_version(version: u32) -> Result<(), DecError> {
    if version == 0 || version > MAX_VERSION {
        return Err(invalid("version"));
    }
    Ok(())
}

pub(crate) fn check_kind(kind: RuleKind, id: &RuleId, origin: Origin) -> Result<(), DecError> {
    let ok = match origin {
        Origin::Customer => kind != RuleKind::LabelOnly,
        Origin::Builtin => id.kind() == Some(kind),
    };
    if ok {
        Ok(())
    } else {
        Err(invalid("kind"))
    }
}

pub(crate) fn check_params(params: &RuleParams, origin: Origin) -> Result<(), DecError> {
    match params {
        RuleParams::ConformanceProbability { p_min } => {
            let half = Decimal::new(false, 5, -1).map_err(|_| invalid("p_min"))?;
            let one = Decimal::new(false, 1, 0).map_err(|_| invalid("p_min"))?;
            let v = p_min.value();
            let inside = cmp_dec(&v, &half).is_gt() && cmp_dec(&v, &one).is_lt();
            if !inside {
                return Err(invalid("p_min"));
            }
        }
        RuleParams::GuardBand { r } => match (r, origin) {
            (None, Origin::Customer) | (Some(_), Origin::Builtin) => return Err(invalid("r")),
            (Some(r), Origin::Customer) => {
                if r.is_negative() {
                    return Err(invalid("r"));
                }
            }
            (None, Origin::Builtin) => {}
        },
        RuleParams::SimpleAcceptance | RuleParams::LabelOnly => {}
    }
    Ok(())
}

pub(crate) fn check_pdf(kind: RuleKind, pdf: PdfSource) -> Result<(), DecError> {
    if (kind == RuleKind::LabelOnly) != (pdf == PdfSource::NotUsed) {
        return Err(invalid("pdf_source"));
    }
    Ok(())
}

pub(crate) fn check_risk(kind: RuleKind, risk: RiskReporting) -> Result<(), DecError> {
    if kind == RuleKind::LabelOnly && risk != RiskReporting::NotReported {
        return Err(invalid("risk_reporting"));
    }
    Ok(())
}

fn validate(p: &RuleParts, origin: Origin) -> Result<(), DecError> {
    check_id(&p.id, origin)?;
    check_version(p.version)?;
    let kind = p.params.kind();
    check_kind(kind, &p.id, origin)?;
    check_params(&p.params, origin)?;
    if let Some(s) = &p.standards_ref {
        // Fields are private and set only by `StandardsRef::new`; re-checked for clarity.
        StandardsRef::new(&s.designator, &s.edition, &s.clause)?;
    }
    check_pdf(kind, p.pdf_source)?;
    check_risk(kind, p.risk_reporting)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standards_grammars() {
        for ok in ["ISO 14253-1", "JCGM 106", "ASME B89.7.3.1", "ISO/TS 15530-4", "VDA 5"] {
            assert!(is_designator(ok), "{ok}");
        }
        let long = "A".repeat(33);
        for bad in ["", "I", "iso 14253-1", "ISO  14253", "ISO ", " ISO", "1SO", "ISO_1", long.as_str()] {
            assert!(!is_designator(bad), "{bad:?}");
        }
        assert!(is_edition("2017") && !is_edition("17") && !is_edition("2017a"));
        for ok in ["5", "8.2", "8.3.2", "A", "A.1", "1.0", "999.999.999.999.999"] {
            assert!(is_clause(ok), "{ok}");
        }
        for bad in ["", "0", "05", "5.", ".5", "5..1", "1.01", "AB", "a", "1.2.3.4.5.6", "1000", "5 a"] {
            assert!(!is_clause(bad), "{bad:?}");
        }
    }

    #[test]
    fn outcome_wire_round_trips() {
        for o in [Outcome::Conforms, Outcome::Nonconforms, Outcome::Indeterminate] {
            assert_eq!(Outcome::from_wire(o.wire(), None), Ok(o));
        }
        for r in NotDecidableReason::ALL {
            let o = Outcome::NotDecidable(*r);
            assert_eq!(Outcome::from_wire(o.wire(), o.reason().map(NotDecidableReason::wire)), Ok(o));
        }
    }
}
