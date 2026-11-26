//! Errors of the IGES section parser and writer.

use fid_io_reg::{IoError, LimitKind, Position};
use std::fmt;

/// One of the five sections of the fixed-column ASCII form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Section {
    /// Start (`S`).
    Start,
    /// Global (`G`).
    Global,
    /// Directory Entry (`D`).
    Directory,
    /// Parameter Data (`P`).
    Parameter,
    /// Terminate (`T`).
    Terminate,
}

impl Section {
    /// Every section in file order.
    pub const ALL: [Section; 5] =
        [Section::Start, Section::Global, Section::Directory, Section::Parameter, Section::Terminate];

    /// The section letter in column 73.
    pub fn letter(self) -> u8 {
        match self {
            Section::Start => b'S',
            Section::Global => b'G',
            Section::Directory => b'D',
            Section::Parameter => b'P',
            Section::Terminate => b'T',
        }
    }

    /// The stable wire name: the section letter (`S`, `G`, `D`, `P`, `T`).
    pub fn as_str(self) -> &'static str {
        match self {
            Section::Start => "S",
            Section::Global => "G",
            Section::Directory => "D",
            Section::Parameter => "P",
            Section::Terminate => "T",
        }
    }

    pub(crate) fn from_letter(b: u8) -> Option<Section> {
        Section::ALL.into_iter().find(|s| s.letter() == b)
    }

    pub(crate) fn index(self) -> usize {
        match self {
            Section::Start => 0,
            Section::Global => 1,
            Section::Directory => 2,
            Section::Parameter => 3,
            Section::Terminate => 4,
        }
    }
}

impl fmt::Display for Section {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Which bound a [`IgesError::LimitExceeded`] names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Bound {
    /// Input bytes (file size), from the `IO.REG.LIMITS` budget.
    Bytes,
    /// Elements (directory entries plus parameters), from the budget.
    Elements,
    /// Nesting depth, from the budget (not charged by this reader; kept for totality).
    Depth,
    /// Tracked allocation, from the budget.
    Alloc,
    /// Wall time, from the budget.
    Wall,
    /// Length of one integer or real token, in bytes ([`crate::ParseOptions::max_token_bytes`]).
    Token,
}

impl Bound {
    /// The stable wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            Bound::Bytes => "bytes",
            Bound::Elements => "elements",
            Bound::Depth => "depth",
            Bound::Alloc => "alloc",
            Bound::Wall => "wall",
            Bound::Token => "token",
        }
    }

    fn from_limit(kind: LimitKind) -> Bound {
        match kind {
            LimitKind::Bytes => Bound::Bytes,
            LimitKind::Elements => Bound::Elements,
            LimitKind::Depth => Bound::Depth,
            LimitKind::Alloc => Bound::Alloc,
            LimitKind::Wall => Bound::Wall,
        }
    }
}

impl fmt::Display for Bound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Every outcome other than success. Reasons are static strings chosen by the reader or
/// writer, never text copied from the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IgesError {
    /// A bound was exceeded.
    LimitExceeded {
        /// The bound.
        bound: Bound,
        /// Its configured value.
        value: u64,
        /// Input position reached.
        at: Position,
    },
    /// The input breaks the fixed-column form. `de` names the directory entry (its
    /// sequence number) when one is concerned; `back_pointer` is the back-pointer found
    /// in a parameter line that does not match `de` (REQ-IO.CAD.IGES.SECT-002).
    Malformed {
        /// Where (byte offset, 1-based line and column).
        at: Position,
        /// Static reason code.
        reason: &'static str,
        /// The directory entry concerned, if any.
        de: Option<u32>,
        /// The mismatching back-pointer, for `back_pointer_mismatch`.
        back_pointer: Option<u32>,
    },
    /// The Terminate section is missing, or its count for `section` disagrees with the
    /// number of lines in that section (REQ-IO.CAD.IGES.SECT-004).
    TruncatedFile {
        /// The section whose count disagrees (`T` when the Terminate line is missing).
        section: Section,
        /// The Terminate line, or the end of the input.
        at: Position,
    },
    /// Neither the Global unit flag nor the unit name resolves to a unit, and the caller
    /// supplied none (REQ-IO.CAD.IGES.SECT-003).
    UnitRequired,
    /// The file declares a unit and the caller supplied a different one.
    UnitConflict,
    /// The compressed ASCII or binary form, which this reader does not support.
    FormUnsupported {
        /// Where the form was recognised.
        at: Position,
        /// `compressed` or `binary`.
        form: &'static str,
    },
    /// The writer cannot represent the given file in the fixed-column form.
    Unwritable {
        /// Static reason code.
        reason: &'static str,
        /// The directory entry concerned, if any.
        de: Option<u32>,
    },
    /// Any other harness error (I/O failure, forbidden limit raise).
    Reader(IoError),
}

impl IgesError {
    /// The stable snake_case code.
    pub fn code(&self) -> &'static str {
        match self {
            IgesError::LimitExceeded { .. } => "limit_exceeded",
            IgesError::Malformed { .. } => "malformed",
            IgesError::TruncatedFile { .. } => "truncated_file",
            IgesError::UnitRequired => "unit_required",
            IgesError::UnitConflict => "unit_conflict",
            IgesError::FormUnsupported { .. } => "iges_form_unsupported",
            IgesError::Unwritable { .. } => "unwritable",
            IgesError::Reader(e) => e.code(),
        }
    }

    /// The directory entry sequence number the error names, if any.
    pub fn de(&self) -> Option<u32> {
        match self {
            IgesError::Malformed { de, .. } | IgesError::Unwritable { de, .. } => *de,
            _ => None,
        }
    }

    /// The input position, for the variants that carry one.
    pub fn position(&self) -> Option<Position> {
        match self {
            IgesError::LimitExceeded { at, .. }
            | IgesError::Malformed { at, .. }
            | IgesError::TruncatedFile { at, .. }
            | IgesError::FormUnsupported { at, .. } => Some(*at),
            IgesError::UnitRequired | IgesError::UnitConflict | IgesError::Unwritable { .. } => None,
            IgesError::Reader(e) => e.position(),
        }
    }

    pub(crate) fn malformed(at: Position, reason: &'static str) -> IgesError {
        IgesError::Malformed { at, reason, de: None, back_pointer: None }
    }

    pub(crate) fn malformed_de(at: Position, reason: &'static str, de: u32) -> IgesError {
        IgesError::Malformed { at, reason, de: Some(de), back_pointer: None }
    }
}

impl From<IoError> for IgesError {
    fn from(e: IoError) -> IgesError {
        match e {
            IoError::LimitExceeded { limit, value, at } => {
                IgesError::LimitExceeded { bound: Bound::from_limit(limit), value, at }
            }
            IoError::UnitRequired => IgesError::UnitRequired,
            IoError::UnitConflict => IgesError::UnitConflict,
            other => IgesError::Reader(other),
        }
    }
}

impl fmt::Display for IgesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let IgesError::Reader(e) = self {
            return write!(f, "{e}");
        }
        f.write_str(self.code())?;
        match self {
            IgesError::LimitExceeded { bound, value, at } => write!(f, ": bound {bound} = {value} reached at {at}"),
            IgesError::Malformed { at, reason, de, back_pointer } => {
                write!(f, ": {reason}")?;
                if let Some(de) = de {
                    write!(f, " (directory entry {de}")?;
                    if let Some(bp) = back_pointer {
                        write!(f, ", back-pointer {bp}")?;
                    }
                    f.write_str(")")?;
                }
                write!(f, " at {at}")
            }
            IgesError::TruncatedFile { section, at } => write!(f, ": section {section} at {at}"),
            IgesError::FormUnsupported { at, form } => write!(f, ": {form} form at {at}"),
            IgesError::Unwritable { reason, de: Some(de) } => write!(f, ": {reason} (directory entry {de})"),
            IgesError::Unwritable { reason, de: None } => write!(f, ": {reason}"),
            IgesError::UnitRequired | IgesError::UnitConflict | IgesError::Reader(_) => Ok(()),
        }
    }
}

impl std::error::Error for IgesError {}
