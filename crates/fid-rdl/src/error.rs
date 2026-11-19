//! The error type of the crate: a closed set with stable codes (spec node UNC.RDL.FORMAT,
//! Design, Errors).
//!
//! Every error names the field it is about as a path built only from schema field names and
//! array indices (`inputs.0.unit`, `reference.outputs.2.test_uncertainty_hex`), never from
//! the input. Reasons are static text. So no error, `Display` included, can carry a value
//! from the record: a reference-only or blind record cannot leak through an error message.

use std::fmt;

/// Every way this crate refuses a record or a command call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RdlError {
    /// The record or a field does not match the `rdl-dataset/v1` schema.
    SchemaInvalid { field: String, reason: &'static str },
    /// The ID does not match `rdl:<tier>/<aim>/<case>/<nnnn>` or disagrees with `tier`/`aim`.
    BadId { field: String },
    /// An input unit outside {m, mm, rad, deg, µm}.
    BadUnit { field: String },
    /// The record has no licence (absent, null or empty).
    MissingLicence,
    /// The record has no provenance or no provenance source.
    MissingProvenance { field: String },
    /// A reference output has no test uncertainty.
    MissingTestUncertainty { field: String },
    /// `rdl.show_dataset@1` found no record with the requested ID.
    DatasetNotFound,
    /// Command parameters (CLI arguments or MCP JSON) do not match the command's shape.
    BadParams { field: String, reason: &'static str },
    /// A fault that cannot be the caller's mistake, such as canonical encoding failing on a
    /// value this crate built. Holds a static reason.
    Internal { reason: &'static str },
}

impl RdlError {
    /// Stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            RdlError::SchemaInvalid { .. } => "schema_invalid",
            RdlError::BadId { .. } => "bad_id",
            RdlError::BadUnit { .. } => "bad_unit",
            RdlError::MissingLicence => "missing_licence",
            RdlError::MissingProvenance { .. } => "missing_provenance",
            RdlError::MissingTestUncertainty { .. } => "missing_test_uncertainty",
            RdlError::DatasetNotFound => "dataset_not_found",
            RdlError::BadParams { .. } => "bad_params",
            RdlError::Internal { .. } => "internal_error",
        }
    }

    /// The field the error is about (`record` for the record as a whole).
    pub fn field(&self) -> &str {
        match self {
            RdlError::SchemaInvalid { field, .. }
            | RdlError::BadId { field }
            | RdlError::BadUnit { field }
            | RdlError::MissingProvenance { field }
            | RdlError::MissingTestUncertainty { field }
            | RdlError::BadParams { field, .. } => field,
            RdlError::MissingLicence => "licence",
            RdlError::DatasetNotFound => "id",
            RdlError::Internal { .. } => "record",
        }
    }

    fn reason(&self) -> &'static str {
        match self {
            RdlError::SchemaInvalid { reason, .. }
            | RdlError::BadParams { reason, .. }
            | RdlError::Internal { reason } => reason,
            RdlError::BadId { .. } => "not rdl:<tier>/<aim>/<case>/<nnnn> matching tier and aim",
            RdlError::BadUnit { .. } => "unit is not one of m, mm, rad, deg, µm",
            RdlError::MissingLicence => "licence is required",
            RdlError::MissingProvenance { .. } => "provenance and its source are required",
            RdlError::MissingTestUncertainty { .. } => "every reference output needs a test uncertainty",
            RdlError::DatasetNotFound => "no dataset with this id",
        }
    }
}

impl fmt::Display for RdlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}: {}", self.code(), self.field(), self.reason())
    }
}

impl std::error::Error for RdlError {}

pub(crate) fn invalid(field: impl Into<String>, reason: &'static str) -> RdlError {
    RdlError::SchemaInvalid { field: field.into(), reason }
}

pub(crate) fn internal(reason: &'static str) -> RdlError {
    RdlError::Internal { reason }
}

pub(crate) fn bad_params(field: impl Into<String>, reason: &'static str) -> RdlError {
    RdlError::BadParams { field: field.into(), reason }
}
