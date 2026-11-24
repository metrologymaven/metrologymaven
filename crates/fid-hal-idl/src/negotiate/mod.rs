//! Capability versioning and negotiation (spec node `HAL.IDL.NEG`): the pure function that,
//! at session open, compares what the runtime supports ([`RuntimeSupport`]) with a driver's
//! [`CapabilityDeclaration`] and picks, per capability, a version and a feature set, or marks
//! the capability `UNSUPPORTED`; plus the semver rules every capability package follows
//! ([`check_version_bump`], [`check_major_retention`]).
//!
//! - **REQ-HAL.IDL.NEG-001:** per capability, the highest major both sides support, then the
//!   highest minor both support within it. A range `>=a.b <c` covers every minor of majors
//!   `a+1 .. c-1` and minors `b..` of major `a`, so the common minor is the runtime's highest
//!   minor of that major, provided it is at least the driver's lowest.
//! - **REQ-HAL.IDL.NEG-002:** features are the sorted intersection of declared and
//!   runtime-known features of the chosen major; declared features the runtime does not know
//!   are listed in [`NegotiationResult::ignored_features`], sorted by capability then name.
//! - **REQ-HAL.IDL.NEG-003:** no common major means [`Outcome::Unsupported`]; if `Device` is
//!   not negotiated (absent, unknown or without a common major), the call fails `UNSUPPORTED`.
//! - **REQ-HAL.IDL.NEG-004:** a superseded major stays in the table marked `deprecated` and is
//!   still selectable; [`RuntimeSupport::new`] refuses a table whose superseded majors are
//!   not marked, and [`check_major_retention`] refuses a release that drops the previous
//!   current major.
//! - **REQ-HAL.IDL.NEG-005:** the result holds only ordered maps and sorted lists, so its
//!   canonical JSON ([`NegotiationResult::canonical_json`]) does not depend on input order.
//! - **REQ-HAL.IDL.NEG-006:** a malformed version range, a duplicated capability name and any
//!   other malformed entry fail with `declaration_invalid` naming the entry.
//!
//! **Fail closed.** Nothing is granted that both sides do not state: a capability the
//! runtime does not know, or the driver does not declare, is never negotiated; a feature is
//! granted only if declared *and* known for the chosen major; limits are not echoed into the
//! result. [`negotiate_with_profile`] additionally refuses every capability (other than the
//! `Device` base) that an `instrument-profile/v1` descriptor does not list in its
//! `device_class`. Every check that [`CapabilityDeclaration::from_json`] makes is repeated by
//! [`negotiate`], so a declaration assembled in code cannot skip them.
//!
//! Device-supplied strings in a declaration (make, model, serial, firmware) are
//! [`crate::DeviceText`] values inside [`crate::DeviceInstance`]: data, never instructions.
//! They are never copied into errors or results.

mod semver;
mod wire;

pub use semver::{check_major_retention, check_version_bump, Change};

use crate::base::device::DeviceInstance;
use crate::base::error::{DeviceText, ErrorCode};
use crate::profile::InstrumentProfile;
use crate::units::{Quantity, TimeSource};
use fid_canon::CValue;
use fid_types::Digest;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Schema tag of a declaration document.
pub const DECLARATION_SCHEMA: &str = "capability-declaration/v1";

/// Schema tag of a runtime-support document.
pub const SUPPORT_SCHEMA: &str = "runtime-support/v1";

/// Name of the mandatory base capability.
pub const DEVICE: &str = "Device";

/// Largest accepted declaration or support document (256 KiB).
pub const MAX_INPUT_BYTES: usize = 256 * 1024;

/// Deepest accepted JSON nesting of either document.
pub const MAX_DEPTH: usize = 6;

/// Most JSON elements (values plus object keys) in either document.
pub const MAX_ELEMENTS: usize = 16 * 1024;

/// Most capabilities in a declaration or a support table.
pub const MAX_CAPABILITIES: usize = 64;

/// Most majors of one capability in a support table.
pub const MAX_MAJORS: usize = 16;

/// Most features of one offer or one supported major.
pub const MAX_FEATURES: usize = 128;

/// Most limits of one offer.
pub const MAX_LIMITS: usize = 64;

/// Longest capability, feature or limit name, in bytes.
pub const MAX_NAME_BYTES: usize = 64;

// ------------------------------------------------------------------ errors

/// What kind of failure a [`NegError`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NegErrorKind {
    /// `UNSUPPORTED`: the `Device` base could not be negotiated (REQ-003).
    Unsupported,
    /// `declaration_invalid`: the driver's declaration is malformed (REQ-006).
    DeclarationInvalid,
    /// `support_invalid`: the runtime-support table is malformed, or a release dropped a
    /// major it had to keep (REQ-004).
    SupportInvalid,
    /// `version_bump_invalid`: a package version change does not follow the semver rules.
    VersionBumpInvalid,
}

/// A negotiation or versioning failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NegError {
    pub kind: NegErrorKind,
    /// The entry at fault: a capability name once it passed the name check, otherwise an
    /// RFC 6901 JSON pointer (`""` for the whole document). Never device-supplied text.
    pub entry: String,
    /// Which rule failed, e.g. `duplicate_capability`, `version_range_invalid`.
    pub reason: &'static str,
}

impl NegError {
    pub(crate) fn new(kind: NegErrorKind, entry: impl Into<String>, reason: &'static str) -> NegError {
        NegError { kind, entry: entry.into(), reason }
    }

    pub(crate) fn decl(entry: impl Into<String>, reason: &'static str) -> NegError {
        NegError::new(NegErrorKind::DeclarationInvalid, entry, reason)
    }

    pub(crate) fn support(entry: impl Into<String>, reason: &'static str) -> NegError {
        NegError::new(NegErrorKind::SupportInvalid, entry, reason)
    }

    /// Stable machine-readable error code.
    pub const fn code(&self) -> &'static str {
        match self.kind {
            NegErrorKind::Unsupported => "UNSUPPORTED",
            NegErrorKind::DeclarationInvalid => "declaration_invalid",
            NegErrorKind::SupportInvalid => "support_invalid",
            NegErrorKind::VersionBumpInvalid => "version_bump_invalid",
        }
    }

    /// The device error code a session open reports for this failure, if any: only
    /// `UNSUPPORTED` maps onto the `HAL.IDL.BASE.ERR` set.
    pub const fn device_error_code(&self) -> Option<ErrorCode> {
        match self.kind {
            NegErrorKind::Unsupported => Some(ErrorCode::Unsupported),
            _ => None,
        }
    }
}

impl fmt::Display for NegError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{{{}}}: {}", self.code(), self.entry, self.reason)
    }
}

impl std::error::Error for NegError {}

// ------------------------------------------------------------------ names and ranges

/// A capability name: `[A-Z][A-Za-z0-9]*`, at most [`MAX_NAME_BYTES`] bytes.
pub fn is_capability_name(s: &str) -> bool {
    s.len() <= MAX_NAME_BYTES
        && s.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
        && s.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// A feature or limit name: `[a-z][a-z0-9_]*`, at most [`MAX_NAME_BYTES`] bytes.
pub fn is_feature_name(s: &str) -> bool {
    s.len() <= MAX_NAME_BYTES
        && s.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
        && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// A driver's version range `>=a.b <c`: major `a` from minor `b`, every minor of majors
/// `a+1 .. c-1`. Never empty (`c > a`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VersionRange {
    lowest_major: u32,
    lowest_minor: u32,
    below_major: u32,
}

/// Longest decimal number in a range (keeps every value inside `u32`).
const MAX_RANGE_DIGITS: usize = 9;

impl VersionRange {
    /// A range from `lowest_major.lowest_minor` up to (not including) major `below_major`;
    /// `None` if it would be empty.
    pub fn new(lowest_major: u32, lowest_minor: u32, below_major: u32) -> Option<VersionRange> {
        (below_major > lowest_major).then_some(VersionRange { lowest_major, lowest_minor, below_major })
    }

    /// Parses exactly `>=<major>.<minor> <<major>`: one space, decimal numbers without
    /// leading zeros and of at most nine digits. Anything else, or an empty range, is `None`.
    pub fn parse(s: &str) -> Option<VersionRange> {
        let rest = s.strip_prefix(">=")?;
        let (lower, upper) = rest.split_once(' ')?;
        let (major, minor) = lower.split_once('.')?;
        let below = upper.strip_prefix('<')?;
        VersionRange::new(number(major)?, number(minor)?, number(below)?)
    }

    pub fn lowest_major(&self) -> u32 {
        self.lowest_major
    }

    pub fn lowest_minor(&self) -> u32 {
        self.lowest_minor
    }

    pub fn below_major(&self) -> u32 {
        self.below_major
    }

    /// The lowest minor of `major` inside the range, or `None` if the major is outside it.
    pub fn lowest_minor_in(&self, major: u32) -> Option<u32> {
        if major == self.lowest_major {
            Some(self.lowest_minor)
        } else if major > self.lowest_major && major < self.below_major {
            Some(0)
        } else {
            None
        }
    }
}

impl fmt::Display for VersionRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, ">={}.{} <{}", self.lowest_major, self.lowest_minor, self.below_major)
    }
}

fn number(s: &str) -> Option<u32> {
    let ok = !s.is_empty()
        && s.len() <= MAX_RANGE_DIGITS
        && s.bytes().all(|b| b.is_ascii_digit())
        && (s == "0" || !s.starts_with('0'));
    if ok {
        s.parse().ok()
    } else {
        None
    }
}

// ------------------------------------------------------------------ declaration

/// One capability a driver offers.
#[derive(Debug, Clone, PartialEq)]
pub struct CapabilityOffer {
    pub name: String,
    pub versions: VersionRange,
    /// Declared features, in any order; duplicates are refused.
    pub features: Vec<String>,
    /// Declared limits as SI quantities. Not part of the negotiation result.
    pub limits: BTreeMap<String, Quantity>,
}

/// The tested firmware range a driver states, as device-style untrusted text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirmwareRange {
    pub lowest: DeviceText,
    pub highest: DeviceText,
}

/// What a driver declares at session open.
///
/// Wire form (`capability-declaration/v1`): `{"schema", "driver_version": <semver>,
/// "device": <DeviceInstance>, "capabilities": [{"name", "versions": ">=a.b <c",
/// "features": [..], "limits": {<name>: <Quantity>}}], "time_sources": [<TimeSource>],
/// "tested_firmware": {"lowest": <DeviceText>, "highest": <DeviceText>},
/// "conformance_report": <sha256:..>}`. Keys are closed; duplicate keys are refused.
#[derive(Debug, Clone, PartialEq)]
pub struct CapabilityDeclaration {
    /// The driver's own version (SemVer).
    pub driver_version: String,
    /// Device identity (make, model, serial, firmware as untrusted text) and driver ID.
    pub device: DeviceInstance,
    /// Offered capabilities, in any order.
    pub capabilities: Vec<CapabilityOffer>,
    /// Time sources the device can provide; non-empty, no duplicates.
    pub time_sources: Vec<TimeSource>,
    pub tested_firmware: FirmwareRange,
    /// Digest of the driver's conformance report.
    pub conformance_report: Digest,
}

impl CapabilityDeclaration {
    /// Parses and checks a declaration document. Duplicate capability names are left for
    /// [`negotiate`] to refuse, so the whole declaration is always checked there.
    pub fn from_json(bytes: &[u8]) -> Result<CapabilityDeclaration, NegError> {
        wire::declaration(bytes)
    }
}

/// Checks one offer's name, features and limits.
fn check_offer(o: &CapabilityOffer) -> Result<(), NegError> {
    if !is_capability_name(&o.name) {
        return Err(NegError::decl("/capabilities", "capability_name_invalid"));
    }
    if o.features.len() > MAX_FEATURES {
        return Err(NegError::decl(o.name.clone(), "too_many_features"));
    }
    let mut seen = BTreeSet::new();
    for f in &o.features {
        if !is_feature_name(f) {
            return Err(NegError::decl(o.name.clone(), "feature_invalid"));
        }
        if !seen.insert(f.as_str()) {
            return Err(NegError::decl(o.name.clone(), "duplicate_feature"));
        }
    }
    if o.limits.len() > MAX_LIMITS {
        return Err(NegError::decl(o.name.clone(), "too_many_limits"));
    }
    if o.limits.keys().any(|k| !is_feature_name(k)) {
        return Err(NegError::decl(o.name.clone(), "limit_invalid"));
    }
    Ok(())
}

/// Checks every offer, then refuses duplicated capability names (REQ-006).
fn check_offers(offers: &[CapabilityOffer]) -> Result<(), NegError> {
    if offers.len() > MAX_CAPABILITIES {
        return Err(NegError::decl("/capabilities", "too_many_capabilities"));
    }
    for o in offers {
        check_offer(o)?;
    }
    let mut seen = BTreeSet::new();
    for o in offers {
        if !seen.insert(o.name.as_str()) {
            return Err(NegError::decl(o.name.clone(), "duplicate_capability"));
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ runtime support

/// What the runtime supports for one major of one capability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MajorSupport {
    /// Highest minor the runtime implements; every lower minor is implied.
    pub highest_minor: u32,
    /// Features the runtime knows for this major.
    pub features: BTreeSet<String>,
    /// A superseded major kept negotiable for at least one release (REQ-004).
    pub deprecated: bool,
}

/// The runtime's support table: per capability, per major, a [`MajorSupport`].
///
/// Wire form (`runtime-support/v1`): `{"schema", "capabilities": {<name>: [{"major",
/// "highest_minor", "features": [..], "deprecated"}]}}`, rows in any order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeSupport {
    capabilities: BTreeMap<String, BTreeMap<u32, MajorSupport>>,
}

impl RuntimeSupport {
    /// Checks a table: valid names, 1 to [`MAX_MAJORS`] majors per capability, valid feature
    /// names, the highest major not deprecated, and every lower major marked deprecated.
    pub fn new(capabilities: BTreeMap<String, BTreeMap<u32, MajorSupport>>) -> Result<RuntimeSupport, NegError> {
        if capabilities.len() > MAX_CAPABILITIES {
            return Err(NegError::support("/capabilities", "too_many_capabilities"));
        }
        for (name, majors) in &capabilities {
            if !is_capability_name(name) {
                return Err(NegError::support("/capabilities", "capability_name_invalid"));
            }
            if majors.is_empty() || majors.len() > MAX_MAJORS {
                return Err(NegError::support(name.clone(), "major_count_invalid"));
            }
            let current = majors.keys().next_back().copied();
            for (major, row) in majors {
                if row.features.len() > MAX_FEATURES || row.features.iter().any(|f| !is_feature_name(f)) {
                    return Err(NegError::support(name.clone(), "feature_invalid"));
                }
                let is_current = Some(*major) == current;
                if is_current && row.deprecated {
                    return Err(NegError::support(name.clone(), "current_major_deprecated"));
                }
                if !is_current && !row.deprecated {
                    return Err(NegError::support(name.clone(), "superseded_major_not_deprecated"));
                }
            }
        }
        Ok(RuntimeSupport { capabilities })
    }

    /// Parses and checks a support document.
    pub fn from_json(bytes: &[u8]) -> Result<RuntimeSupport, NegError> {
        RuntimeSupport::new(wire::support(bytes)?)
    }

    /// The table.
    pub fn capabilities(&self) -> &BTreeMap<String, BTreeMap<u32, MajorSupport>> {
        &self.capabilities
    }
}

// ------------------------------------------------------------------ result

/// Why a capability was not negotiated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UnsupportedReason {
    /// The runtime has no entry for the capability.
    UnknownCapability,
    /// No major is supported by both sides (REQ-003).
    NoCommonMajor,
    /// The instrument profile does not list the capability ([`negotiate_with_profile`]).
    NotInProfile,
}

impl UnsupportedReason {
    pub fn as_str(self) -> &'static str {
        match self {
            UnsupportedReason::UnknownCapability => "unknown_capability",
            UnsupportedReason::NoCommonMajor => "no_common_major",
            UnsupportedReason::NotInProfile => "not_in_profile",
        }
    }
}

/// A negotiated capability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Negotiated {
    pub major: u32,
    pub minor: u32,
    /// Granted features, sorted.
    pub features: Vec<String>,
    /// The chosen major is a superseded one kept for compatibility (REQ-004).
    pub deprecated: bool,
}

/// The outcome for one declared capability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Negotiated(Negotiated),
    Unsupported(UnsupportedReason),
}

/// A declared feature the runtime does not know for the chosen major.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IgnoredFeature {
    pub capability: String,
    pub feature: String,
}

/// The negotiation result. Wire form: `{"per_capability": {<name>: {"status": "negotiated",
/// "version": "<major>.<minor>", "features": [..], "deprecated": <bool>} | {"status":
/// "UNSUPPORTED", "reason": <reason>}}, "ignored_features": [{"capability", "feature"}]}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NegotiationResult {
    pub per_capability: BTreeMap<String, Outcome>,
    /// Sorted by capability, then feature.
    pub ignored_features: Vec<IgnoredFeature>,
}

impl NegotiationResult {
    /// The negotiated form of `capability`, if it was granted.
    pub fn granted(&self, capability: &str) -> Option<&Negotiated> {
        match self.per_capability.get(capability) {
            Some(Outcome::Negotiated(n)) => Some(n),
            _ => None,
        }
    }

    /// Whether `feature` of `capability` was granted.
    pub fn grants_feature(&self, capability: &str, feature: &str) -> bool {
        self.granted(capability).is_some_and(|n| n.features.binary_search_by(|f| f.as_str().cmp(feature)).is_ok())
    }

    /// The wire value.
    pub fn to_value(&self) -> CValue {
        let text = |s: &str| CValue::Str(s.to_string());
        let per = self
            .per_capability
            .iter()
            .map(|(name, outcome)| {
                let mut m = BTreeMap::new();
                match outcome {
                    Outcome::Negotiated(n) => {
                        m.insert("status".to_string(), text("negotiated"));
                        m.insert("version".to_string(), CValue::Str(format!("{}.{}", n.major, n.minor)));
                        m.insert(
                            "features".to_string(),
                            CValue::Arr(n.features.iter().map(|f| CValue::Str(f.clone())).collect()),
                        );
                        m.insert("deprecated".to_string(), CValue::Bool(n.deprecated));
                    }
                    Outcome::Unsupported(r) => {
                        m.insert("status".to_string(), text("UNSUPPORTED"));
                        m.insert("reason".to_string(), text(r.as_str()));
                    }
                }
                (name.clone(), CValue::Obj(m))
            })
            .collect();
        let ignored = self
            .ignored_features
            .iter()
            .map(|i| {
                CValue::Obj(BTreeMap::from([
                    ("capability".to_string(), text(&i.capability)),
                    ("feature".to_string(), text(&i.feature)),
                ]))
            })
            .collect();
        CValue::Obj(BTreeMap::from([
            ("per_capability".to_string(), CValue::Obj(per)),
            ("ignored_features".to_string(), CValue::Arr(ignored)),
        ]))
    }

    /// RFC 8785 canonical JSON of [`NegotiationResult::to_value`] (REQ-005).
    pub fn canonical_json(&self) -> Result<Vec<u8>, fid_canon::CanonError> {
        fid_canon::to_canonical_bytes(&self.to_value())
    }
}

// ------------------------------------------------------------------ negotiation

/// Negotiates `decl` against `support` (REQ-001..006). Fails `declaration_invalid` on a
/// malformed or duplicated entry, and `UNSUPPORTED` if `Device` is not negotiated.
pub fn negotiate(support: &RuntimeSupport, decl: &CapabilityDeclaration) -> Result<NegotiationResult, NegError> {
    run(support, decl, None)
}

/// [`negotiate`], restricted to the capabilities an instrument profile declares: every
/// capability other than `Device` that is not in `profile.device_class` is
/// [`UnsupportedReason::NotInProfile`] and its features are neither granted nor listed.
pub fn negotiate_with_profile(
    support: &RuntimeSupport,
    decl: &CapabilityDeclaration,
    profile: &InstrumentProfile,
) -> Result<NegotiationResult, NegError> {
    run(support, decl, Some(&profile.device_class))
}

/// Parses a declaration document and negotiates it: the wire-level entry point.
pub fn negotiate_json(support: &RuntimeSupport, decl: &[u8]) -> Result<NegotiationResult, NegError> {
    negotiate(support, &CapabilityDeclaration::from_json(decl)?)
}

fn run(
    support: &RuntimeSupport,
    decl: &CapabilityDeclaration,
    allowed: Option<&[String]>,
) -> Result<NegotiationResult, NegError> {
    check_offers(&decl.capabilities)?;
    let mut per_capability = BTreeMap::new();
    let mut ignored_features = Vec::new();
    for offer in &decl.capabilities {
        let in_profile = offer.name == DEVICE || allowed.map_or(true, |a| a.contains(&offer.name));
        let outcome = if !in_profile {
            Outcome::Unsupported(UnsupportedReason::NotInProfile)
        } else {
            match support.capabilities.get(&offer.name) {
                None => Outcome::Unsupported(UnsupportedReason::UnknownCapability),
                Some(majors) => match pick(majors, offer.versions) {
                    None => Outcome::Unsupported(UnsupportedReason::NoCommonMajor),
                    Some((major, row)) => {
                        let mut features = Vec::new();
                        for f in &offer.features {
                            if row.features.contains(f) {
                                features.push(f.clone());
                            } else {
                                ignored_features
                                    .push(IgnoredFeature { capability: offer.name.clone(), feature: f.clone() });
                            }
                        }
                        features.sort();
                        Outcome::Negotiated(Negotiated {
                            major,
                            minor: row.highest_minor,
                            features,
                            deprecated: row.deprecated,
                        })
                    }
                },
            }
        };
        per_capability.insert(offer.name.clone(), outcome);
    }
    if !matches!(per_capability.get(DEVICE), Some(Outcome::Negotiated(_))) {
        return Err(NegError::new(NegErrorKind::Unsupported, DEVICE, "device_unsupported"));
    }
    ignored_features.sort();
    Ok(NegotiationResult { per_capability, ignored_features })
}

/// The highest major both sides support, with the runtime row for it (REQ-001).
fn pick(majors: &BTreeMap<u32, MajorSupport>, range: VersionRange) -> Option<(u32, &MajorSupport)> {
    majors.iter().rev().find_map(|(major, row)| {
        let lowest = range.lowest_minor_in(*major)?;
        (row.highest_minor >= lowest).then_some((*major, row))
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn ranges_parse_strictly() {
        let r = VersionRange::parse(">=1.2 <3").unwrap();
        assert_eq!((r.lowest_major(), r.lowest_minor(), r.below_major()), (1, 2, 3));
        assert_eq!(r.to_string(), ">=1.2 <3");
        assert_eq!(VersionRange::parse(">=0.0 <1"), VersionRange::new(0, 0, 1));
        for bad in [
            "",
            ">=1.2",
            ">=1 <2",
            ">=1.2 < 2",
            ">=1.2  <2",
            ">= 1.2 <2",
            ">=1.2 <2 ",
            " >=1.2 <2",
            ">=01.2 <2",
            ">=1.02 <2",
            ">=1.2 <02",
            ">=2.0 <2",
            ">=3.0 <2",
            ">=1.2 <2.0",
            ">=1.2 <=2",
            ">1.2 <2",
            ">=1.2.0 <2",
            ">=+1.2 <2",
            ">=1.-2 <2",
            ">=1.2 <1000000000",
            ">=1.2\t<2",
            ">=1.2 <2\n",
            ">=a.b <c",
        ] {
            assert_eq!(VersionRange::parse(bad), None, "{bad:?}");
        }
        assert!(VersionRange::parse(">=999999999.999999999 <999999999").is_none());
        assert!(VersionRange::parse(">=999999998.999999999 <999999999").is_some());
    }

    #[test]
    fn range_membership() {
        let r = VersionRange::new(1, 2, 3).unwrap();
        assert_eq!(r.lowest_minor_in(0), None);
        assert_eq!(r.lowest_minor_in(1), Some(2));
        assert_eq!(r.lowest_minor_in(2), Some(0));
        assert_eq!(r.lowest_minor_in(3), None);
    }

    #[test]
    fn names_are_checked() {
        for ok in ["Device", "LineScan", "A", "Gauge2"] {
            assert!(is_capability_name(ok), "{ok}");
        }
        for bad in ["", "device", "Line Scan", "Line_Scan", "1Scan", "Ünit", &"A".repeat(MAX_NAME_BYTES + 1)] {
            assert!(!is_capability_name(bad), "{bad}");
        }
        for ok in ["intensity", "raw_profiles", "x2"] {
            assert!(is_feature_name(ok), "{ok}");
        }
        for bad in ["", "Intensity", "_x", "2x", "raw-profiles", "a b", &"a".repeat(MAX_NAME_BYTES + 1)] {
            assert!(!is_feature_name(bad), "{bad}");
        }
    }
}
