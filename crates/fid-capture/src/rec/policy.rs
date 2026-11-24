//! Recording-policy keys and policy evaluation (spec node `VIZ.CAPTURE.REC.POLICY`).
//!
//! **Keys** ([`capture_settings`], declared through `CORE.STO.SET`): four admin-flagged
//! settings allowed only in machine scope, so they change only through the A0 command
//! `settings.set_admin_value@1` and never through this module (REQ-001):
//!
//! | Key | Values | Default |
//! |---|---|---|
//! | `capture.recording` | `allowed`, `forbidden`, `mandated` | `allowed` |
//! | `capture.window_capture` | `allowed`, `forbidden` | `allowed` |
//! | `capture.screenshots` | `allowed`, `forbidden` | `allowed` |
//! | `capture.retention_days` | integer 0 to 36 500, 0 meaning "keep" | 0 |
//!
//! `capture.screenshots` belongs to `VIZ.CAPTURE.SHOT`, which is not built yet; it is declared
//! here, in the same crate and with the values that leaf names, so the policy can read it.
//!
//! **Evaluation** ([`RecordingPolicy`]). [`RecordingPolicy::from_settings`] reads the four keys
//! once through the settings resolver into a [`Policy`] snapshot; [`RecordingPolicy::check`],
//! [`RecordingPolicy::current`] and [`RecordingPolicy::retain_until`] are pure over that
//! snapshot, read-only and total (REQ-005).
//!
//! **Fail closed.** A key whose value is not known is [`None`] in the [`Policy`], and every
//! capture that depends on it is refused with [`ForbidCause::Unknown`]. A value is unknown when
//! the snapshot lacks the key (not declared, not resolved, or resolved twice), when it came from
//! a scope other than machine or the default, when the machine-scope value or the machine file
//! itself could not be used (any resolver warning for that key or file in machine scope), or
//! when it is not one of the key's values. CORE.STO.SET falls back to the default (`allowed`)
//! for an unusable stored value; for these keys that would turn a corrupted or hand-edited
//! machine file into "recording allowed", so this module does not take the fallback. Warnings
//! about other scopes are ignored: those values are never used for a machine-only key, so a user
//! cannot change the policy by writing to the user file. An unknown `capture.recording` is never
//! `mandated`, so nothing is recorded because of a policy nobody can read.
//!
//! The defaults allow capture but mandate nothing: no recording starts unless a person starts
//! one or an admin sets `capture.recording = mandated`. The snapshot holds only the four policy
//! values: no principal, user name, path or other personal data.

use fid_canon::CValue;
use fid_store::settings::{resolve, Resolved, Scope, SettingWarning, SettingsRegistry, Source, Sources, WarningReason};
use std::fmt;

/// Key of the recording policy (`allowed`, `forbidden`, `mandated`).
pub const RECORDING_KEY: &str = "capture.recording";
/// Key of the full-window capture policy (`allowed`, `forbidden`).
pub const WINDOW_CAPTURE_KEY: &str = "capture.window_capture";
/// Key of the screenshot policy (`allowed`, `forbidden`), owned by `VIZ.CAPTURE.SHOT`.
pub const SCREENSHOTS_KEY: &str = "capture.screenshots";
/// Key of the retention of new recordings, in days; 0 keeps them.
pub const RETENTION_DAYS_KEY: &str = "capture.retention_days";
/// Every policy key, sorted.
pub const POLICY_KEYS: [&str; 4] = [RECORDING_KEY, RETENTION_DAYS_KEY, SCREENSHOTS_KEY, WINDOW_CAPTURE_KEY];
/// Largest `capture.retention_days`.
pub const MAX_RETENTION_DAYS: u32 = 36_500;
/// Seconds in a UTC day (UTC instants here carry no leap seconds).
const SECONDS_PER_DAY: i64 = 86_400;

fid_store::fid_settings! {
    pub fn capture_settings;
    {
        key: "capture.recording",
        owner: "capture",
        schema: r#"{"type": "string", "enum": ["allowed", "forbidden", "mandated"]}"#,
        default: r#""allowed""#,
        scopes: [Machine],
        admin: true,
    }
    {
        key: "capture.retention_days",
        owner: "capture",
        schema: r#"{"type": "integer", "minimum": 0, "maximum": 36500}"#,
        default: "0",
        scopes: [Machine],
        admin: true,
    }
    {
        key: "capture.screenshots",
        owner: "capture",
        schema: r#"{"type": "string", "enum": ["allowed", "forbidden"]}"#,
        default: r#""allowed""#,
        scopes: [Machine],
        admin: true,
    }
    {
        key: "capture.window_capture",
        owner: "capture",
        schema: r#"{"type": "string", "enum": ["allowed", "forbidden"]}"#,
        default: r#""allowed""#,
        scopes: [Machine],
        admin: true,
    }
}

// ------------------------------------------------------------------ policy values

/// A capture medium that asks the policy before it captures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MediaKind {
    /// A screenshot of a viewport or NamedView (`VIZ.CAPTURE.SHOT`).
    Screenshot,
    /// The sampled camera track of a recording (`VIZ.CAPTURE.TRACK`).
    ViewTrack,
    /// Video of a viewport (`VIZ.CAPTURE.VIDEO`).
    ViewportVideo,
    /// Video of the whole window, panels included (`VIZ.CAPTURE.VIDEO`, `VIZ.CAPTURE.OSCAP`).
    WindowVideo,
}

impl MediaKind {
    /// Every kind.
    pub const ALL: [MediaKind; 4] =
        [MediaKind::Screenshot, MediaKind::ViewTrack, MediaKind::ViewportVideo, MediaKind::WindowVideo];

    /// The wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            MediaKind::Screenshot => "screenshot",
            MediaKind::ViewTrack => "view_track",
            MediaKind::ViewportVideo => "viewport_video",
            MediaKind::WindowVideo => "window_video",
        }
    }

    /// The kind with this exact wire string.
    pub fn parse(s: &str) -> Option<MediaKind> {
        MediaKind::ALL.into_iter().find(|k| k.as_str() == s)
    }

    /// Whether this medium is a recording (subject to `capture.recording` and retention).
    pub fn is_recording(self) -> bool {
        !matches!(self, MediaKind::Screenshot)
    }
}

impl fmt::Display for MediaKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The value of `capture.recording`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Recording {
    Allowed,
    Forbidden,
    Mandated,
}

impl Recording {
    /// The wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            Recording::Allowed => "allowed",
            Recording::Forbidden => "forbidden",
            Recording::Mandated => "mandated",
        }
    }

    /// The value with this exact wire string.
    pub fn parse(s: &str) -> Option<Recording> {
        [Recording::Allowed, Recording::Forbidden, Recording::Mandated].into_iter().find(|v| v.as_str() == s)
    }
}

/// The value of `capture.window_capture` and `capture.screenshots`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Permission {
    Allowed,
    Forbidden,
}

impl Permission {
    /// The wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            Permission::Allowed => "allowed",
            Permission::Forbidden => "forbidden",
        }
    }

    /// The value with this exact wire string.
    pub fn parse(s: &str) -> Option<Permission> {
        [Permission::Allowed, Permission::Forbidden].into_iter().find(|v| v.as_str() == s)
    }
}

/// One snapshot of the four policy keys. `None` means the value is not known (see the module
/// documentation); every capture that depends on an unknown value is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Policy {
    pub recording: Option<Recording>,
    pub window_capture: Option<Permission>,
    pub screenshots: Option<Permission>,
    /// Days; 0 keeps recordings.
    pub retention_days: Option<u32>,
}

impl Policy {
    /// Whether `capture.recording` is `mandated` (REQ-003). An unknown value is not.
    pub fn mandated(&self) -> bool {
        self.recording == Some(Recording::Mandated)
    }
}

/// Why a capture is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ForbidCause {
    /// The key's value forbids it.
    Forbidden,
    /// The key's value is not known; the policy fails closed.
    Unknown,
}

impl ForbidCause {
    /// Stable machine-readable code.
    pub fn code(self) -> &'static str {
        match self {
            ForbidCause::Forbidden => "forbidden",
            ForbidCause::Unknown => "unknown",
        }
    }
}

/// The answer of [`RecordingPolicy::check`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Decision {
    Allow,
    /// Refused because of `key` (one of [`POLICY_KEYS`]).
    Forbid {
        key: &'static str,
        cause: ForbidCause,
    },
}

impl Decision {
    /// Whether the capture may proceed.
    pub fn is_allowed(&self) -> bool {
        matches!(self, Decision::Allow)
    }
}

impl fmt::Display for Decision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Decision::Allow => f.write_str("allow"),
            Decision::Forbid { key, cause } => write!(f, "forbid:{key}:{}", cause.code()),
        }
    }
}

// ------------------------------------------------------------------ UTC instants

/// A UTC instant to the second, from 0001-01-01T00:00:00Z to 9999-12-31T23:59:59Z, written
/// `YYYY-MM-DDTHH:MM:SSZ` (RFC 3339, no fraction, no offset other than `Z`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UtcInstant {
    /// Seconds since 1970-01-01T00:00:00Z.
    secs: i64,
}

/// Days from 1970-01-01 to the given proleptic Gregorian date (H. Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The proleptic Gregorian date of a day count from 1970-01-01.
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        2 if is_leap(y) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

impl UtcInstant {
    /// The earliest instant, 0001-01-01T00:00:00Z.
    pub const MIN: UtcInstant = UtcInstant { secs: -62_135_596_800 };
    /// The latest instant, 9999-12-31T23:59:59Z.
    pub const MAX: UtcInstant = UtcInstant { secs: 253_402_300_799 };

    /// The instant `secs` seconds after 1970-01-01T00:00:00Z, if in range.
    pub fn from_unix_seconds(secs: i64) -> Option<UtcInstant> {
        (UtcInstant::MIN.secs..=UtcInstant::MAX.secs).contains(&secs).then_some(UtcInstant { secs })
    }

    /// Seconds since 1970-01-01T00:00:00Z.
    pub fn unix_seconds(self) -> i64 {
        self.secs
    }

    /// Parses exactly `YYYY-MM-DDTHH:MM:SSZ` with a valid calendar date and time.
    pub fn parse(s: &str) -> Option<UtcInstant> {
        let b = s.as_bytes();
        if b.len() != 20 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' || b[16] != b':' {
            return None;
        }
        if b[19] != b'Z' {
            return None;
        }
        let num = |from: usize, to: usize| -> Option<i64> {
            let digits = b.get(from..to)?;
            if !digits.iter().all(u8::is_ascii_digit) {
                return None;
            }
            Some(digits.iter().fold(0i64, |acc, c| acc * 10 + i64::from(c - b'0')))
        };
        let (y, mo, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
        let (h, mi, sec) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
        if y < 1 || !(1..=12).contains(&mo) || d < 1 || d > days_in_month(y, mo) || h > 23 || mi > 59 || sec > 59 {
            return None;
        }
        UtcInstant::from_unix_seconds(days_from_civil(y, mo, d) * SECONDS_PER_DAY + h * 3600 + mi * 60 + sec)
    }

    /// This instant plus `days` days of 86 400 s, if in range.
    pub fn plus_days(self, days: u32) -> Option<UtcInstant> {
        let delta = i64::from(days).checked_mul(SECONDS_PER_DAY)?;
        UtcInstant::from_unix_seconds(self.secs.checked_add(delta)?)
    }
}

impl fmt::Display for UtcInstant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let days = self.secs.div_euclid(SECONDS_PER_DAY);
        let rem = self.secs.rem_euclid(SECONDS_PER_DAY);
        let (y, m, d) = civil_from_days(days);
        write!(f, "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
    }
}

/// Why [`RecordingPolicy::retain_until`] has no answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RetentionError {
    /// `capture.retention_days` is not known; no recording may start (fail closed).
    Unknown,
    /// The end of retention is after [`UtcInstant::MAX`].
    OutOfRange,
}

impl RetentionError {
    /// Stable machine-readable code.
    pub fn code(self) -> &'static str {
        match self {
            RetentionError::Unknown => "retention_unknown",
            RetentionError::OutOfRange => "retention_out_of_range",
        }
    }
}

impl fmt::Display for RetentionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl std::error::Error for RetentionError {}

// ------------------------------------------------------------------ evaluation

/// The recording policy over one settings snapshot. [`Default`] is the policy with every value
/// unknown, which refuses every capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct RecordingPolicy {
    policy: Policy,
}

fn as_retention(v: &CValue) -> Option<u32> {
    let n = match v {
        CValue::U64(n) => *n,
        CValue::I64(n) => u64::try_from(*n).ok()?,
        // Exact: an integer below 2^53.
        CValue::Num(f) if f.is_finite() && f.fract() == 0.0 && *f >= 0.0 && *f < 9_007_199_254_740_992.0 => *f as u64,
        _ => return None,
    };
    u32::try_from(n).ok().filter(|d| *d <= MAX_RETENTION_DAYS)
}

fn as_str(v: &CValue) -> Option<&str> {
    match v {
        CValue::Str(s) => Some(s),
        _ => None,
    }
}

impl RecordingPolicy {
    /// Reads the four keys through the settings resolver with this module's own declarations
    /// ([`capture_settings`]), never declarations supplied by the caller, and returns the policy
    /// with the warnings of the read (for the Activity Log). If the declarations do not build,
    /// every value is unknown.
    pub fn from_settings(sources: &Sources<'_>) -> (RecordingPolicy, Vec<SettingWarning>) {
        let Ok(registry) = SettingsRegistry::build(&capture_settings()) else {
            return (RecordingPolicy::default(), Vec::new());
        };
        let (values, warnings) = resolve(&registry, sources, |d| POLICY_KEYS.contains(&d.key()));
        (RecordingPolicy::from_snapshot(&values, &warnings), warnings)
    }

    /// The policy from resolved values and the warnings of the same read (see the module
    /// documentation for when a value is unknown).
    pub fn from_snapshot(values: &[Resolved], warnings: &[SettingWarning]) -> RecordingPolicy {
        let machine_file_unusable = warnings
            .iter()
            .any(|w| w.scope == Scope::Machine && w.key.is_none() && w.reason == WarningReason::Unreadable);
        let value = |key: &str| -> Option<&CValue> {
            if machine_file_unusable {
                return None;
            }
            if warnings.iter().any(|w| w.scope == Scope::Machine && w.key.as_deref() == Some(key)) {
                return None;
            }
            let mut found = values.iter().filter(|r| r.key == key);
            let r = found.next()?;
            if found.next().is_some() || !matches!(r.source, Source::Machine | Source::Default) {
                return None;
            }
            Some(&r.value)
        };
        RecordingPolicy {
            policy: Policy {
                recording: value(RECORDING_KEY).and_then(as_str).and_then(Recording::parse),
                window_capture: value(WINDOW_CAPTURE_KEY).and_then(as_str).and_then(Permission::parse),
                screenshots: value(SCREENSHOTS_KEY).and_then(as_str).and_then(Permission::parse),
                retention_days: value(RETENTION_DAYS_KEY).and_then(as_retention),
            },
        }
    }

    /// The policy over an explicit snapshot (for callers that cache one).
    pub fn from_policy(policy: Policy) -> RecordingPolicy {
        RecordingPolicy { policy }
    }

    /// The snapshot, with [`Policy::mandated`] (REQ-003).
    pub fn current(&self) -> Policy {
        self.policy
    }

    /// Whether `kind` may be captured (REQ-002, fail closed). The keys are checked in this
    /// order and the first that refuses is reported:
    ///
    /// - `Screenshot`: `capture.screenshots`.
    /// - `ViewTrack`, `ViewportVideo`: `capture.recording`, then `capture.retention_days`
    ///   (refused only when unknown).
    /// - `WindowVideo`: `capture.recording`, `capture.window_capture`, then
    ///   `capture.retention_days` (refused only when unknown).
    pub fn check(&self, kind: MediaKind) -> Decision {
        let p = &self.policy;
        let permission = |key: &'static str, v: Option<Permission>| match v {
            Some(Permission::Allowed) => None,
            Some(Permission::Forbidden) => Some(Decision::Forbid { key, cause: ForbidCause::Forbidden }),
            None => Some(Decision::Forbid { key, cause: ForbidCause::Unknown }),
        };
        let recording = match p.recording {
            Some(Recording::Allowed | Recording::Mandated) => None,
            Some(Recording::Forbidden) => Some(Decision::Forbid { key: RECORDING_KEY, cause: ForbidCause::Forbidden }),
            None => Some(Decision::Forbid { key: RECORDING_KEY, cause: ForbidCause::Unknown }),
        };
        let retention = match p.retention_days {
            Some(_) => None,
            None => Some(Decision::Forbid { key: RETENTION_DAYS_KEY, cause: ForbidCause::Unknown }),
        };
        let refusal = match kind {
            MediaKind::Screenshot => permission(SCREENSHOTS_KEY, p.screenshots),
            MediaKind::ViewTrack | MediaKind::ViewportVideo => recording.or(retention),
            MediaKind::WindowVideo => recording.or(permission(WINDOW_CAPTURE_KEY, p.window_capture)).or(retention),
        };
        refusal.unwrap_or(Decision::Allow)
    }

    /// The end of retention of a recording started at `start` (REQ-004): `start` plus
    /// `capture.retention_days` days of 86 400 s, or `Ok(None)` when the value is 0 (keep).
    /// An unknown retention is an error, never "keep".
    pub fn retain_until(&self, start: UtcInstant) -> Result<Option<UtcInstant>, RetentionError> {
        match self.policy.retention_days {
            None => Err(RetentionError::Unknown),
            Some(0) => Ok(None),
            Some(days) => start.plus_days(days).map(Some).ok_or(RetentionError::OutOfRange),
        }
    }
}
