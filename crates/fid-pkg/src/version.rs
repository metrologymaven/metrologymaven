//! Semantic format version and the compatibility rule (REQ-TRUST.PKG.MANIFEST-001, -008).

use crate::error::PkgError;
use std::fmt;

/// A semantic version `MAJOR.MINOR.PATCH` (decimal, no leading zeros, no pre-release or
/// build suffix).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SemVer {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

/// The format version this crate writes.
pub const FORMAT_VERSION: SemVer = SemVer::new(1, 0, 0);

impl SemVer {
    pub const fn new(major: u64, minor: u64, patch: u64) -> SemVer {
        SemVer { major, minor, patch }
    }

    /// Parses `MAJOR.MINOR.PATCH`. Anything else is [`PkgError::SchemaInvalid`].
    pub fn parse(s: &str) -> Result<SemVer, PkgError> {
        let bad = || PkgError::schema(format!("format_version: not MAJOR.MINOR.PATCH: {s:?}"));
        let mut parts = s.split('.');
        let mut next = || -> Result<u64, PkgError> {
            let p = parts.next().ok_or_else(bad)?;
            let digits = !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit());
            if !digits || (p.len() > 1 && p.starts_with('0')) {
                return Err(bad());
            }
            p.parse::<u64>().map_err(|_| bad())
        };
        let v = SemVer::new(next()?, next()?, next()?);
        if parts.next().is_some() {
            return Err(bad());
        }
        Ok(v)
    }
}

impl fmt::Display for SemVer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// A reader of [`FORMAT_VERSION`]'s major version accepts any minor and patch of that major
/// and rejects every other major with [`PkgError::UnsupportedVersion`].
pub fn check_compatible(v: &SemVer) -> Result<(), PkgError> {
    if v.major == FORMAT_VERSION.major {
        Ok(())
    } else {
        Err(PkgError::UnsupportedVersion(v.to_string()))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_print() {
        assert_eq!(SemVer::parse("1.0.0"), Ok(FORMAT_VERSION));
        assert_eq!(SemVer::parse("1.4.10").unwrap().to_string(), "1.4.10");
        for bad in ["", "1", "1.0", "1.0.0.0", "01.0.0", "1.00.0", "1.0.0-rc1", "1.0.0+b", "a.b.c", "1..0", " 1.0.0"] {
            assert_eq!(SemVer::parse(bad).map_err(|e| e.code()), Err("pkg.schema_invalid"), "{bad:?}");
        }
        assert!(SemVer::parse("99999999999999999999.0.0").is_err());
    }

    #[test]
    fn compatibility() {
        assert!(check_compatible(&SemVer::new(1, 4, 0)).is_ok());
        assert!(check_compatible(&SemVer::new(1, 0, 7)).is_ok());
        for v in [SemVer::new(0, 9, 0), SemVer::new(2, 0, 0)] {
            assert_eq!(check_compatible(&v).map_err(|e| e.code()), Err("pkg.unsupported_version"));
        }
    }
}
