//! Errors of the Part 21 reader and printer.

use fid_io_reg::{IoError, LimitKind, Position};
use std::fmt;

/// Which bound a [`StepError::LimitExceeded`] names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Bound {
    /// Input bytes (file size), from the `IO.REG.LIMITS` budget.
    Bytes,
    /// Elements (instances plus parameters), from the budget.
    Elements,
    /// Nesting depth of list and typed parameters.
    Depth,
    /// Tracked allocation, from the budget.
    Alloc,
    /// Wall time, from the budget.
    Wall,
    /// Number of instances (header plus data).
    Instances,
    /// Decoded length of one string parameter, in UTF-8 bytes.
    String,
    /// Hex digits of one binary parameter.
    Binary,
    /// Length of one keyword, number or entity-number token.
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
            Bound::Instances => "instances",
            Bound::String => "string",
            Bound::Binary => "binary",
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

/// Every outcome other than success. Reasons are static strings chosen by the reader,
/// never text copied from the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepError {
    /// A bound was exceeded.
    LimitExceeded {
        /// The bound.
        bound: Bound,
        /// Its configured value.
        value: u64,
        /// Input position reached.
        at: Position,
    },
    /// The input breaks the exchange-structure grammar. `entity` names the entity
    /// number for `duplicate_entity`.
    Malformed {
        /// Where.
        at: Position,
        /// Static reason code.
        reason: &'static str,
        /// The entity number concerned, if any.
        entity: Option<u64>,
    },
    /// The input ends inside a construct.
    TruncatedFile {
        /// Where the input ended.
        at: Position,
    },
    /// A reference names an entity number that the file does not define.
    DanglingReference {
        /// The referring instance.
        from: u64,
        /// The missing entity number.
        to: u64,
    },
    /// A construct this reader deliberately does not support (edition-3 sections,
    /// value instances, code pages other than A).
    FeatureUnsupported {
        /// Where.
        at: Position,
        /// Static name of the feature.
        feature: &'static str,
    },
    /// Any other harness error (I/O failure, forbidden limit raise).
    Reader(IoError),
}

impl StepError {
    /// The stable snake_case code.
    pub fn code(&self) -> &'static str {
        match self {
            StepError::LimitExceeded { .. } => "limit_exceeded",
            StepError::Malformed { .. } => "malformed",
            StepError::TruncatedFile { .. } => "truncated_file",
            StepError::DanglingReference { .. } => "dangling_reference",
            StepError::FeatureUnsupported { .. } => "step_feature_unsupported",
            StepError::Reader(e) => e.code(),
        }
    }

    /// The input position, for the variants that carry one.
    pub fn position(&self) -> Option<Position> {
        match self {
            StepError::LimitExceeded { at, .. }
            | StepError::Malformed { at, .. }
            | StepError::TruncatedFile { at }
            | StepError::FeatureUnsupported { at, .. } => Some(*at),
            StepError::DanglingReference { .. } => None,
            StepError::Reader(e) => e.position(),
        }
    }
}

impl From<IoError> for StepError {
    fn from(e: IoError) -> StepError {
        match e {
            IoError::LimitExceeded { limit, value, at } => {
                StepError::LimitExceeded { bound: Bound::from_limit(limit), value, at }
            }
            IoError::Malformed { at, reason } => StepError::Malformed { at, reason, entity: None },
            IoError::TruncatedFile { at } => StepError::TruncatedFile { at },
            other => StepError::Reader(other),
        }
    }
}

impl fmt::Display for StepError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let StepError::Reader(e) = self {
            return write!(f, "{e}");
        }
        f.write_str(self.code())?;
        match self {
            StepError::LimitExceeded { bound, value, at } => write!(f, ": bound {bound} = {value} reached at {at}"),
            StepError::Malformed { at, reason, entity: Some(n) } => write!(f, ": {reason} (entity #{n}) at {at}"),
            StepError::Malformed { at, reason, entity: None } => write!(f, ": {reason} at {at}"),
            StepError::TruncatedFile { at } => write!(f, ": input ends at {at}"),
            StepError::DanglingReference { from, to } => write!(f, ": #{from} refers to #{to}, which is not defined"),
            StepError::FeatureUnsupported { at, feature } => write!(f, ": {feature} at {at}"),
            StepError::Reader(_) => Ok(()),
        }
    }
}

impl std::error::Error for StepError {}
