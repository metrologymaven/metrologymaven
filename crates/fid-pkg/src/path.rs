//! Validated member paths (REQ-TRUST.PKG.MANIFEST-006).

use crate::error::PkgError;
use std::fmt;
use unicode_normalization::is_nfc;

/// Longest member path, in UTF-8 bytes.
pub const MAX_PATH_BYTES: usize = 4096;

/// A member path that passed [`PkgPath::parse`]: relative, `/`-separated, Unicode NFC, with
/// no empty, `.` or `..` segment, no backslash, no NUL, no drive prefix and at most
/// [`MAX_PATH_BYTES`] bytes.
///
/// Ordering is that of the UTF-8 bytes, which is the manifest and container order.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PkgPath(String);

impl PkgPath {
    /// Checks `s` against the path rules. Every failure is [`PkgError::BadPath`] with a
    /// short reason tag; this never panics on any input.
    pub fn parse(s: &str) -> Result<PkgPath, PkgError> {
        check(s).map_err(|reason| PkgError::bad_path(s, reason))?;
        Ok(PkgPath(s.to_string()))
    }

    /// The path text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The first segment (the whole path when it has no `/`).
    pub fn top_level(&self) -> &str {
        self.0.split('/').next().unwrap_or_default()
    }
}

fn check(s: &str) -> Result<(), &'static str> {
    let b = s.as_bytes();
    if b.is_empty() {
        return Err("empty");
    }
    if b.len() > MAX_PATH_BYTES {
        return Err("too_long");
    }
    if b.contains(&0) {
        return Err("nul");
    }
    if b.contains(&b'\\') {
        return Err("backslash");
    }
    if b.first() == Some(&b'/') {
        return Err("absolute");
    }
    if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        return Err("drive_prefix");
    }
    if !is_nfc(s) {
        return Err("not_nfc");
    }
    for seg in s.split('/') {
        match seg {
            "" => return Err("empty_segment"),
            "." | ".." => return Err("dot_segment"),
            _ => {}
        }
    }
    Ok(())
}

impl fmt::Display for PkgPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for PkgPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PkgPath({:?})", self.0)
    }
}

impl AsRef<str> for PkgPath {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn reason(s: &str) -> &'static str {
        match PkgPath::parse(s) {
            Err(PkgError::BadPath { reason, .. }) => reason,
            other => panic!("{s:?}: expected bad_path, got {other:?}"),
        }
    }

    #[test]
    fn reasons() {
        assert_eq!(reason(""), "empty");
        assert_eq!(reason(&"a".repeat(4097)), "too_long");
        assert_eq!(reason("a\0b"), "nul");
        assert_eq!(reason("a\\b"), "backslash");
        assert_eq!(reason("/a"), "absolute");
        assert_eq!(reason("C:x"), "drive_prefix");
        assert_eq!(reason("e\u{301}"), "not_nfc");
        assert_eq!(reason("a//b"), "empty_segment");
        assert_eq!(reason("a/.."), "dot_segment");
    }

    #[test]
    fn accepted() {
        let p = PkgPath::parse("results/records.json").unwrap();
        assert_eq!(p.as_str(), "results/records.json");
        assert_eq!(p.top_level(), "results");
        assert_eq!(PkgPath::parse("a").unwrap().top_level(), "a");
        assert!(PkgPath::parse(&"a".repeat(4096)).is_ok());
        assert!(PkgPath::parse("\u{e9}").is_ok());
    }
}
