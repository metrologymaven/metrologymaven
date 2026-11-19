//! Identifier grammars of defaults profiles: edition, semantic version, profile ID, profile
//! reference `dp:<dialect>-<edition>@<x.y.z>#sha256:<hex>` and characteristic ID.
//!
//! Every parser is total: it returns `None` (or an error) for any input and never panics.

use crate::dmt_flow::roster::Dialect;
use fid_types::Digest;
use std::fmt;

/// Longest edition text, in bytes.
pub const MAX_EDITION_LEN: usize = 32;
/// Most decimal digits in one semantic-version part.
pub const MAX_SEMVER_PART_DIGITS: usize = 9;
/// Longest profile reference text, in bytes.
pub const MAX_REF_LEN: usize = 160;
/// Longest characteristic ID, in bytes.
pub const MAX_CHAR_ID_LEN: usize = 128;
/// The scheme prefix of a profile reference.
pub const REF_PREFIX: &str = "dp:";

/// A standard edition label such as `y14.5-2018`: 1 to 32 bytes of `[a-z0-9.-]`, starting
/// and ending with `[a-z0-9]`, with no two separators (`.` or `-`) in a row.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Edition(String);

impl Edition {
    /// Parses an edition label.
    pub fn parse(s: &str) -> Option<Edition> {
        let b = s.as_bytes();
        if b.is_empty() || b.len() > MAX_EDITION_LEN {
            return None;
        }
        let alnum = |c: u8| c.is_ascii_lowercase() || c.is_ascii_digit();
        let sep = |c: u8| c == b'.' || c == b'-';
        if !alnum(b[0]) || !alnum(b[b.len() - 1]) {
            return None;
        }
        if !b.iter().all(|&c| alnum(c) || sep(c)) {
            return None;
        }
        if b.windows(2).any(|w| sep(w[0]) && sep(w[1])) {
            return None;
        }
        Some(Edition(s.to_owned()))
    }

    /// The label.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A semantic version `MAJOR.MINOR.PATCH`: each part `0` or a decimal without a leading
/// zero, at most nine digits. Pre-release and build suffixes are refused. Ordered
/// numerically by (major, minor, patch). Fields are private: [`SemVer::new`] and
/// [`SemVer::parse`] apply the same rules, so every value prints as text `parse` accepts.
///
/// ```compile_fail
/// let _ = fid_gdt::chars::dprof::SemVer { major: 1_000_000_000, minor: 0, patch: 0 };
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SemVer {
    major: u32,
    minor: u32,
    patch: u32,
}

/// Largest semantic-version part (nine digits).
pub const MAX_SEMVER_PART: u32 = 999_999_999;

fn semver_part(s: &str) -> Option<u32> {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > MAX_SEMVER_PART_DIGITS || !b.iter().all(u8::is_ascii_digit) {
        return None;
    }
    if b.len() > 1 && b[0] == b'0' {
        return None;
    }
    s.parse().ok()
}

impl SemVer {
    /// Builds a version; `None` when a part exceeds [`MAX_SEMVER_PART`].
    pub fn new(major: u32, minor: u32, patch: u32) -> Option<SemVer> {
        if major > MAX_SEMVER_PART || minor > MAX_SEMVER_PART || patch > MAX_SEMVER_PART {
            return None;
        }
        Some(SemVer { major, minor, patch })
    }

    /// Major part.
    pub fn major(&self) -> u32 {
        self.major
    }

    /// Minor part.
    pub fn minor(&self) -> u32 {
        self.minor
    }

    /// Patch part.
    pub fn patch(&self) -> u32 {
        self.patch
    }

    /// Parses `x.y.z`.
    pub fn parse(s: &str) -> Option<SemVer> {
        let mut it = s.split('.');
        let major = semver_part(it.next()?)?;
        let minor = semver_part(it.next()?)?;
        let patch = semver_part(it.next()?)?;
        if it.next().is_some() {
            return None;
        }
        Some(SemVer { major, minor, patch })
    }
}

impl fmt::Display for SemVer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// The identity of one profile version without its digest: dialect, edition and version.
/// The store holds at most one digest per `ProfileId` (REQ-GDT.CHAR.DPROF-002). Fields are
/// private; every part is itself a checked type, so every value prints as text
/// [`ProfileRef::parse`] accepts.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProfileId {
    dialect: Dialect,
    edition: Edition,
    version: SemVer,
}

impl ProfileId {
    /// Builds an ID from checked parts.
    pub fn new(dialect: Dialect, edition: Edition, version: SemVer) -> ProfileId {
        ProfileId { dialect, edition, version }
    }

    /// Dialect.
    pub fn dialect(&self) -> Dialect {
        self.dialect
    }

    /// Edition.
    pub fn edition(&self) -> &Edition {
        &self.edition
    }

    /// Version.
    pub fn version(&self) -> SemVer {
        self.version
    }
}

impl fmt::Display for ProfileId {
    /// `dp:<dialect>-<edition>@<version>`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{REF_PREFIX}{}-{}@{}", self.dialect.as_str(), self.edition.as_str(), self.version)
    }
}

/// A full profile reference: [`ProfileId`] plus the SHA-256 digest of the profile's
/// RFC 8785 canonical JSON (REQ-GDT.CHAR.DPROF-001). Fields are private; every value
/// round-trips through its text and [`ProfileRef::parse`].
///
/// ```compile_fail
/// fn forge(r: fid_gdt::chars::dprof::ProfileRef) -> fid_gdt::chars::dprof::ProfileRef {
///     fid_gdt::chars::dprof::ProfileRef { id: r.id().clone(), digest: *r.digest() }
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProfileRef {
    id: ProfileId,
    digest: Digest,
}

impl ProfileRef {
    /// Builds a reference from checked parts.
    pub fn new(id: ProfileId, digest: Digest) -> ProfileRef {
        ProfileRef { id, digest }
    }

    /// Dialect, edition and version.
    pub fn id(&self) -> &ProfileId {
        &self.id
    }

    /// Content digest.
    pub fn digest(&self) -> &Digest {
        &self.digest
    }

    /// Parses `dp:<dialect>-<edition>@<x.y.z>#sha256:<64 hex>`. The dialect ends at the
    /// first `-` (dialects hold none); the edition ends at the `@`; the version at the `#`.
    pub fn parse(s: &str) -> Option<ProfileRef> {
        if s.len() > MAX_REF_LEN {
            return None;
        }
        let rest = s.strip_prefix(REF_PREFIX)?;
        let (head, digest) = rest.split_once('#')?;
        let (de, version) = head.split_once('@')?;
        let (dialect, edition) = de.split_once('-')?;
        Some(ProfileRef {
            id: ProfileId {
                dialect: Dialect::parse(dialect)?,
                edition: Edition::parse(edition)?,
                version: SemVer::parse(version)?,
            },
            digest: Digest::parse(digest).ok()?,
        })
    }

    /// The dialect.
    pub fn dialect(&self) -> Dialect {
        self.id.dialect
    }
}

impl fmt::Display for ProfileRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}#{}", self.id, self.digest.as_str())
    }
}

/// A characteristic ID as named on the command line: 1 to 128 bytes, first byte
/// `[A-Za-z0-9]`, the rest `[A-Za-z0-9._:-]` (covers QPIds and designators).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CharId(String);

impl CharId {
    /// Parses a characteristic ID.
    pub fn parse(s: &str) -> Option<CharId> {
        let b = s.as_bytes();
        if b.is_empty() || b.len() > MAX_CHAR_ID_LEN || !b[0].is_ascii_alphanumeric() {
            return None;
        }
        if !b.iter().all(|&c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b':' | b'-')) {
            return None;
        }
        Some(CharId(s.to_owned()))
    }

    /// The ID text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
