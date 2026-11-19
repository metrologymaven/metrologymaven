//! The formatter registry, `fmt::format` dispatch and `fmt.hexbits@1`. Spec node
//! `KRN.NUM.FMT.REG`.
//!
//! # Registry
//!
//! [`registry`] lists every formatter `fmt.<kind>@<major>` as a [`FormatterDescriptor`],
//! sorted by `(kind, major)` (byte order of the kind, then numeric major), with no two
//! entries sharing an ID (REQ-KRN.NUM.FMT.REG-001). The list is static data: no lookup
//! allocates, and every lookup is a scan of at most [`registry`]`().len()` entries.
//!
//! # Dispatch and version identity
//!
//! [`format`] first looks up the registry descriptor of `spec.formatter` ([`descriptor`]),
//! so an ID with no registry entry fails before any implementation runs, then dispatches on
//! the descriptor's exact `(kind, major)` to `fmt::len`
//! (`fmt.length@1`, `fmt.scalar@1`), `fmt::ang` (`fmt.angle@1`) or [`hexbits`]
//! (`fmt.hexbits@1`) and returns their [`Reported`] record unchanged: `value`, `side`,
//! `digits_added`, `tie_at_max_precision` and `exact_conversion` (with `pi_enclosure`) are
//! the implementation's, so a consumer such as `RPT.MODEL.FMT` takes `side` from the
//! formatter and never from the printed string. Any other ID, including another major of a
//! known kind, fails with [`NumError::UnknownFormatter`] naming the ID; no other version is
//! ever substituted (REQ-KRN.NUM.FMT.REG-002).
//!
//! [`lookup`] resolves a textual ID. It accepts exactly `fmt.<kind>@<major>` with `kind`
//! 1 to 16 bytes of `[a-z][a-z0-9_]*` and `major` a decimal in `1..=65535` without leading
//! zeros, at most [`MAX_ID_LEN`] bytes. A text without `@` (for example `fmt.length`) is
//! [`LookupError::Ambiguous`]: it names no version and is refused rather than resolved to
//! some major. Any other deviation (case, whitespace, `@01`, `@0`, `@`, a missing `fmt.`)
//! is [`LookupError::Malformed`]; a well-formed ID not in the registry is
//! [`LookupError::Unknown`].
//!
//! # `fmt.hexbits@1`
//!
//! [`hexbits`] renders a finite `f64` as `0x` followed by the 16 lowercase hex digits of
//! its IEEE-754 bits, most significant first, so `-0.0` gives `0x8000000000000000` and
//! subnormals keep their bits. This is the bit-significant f64 encoding of `CORE.JRN.CANON`
//! (REQ-CORE.JRN.CANON-003) byte for byte. NaN and infinities fail with `non_finite`.
//!
//! Through [`format`] the formatter is pinned as follows; errors are checked in this order:
//!
//! 1. `unit_not_supported` unless the unit is `m`, `1` or `rad`: the SI input units of the
//!    value formatters. The bits are those of the SI value as given, so the conversion is
//!    the identity `unit -> unit`, factor `1`, no pi enclosure.
//! 2. `precision_out_of_range` unless the precision is `Decimals(0)` (the bits have no
//!    decimal digits; precision bounds `(0, 0, 0)`).
//! 3. `limits_inverted` when both limits are given and `lower > upper`.
//! 4. `non_finite` for NaN and infinities.
//!
//! The side is decided exactly with `kdec::cmp`, like `fmt::len`: `above` when `x > upper`,
//! else `below` when `x < lower`, else `within`, and `none` when no limit value is given.
//! The bits are the exact value, so the text is always strictly beyond a violated limit;
//! `digits_added` is 0 and `tie_at_max_precision` false.
//!
//! # Version guard
//!
//! Each descriptor's [`FormatterDescriptor::corpus_digest`] is the SHA-256 of the formatter's
//! outputs over its frozen golden corpus, as written by the independent oracle
//! `fixtures/num/fmt/make_reg_fixtures.py` (its documentation pins the serialisation).
//! The test `tests/fmt_reg.rs` recomputes it from [`format`] on every CI target and fails,
//! naming the formatter, when it differs: a change to any output string needs a new major
//! with its own corpus and digest, and the old major stays listed (REQ-KRN.NUM.FMT.REG-003).

use core::cmp::Ordering;
use core::fmt;

use super::types::{Conversion, FmtError, FormatSpec, FormatterId, Precision, Reported, SideOutcome, Unit};
use super::{ang, len};
use crate::err::NumError;
use crate::kdec;

/// Longest textual formatter ID [`lookup`] accepts: `fmt.` + 16-byte kind + `@` + 5 digits.
pub const MAX_ID_LEN: usize = 26;
/// Longest formatter kind.
pub const MAX_KIND_LEN: usize = 16;

const HEX_OP: &str = "fmt.hexbits@1";
const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

/// One registered formatter (REQ-KRN.NUM.FMT.REG-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatterDescriptor {
    /// The formatter ID `fmt.<kind>@<major>`.
    pub id: FormatterId,
    /// The display units the formatter accepts, in [`Unit`] declaration order.
    pub units: &'static [Unit],
    /// `(max_decimals, min_sig_digits, max_sig_digits)`: `Decimals(0..=max_decimals)` and
    /// `SigDigits(min..=max)` are accepted; `(_, 0, 0)` means significant digits are not.
    /// The bounds hold over all units; a unit may accept less (`fmt.angle@1` in `dms`
    /// accepts no significant digits).
    pub precision_bounds: (u8, u8, u8),
    /// The frozen golden corpus, as a repository path; the formatter's cases are the lines
    /// whose formatter column equals its kind.
    pub corpus: &'static str,
    /// `sha256:` plus 64 lowercase hex digits: the digest of the formatter's outputs over
    /// its corpus (see the module documentation).
    pub corpus_digest: &'static str,
}

impl FormatterDescriptor {
    /// The formatter kind, for example `length`.
    pub const fn kind(&self) -> &'static str {
        self.id.kind
    }

    /// The major version.
    pub const fn major(&self) -> u16 {
        self.id.major
    }
}

static REGISTRY: [FormatterDescriptor; 4] = [
    FormatterDescriptor {
        id: FormatterId::ANGLE_V1,
        units: &[Unit::Rad, Unit::Deg, Unit::Dms],
        precision_bounds: (20, 1, 17),
        corpus: "fixtures/num/fmt/ang-golden-v1.txt",
        corpus_digest: "sha256:e88c6e7c284b288ae1015074d0e424dd90d48c4417798d405943668c0c015327",
    },
    FormatterDescriptor {
        id: FormatterId::HEXBITS_V1,
        units: &[Unit::M, Unit::One, Unit::Rad],
        precision_bounds: (0, 0, 0),
        corpus: "fixtures/num/fmt/hexbits-golden-v1.txt",
        corpus_digest: "sha256:913c8514186b91bcf9936709ca075fc54f98555e2ff0d83790a57f4124c34e1c",
    },
    FormatterDescriptor {
        id: FormatterId::LENGTH_V1,
        units: &[Unit::M, Unit::Mm, Unit::Um, Unit::In],
        precision_bounds: (20, 1, 17),
        corpus: "fixtures/num/fmt/len-golden-v1.txt",
        corpus_digest: "sha256:355fc715d6fd919202934d4fe121427068d0fdcb26f5ee25b476c570cb06fb1e",
    },
    FormatterDescriptor {
        id: FormatterId::SCALAR_V1,
        units: &[Unit::One],
        precision_bounds: (20, 1, 17),
        corpus: "fixtures/num/fmt/len-golden-v1.txt",
        corpus_digest: "sha256:1b5abca58e9fc8387d313e8e80383d63fea2387178830fff01d97e5d6b40f452",
    },
];

/// Every registered formatter, sorted by `(kind, major)`, IDs unique
/// (REQ-KRN.NUM.FMT.REG-001). `KRN.MAN.MANI` reads it at build time.
pub fn registry() -> &'static [FormatterDescriptor] {
    &REGISTRY
}

/// The descriptor of exactly `id`, or [`NumError::UnknownFormatter`] naming it.
pub fn descriptor(id: FormatterId) -> Result<&'static FormatterDescriptor, NumError> {
    REGISTRY.iter().find(|d| d.id == id).ok_or(NumError::UnknownFormatter { id })
}

/// Formats `x` with the formatter `spec.formatter` (REQ-KRN.NUM.FMT.REG-002): the registry
/// descriptor is looked up first, so an ID without a registry entry is
/// [`NumError::UnknownFormatter`] even if a dispatch arm matched it; then dispatch on the
/// exact `(kind, major)`; the implementation's [`Reported`] is returned unchanged.
pub fn format(x: f64, spec: &FormatSpec) -> Result<Reported, NumError> {
    let id = descriptor(spec.formatter)?.id;
    match (id.kind, id.major) {
        ("length", 1) | ("scalar", 1) => len::format(x, spec),
        ("angle", 1) => ang::format(x, spec),
        ("hexbits", 1) => hex_format(x, spec),
        _ => Err(NumError::UnknownFormatter { id }),
    }
}

/// `fmt.hexbits@1`: `0x` plus the 16 lowercase hex digits of the IEEE-754 bits of `x`
/// (REQ-KRN.NUM.FMT.REG-004, -005). NaN and infinities fail with `non_finite`.
pub fn hexbits(x: f64) -> Result<String, NumError> {
    if !x.is_finite() {
        return Err(FmtError::NonFinite.at(HEX_OP));
    }
    let bits = x.to_bits();
    let mut s = String::new();
    s.try_reserve_exact(18).map_err(|_| FmtError::Alloc.at(HEX_OP))?;
    s.push_str("0x");
    for i in (0..16u32).rev() {
        let nibble = (bits >> (4 * i)) & 0xf;
        let digit = usize::try_from(nibble).ok().and_then(|n| HEX_DIGITS.get(n)).copied().unwrap_or(b'0');
        s.push(char::from(digit));
    }
    Ok(s)
}

/// `fmt.hexbits@1` through [`format`], as pinned in the module documentation.
fn hex_format(x: f64, spec: &FormatSpec) -> Result<Reported, NumError> {
    if !matches!(spec.unit, Unit::M | Unit::One | Unit::Rad) {
        return Err(FmtError::UnitNotSupported.at(HEX_OP));
    }
    if spec.precision != Precision::Decimals(0) {
        return Err(FmtError::PrecisionOutOfRange.at(HEX_OP));
    }
    let (_, limits) = len::check_spec(HEX_OP, spec, false)?;
    let value = hexbits(x)?;
    let mut side = len::initial_outcome(&limits);
    if let Some(hi) = &limits.upper {
        if kdec::cmp(x, hi)? == Ordering::Greater {
            side = SideOutcome::AboveUpper;
        }
    }
    if side != SideOutcome::AboveUpper {
        if let Some(lo) = &limits.lower {
            if kdec::cmp(x, lo)? == Ordering::Less {
                side = SideOutcome::BelowLower;
            }
        }
    }
    Ok(Reported {
        value,
        unit: spec.unit,
        exact_conversion: Conversion { from: spec.unit, to: spec.unit, factor: "1", pi_enclosure: None },
        formatter: spec.formatter,
        digits_added: 0,
        tie_at_max_precision: false,
        side,
    })
}

/// A textual formatter ID or kind held inline (at most [`MAX_ID_LEN`] ASCII bytes), so a
/// [`LookupError`] names what was refused without allocating.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct IdText {
    buf: [u8; MAX_ID_LEN],
    len: u8,
}

impl IdText {
    fn new(b: &[u8]) -> IdText {
        let mut buf = [0u8; MAX_ID_LEN];
        let mut len = 0u8;
        for (d, s) in buf.iter_mut().zip(b) {
            *d = *s;
            len = len.saturating_add(1);
        }
        IdText { buf, len }
    }

    /// The text.
    pub fn as_str(&self) -> &str {
        let b = self.buf.get(..usize::from(self.len)).unwrap_or(&[]);
        core::str::from_utf8(b).unwrap_or("")
    }
}

impl fmt::Debug for IdText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}

impl fmt::Display for IdText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why [`lookup`] refused a textual formatter ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LookupError {
    /// Not exactly `fmt.<kind>@<major>` (see the module documentation). The text is not
    /// echoed, since it may be anything.
    Malformed,
    /// `fmt.<kind>` without `@<major>`: it names no version, so no entry is chosen.
    Ambiguous {
        /// The kind that was given.
        kind: IdText,
    },
    /// A well-formed ID that is not in the registry.
    Unknown {
        /// The refused ID.
        id: IdText,
    },
}

impl LookupError {
    /// Short ASCII name: `malformed_formatter_id`, `ambiguous_formatter`, `unknown_formatter`.
    pub const fn name(&self) -> &'static str {
        match self {
            LookupError::Malformed => "malformed_formatter_id",
            LookupError::Ambiguous { .. } => "ambiguous_formatter",
            LookupError::Unknown { .. } => "unknown_formatter",
        }
    }
}

impl fmt::Display for LookupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LookupError::Malformed => write!(f, "fmt: {}", self.name()),
            LookupError::Ambiguous { kind } => write!(f, "fmt: {}: fmt.{kind} names no major", self.name()),
            LookupError::Unknown { id } => write!(f, "fmt: {}: {id}", self.name()),
        }
    }
}

impl core::error::Error for LookupError {}

fn valid_kind(k: &[u8]) -> bool {
    match k.split_first() {
        Some((first, rest)) => {
            k.len() <= MAX_KIND_LEN
                && first.is_ascii_lowercase()
                && rest.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_')
        }
        None => false,
    }
}

fn parse_major(m: &[u8]) -> Option<u16> {
    let (first, _) = m.split_first()?;
    if m.len() > 5 || *first == b'0' || !m.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let mut v: u32 = 0;
    for c in m {
        v = v * 10 + u32::from(c - b'0');
    }
    u16::try_from(v).ok()
}

/// Resolves a textual formatter ID such as `fmt.length@1` to its descriptor, refusing
/// malformed, ambiguous (versionless) and unknown IDs with a typed [`LookupError`].
pub fn lookup(text: &str) -> Result<&'static FormatterDescriptor, LookupError> {
    let b = text.as_bytes();
    if b.len() > MAX_ID_LEN {
        return Err(LookupError::Malformed);
    }
    let rest = b.strip_prefix(b"fmt.").ok_or(LookupError::Malformed)?;
    let (kind, major) = match rest.iter().position(|&c| c == b'@') {
        None => (rest, None),
        Some(i) => match (rest.get(..i), rest.get(i + 1..)) {
            (Some(k), Some(m)) => (k, Some(m)),
            _ => return Err(LookupError::Malformed),
        },
    };
    if !valid_kind(kind) {
        return Err(LookupError::Malformed);
    }
    let Some(major) = major else {
        return Err(LookupError::Ambiguous { kind: IdText::new(kind) });
    };
    let major = parse_major(major).ok_or(LookupError::Malformed)?;
    REGISTRY
        .iter()
        .find(|d| d.id.kind.as_bytes() == kind && d.id.major == major)
        .ok_or(LookupError::Unknown { id: IdText::new(b) })
}
