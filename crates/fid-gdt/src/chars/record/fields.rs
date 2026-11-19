//! The checked field types of a ResultRecord (spec node `GDT.CHAR.RECORD`). Every type has
//! private fields and one checked constructor, so no value of these types exists that the
//! record would refuse.

use super::RecordError;
use fid_canon::CValue;
use fid_types::Digest;
use std::fmt;

/// Longest run ID, in bytes.
pub const MAX_RUN_ID_LEN: usize = 128;
/// Longest measurand text, in bytes.
pub const MAX_MEASURAND_LEN: usize = 256;
/// Longest algorithm reference (`<module>.<feature>.<method>@<semver>`), in bytes.
pub const MAX_ALGORITHM_LEN: usize = 128;
/// Most input digests one record names.
pub const MAX_INPUT_DIGESTS: usize = 1024;
/// Most digits of one semver number.
const MAX_SEMVER_DIGITS: usize = 9;

fn invalid(field: &'static str) -> RecordError {
    RecordError::InvalidField { field }
}

/// A run ID: 1 to 128 bytes, first byte `[A-Za-z0-9]`, the rest `[A-Za-z0-9._:-]` (the
/// characteristic-ID grammar).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RunId(String);

impl RunId {
    /// Parses a run ID (`invalid_field{run}`).
    pub fn parse(s: &str) -> Result<RunId, RecordError> {
        let b = s.as_bytes();
        let ok = !b.is_empty()
            && b.len() <= MAX_RUN_ID_LEN
            && b[0].is_ascii_alphanumeric()
            && b.iter().all(|&c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b':' | b'-'));
        if ok {
            Ok(RunId(s.to_owned()))
        } else {
            Err(invalid("run"))
        }
    }

    /// The ID text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RunId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What was measured, as text: 1 to 256 bytes of printable ASCII (space to `~`), no
/// leading or trailing space.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Measurand(String);

impl Measurand {
    /// Parses a measurand (`invalid_field{measurand}`).
    pub fn parse(s: &str) -> Result<Measurand, RecordError> {
        let b = s.as_bytes();
        let ok = !b.is_empty()
            && b.len() <= MAX_MEASURAND_LEN
            && b.iter().all(|&c| (0x20..=0x7e).contains(&c))
            && !s.starts_with(' ')
            && !s.ends_with(' ');
        if ok {
            Ok(Measurand(s.to_owned()))
        } else {
            Err(invalid("measurand"))
        }
    }

    /// The text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// How the evaluated value was obtained (CONSTITUTION §5 `evaluation_basis`). The record
/// stores it exactly as the evaluating child declared it and never upgrades it
/// (REQ-GDT.CHAR-008).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EvaluationBasis {
    /// Evaluated from measured points.
    Points,
    /// Evaluated from fitted features.
    Features,
    /// Transcribed results only.
    ResultsOnly,
    /// Read directly from an instrument.
    DirectReading,
}

impl EvaluationBasis {
    /// Every basis, in declaration order.
    pub const ALL: [EvaluationBasis; 4] = [
        EvaluationBasis::Points,
        EvaluationBasis::Features,
        EvaluationBasis::ResultsOnly,
        EvaluationBasis::DirectReading,
    ];

    /// The wire name.
    pub const fn wire(self) -> &'static str {
        match self {
            EvaluationBasis::Points => "POINTS",
            EvaluationBasis::Features => "FEATURES",
            EvaluationBasis::ResultsOnly => "RESULTS_ONLY",
            EvaluationBasis::DirectReading => "DIRECT_READING",
        }
    }

    /// Parses the wire name.
    pub fn from_wire(s: &str) -> Option<EvaluationBasis> {
        EvaluationBasis::ALL.into_iter().find(|b| b.wire() == s)
    }
}

/// A value with its unit. The unit is one of the uncertainty-statement units
/// (`si:m`, `si:rad`, `si:1`); the value is finite, not subnormal and not negative zero, so
/// each value has one bit pattern and one canonical form.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quantity {
    value: f64,
    unit: &'static str,
}

impl Quantity {
    /// Checks a value (`invalid_field{value}`) and its unit (`invalid_field{unit}`).
    pub fn new(value: f64, unit: &str) -> Result<Quantity, RecordError> {
        if !value.is_finite() || value.is_subnormal() || value.to_bits() == (-0.0f64).to_bits() {
            return Err(invalid("value"));
        }
        let unit = fid_kunc::statement::UNITS.iter().copied().find(|u| *u == unit).ok_or(invalid("unit"))?;
        Ok(Quantity { value, unit })
    }

    /// The value.
    pub fn value(&self) -> f64 {
        self.value
    }

    /// The unit.
    pub fn unit(&self) -> &'static str {
        self.unit
    }

    pub(crate) fn to_cvalue(self) -> CValue {
        let mut m = std::collections::BTreeMap::new();
        m.insert("value".to_owned(), CValue::F64Bits(self.value));
        m.insert("unit".to_owned(), CValue::Str(self.unit.to_owned()));
        CValue::Obj(m)
    }
}

fn is_semver(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.len() <= MAX_SEMVER_DIGITS
                && p.bytes().all(|c| c.is_ascii_digit())
                && (p.len() == 1 || !p.starts_with('0'))
        })
}

fn is_alg_segment(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b[0].is_ascii_lowercase()
        && b.iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_' || c == b'-')
}

/// An algorithm ID with its version, `<module>.<feature>.<method>@<semver>` (CONSTITUTION
/// §3): at least three dot-separated segments `[a-z][a-z0-9_-]*`, then `@MAJOR.MINOR.PATCH`
/// without leading zeros; at most 128 bytes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AlgorithmRef {
    text: String,
    at: usize,
}

impl AlgorithmRef {
    /// Parses an algorithm reference (`invalid_field{algorithm}`).
    pub fn parse(s: &str) -> Result<AlgorithmRef, RecordError> {
        if s.len() > MAX_ALGORITHM_LEN {
            return Err(invalid("algorithm"));
        }
        let (id, version) = s.split_once('@').ok_or(invalid("algorithm"))?;
        let segments: Vec<&str> = id.split('.').collect();
        if segments.len() < 3 || !segments.iter().all(|g| is_alg_segment(g)) || !is_semver(version) {
            return Err(invalid("algorithm"));
        }
        Ok(AlgorithmRef { text: s.to_owned(), at: id.len() })
    }

    /// The whole reference.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// The algorithm ID (before `@`).
    pub fn id(&self) -> &str {
        self.text.get(..self.at).unwrap_or("")
    }

    /// The version (after `@`).
    pub fn version(&self) -> &str {
        self.text.get(self.at + 1..).unwrap_or("")
    }
}

/// The kernel version, `MAJOR.MINOR.PATCH` without leading zeros.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KernelVersion(String);

impl KernelVersion {
    /// Parses a kernel version (`invalid_field{kernel_version}`).
    pub fn parse(s: &str) -> Result<KernelVersion, RecordError> {
        if is_semver(s) {
            Ok(KernelVersion(s.to_owned()))
        } else {
            Err(invalid("kernel_version"))
        }
    }

    /// The version text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The digests of the evaluation's inputs: 1 to 1024 distinct digests, kept sorted.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InputDigests(Vec<Digest>);

impl InputDigests {
    /// Checks and sorts the list: empty or more than [`MAX_INPUT_DIGESTS`] is
    /// `invalid_field{input_digests}`, a repeated digest is `duplicate_input_digest`.
    pub fn new(mut digests: Vec<Digest>) -> Result<InputDigests, RecordError> {
        if digests.is_empty() || digests.len() > MAX_INPUT_DIGESTS {
            return Err(invalid("input_digests"));
        }
        digests.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        if digests.windows(2).any(|w| w[0] == w[1]) {
            return Err(RecordError::DuplicateInputDigest);
        }
        Ok(InputDigests(digests))
    }

    /// The digests, sorted.
    pub fn as_slice(&self) -> &[Digest] {
        &self.0
    }
}
