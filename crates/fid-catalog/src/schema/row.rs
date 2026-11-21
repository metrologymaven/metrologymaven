//! Row types of the catalog tables, the fixed-string enums stored in them and the untrusted
//! text wrapper (REQ-RPT.CATALOG.STORE.SCHEMA-001, -003).
//!
//! Every enum is stored as one fixed string and parsed back exactly; an unknown string read
//! from a file is `stats.catalog_corrupt`, never a guess. The evidence labels of an entry
//! (`claimed`, `declared_input`, `uncharacterized_input`, `interpretation`) have no default:
//! a writer states each one, so a claimed value or a provisional interpretation cannot enter
//! the catalog looking measured or settled (D-045, `UNC.UEC.CLASS`). The table checks back
//! this up for any connection: a claimed value must be UEC-1, a UEC-0 entry has no value.

use fid_types::text::is_forbidden_char;
use fid_types::Digest;
use fid_uuid::Uuid;
use std::fmt::{self, Write as _};

/// Text that came from project content (part names, characteristic names, run and plan
/// labels, record IDs, tags). It is data, never instructions (CONSTITUTION C6).
///
/// Deliberately thin: no `Deref<Target = str>`, no `AsRef<str>`, no `Display`, no
/// conversion into `String`. People see it only through [`CatalogText::as_display`], which
/// escapes `\` and every control, format, bidi and separator character; the raw text is
/// reachable only inside this crate (to bind it as an SQL parameter and for the canonical
/// forms of the sibling modules).
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CatalogText(Box<str>);

#[allow(clippy::len_without_is_empty)]
impl CatalogText {
    /// Wraps text taken from project content.
    pub fn new(text: &str) -> CatalogText {
        CatalogText(Box::from(text))
    }

    /// Wraps an owned string taken from project content.
    pub fn from_string(text: String) -> CatalogText {
        CatalogText(text.into_boxed_str())
    }

    /// Length in bytes (UTF-8).
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// The raw text, for SQL parameters and machine-readable forms inside this crate only.
    pub(crate) fn raw(&self) -> &str {
        &self.0
    }

    /// A view for showing the text to a person: `\` becomes `\\`, and every character that
    /// `fid_types::text::is_forbidden_char` names becomes a `\u{..}` escape (lowercase hex).
    pub fn as_display(&self) -> CatalogTextDisplay<'_> {
        CatalogTextDisplay(&self.0)
    }
}

impl fmt::Debug for CatalogText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CatalogText({:?})", self.as_display().to_string())
    }
}

/// Display view of a [`CatalogText`]; see [`CatalogText::as_display`].
#[derive(Debug, Clone, Copy)]
pub struct CatalogTextDisplay<'a>(&'a str);

impl fmt::Display for CatalogTextDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        escape_into(self.0, f)
    }
}

fn escape_into(s: &str, f: &mut dyn fmt::Write) -> fmt::Result {
    for c in s.chars() {
        if c == '\\' {
            f.write_str("\\\\")?;
        } else if is_forbidden_char(c) {
            write!(f, "\\u{{{:x}}}", u32::from(c))?;
        } else {
            f.write_char(c)?;
        }
    }
    Ok(())
}

/// `s` escaped as [`CatalogText::as_display`] does, for error details that quote a file.
pub(crate) fn escaped(s: &str) -> String {
    let mut out = String::new();
    let _ = escape_into(s, &mut out);
    out
}

macro_rules! fixed_strings {
    ($(#[$m:meta])* $name:ident { $($(#[$vm:meta])* $v:ident => $s:literal),* $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $name {
            $($(#[$vm])* $v),*
        }

        impl $name {
            /// Every value, in declaration order.
            pub const ALL: &'static [$name] = &[$($name::$v),*];

            /// The stored string.
            pub const fn as_str(self) -> &'static str {
                match self {
                    $($name::$v => $s),*
                }
            }

            /// The value stored as exactly `s`.
            pub fn parse(s: &str) -> Option<$name> {
                $name::ALL.iter().copied().find(|v| v.as_str() == s)
            }
        }
    };
}

fixed_strings! {
    /// Integrity status of an entry, written by `RPT.CATALOG.CHECK` and the indexer. Not part
    /// of the content digest.
    EntryStatus {
        Ok => "ok",
        /// Not checked since its project became unreadable (REQ-RPT.CATALOG.STORE-006).
        Unverified => "unverified",
        Stale => "stale",
        Unavailable => "unavailable",
    }
}

fixed_strings! {
    /// Status of the ResultRecord in its source project.
    RecordStatus {
        Active => "active",
        Voided => "voided",
        Superseded => "superseded",
    }
}

fixed_strings! {
    /// CONSTITUTION §5 `evaluation_basis`, stored exactly as the record declares it.
    EvaluationBasis {
        Points => "POINTS",
        Features => "FEATURES",
        ResultsOnly => "RESULTS_ONLY",
        DirectReading => "DIRECT_READING",
    }
}

fixed_strings! {
    /// The decision outcome of the record (UNC.DEC.TYPE wire names).
    DecisionOutcome {
        Conforms => "CONFORMS",
        Nonconforms => "NONCONFORMS",
        Indeterminate => "INDETERMINATE",
        NotDecidable => "NOT_DECIDABLE",
    }
}

fixed_strings! {
    /// The uncertainty evidence class (UNC.UEC).
    UecClass {
        Uec0 => "UEC-0",
        Uec1 => "UEC-1",
        Uec2 => "UEC-2",
        Uec3 => "UEC-3",
    }
}

fixed_strings! {
    /// The interpretation label of the record (D-045, GDT.CHAR.INTERP).
    Interpretation {
        Provisional => "provisional",
        SignedOff => "signed_off",
    }
}

fixed_strings! {
    /// Whether a registered project could be opened at the last catalog command.
    Availability {
        Available => "available",
        Unavailable => "unavailable",
    }
}

/// The evidence labels of an entry, copied from the record's decision basis
/// (`UNC.UEC.CLASS` Labels). No `Default`: a writer states every label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EvidenceLabels {
    /// The value is declared or specification-derived, not measured.
    pub claimed: bool,
    /// Some input of the uncertainty is declared.
    pub declared_input: bool,
    /// Some contributor is not a verified characterized source.
    pub uncharacterized_input: bool,
}

/// A value with its unit: the literal as recorded, its unit, and the value in SI units.
#[derive(Debug, Clone, PartialEq)]
pub struct EntryValue {
    /// Decimal literal as recorded (digits, `.`, `e`, `E`, `+`, `-`; at most 64 bytes).
    pub literal: CatalogText,
    pub unit: CatalogText,
    /// Finite. Negative zero is stored as zero.
    pub si: f64,
}

/// One `entry` row: the summary of one ResultRecord of a registered project.
#[derive(Debug, Clone, PartialEq)]
pub struct CatalogEntryRow {
    pub project_id: Uuid,
    pub record_id: CatalogText,
    pub part: CatalogText,
    pub revision: Option<CatalogText>,
    pub plan: CatalogText,
    pub run: CatalogText,
    pub stage: CatalogText,
    pub characteristic: CatalogText,
    /// RFC 3339 UTC instant as recorded (`...Z`).
    pub timestamp: CatalogText,
    /// `None` for a UEC-0 entry or a result without a value.
    pub value: Option<EntryValue>,
    pub decision_outcome: DecisionOutcome,
    /// The decision rule; required unless the outcome is `NOT_DECIDABLE`.
    pub rule_id: Option<CatalogText>,
    pub evaluation_basis: EvaluationBasis,
    pub uec_class: UecClass,
    pub labels: EvidenceLabels,
    pub interpretation: Interpretation,
    pub record_status: RecordStatus,
    /// Sequence of the source journal entry in the project's Record Journal.
    pub journal_seq: u64,
    /// Hash of the source journal entry.
    pub journal_entry_hash: Digest,
    pub entry_status: EntryStatus,
}

impl CatalogEntryRow {
    /// Whether every view of this entry must label its value as claimed, not measured.
    pub fn is_claimed(&self) -> bool {
        self.labels.claimed
    }

    /// Whether every view of this entry must show `interpretation: provisional`.
    pub fn is_provisional(&self) -> bool {
        self.interpretation == Interpretation::Provisional
    }
}

/// One `project` row: a registered project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRow {
    pub project_id: Uuid,
    /// The handle agents use for the project (never a raw path). Unique, ignoring ASCII case.
    pub handle: CatalogText,
    /// Last indexed journal position: sequence and head hash, `None` before the first index.
    pub indexed: Option<(u64, Digest)>,
    pub availability: Availability,
}
