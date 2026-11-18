//! Package semantic versions: `MAJOR.MINOR.PATCH`, decimal, no leading zeros, no pre-release
//! or build suffix.

use std::fmt;

/// A package version. Ordering is semantic-version precedence (major, then minor, then patch).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl Version {
    pub const fn new(major: u64, minor: u64, patch: u64) -> Self {
        Version { major, minor, patch }
    }

    /// Parses `MAJOR.MINOR.PATCH`. Returns `None` for anything else.
    pub fn parse(s: &str) -> Option<Version> {
        let mut parts = s.split('.');
        let major = number(parts.next()?)?;
        let minor = number(parts.next()?)?;
        let patch = number(parts.next()?)?;
        if parts.next().is_some() {
            return None;
        }
        Some(Version { major, minor, patch })
    }

    /// `(major, minor)`: the release line, used for "at least one further minor release".
    pub fn line(&self) -> (u64, u64) {
        (self.major, self.minor)
    }
}

fn number(s: &str) -> Option<u64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) || (s.len() > 1 && s.starts_with('0')) {
        return None;
    }
    s.parse().ok()
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[cfg(test)]
mod tests {
    use super::Version;

    #[test]
    fn parses_strict_core_versions() {
        assert_eq!(Version::parse("1.10.0"), Some(Version::new(1, 10, 0)));
        for bad in ["", "1", "1.0", "1.0.0.0", "01.0.0", "1.0.0-rc.1", "v1.0.0", "1..0", "1.0.x"] {
            assert_eq!(Version::parse(bad), None, "{bad}");
        }
        assert!(Version::new(1, 10, 0) > Version::new(1, 9, 9));
    }
}
