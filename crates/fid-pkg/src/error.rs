//! The error type of this crate.

use fid_types::Digest;
use std::fmt;

/// Longest input kept in an error, in bytes; longer inputs are cut at a char boundary.
const KEEP: usize = 128;

/// Typed package errors with stable codes (`pkg.*`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PkgError {
    /// A member path breaks the path or layout rules. `reason` is a stable short tag.
    BadPath { path: String, reason: &'static str },
    /// Two members share a path, or one member's path is a folder of another.
    DuplicatePath(String),
    /// The manifest's format major version is not the one this reader supports.
    UnsupportedVersion(String),
    /// The manifest or statement does not have the required shape.
    SchemaInvalid(String),
    /// Reading member bytes failed.
    Io(String),
    /// One or more point-set digests referenced by included results are absent from the CAS
    /// (`TRUST.PKG.CONTENT.EVID` REQ-001).
    PointsMissing(Vec<Digest>),
    /// A point-set blob read from the CAS did not hash to its requested digest
    /// (`TRUST.PKG.CONTENT.EVID` REQ-004).
    PointsCorrupt(Digest),
    /// An interpretation register or its named roster blob is absent from the CAS, malformed,
    /// or did not hash to its expected digest (`TRUST.PKG.CONTENT.EVID` REQ-006, D-045).
    RegisterMissing(Digest),
}

impl PkgError {
    /// Stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            PkgError::BadPath { .. } => "pkg.bad_path",
            PkgError::DuplicatePath(_) => "pkg.duplicate_path",
            PkgError::UnsupportedVersion(_) => "pkg.unsupported_version",
            PkgError::SchemaInvalid(_) => "pkg.schema_invalid",
            PkgError::Io(_) => "pkg.io",
            PkgError::PointsMissing(_) => "pkg.points_missing",
            PkgError::PointsCorrupt(_) => "pkg.points_corrupt",
            PkgError::RegisterMissing(_) => "pkg.register_missing",
        }
    }

    pub(crate) fn bad_path(path: &str, reason: &'static str) -> PkgError {
        PkgError::BadPath { path: kept(path), reason }
    }

    pub(crate) fn duplicate(path: &str) -> PkgError {
        PkgError::DuplicatePath(kept(path))
    }

    pub(crate) fn schema(detail: impl Into<String>) -> PkgError {
        PkgError::SchemaInvalid(kept(&detail.into()))
    }
}

/// Copies at most [`KEEP`] bytes of `s`, cut at a char boundary.
pub(crate) fn kept(s: &str) -> String {
    if s.len() <= KEEP {
        return s.to_string();
    }
    let mut end = KEEP;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s.get(..end).unwrap_or_default().to_string()
}

impl fmt::Display for PkgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PkgError::BadPath { path, reason } => write!(f, "{}: {reason}: {path:?}", self.code()),
            PkgError::DuplicatePath(s) | PkgError::UnsupportedVersion(s) => write!(f, "{}: {s:?}", self.code()),
            PkgError::SchemaInvalid(s) | PkgError::Io(s) => write!(f, "{}: {s}", self.code()),
            PkgError::PointsMissing(ds) => {
                write!(f, "{}: ", self.code())?;
                for (i, d) in ds.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    f.write_str(d.as_str())?;
                }
                Ok(())
            }
            PkgError::PointsCorrupt(d) | PkgError::RegisterMissing(d) => {
                write!(f, "{}: {}", self.code(), d.as_str())
            }
        }
    }
}

impl std::error::Error for PkgError {}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_stable() {
        let d = Digest::from_bytes([0x11; 32]);
        let all = [
            (PkgError::bad_path("x", "absolute"), "pkg.bad_path"),
            (PkgError::duplicate("x"), "pkg.duplicate_path"),
            (PkgError::UnsupportedVersion("2.0.0".into()), "pkg.unsupported_version"),
            (PkgError::schema("x"), "pkg.schema_invalid"),
            (PkgError::Io("x".into()), "pkg.io"),
            (PkgError::PointsMissing(vec![d]), "pkg.points_missing"),
            (PkgError::PointsCorrupt(d), "pkg.points_corrupt"),
            (PkgError::RegisterMissing(d), "pkg.register_missing"),
        ];
        for (e, code) in all {
            assert_eq!(e.code(), code);
            assert!(e.to_string().starts_with(code));
        }
    }

    #[test]
    fn long_input_is_cut_at_a_char_boundary() {
        let long = "é".repeat(100);
        let k = kept(&long);
        assert!(k.len() <= KEEP && !k.is_empty());
        assert!(long.starts_with(&k));
    }
}
