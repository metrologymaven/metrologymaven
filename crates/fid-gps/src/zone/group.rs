//! Shared-pose group identity, pose digest and the all-or-nothing rule for incomplete
//! groups (spec node `GDT.ZONE.GROUP.IDS`).
//!
//! Every shared-pose group (`pattern` or `simreq`) gets:
//! - a deterministic UUIDv5 in the project namespace over the RFC 8785 canonical JSON of
//!   `{"kind": ..., "label": ..., "members": [...]}` with member IDs sorted in ascending
//!   byte order (REQ-GDT.ZONE.GROUP.IDS-001);
//! - a SHA-256 pose digest over the RFC 8785 canonical JSON of the shared [`Pose`] with
//!   every `f64` encoded as an IEEE-754 hex string `"0x..."` (REQ-GDT.ZONE.GROUP.IDS-002);
//! - a shared [`GroupRef`] attached identically to every member's ResultRecord payload
//!   (REQ-GDT.ZONE.GROUP.IDS-003);
//! - an all-or-nothing guard [`all_or_nothing`] that turns any member failure into
//!   `group_incomplete{member, cause}` naming the lowest failing member ID and emits no
//!   member values (REQ-GDT.ZONE.GROUP.IDS-004);
//! - membership validation rejecting groups with fewer than two members (`too_few`) or
//!   repeated members (`duplicate`) (REQ-GDT.ZONE.GROUP.IDS-005).

use std::collections::BTreeMap;
use std::fmt;

use fid_canon::CValue;
use fid_types::Digest;
use fid_uuid::{uuid_v5, Uuid, UuidError};

use crate::zone::shape::Pose;

/// Kind of shared-pose tolerance group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GroupKind {
    /// A pattern of features sharing one zone pose (`pattern`).
    Pattern,
    /// A simultaneous requirement coupling multiple callouts (`simreq`).
    SimReq,
}

impl GroupKind {
    /// Stable wire representation (`"pattern"` or `"simreq"`).
    pub const fn as_str(self) -> &'static str {
        match self {
            GroupKind::Pattern => "pattern",
            GroupKind::SimReq => "simreq",
        }
    }

    /// Parses `"pattern"` or `"simreq"` strictly.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "pattern" => Some(GroupKind::Pattern),
            "simreq" => Some(GroupKind::SimReq),
            _ => None,
        }
    }
}

impl fmt::Display for GroupKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Characteristic identifier newtype over a UUID (`GDT.CHAR.IDENT`), ordered by its
/// canonical big-endian bytes (identical to lowercase ASCII UUID byte order).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CharId(pub Uuid);

impl CharId {
    /// Wraps a UUID as a characteristic ID.
    pub const fn new(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Underlying UUID.
    pub const fn as_uuid(self) -> Uuid {
        self.0
    }

    /// Canonical 16-byte big-endian representation.
    pub const fn as_bytes(&self) -> &[u8; 16] {
        self.0.as_bytes()
    }

    /// Parses a UUID string strictly.
    pub fn parse(text: &str) -> Result<Self, UuidError> {
        Uuid::parse(text).map(Self)
    }
}

impl From<Uuid> for CharId {
    fn from(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl fmt::Display for CharId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Why a group's membership list is invalid (REQ-GDT.ZONE.GROUP.IDS-005).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BadGroupReason {
    /// Fewer than two members (`too_few`).
    TooFew,
    /// A member ID appears more than once (`duplicate`).
    Duplicate,
    /// Member set does not match the expected group (`member_mismatch`).
    MemberMismatch,
}

impl BadGroupReason {
    /// Stable reason string.
    pub const fn as_str(self) -> &'static str {
        match self {
            BadGroupReason::TooFew => "too_few",
            BadGroupReason::Duplicate => "duplicate",
            BadGroupReason::MemberMismatch => "member_mismatch",
        }
    }
}

impl fmt::Display for BadGroupReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Typed errors of shared-pose group identity, pose digesting and all-or-nothing evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupError {
    /// `bad_group{reason}`: group has fewer than two members, duplicate members, or mismatched
    /// member IDs.
    BadGroup {
        /// Why the group was rejected.
        reason: BadGroupReason,
    },
    /// `group_incomplete{member, cause}`: at least one member failed precheck or evaluation;
    /// `member` is the lowest failing [`CharId`] in byte order, and no member receives a value.
    GroupIncomplete {
        /// Lowest failing member characteristic ID.
        member: CharId,
        /// Human-readable cause from the failing member.
        cause: String,
    },
    /// `non_finite_pose`: at least one `f64` coordinate in the pose is NaN or infinite.
    NonFinitePose,
    /// `canonical_json`: canonical JSON encoding failed.
    CanonicalJson,
}

impl GroupError {
    /// Stable machine-readable error code.
    pub const fn code(&self) -> &'static str {
        match self {
            GroupError::BadGroup { .. } => "bad_group",
            GroupError::GroupIncomplete { .. } => "group_incomplete",
            GroupError::NonFinitePose => "non_finite_pose",
            GroupError::CanonicalJson => "canonical_json",
        }
    }
}

impl fmt::Display for GroupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GroupError::BadGroup { reason } => write!(f, "bad_group{{{reason}}}"),
            GroupError::GroupIncomplete { member, cause } => {
                write!(f, "group_incomplete{{member: {member}, cause: {cause}}}")
            }
            GroupError::NonFinitePose => f.write_str("non_finite_pose"),
            GroupError::CanonicalJson => f.write_str("canonical_json"),
        }
    }
}

impl std::error::Error for GroupError {}

/// Shared group reference carried identically by every member's ResultRecord payload
/// (REQ-GDT.ZONE.GROUP.IDS-003).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupRef {
    /// Deterministic UUIDv5 of `{kind, label, sorted member IDs}` in the project namespace.
    pub group_id: Uuid,
    /// Group kind (`pattern` or `simreq`).
    pub kind: GroupKind,
    /// Member characteristic IDs, strictly sorted in ascending byte order with no duplicates.
    pub members: Vec<CharId>,
    /// SHA-256 digest of the canonical shared pose (with `f64` values as IEEE-754 hex).
    pub pose_digest: Digest,
}

impl GroupRef {
    /// Encodes the group reference as a canonical JSON value.
    pub fn to_cvalue(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("group_id".to_owned(), CValue::Str(self.group_id.to_string()));
        m.insert("kind".to_owned(), CValue::Str(self.kind.as_str().to_owned()));
        m.insert(
            "members".to_owned(),
            CValue::Arr(self.members.iter().map(|id| CValue::Str(id.to_string())).collect()),
        );
        m.insert("pose_digest".to_owned(), CValue::Str(self.pose_digest.as_str().to_owned()));
        CValue::Obj(m)
    }

    /// RFC 8785 canonical JSON bytes of the group reference.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, GroupError> {
        fid_canon::to_canonical_bytes(&self.to_cvalue()).map_err(|_| GroupError::CanonicalJson)
    }

    /// SHA-256 digest of [`Self::canonical_bytes`].
    pub fn digest(&self) -> Result<Digest, GroupError> {
        let bytes = self.canonical_bytes()?;
        Ok(Digest::from_bytes(fid_canon::digest_bytes(&bytes).0))
    }
}

/// Validates and sorts group members in total ascending byte order.
///
/// Rejects fewer than two members with `BadGroup { reason: TooFew }` and any repeated
/// member with `BadGroup { reason: Duplicate }`.
pub fn normalize_members(members: &[CharId]) -> Result<Vec<CharId>, GroupError> {
    if members.len() < 2 {
        return Err(GroupError::BadGroup { reason: BadGroupReason::TooFew });
    }
    let mut sorted = members.to_vec();
    sorted.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    for pair in sorted.windows(2) {
        if pair[0] == pair[1] {
            return Err(GroupError::BadGroup { reason: BadGroupReason::Duplicate });
        }
    }
    Ok(sorted)
}

/// Canonical JSON input hashed by [`group_id`]:
/// `{"kind":"pattern"|"simreq","label":"...","members":["..."]}` with sorted members.
pub fn group_id_canonical_bytes(kind: GroupKind, label: &str, members: &[CharId]) -> Result<Vec<u8>, GroupError> {
    let sorted = normalize_members(members)?;
    let mut m = BTreeMap::new();
    m.insert("kind".to_owned(), CValue::Str(kind.as_str().to_owned()));
    m.insert("label".to_owned(), CValue::Str(label.to_owned()));
    m.insert("members".to_owned(), CValue::Arr(sorted.iter().map(|id| CValue::Str(id.to_string())).collect()));
    fid_canon::to_canonical_bytes(&CValue::Obj(m)).map_err(|_| GroupError::CanonicalJson)
}

/// Computes the deterministic UUIDv5 group ID in `namespace` over the RFC 8785 canonical
/// JSON of `{kind, label, sorted member IDs}` (REQ-GDT.ZONE.GROUP.IDS-001).
pub fn group_id(namespace: &Uuid, kind: GroupKind, label: &str, members: &[CharId]) -> Result<Uuid, GroupError> {
    let bytes = group_id_canonical_bytes(kind, label, members)?;
    Ok(uuid_v5(namespace, &bytes))
}

/// Encodes `pose` as RFC 8785 canonical JSON with every `f64` value written as an IEEE-754
/// lowercase hex string `"0x..."`. Fails [`GroupError::NonFinitePose`] if any component is
/// NaN or infinite.
pub fn pose_canonical_bytes(pose: &Pose) -> Result<Vec<u8>, GroupError> {
    for v in pose.origin.iter().chain(pose.direction.iter()) {
        if !v.is_finite() {
            return Err(GroupError::NonFinitePose);
        }
    }
    let mut m = BTreeMap::new();
    m.insert(
        "direction".to_owned(),
        CValue::Arr(vec![
            CValue::F64Bits(pose.direction[0]),
            CValue::F64Bits(pose.direction[1]),
            CValue::F64Bits(pose.direction[2]),
        ]),
    );
    m.insert(
        "origin".to_owned(),
        CValue::Arr(vec![
            CValue::F64Bits(pose.origin[0]),
            CValue::F64Bits(pose.origin[1]),
            CValue::F64Bits(pose.origin[2]),
        ]),
    );
    fid_canon::to_canonical_bytes(&CValue::Obj(m)).map_err(|_| GroupError::NonFinitePose)
}

/// Computes the SHA-256 digest of the canonical shared [`Pose`] with `f64` values as
/// IEEE-754 hex strings (REQ-GDT.ZONE.GROUP.IDS-002).
pub fn pose_digest(pose: &Pose) -> Result<Digest, GroupError> {
    let bytes = pose_canonical_bytes(pose)?;
    Ok(Digest::from_bytes(fid_canon::digest_bytes(&bytes).0))
}

/// Builds the shared [`GroupRef`] from the project namespace, group kind, callout/group
/// label, member IDs (any order) and solved shared pose.
pub fn build_group_ref(
    namespace: &Uuid,
    kind: GroupKind,
    label: &str,
    members: &[CharId],
    pose: &Pose,
) -> Result<GroupRef, GroupError> {
    let sorted = normalize_members(members)?;
    let id = group_id(namespace, kind, label, &sorted)?;
    let pd = pose_digest(pose)?;
    Ok(GroupRef { group_id: id, kind, members: sorted, pose_digest: pd })
}

/// Applies the all-or-nothing rule for a shared-pose group (REQ-GDT.ZONE.GROUP.IDS-004, -005).
///
/// Validates membership (`too_few`, `duplicate`), sorts entries by [`CharId`] ascending byte
/// order, and if any member failed returns `Err(GroupError::GroupIncomplete { member, cause })`
/// naming the lowest failing member ID with no member values returned. Only when every member
/// succeeds are all values returned in sorted [`CharId`] order.
pub fn all_or_nothing<T, E: fmt::Display>(
    results: Vec<(CharId, Result<T, E>)>,
) -> Result<Vec<(CharId, T)>, GroupError> {
    let ids: Vec<CharId> = results.iter().map(|(id, _)| *id).collect();
    let _ = normalize_members(&ids)?;

    let mut ordered = results;
    ordered.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));

    let mut ok_values = Vec::with_capacity(ordered.len());
    for (member, res) in ordered {
        match res {
            Ok(v) => ok_values.push((member, v)),
            Err(e) => {
                return Err(GroupError::GroupIncomplete { member, cause: e.to_string() });
            }
        }
    }
    Ok(ok_values)
}

/// Attaches the identical [`GroupRef`] to every member's evaluated payload after checking that
/// the evaluated member IDs match `group_ref.members` exactly (REQ-GDT.ZONE.GROUP.IDS-003).
pub fn attach_group_ref<T>(
    group_ref: &GroupRef,
    values: Vec<(CharId, T)>,
) -> Result<Vec<(CharId, T, GroupRef)>, GroupError> {
    let ids: Vec<CharId> = values.iter().map(|(id, _)| *id).collect();
    let sorted_ids = normalize_members(&ids)?;
    if sorted_ids != group_ref.members {
        return Err(GroupError::BadGroup { reason: BadGroupReason::MemberMismatch });
    }
    let mut ordered = values;
    ordered.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    Ok(ordered.into_iter().map(|(id, v)| (id, v, group_ref.clone())).collect())
}
