//! The reader's error type: the six codes of TRUST.PKG.READER, each with a stable reason tag.

use crate::error::kept;
use std::fmt;

/// Every reason tag [`ReadError::Limit`] uses, sorted.
pub const LIMIT_REASONS: [&str; 8] = [
    "buffer_cap",
    "index_budget",
    "manifest_size",
    "member_count",
    "member_length",
    "package_size",
    "path_length",
    "ratio",
];

/// Every reason tag [`ReadError::Malformed`] uses, sorted.
pub const MALFORMED_REASONS: [&str; 41] = [
    "archive_comment",
    "bad_path",
    "case_collision",
    "cd_bounds",
    "cd_signature",
    "cd_size",
    "control_char",
    "crc",
    "data_descriptor",
    "deflate",
    "duplicate_entry",
    "encrypted",
    "entry_comment",
    "eocd_mismatch",
    "extra_field",
    "flags",
    "gap",
    "host",
    "length",
    "local_mismatch",
    "local_signature",
    "lookalike",
    "manifest",
    "member_truncated",
    "method",
    "missing_member",
    "multi_disk",
    "name_encoding",
    "no_eocd",
    "no_manifest",
    "overlap",
    "path_conflict",
    "special_file",
    "stored_size",
    "symlink",
    "trailing_data",
    "truncated",
    "version_needed",
    "zip64_extra",
    "zip64_locator",
    "zip64_record",
];

/// A typed reader error. [`ReadError::code`] is one of the six codes of the leaf spec;
/// [`ReadError::reason`] narrows it to a stable tag. The reader never panics: every hostile
/// input ends in one of these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadError {
    /// `pkg.limit`: a published limit was exceeded (reason in [`LIMIT_REASONS`]).
    Limit { reason: &'static str, detail: String },
    /// `pkg.malformed`: the container or a member stream breaks the format (reason in
    /// [`MALFORMED_REASONS`]).
    Malformed { reason: &'static str, detail: String },
    /// `pkg.digest_mismatch`: a member's length (`reason` `length`) or SHA-256 (`sha256`)
    /// differs from its manifest entry.
    DigestMismatch { path: String, reason: &'static str },
    /// `pkg.unlisted_member`: an entry that is not in the manifest and is neither
    /// `statement.json` nor under `signatures/`.
    UnlistedMember { path: String },
    /// `pkg.unsupported_version`: the manifest's format major version is not supported.
    UnsupportedVersion(String),
    /// `pkg.io`: reading the source failed for a reason other than its end.
    Io(String),
}

impl ReadError {
    /// The stable code.
    pub fn code(&self) -> &'static str {
        match self {
            ReadError::Limit { .. } => "pkg.limit",
            ReadError::Malformed { .. } => "pkg.malformed",
            ReadError::DigestMismatch { .. } => "pkg.digest_mismatch",
            ReadError::UnlistedMember { .. } => "pkg.unlisted_member",
            ReadError::UnsupportedVersion(_) => "pkg.unsupported_version",
            ReadError::Io(_) => "pkg.io",
        }
    }

    /// The stable reason tag.
    pub fn reason(&self) -> &'static str {
        match self {
            ReadError::Limit { reason, .. } | ReadError::Malformed { reason, .. } => reason,
            ReadError::DigestMismatch { reason, .. } => reason,
            ReadError::UnlistedMember { .. } => "unlisted",
            ReadError::UnsupportedVersion(_) => "version",
            ReadError::Io(_) => "io",
        }
    }

    pub(crate) fn limit(reason: &'static str, detail: impl Into<String>) -> ReadError {
        ReadError::Limit { reason, detail: kept(&detail.into()) }
    }

    pub(crate) fn malformed(reason: &'static str, detail: impl Into<String>) -> ReadError {
        ReadError::Malformed { reason, detail: kept(&detail.into()) }
    }

    pub(crate) fn mismatch(path: &str, reason: &'static str) -> ReadError {
        ReadError::DigestMismatch { path: kept(path), reason }
    }

    pub(crate) fn unlisted(path: &str) -> ReadError {
        ReadError::UnlistedMember { path: kept(path) }
    }

    /// Maps an I/O error: the end of the source is `truncated`, anything else `pkg.io`.
    pub(crate) fn from_io(e: &std::io::Error) -> ReadError {
        if e.kind() == std::io::ErrorKind::UnexpectedEof {
            ReadError::malformed("truncated", "unexpected end of the package")
        } else {
            ReadError::Io(kept(&e.to_string()))
        }
    }
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReadError::Limit { reason, detail } | ReadError::Malformed { reason, detail } => {
                write!(f, "{} ({reason}): {detail}", self.code())
            }
            ReadError::DigestMismatch { path, reason } => write!(f, "{} ({reason}): {path:?}", self.code()),
            ReadError::UnlistedMember { path } => write!(f, "{}: {path:?}", self.code()),
            ReadError::UnsupportedVersion(v) => write!(f, "{}: {v:?}", self.code()),
            ReadError::Io(s) => write!(f, "{}: {s}", self.code()),
        }
    }
}

impl std::error::Error for ReadError {}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn reason_lists_are_sorted_and_unique() {
        for list in [&LIMIT_REASONS[..], &MALFORMED_REASONS[..]] {
            assert!(list.windows(2).all(|w| w[0] < w[1]), "{list:?}");
        }
    }

    #[test]
    fn codes_and_reasons() {
        let all = [
            (ReadError::limit("ratio", "x"), "pkg.limit", "ratio"),
            (ReadError::malformed("gap", "x"), "pkg.malformed", "gap"),
            (ReadError::mismatch("results/a", "sha256"), "pkg.digest_mismatch", "sha256"),
            (ReadError::unlisted("results/b"), "pkg.unlisted_member", "unlisted"),
            (ReadError::UnsupportedVersion("2.0.0".into()), "pkg.unsupported_version", "version"),
            (ReadError::Io("denied".into()), "pkg.io", "io"),
        ];
        for (e, code, reason) in all {
            assert_eq!((e.code(), e.reason()), (code, reason));
            assert!(e.to_string().starts_with(code));
        }
        let eof = std::io::Error::from(std::io::ErrorKind::UnexpectedEof);
        assert_eq!(ReadError::from_io(&eof).reason(), "truncated");
        let other = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        assert_eq!(ReadError::from_io(&other).code(), "pkg.io");
    }
}
