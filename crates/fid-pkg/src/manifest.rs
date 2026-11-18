//! `manifest.json`, `statement.json` and member digests (REQ-TRUST.PKG.MANIFEST-001 to -004, -008).
//!
//! Wire form of the manifest (keys in RFC 8785 order when written):
//!
//! ```text
//! { "created_by": "...", "evidence_tier": "results" | "features" | "points",
//!   "format": FORMAT_NAME, "format_version": "1.0.0",
//!   "kernel_manifest_digest": "sha256:<hex>",
//!   "members": [ { "length": "<decimal>", "media_type": "type/subtype",
//!                  "path": "<PkgPath>", "sha256": "<64 lowercase hex>" }, ... ],
//!   "profile_digest": "sha256:<hex>" }
//! ```
//!
//! The statement is an in-toto Statement v1: `_type` [`STATEMENT_TYPE`], `subject` (each a
//! `name` and a `digest` object holding only `sha256`), `predicateType` [`PREDICATE_TYPE`]
//! and `predicate` [`PackagePredicate`].

use crate::error::PkgError;
use crate::path::PkgPath;
use crate::version::{check_compatible, SemVer, FORMAT_VERSION};
use fid_canon::CValue;
use fid_types::Digest;
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{ErrorKind, Read};

/// The fixed package format name carried in every manifest's `format`.
pub const FORMAT_NAME: &str = "metrologymaven-result-package";
/// The in-toto Statement v1 type URI.
pub const STATEMENT_TYPE: &str = "https://in-toto.io/Statement/v1";
/// The predicate type of `statement.json`. A placeholder until the public URI is registered
/// (spec open question, D-002); this is the only place it is spelled.
pub const PREDICATE_TYPE: &str = "https://metrologymaven.io/result-package/predicate/v1";
/// Path of the manifest member (first in the container).
pub const MANIFEST_PATH: &str = "manifest.json";
/// Path of the statement member (last in the container, before any signatures).
pub const STATEMENT_PATH: &str = "statement.json";
/// Folder of DSSE envelopes, added after export and never listed in the manifest.
pub const SIGNATURES_DIR: &str = "signatures";
/// Media type of `manifest.json` and `statement.json`.
pub const JSON_MEDIA_TYPE: &str = "application/json";
/// The reserved top-level folders a manifest member may live in, sorted.
pub const MEMBER_DIRS: [&str; 5] = ["journal", "kernel", "points", "profile", "results"];

const MAX_MEDIA_TYPE_BYTES: usize = 255;

/// How much evidence a package carries (matches `evaluation_basis`, TRUST.PKG.CONTENT).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EvidenceTier {
    Results,
    Features,
    Points,
}

impl EvidenceTier {
    /// Wire string.
    pub fn as_str(&self) -> &'static str {
        match self {
            EvidenceTier::Results => "results",
            EvidenceTier::Features => "features",
            EvidenceTier::Points => "points",
        }
    }

    /// Parses the wire string; anything else is [`PkgError::SchemaInvalid`].
    pub fn parse(s: &str) -> Result<EvidenceTier, PkgError> {
        match s {
            "results" => Ok(EvidenceTier::Results),
            "features" => Ok(EvidenceTier::Features),
            "points" => Ok(EvidenceTier::Points),
            _ => Err(PkgError::schema(format!("evidence_tier: unknown tier {s:?}"))),
        }
    }
}

/// One manifest entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub path: PkgPath,
    pub media_type: String,
    pub length: u64,
    pub sha256: Digest,
}

/// The package manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub format: String,
    pub format_version: SemVer,
    pub created_by: String,
    pub evidence_tier: EvidenceTier,
    pub profile_digest: Digest,
    pub kernel_manifest_digest: Digest,
    /// Sorted by the UTF-8 bytes of the path, unique.
    pub members: Vec<Member>,
}

// ------------------------------------------------------------------ digests

/// SHA-256 of a byte slice.
pub fn hash_bytes(bytes: &[u8]) -> Digest {
    Digest::from_bytes(Sha256::digest(bytes).into())
}

/// Streaming SHA-256 of everything `reader` yields, with the byte count. A read error is
/// [`PkgError::Io`].
pub fn hash_reader<R: Read>(reader: &mut R) -> Result<(u64, Digest), PkgError> {
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut total: u64 = 0;
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(PkgError::Io(e.to_string())),
        };
        let chunk = buf.get(..n).ok_or_else(|| PkgError::Io("reader returned more bytes than asked".into()))?;
        hasher.update(chunk);
        total = total.saturating_add(n as u64);
    }
    Ok((total, Digest::from_bytes(hasher.finalize().into())))
}

// ------------------------------------------------------------------ rules

/// Layout rule for a manifest member: it lives inside one of [`MEMBER_DIRS`] (so it is never
/// `manifest.json`, `statement.json` or under `signatures/`). Otherwise [`PkgError::BadPath`].
pub fn check_member_path(path: &PkgPath) -> Result<(), PkgError> {
    let top = path.top_level();
    if top.len() == path.as_str().len() {
        return Err(PkgError::bad_path(path.as_str(), "not_in_member_folder"));
    }
    if !MEMBER_DIRS.contains(&top) {
        return Err(PkgError::bad_path(path.as_str(), "not_in_member_folder"));
    }
    Ok(())
}

/// A media type is `type/subtype` in lowercase ASCII, without parameters, at most 255 bytes.
/// Otherwise [`PkgError::SchemaInvalid`].
pub fn check_media_type(s: &str) -> Result<(), PkgError> {
    fn token(t: &str) -> bool {
        let mut bytes = t.bytes();
        let first_ok = matches!(bytes.next(), Some(b'a'..=b'z' | b'0'..=b'9'));
        first_ok && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"!#$&^_.+-".contains(&b))
    }
    let ok = s.len() <= MAX_MEDIA_TYPE_BYTES
        && match s.split_once('/') {
            Some((t, sub)) => token(t) && token(sub),
            None => false,
        };
    if ok {
        Ok(())
    } else {
        Err(PkgError::schema(format!("media_type: not a lowercase type/subtype: {s:?}")))
    }
}

/// Order and uniqueness of manifest members: strictly increasing by path bytes
/// ([`PkgError::DuplicatePath`] for a repeat, [`PkgError::SchemaInvalid`] for disorder), and
/// no member path is a folder of another member ([`PkgError::DuplicatePath`]).
pub fn check_members(members: &[Member]) -> Result<(), PkgError> {
    for pair in members.windows(2) {
        if let [a, b] = pair {
            if a.path == b.path {
                return Err(PkgError::duplicate(a.path.as_str()));
            }
            if a.path > b.path {
                return Err(PkgError::schema(format!("members: not sorted at {:?}", b.path.as_str())));
            }
        }
    }
    let all: BTreeSet<&str> = members.iter().map(|m| m.path.as_str()).collect();
    for m in members {
        let p = m.path.as_str();
        for (i, _) in p.match_indices('/') {
            if let Some(folder) = p.get(..i) {
                if all.contains(folder) {
                    return Err(PkgError::duplicate(folder));
                }
            }
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ manifest

fn s(v: &str) -> CValue {
    CValue::Str(v.to_string())
}

fn obj(entries: Vec<(&str, CValue)>) -> CValue {
    CValue::Obj(entries.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn canonical(v: &CValue) -> Result<Vec<u8>, PkgError> {
    fid_canon::to_canonical_bytes(v).map_err(|e| PkgError::schema(format!("canonical JSON: {e}")))
}

impl Manifest {
    /// Checks every rule a written manifest must meet: format name, a supported major
    /// version, a non-empty `created_by`, member layout, media types, order and uniqueness.
    pub fn validate(&self) -> Result<(), PkgError> {
        if self.format != FORMAT_NAME {
            return Err(PkgError::schema(format!("format: expected {FORMAT_NAME:?}, got {:?}", self.format)));
        }
        check_compatible(&self.format_version)?;
        if self.created_by.is_empty() {
            return Err(PkgError::schema("created_by: empty"));
        }
        for m in &self.members {
            check_member_path(&m.path)?;
            check_media_type(&m.media_type)?;
        }
        check_members(&self.members)
    }

    /// The JSON value of the manifest.
    pub fn to_value(&self) -> CValue {
        let members = self
            .members
            .iter()
            .map(|m| {
                obj(vec![
                    ("path", s(m.path.as_str())),
                    ("media_type", s(&m.media_type)),
                    ("length", CValue::U64(m.length)),
                    ("sha256", s(m.sha256.hex())),
                ])
            })
            .collect();
        obj(vec![
            ("format", s(&self.format)),
            ("format_version", s(&self.format_version.to_string())),
            ("created_by", s(&self.created_by)),
            ("evidence_tier", s(self.evidence_tier.as_str())),
            ("profile_digest", s(self.profile_digest.as_str())),
            ("kernel_manifest_digest", s(self.kernel_manifest_digest.as_str())),
            ("members", CValue::Arr(members)),
        ])
    }

    /// Validates, then writes RFC 8785 canonical bytes with no trailing newline.
    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, PkgError> {
        self.validate()?;
        canonical(&self.to_value())
    }

    /// Reads a manifest. The format name is checked first, then the version (a different
    /// major is [`PkgError::UnsupportedVersion`]), then the rest. Properties this version
    /// does not know are ignored, so later minor versions stay readable.
    pub fn parse(bytes: &[u8]) -> Result<Manifest, PkgError> {
        let root = fid_canon::parse(bytes).map_err(|e| PkgError::schema(format!("manifest: {e}")))?;
        let o = as_obj(&root, "manifest")?;
        let format = get_str(o, "format")?;
        if format != FORMAT_NAME {
            return Err(PkgError::schema(format!("format: expected {FORMAT_NAME:?}, got {format:?}")));
        }
        let format_version = SemVer::parse(get_str(o, "format_version")?)?;
        check_compatible(&format_version)?;
        let mut members = Vec::new();
        let list = match o.get("members") {
            Some(CValue::Arr(a)) => a,
            _ => return Err(PkgError::schema("members: missing or not an array")),
        };
        for item in list {
            let m = as_obj(item, "member")?;
            members.push(Member {
                path: PkgPath::parse(get_str(m, "path")?)?,
                media_type: get_str(m, "media_type")?.to_string(),
                length: parse_length(get_str(m, "length")?)?,
                sha256: parse_hex_digest(get_str(m, "sha256")?)?,
            });
        }
        let manifest = Manifest {
            format: format.to_string(),
            format_version,
            created_by: get_str(o, "created_by")?.to_string(),
            evidence_tier: EvidenceTier::parse(get_str(o, "evidence_tier")?)?,
            profile_digest: parse_digest(get_str(o, "profile_digest")?, "profile_digest")?,
            kernel_manifest_digest: parse_digest(get_str(o, "kernel_manifest_digest")?, "kernel_manifest_digest")?,
            members,
        };
        manifest.validate()?;
        Ok(manifest)
    }
}

fn as_obj<'a>(v: &'a CValue, what: &str) -> Result<&'a BTreeMap<String, CValue>, PkgError> {
    match v {
        CValue::Obj(o) => Ok(o),
        _ => Err(PkgError::schema(format!("{what}: not an object"))),
    }
}

fn get_str<'a>(o: &'a BTreeMap<String, CValue>, key: &str) -> Result<&'a str, PkgError> {
    match o.get(key) {
        Some(CValue::Str(v)) => Ok(v),
        Some(_) => Err(PkgError::schema(format!("{key}: not a string"))),
        None => Err(PkgError::schema(format!("{key}: missing"))),
    }
}

fn parse_length(v: &str) -> Result<u64, PkgError> {
    let digits = !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit());
    if !digits || (v.len() > 1 && v.starts_with('0')) {
        return Err(PkgError::schema(format!("length: not a decimal string: {v:?}")));
    }
    v.parse::<u64>().map_err(|_| PkgError::schema(format!("length: out of range: {v:?}")))
}

fn parse_hex_digest(hex: &str) -> Result<Digest, PkgError> {
    Digest::parse(&format!("sha256:{hex}"))
        .map_err(|_| PkgError::schema(format!("sha256: not 64 lowercase hex: {hex:?}")))
}

fn parse_digest(v: &str, key: &str) -> Result<Digest, PkgError> {
    Digest::parse(v).map_err(|_| PkgError::schema(format!("{key}: not a sha256: digest: {v:?}")))
}

// ------------------------------------------------------------------ statement

/// One in-toto subject: a member path and its SHA-256.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subject {
    pub name: String,
    pub sha256: Digest,
}

/// The package predicate carried by the statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackagePredicate {
    pub manifest_sha256: Digest,
    pub format_version: SemVer,
    pub result_count: u64,
    pub evidence_tier: EvidenceTier,
}

/// The in-toto Statement v1 written to `statement.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Statement {
    /// Written as `_type`; always [`STATEMENT_TYPE`] when built here.
    pub statement_type: String,
    pub subject: Vec<Subject>,
    /// Written as `predicateType`; always [`PREDICATE_TYPE`] when built here.
    pub predicate_type: String,
    pub predicate: PackagePredicate,
}

impl Statement {
    /// The statement for `manifest`, whose canonical bytes are `manifest_bytes`: subjects are
    /// `manifest.json` then every member in manifest order.
    pub fn for_manifest(manifest: &Manifest, manifest_bytes: &[u8], result_count: u64) -> Statement {
        let manifest_sha256 = hash_bytes(manifest_bytes);
        let mut subject = Vec::with_capacity(manifest.members.len() + 1);
        subject.push(Subject { name: MANIFEST_PATH.to_string(), sha256: manifest_sha256 });
        for m in &manifest.members {
            subject.push(Subject { name: m.path.as_str().to_string(), sha256: m.sha256 });
        }
        Statement {
            statement_type: STATEMENT_TYPE.to_string(),
            subject,
            predicate_type: PREDICATE_TYPE.to_string(),
            predicate: PackagePredicate {
                manifest_sha256,
                format_version: manifest.format_version,
                result_count,
                evidence_tier: manifest.evidence_tier,
            },
        }
    }

    /// The JSON value of the statement.
    pub fn to_value(&self) -> CValue {
        let subject = self
            .subject
            .iter()
            .map(|sub| obj(vec![("name", s(&sub.name)), ("digest", obj(vec![("sha256", s(sub.sha256.hex()))]))]))
            .collect();
        let p = &self.predicate;
        obj(vec![
            ("_type", s(&self.statement_type)),
            ("subject", CValue::Arr(subject)),
            ("predicateType", s(&self.predicate_type)),
            (
                "predicate",
                obj(vec![
                    ("manifest_sha256", s(p.manifest_sha256.hex())),
                    ("format_version", s(&p.format_version.to_string())),
                    ("result_count", CValue::U64(p.result_count)),
                    ("evidence_tier", s(p.evidence_tier.as_str())),
                ]),
            ),
        ])
    }

    /// RFC 8785 canonical bytes with no trailing newline.
    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, PkgError> {
        canonical(&self.to_value())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn media_types() {
        for ok in ["application/json", "application/x-ndjson", "image/png", "application/vnd.a+json"] {
            assert!(check_media_type(ok).is_ok(), "{ok}");
        }
        for bad in ["", "json", "Application/json", "application/", "/json", "text/plain; charset=utf-8", "a/b/c"] {
            assert_eq!(check_media_type(bad).map_err(|e| e.code()), Err("pkg.schema_invalid"), "{bad:?}");
        }
        assert!(check_media_type(&format!("a/{}", "b".repeat(254))).is_err());
    }

    #[test]
    fn member_layout() {
        for ok in ["results/a.json", "points/x/y.e57", "journal/checkpoints/0.json"] {
            assert!(check_member_path(&PkgPath::parse(ok).unwrap()).is_ok(), "{ok}");
        }
        for bad in ["manifest.json", "statement.json", "signatures/0.dsse.json", "results", "other/x", "Results/x"] {
            assert_eq!(check_member_path(&PkgPath::parse(bad).unwrap()).map_err(|e| e.code()), Err("pkg.bad_path"));
        }
    }

    #[test]
    fn tiers_round_trip() {
        for t in [EvidenceTier::Results, EvidenceTier::Features, EvidenceTier::Points] {
            assert_eq!(EvidenceTier::parse(t.as_str()), Ok(t));
        }
        assert!(EvidenceTier::parse("all").is_err());
    }

    #[test]
    fn lengths() {
        assert_eq!(parse_length("0"), Ok(0));
        assert_eq!(parse_length("18446744073709551615"), Ok(u64::MAX));
        for bad in ["", "01", "-1", "1.0", "18446744073709551616", " 1"] {
            assert!(parse_length(bad).is_err(), "{bad:?}");
        }
    }
}
