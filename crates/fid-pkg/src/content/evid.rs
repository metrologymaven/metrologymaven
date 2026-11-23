//! Evidence tiers, streamed point-set gathering and D-045 interpretation register/roster
//! gathering (spec node `TRUST.PKG.CONTENT.EVID`, feature `export`).
//!
//! - **REQ-001 / REQ-003:** at tier [`EvidenceTier::Points`], gathers every distinct input
//!   point-set digest referenced by the included records, deduplicated into
//!   `points/<lowercase-hex-sha256>.<ext>` sorted by path, failing once with
//!   [`PkgError::PointsMissing`] listing every absent digest.
//! - **REQ-002:** at [`EvidenceTier::Results`] or [`EvidenceTier::Features`], emits no
//!   point-set members and records `evidence_tier` in [`GatherOutput`].
//! - **REQ-004 / REQ-005:** streams each blob through one fixed 64 KiB buffer (never
//!   loading more than 64 MiB into memory), recomputing SHA-256 on the fly and failing
//!   with [`PkgError::PointsCorrupt`] on mismatch.
//! - **REQ-006 (D-045):** at every tier, gathers one `interp/registers/<sha256>.json`
//!   member per distinct `interpretation_register` digest (excluding literal `"none"`) and
//!   one `interp/rosters/<sha256>.json` member per roster digest those registers name,
//!   failing with [`PkgError::RegisterMissing`] if any register or roster blob is absent,
//!   malformed or fails its SHA-256 check.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read};

use fid_canon::{parse_with_limits, CValue};
use fid_types::Digest;
use sha2::{Digest as Sha2Digest, Sha256};

use crate::error::PkgError;
use crate::manifest::{check_media_type, check_member_path, EvidenceTier, JSON_MEDIA_TYPE};
use crate::path::PkgPath;

/// Fixed streaming buffer size (64 KiB, well under REQ-005's 64 MiB ceiling).
pub const STREAM_BUFFER_BYTES: usize = 64 * 1024;
/// Maximum byte length accepted for a single interpretation register JSON document (4 MiB).
pub const MAX_REGISTER_BYTES: usize = 4 * 1024 * 1024;
/// Maximum JSON nesting depth when parsing an interpretation register document.
pub const MAX_REGISTER_DEPTH: usize = 16;
/// Default point-set member extension (`[unverified: name owned by the point-set format]`).
pub const DEFAULT_POINTS_EXT: &str = "fpts";
/// Default point-set media type.
pub const DEFAULT_POINTS_MEDIA_TYPE: &str = "application/vnd.fiducial.points";
/// Sentinel value in `interpretation_register` meaning no register is pinned.
pub const REGISTER_NONE: &str = "none";

/// Minimal projection of an included ResultRecord needed for evidence gathering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultRecord {
    /// Result identifier (for caller bookkeeping; never affects digest deduplication).
    pub id: String,
    /// Input point-set digests referenced by this result.
    pub input_digests: Vec<Digest>,
    /// Optional pinned interpretation register digest (`None` corresponds to literal `"none"`).
    pub interpretation_register: Option<Digest>,
}

impl ResultRecord {
    /// Parses an `interpretation_register` wire field where `"none"` (or empty) maps to
    /// `Ok(None)` and a `"sha256:..."` string maps to `Ok(Some(digest))`.
    pub fn parse_register_ref(raw: &str) -> Result<Option<Digest>, PkgError> {
        if raw.is_empty() || raw == REGISTER_NONE {
            return Ok(None);
        }
        Digest::parse(raw)
            .map(Some)
            .map_err(|_| PkgError::schema(format!("interpretation_register: invalid digest {raw:?}")))
    }
}

/// A streamed blob handle returned by a [`BlobStore`].
pub struct StoredBlob<'a> {
    /// Stored media type of the blob (used to derive the file extension for point sets).
    pub media_type: String,
    /// Byte length reported by the store.
    pub length: u64,
    /// Stream reader yielding the raw blob bytes.
    pub reader: Box<dyn Read + 'a>,
}

/// Content-addressed blob store interface used by [`gather`] (CORE.STO.CAS abstraction).
pub trait BlobStore {
    /// Opens the blob stored under `digest`, or returns `Ok(None)` if absent.
    fn open(&self, digest: &Digest) -> Result<Option<StoredBlob<'_>>, PkgError>;
}

/// Deterministic in-memory [`BlobStore`] backed by a `BTreeMap` (zero `HashMap`).
#[derive(Debug, Default, Clone)]
pub struct MemoryBlobStore {
    blobs: BTreeMap<Digest, (String, Vec<u8>)>,
}

impl MemoryBlobStore {
    /// Creates an empty in-memory store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts `bytes` under their computed SHA-256 digest and returns that digest.
    pub fn insert(&mut self, media_type: impl Into<String>, bytes: Vec<u8>) -> Digest {
        let digest = sha256_bytes(&bytes);
        self.blobs.insert(digest, (media_type.into(), bytes));
        digest
    }

    /// Inserts `bytes` under an explicit `digest` (useful for testing corruption detection).
    pub fn insert_under_digest(&mut self, digest: Digest, media_type: impl Into<String>, bytes: Vec<u8>) {
        self.blobs.insert(digest, (media_type.into(), bytes));
    }

    /// Removes `digest` from the store, returning `true` if it was present.
    pub fn remove(&mut self, digest: &Digest) -> bool {
        self.blobs.remove(digest).is_some()
    }

    /// Returns `true` if `digest` exists in the store.
    pub fn contains(&self, digest: &Digest) -> bool {
        self.blobs.contains_key(digest)
    }
}

impl BlobStore for MemoryBlobStore {
    fn open(&self, digest: &Digest) -> Result<Option<StoredBlob<'_>>, PkgError> {
        Ok(self.blobs.get(digest).map(|(media_type, bytes)| StoredBlob {
            media_type: media_type.clone(),
            length: bytes.len() as u64,
            reader: Box::new(Cursor::new(bytes.as_slice())),
        }))
    }
}

/// Reference to a verified blob in the CAS (streamed; never holds the full payload in memory).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobRef {
    /// Verified SHA-256 digest of the blob.
    pub digest: Digest,
    /// Verified byte length counted while streaming.
    pub length: u64,
    /// Validated media type.
    pub media_type: String,
}

/// Kind of gathered package member.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GatheredKind {
    /// Point-set member under `points/<sha256>.<ext>` (tier `points` only).
    Points,
    /// Interpretation status register under `interp/registers/<sha256>.json` (every tier, D-045).
    InterpRegister,
    /// Interpretation roster under `interp/rosters/<sha256>.json` (every tier, D-045).
    InterpRoster,
}

/// One gathered, verified package member referencing a CAS blob.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PointMember {
    /// Member kind (point set, interpretation register or interpretation roster).
    pub kind: GatheredKind,
    /// Canonical package path (`points/<hex>.<ext>`, `interp/registers/<hex>.json`, or
    /// `interp/rosters/<hex>.json`).
    pub path: PkgPath,
    /// Verified SHA-256 digest.
    pub digest: Digest,
    /// File extension without leading dot (e.g. `fpts`, `json`).
    pub ext: String,
    /// Streamed CAS reference carrying verified digest, length and media type.
    pub blob: BlobRef,
}

/// Complete output of [`gather`]: the recorded [`EvidenceTier`] and all gathered members
/// sorted by UTF-8 path bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatherResult {
    /// The evidence tier applied (written into `manifest.json`'s `evidence_tier`).
    pub evidence_tier: EvidenceTier,
    /// Point-set members (non-empty only for [`EvidenceTier::Points`]), sorted by path.
    pub point_members: Vec<PointMember>,
    /// Interpretation register and roster members (gathered at every tier, D-045), sorted by path.
    pub interp_members: Vec<PointMember>,
}

impl GatherResult {
    /// Returns all gathered members (point sets + interpretation registers/rosters) in
    /// canonical manifest path order (UTF-8 bytes).
    pub fn all_members(&self) -> Vec<&PointMember> {
        let mut all: Vec<&PointMember> = self.point_members.iter().chain(self.interp_members.iter()).collect();
        all.sort_by(|a, b| a.path.as_str().as_bytes().cmp(b.path.as_str().as_bytes()));
        all
    }
}

/// Maps a point-set blob media type to its member file extension.
pub fn extension_for_media_type(media_type: &str) -> &'static str {
    match media_type {
        "application/vnd.fiducial.points" | "application/octet-stream" => DEFAULT_POINTS_EXT,
        "text/csv" => "csv",
        "application/json" => "json",
        "model/ply" | "application/x-ply" => "ply",
        _ => DEFAULT_POINTS_EXT,
    }
}

/// Gathers evidence members for `records` at `tier` from `cas` (REQ-001..REQ-006):
///
/// - At [`EvidenceTier::Points`]: deduplicates every input digest in `records` via a
///   `BTreeSet`, checks presence (collecting all absent digests and failing once with
///   [`PkgError::PointsMissing`]), streams each blob through one fixed 64 KiB buffer to
///   verify its SHA-256 (failing [`PkgError::PointsCorrupt`] on mismatch), and emits
///   `points/<hex>.<ext>` sorted by path.
/// - At [`EvidenceTier::Results`] or [`EvidenceTier::Features`]: emits no point-set
///   members and records `tier` in [`GatherResult::evidence_tier`].
/// - At **every** tier (D-045 / REQ-006): gathers each distinct non-`none`
///   `interpretation_register` blob (`interp/registers/<hex>.json`) and each distinct
///   roster digest those registers name (`interp/rosters/<hex>.json`), failing with
///   [`PkgError::RegisterMissing`] if any blob is absent, malformed or corrupt.
pub fn gather(cas: &dyn BlobStore, records: &[ResultRecord], tier: EvidenceTier) -> Result<GatherResult, PkgError> {
    let mut buf = vec![0u8; STREAM_BUFFER_BYTES];

    // 1. Point sets (tier == Points only).
    let mut point_members = Vec::new();
    if tier == EvidenceTier::Points {
        let mut wanted_points = BTreeSet::new();
        for rec in records {
            for &d in &rec.input_digests {
                wanted_points.insert(d);
            }
        }

        // First collect every absent digest so pkg.points_missing lists all of them (REQ-001).
        let mut missing = Vec::new();
        for &digest in &wanted_points {
            let Some(mut stored) = cas.open(&digest)? else {
                missing.push(digest);
                continue;
            };
            if !missing.is_empty() {
                // Still scanning for any additional missing digests; skip hashing once we
                // already know we will return PointsMissing.
                continue;
            }
            check_media_type(&stored.media_type)?;
            let (actual, length) = stream_hash(stored.reader.as_mut(), &mut buf)?;
            if actual != digest {
                return Err(PkgError::PointsCorrupt(digest));
            }
            let ext = extension_for_media_type(&stored.media_type);
            let path_str = format!("points/{}.{ext}", digest.hex());
            let path = PkgPath::parse(&path_str)?;
            check_member_path(&path)?;
            point_members.push(PointMember {
                kind: GatheredKind::Points,
                path,
                digest,
                ext: ext.to_owned(),
                blob: BlobRef { digest, length, media_type: stored.media_type },
            });
        }
        if !missing.is_empty() {
            return Err(PkgError::PointsMissing(missing));
        }
        point_members.sort_by(|a, b| a.path.as_str().as_bytes().cmp(b.path.as_str().as_bytes()));
    }

    // 2. D-045 interpretation registers and rosters (at EVERY tier, REQ-006).
    let mut wanted_registers = BTreeSet::new();
    for rec in records {
        if let Some(d) = rec.interpretation_register {
            wanted_registers.insert(d);
        }
    }

    let mut interp_members = Vec::new();
    let mut wanted_rosters = BTreeSet::new();

    for reg_digest in wanted_registers {
        let Some(mut stored) = cas.open(&reg_digest)? else {
            return Err(PkgError::RegisterMissing(reg_digest));
        };
        let (bytes, actual) = read_bounded_and_hash(stored.reader.as_mut(), MAX_REGISTER_BYTES, &mut buf)
            .map_err(|_| PkgError::RegisterMissing(reg_digest))?;
        if actual != reg_digest {
            return Err(PkgError::RegisterMissing(reg_digest));
        }
        let roster_digest = extract_roster_digest(&bytes).ok_or(PkgError::RegisterMissing(reg_digest))?;
        wanted_rosters.insert(roster_digest);

        let path_str = format!("interp/registers/{}.json", reg_digest.hex());
        let path = PkgPath::parse(&path_str)?;
        check_member_path(&path)?;
        interp_members.push(PointMember {
            kind: GatheredKind::InterpRegister,
            path,
            digest: reg_digest,
            ext: "json".to_owned(),
            blob: BlobRef { digest: reg_digest, length: bytes.len() as u64, media_type: JSON_MEDIA_TYPE.to_owned() },
        });
    }

    for roster_digest in wanted_rosters {
        let Some(mut stored) = cas.open(&roster_digest)? else {
            return Err(PkgError::RegisterMissing(roster_digest));
        };
        let (actual, length) = stream_hash(stored.reader.as_mut(), &mut buf)?;
        if actual != roster_digest {
            return Err(PkgError::RegisterMissing(roster_digest));
        }
        let path_str = format!("interp/rosters/{}.json", roster_digest.hex());
        let path = PkgPath::parse(&path_str)?;
        check_member_path(&path)?;
        interp_members.push(PointMember {
            kind: GatheredKind::InterpRoster,
            path,
            digest: roster_digest,
            ext: "json".to_owned(),
            blob: BlobRef { digest: roster_digest, length, media_type: JSON_MEDIA_TYPE.to_owned() },
        });
    }

    interp_members.sort_by(|a, b| a.path.as_str().as_bytes().cmp(b.path.as_str().as_bytes()));

    Ok(GatherResult { evidence_tier: tier, point_members, interp_members })
}

fn stream_hash(reader: &mut dyn Read, buf: &mut [u8]) -> Result<(Digest, u64), PkgError> {
    let mut hasher = Sha256::new();
    let mut total: u64 = 0;
    loop {
        match reader.read(buf) {
            Ok(0) => break,
            Ok(n) => {
                if let Some(chunk) = buf.get(..n) {
                    hasher.update(chunk);
                }
                total = total.saturating_add(n as u64);
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(PkgError::Io(format!("reading CAS blob: {e}"))),
        }
    }
    Ok((Digest::from_bytes(hasher.finalize().into()), total))
}

fn read_bounded_and_hash(
    reader: &mut dyn Read,
    max_bytes: usize,
    buf: &mut [u8],
) -> Result<(Vec<u8>, Digest), PkgError> {
    let mut hasher = Sha256::new();
    let mut out = Vec::new();
    loop {
        match reader.read(buf) {
            Ok(0) => break,
            Ok(n) => {
                if out.len().saturating_add(n) > max_bytes {
                    return Err(PkgError::SchemaInvalid("interpretation register exceeds size limit".to_owned()));
                }
                if let Some(chunk) = buf.get(..n) {
                    hasher.update(chunk);
                    out.extend_from_slice(chunk);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(PkgError::Io(format!("reading register blob: {e}"))),
        }
    }
    Ok((out, Digest::from_bytes(hasher.finalize().into())))
}

/// Extracts `roster_digest` (either top-level `"roster_digest": "sha256:..."` or nested
/// `"roster": { "digest": "sha256:..." }`) from an interpretation register JSON document.
fn extract_roster_digest(bytes: &[u8]) -> Option<Digest> {
    let v = parse_with_limits(bytes, MAX_REGISTER_BYTES, MAX_REGISTER_DEPTH).ok()?;
    let CValue::Obj(map) = v else {
        return None;
    };
    if let Some(CValue::Str(s)) = map.get("roster_digest") {
        return Digest::parse(s).ok();
    }
    if let Some(CValue::Obj(roster)) = map.get("roster") {
        if let Some(CValue::Str(s)) = roster.get("digest") {
            return Digest::parse(s).ok();
        }
    }
    None
}

fn sha256_bytes(bytes: &[u8]) -> Digest {
    Digest::from_bytes(Sha256::digest(bytes).into())
}
