//! Rule-ID grammar (REQ-UNC.DEC.TYPE-001) and the exact parameter decimals.
//!
//! Hand-written, no regular-expression engine (kernel). Every accepted string is in its
//! one canonical form, so `parse(s).to_string() == s` for every accepted `s` and two
//! different strings never name the same rule. The grammar (pinned in NODE.md Design):
//!
//! ```text
//! ref      = id "@" version                       ; at most 256 bytes in all
//! version  = [1-9] [0-9]{0,8}                     ; 1 ..= 999 999 999
//! id       = "dr:iso14253-1:2017/default"
//!          | "dr:simple-acceptance"
//!          | "dr:guard-band"                      ; the catalog template, r unbound
//!          | "dr:guard-band(r=" decimal ")"       ; an instance, r >= 0
//!          | "dr:customer/" segment "/" segment
//!          | "dr:as-reported-by-source"
//! segment  = [a-z0-9] [a-z0-9_-]{0,63}
//! decimal  = ["-"] ("0" | [1-9][0-9]{0,19}) ["." [0-9]{0,19} [1-9]]   ; not "-0"
//! ```
//!
//! A negative guard-band factor matches the grammar but is refused with
//! `invalid_rule{r}`; everything else that does not match is `invalid_rule_id`. The version
//! is checked before the ID.

use std::fmt;

use fid_num::kdec::Decimal;

use super::{DecError, RuleKind};

/// Longest rule reference (`id@version`) or rule ID, in bytes; checked before anything else.
pub const MAX_REF_BYTES: usize = 256;
/// Longest tenant or customer-rule segment, in bytes.
pub const MAX_SEGMENT_BYTES: usize = 64;
/// Most digits before, and most digits after, the decimal point of a parameter.
pub const MAX_DECIMAL_DIGITS: usize = 20;
/// Largest rule version.
pub const MAX_VERSION: u32 = 999_999_999;

const ISO: &str = "dr:iso14253-1:2017/default";
const SIMPLE: &str = "dr:simple-acceptance";
const GUARD: &str = "dr:guard-band";
const GUARD_OPEN: &str = "dr:guard-band(r=";
const CUSTOMER: &str = "dr:customer/";
const AS_REPORTED: &str = "dr:as-reported-by-source";

const SIGN: u64 = 1 << 63;

/// An exact decimal parameter (`p_min`, `r`) kept in its canonical text and as an exact
/// [`Decimal`]. Comparisons use the exact value; [`ParamDecimal::to_f64`] is the one
/// conversion to binary64, done when a rule is evaluated.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ParamDecimal {
    text: String,
    value: Decimal,
}

impl ParamDecimal {
    /// Parses the canonical decimal grammar (see the module documentation). `None` for
    /// anything else: exponents, `+`, leading zeros, trailing fractional zeros, `-0`.
    pub fn parse(s: &str) -> Option<ParamDecimal> {
        if !is_canonical_decimal(s) {
            return None;
        }
        let value = Decimal::parse(s).ok()?;
        Some(ParamDecimal { text: s.to_owned(), value })
    }

    /// The canonical text.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// The exact value.
    pub fn value(&self) -> Decimal {
        self.value
    }

    /// True for a negative value (never zero: `-0` is not canonical).
    pub fn is_negative(&self) -> bool {
        self.value.is_negative()
    }

    /// The correctly rounded binary64 value. Refuses, with `invalid_number{field}`, a
    /// result that is infinite, NaN, subnormal, or zero for a non-zero decimal, so a
    /// parameter never silently becomes a different number. Unreachable within the
    /// grammar bounds (|x| lies in 1e-20 ..= 1e20), and checked anyway.
    pub fn to_f64(&self, field: &'static str) -> Result<f64, DecError> {
        let bad = DecError::InvalidNumber { field };
        let x = self.value.to_f64().map_err(|_| bad)?;
        let underflow = x.to_bits() & !SIGN == 0 && !self.value.is_zero();
        if !x.is_finite() || x.is_subnormal() || underflow {
            return Err(bad);
        }
        Ok(x)
    }
}

impl fmt::Display for ParamDecimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

fn is_canonical_decimal(s: &str) -> bool {
    let b = s.as_bytes();
    let (neg, rest) = match b.split_first() {
        Some((b'-', r)) => (true, r),
        _ => (false, b),
    };
    let int_len = rest.iter().take_while(|c| c.is_ascii_digit()).count();
    let (int, tail) = rest.split_at(int_len);
    let int_ok = match int {
        [] => false,
        [b'0'] => true,
        [first, ..] => *first != b'0' && int.len() <= MAX_DECIMAL_DIGITS,
    };
    let frac_ok = match tail {
        [] => true,
        [b'.', frac @ ..] => {
            !frac.is_empty()
                && frac.len() <= MAX_DECIMAL_DIGITS
                && frac.iter().all(u8::is_ascii_digit)
                && frac.last() != Some(&b'0')
        }
        _ => false,
    };
    int_ok && frac_ok && !(neg && int == b"0" && tail.is_empty())
}

fn is_segment(s: &str) -> bool {
    let b = s.as_bytes();
    match b.split_first() {
        Some((first, rest)) => {
            b.len() <= MAX_SEGMENT_BYTES
                && (first.is_ascii_lowercase() || first.is_ascii_digit())
                && rest.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_' || *c == b'-')
        }
        None => false,
    }
}

/// `[1-9][0-9]{0,8}` as a `u32`.
pub(crate) fn parse_version(s: &str) -> Option<u32> {
    let b = s.as_bytes();
    let ok = !b.is_empty() && b.len() <= 9 && b.iter().all(u8::is_ascii_digit) && b.first() != Some(&b'0');
    if ok {
        s.parse().ok()
    } else {
        None
    }
}

/// A decision-rule ID (without its version).
///
/// The variants are open for matching; any value built by hand is re-checked against the
/// grammar by every constructor that accepts one ([`super::DecisionRule::customer`],
/// [`super::Catalog::resolve`]), so a hand-built ID outside the grammar is refused there.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RuleId {
    /// `dr:iso14253-1:2017/default`.
    Iso14253Default,
    /// `dr:simple-acceptance`.
    SimpleAcceptance,
    /// `dr:guard-band` (template, `r: None`) or `dr:guard-band(r=<decimal>)`.
    GuardBand { r: Option<ParamDecimal> },
    /// `dr:customer/<tenant>/<name>`.
    Customer { tenant: String, name: String },
    /// `dr:as-reported-by-source`.
    AsReportedBySource,
}

impl RuleId {
    /// Parses a rule ID (no `@version`). Errors: `invalid_rule_id`, or `invalid_rule{r}`
    /// for a negative guard-band factor.
    pub fn parse(s: &str) -> Result<RuleId, DecError> {
        if s.len() > MAX_REF_BYTES {
            return Err(DecError::InvalidRuleId);
        }
        match s {
            ISO => return Ok(RuleId::Iso14253Default),
            SIMPLE => return Ok(RuleId::SimpleAcceptance),
            GUARD => return Ok(RuleId::GuardBand { r: None }),
            AS_REPORTED => return Ok(RuleId::AsReportedBySource),
            _ => {}
        }
        if let Some(inner) = s.strip_prefix(GUARD_OPEN).and_then(|t| t.strip_suffix(')')) {
            let r = ParamDecimal::parse(inner).ok_or(DecError::InvalidRuleId)?;
            if r.is_negative() {
                return Err(DecError::InvalidRule { param: "r" });
            }
            return Ok(RuleId::GuardBand { r: Some(r) });
        }
        if let Some((tenant, name)) = s.strip_prefix(CUSTOMER).and_then(|t| t.split_once('/')) {
            if is_segment(tenant) && is_segment(name) {
                return Ok(RuleId::Customer { tenant: tenant.to_owned(), name: name.to_owned() });
            }
        }
        Err(DecError::InvalidRuleId)
    }

    /// The kind the ID implies; `None` for a customer rule, whose kind is in the rule.
    pub fn kind(&self) -> Option<RuleKind> {
        match self {
            RuleId::Iso14253Default => Some(RuleKind::ConformanceProbability),
            RuleId::SimpleAcceptance => Some(RuleKind::SimpleAcceptance),
            RuleId::GuardBand { .. } => Some(RuleKind::GuardBand),
            RuleId::AsReportedBySource => Some(RuleKind::LabelOnly),
            RuleId::Customer { .. } => None,
        }
    }

    /// True for `dr:customer/...`.
    pub fn is_customer(&self) -> bool {
        matches!(self, RuleId::Customer { .. })
    }

    /// Re-checks a value that may have been built by hand: it must be exactly what
    /// [`RuleId::parse`] gives for its own text.
    pub(crate) fn check(&self) -> Result<(), DecError> {
        let text = self.to_string();
        match RuleId::parse(&text) {
            Ok(back) if back == *self => Ok(()),
            Ok(_) => Err(DecError::InvalidRuleId),
            Err(e) => Err(e),
        }
    }
}

impl fmt::Display for RuleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuleId::Iso14253Default => f.write_str(ISO),
            RuleId::SimpleAcceptance => f.write_str(SIMPLE),
            RuleId::GuardBand { r: None } => f.write_str(GUARD),
            RuleId::GuardBand { r: Some(r) } => write!(f, "{GUARD_OPEN}{r})"),
            RuleId::Customer { tenant, name } => write!(f, "{CUSTOMER}{tenant}/{name}"),
            RuleId::AsReportedBySource => f.write_str(AS_REPORTED),
        }
    }
}

/// A rule reference `id@version`, as results carry it (REQ-UNC.DEC-001).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RuleRef {
    /// The rule ID.
    pub id: RuleId,
    /// The rule version, 1 ..= [`MAX_VERSION`].
    pub version: u32,
}

impl RuleRef {
    /// Parses `id@version`. The length is checked first, then the version (after the last
    /// `@`), then the ID. Errors: `invalid_rule_id`, or `invalid_rule{r}` (see [`RuleId::parse`]).
    pub fn parse(s: &str) -> Result<RuleRef, DecError> {
        if s.len() > MAX_REF_BYTES {
            return Err(DecError::InvalidRuleId);
        }
        let (id, version) = s.rsplit_once('@').ok_or(DecError::InvalidRuleId)?;
        let version = parse_version(version).ok_or(DecError::InvalidRuleId)?;
        let id = RuleId::parse(id)?;
        Ok(RuleRef { id, version })
    }

    /// Re-checks a value that may have been built by hand.
    pub(crate) fn check(&self) -> Result<(), DecError> {
        if self.version == 0 || self.version > MAX_VERSION {
            return Err(DecError::InvalidRuleId);
        }
        self.id.check()
    }
}

impl fmt::Display for RuleRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.id, self.version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_grammar() {
        for ok in ["0", "1", "-1", "0.5", "-0.5", "10.01", "12345678901234567890", "0.00000000000000000001"] {
            assert!(is_canonical_decimal(ok), "{ok}");
        }
        for bad in [
            "",
            "-",
            "-0",
            "00",
            "01",
            ".5",
            "1.",
            "0.50",
            "0.0",
            "+1",
            "1e3",
            " 1",
            "1 ",
            "--1",
            "1.2.3",
            "123456789012345678901",
            "0.000000000000000000001",
            "\u{661}",
        ] {
            assert!(!is_canonical_decimal(bad), "{bad:?}");
        }
    }

    #[test]
    fn to_f64_refuses_nothing_in_range_and_is_exactly_rounded() {
        let p = ParamDecimal::parse("0.95").unwrap_or_else(|| unreachable!());
        assert_eq!(p.to_f64("p_min"), Ok(0.95));
        let tiny = ParamDecimal::parse("0.00000000000000000001").unwrap_or_else(|| unreachable!());
        assert_eq!(tiny.to_f64("r"), Ok(1e-20));
        let zero = ParamDecimal::parse("0").unwrap_or_else(|| unreachable!());
        assert_eq!(zero.to_f64("r").map(f64::to_bits), Ok(0));
    }

    #[test]
    fn segments_and_versions() {
        assert!(is_segment("a") && is_segment("0-_") && is_segment(&"a".repeat(64)));
        assert!(!is_segment("") && !is_segment("-a") && !is_segment("_a") && !is_segment("A") && !is_segment("a.b"));
        assert!(!is_segment(&"a".repeat(65)));
        assert_eq!(parse_version("1"), Some(1));
        assert_eq!(parse_version("999999999"), Some(MAX_VERSION));
        for bad in ["", "0", "01", "1000000000", "+1", "-1", "1 "] {
            assert_eq!(parse_version(bad), None, "{bad:?}");
        }
    }
}
