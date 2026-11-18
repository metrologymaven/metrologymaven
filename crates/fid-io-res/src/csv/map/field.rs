//! The field vocabulary of result tables.

use std::fmt;

/// What a column of a result table holds. Wire names ([`Field::as_str`]) are the names
/// used in `colmap@1` files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Field {
    /// Characteristic ID.
    CharacteristicId,
    /// Characteristic name.
    CharacteristicName,
    /// Nominal value.
    Nominal,
    /// Actual (measured or reported) value.
    Actual,
    /// Deviation from nominal.
    Deviation,
    /// Lower limit.
    LowerLimit,
    /// Upper limit.
    UpperLimit,
    /// Unit of the values.
    Unit,
    /// Decision outcome.
    Status,
    /// `evaluation_basis` (CONSTITUTION §5).
    EvaluationBasis,
    /// Uncertainty evaluation class.
    Uec,
    /// Decision rule ID.
    DecisionRule,
    /// Record status (`transcribed`, ...).
    RecordStatus,
    /// Software that produced the result.
    SourceSoftware,
    /// Version of that software.
    SourceVersion,
    /// Part (serial) ID.
    PartId,
    /// Timestamp, kept as text.
    TimestampText,
    /// Free comment.
    Comment,
}

/// The value type of a field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldType {
    /// Free text (untrusted when read from a file).
    Text,
    /// Decimal text, written with the dialect's decimal mark.
    Decimal,
    /// One of a closed list of values.
    Enum(&'static [&'static str]),
}

/// Where a field sits in the wide layout: once per part row, or once per characteristic
/// in each column group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// One column per row (part).
    Part,
    /// One column per characteristic group.
    Characteristic,
}

/// Decision outcomes (CONSTITUTION §5).
pub const STATUS_VALUES: &[&str] = &["CONFORMS", "NONCONFORMS", "INDETERMINATE", "NOT_DECIDABLE"];
/// Evaluation bases (CONSTITUTION §5).
pub const EVALUATION_BASIS_VALUES: &[&str] = &["POINTS", "FEATURES", "RESULTS_ONLY", "DIRECT_READING"];
/// Uncertainty evaluation classes (CONSTITUTION §5).
pub const UEC_VALUES: &[&str] = &["UEC-0", "UEC-1", "UEC-2", "UEC-3"];

impl Field {
    /// Every field, in vocabulary order.
    pub const ALL: &'static [Field] = &[
        Field::CharacteristicId,
        Field::CharacteristicName,
        Field::Nominal,
        Field::Actual,
        Field::Deviation,
        Field::LowerLimit,
        Field::UpperLimit,
        Field::Unit,
        Field::Status,
        Field::EvaluationBasis,
        Field::Uec,
        Field::DecisionRule,
        Field::RecordStatus,
        Field::SourceSoftware,
        Field::SourceVersion,
        Field::PartId,
        Field::TimestampText,
        Field::Comment,
    ];

    /// The fields the lossy check looks for (REQ-IO.RES.CSV.MAP-003), in report order.
    pub const LOSSY_CHECK: &'static [Field] =
        &[Field::EvaluationBasis, Field::Uec, Field::DecisionRule, Field::RecordStatus, Field::SourceSoftware];

    /// The wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            Field::CharacteristicId => "characteristic_id",
            Field::CharacteristicName => "characteristic_name",
            Field::Nominal => "nominal",
            Field::Actual => "actual",
            Field::Deviation => "deviation",
            Field::LowerLimit => "lower_limit",
            Field::UpperLimit => "upper_limit",
            Field::Unit => "unit",
            Field::Status => "status",
            Field::EvaluationBasis => "evaluation_basis",
            Field::Uec => "uec",
            Field::DecisionRule => "decision_rule",
            Field::RecordStatus => "record_status",
            Field::SourceSoftware => "source_software",
            Field::SourceVersion => "source_version",
            Field::PartId => "part_id",
            Field::TimestampText => "timestamp_text",
            Field::Comment => "comment",
        }
    }

    /// The field with wire name `s`; exact, case-sensitive match only.
    pub fn parse(s: &str) -> Option<Field> {
        Field::ALL.iter().copied().find(|f| f.as_str() == s)
    }

    /// The value type.
    pub fn field_type(self) -> FieldType {
        match self {
            Field::Nominal | Field::Actual | Field::Deviation | Field::LowerLimit | Field::UpperLimit => {
                FieldType::Decimal
            }
            Field::Status => FieldType::Enum(STATUS_VALUES),
            Field::EvaluationBasis => FieldType::Enum(EVALUATION_BASIS_VALUES),
            Field::Uec => FieldType::Enum(UEC_VALUES),
            _ => FieldType::Text,
        }
    }

    /// The field's place in the wide layout.
    pub fn wide_scope(self) -> Scope {
        match self {
            Field::PartId | Field::TimestampText | Field::SourceSoftware | Field::SourceVersion => Scope::Part,
            _ => Scope::Characteristic,
        }
    }
}

impl fmt::Display for Field {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
