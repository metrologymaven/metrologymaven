//! Offline operation and telemetry consent (spec node `CORE.STO.PRIV`, CONSTITUTION C7).
//!
//! This module enforces two invariants:
//!
//! 1. **Local-first, offline, no-account operation** (REQ-CORE.STO.PRIV-001, -002): creating,
//!    opening, editing, saving, closing and reopening a project uses only local storage
//!    (`ProjectLocation`, `Store`, `BlobStore`, `FileStore`) with zero network interfaces and no
//!    user account or sign-in.
//! 2. **Explicit, human-only, revocable telemetry consent** (REQ-CORE.STO.PRIV-003 to -007):
//!    - Setting key [`TELEMETRY_CONSENT_KEY`] (`privacy.telemetry_consent`, user scope,
//!      `admin: false`) defaults to off (`{"consent": false}`) and is written only by
//!      `settings.set_telemetry_consent@1` (never by generic `settings.set_value@1` /
//!      `settings.reset_value@1` / `settings.set_admin_value@1`).
//!    - `settings.set_telemetry_consent@1` is W3, `human_only = true`, never projected to MCP
//!      (`mcp_projection = Never`, `exposure = [Cli, Gui]`), and commits only for a
//!      [`PrincipalKind::Human`] caller on `Surface::Cli | Surface::Gui`. Any `agent` or `script`
//!      principal fails with `SettingsError::Policy` and journals nothing.
//!    - Turning consent on records the RFC 8785 canonical SHA-256 digest of the active
//!      [`TelemetryDescription`] (`telemetry-description.json`). Effective consent reported by
//!      `settings.show_privacy@1` (R0, exposed on MCP as `settings_show_privacy`) is `true` only
//!      when stored consent is `true` **and** its recorded digest matches the active description's
//!      digest; otherwise effective consent is `false` with reason [`REASON_DESCRIPTION_CHANGED`].
//!    - Turning consent off invokes [`TelemetryController::stop`] synchronously before the command
//!      returns (REQ-CORE.STO.PRIV-006).

use crate::layout::{OpenMode, ProjectLocation};
use crate::settings::check::validate_value;
use crate::settings::commands::{Caller, ChangeEntry, JournalTarget, PrincipalKind, SettingsJournal};
use crate::settings::files::{remove_created, FileStore};
use crate::settings::{ActivityLog, Scope, SettingWarning, SettingsError, WarningReason};
use crate::txn::{Store, Value};
use fid_canon::{digest as canon_digest, parse_with_limits, to_canonical_bytes, CValue};
use fid_registry::{fid_command, fid_commands, FidSchema, Surface};
use fid_types::Digest;
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use std::path::Path;

/// User-scope setting key owned by `CORE.STO.PRIV`.
pub const TELEMETRY_CONSENT_KEY: &str = "privacy.telemetry_consent";
/// `settings.set_telemetry_consent@1` (W3, human-only, never projected to MCP).
pub const SET_TELEMETRY_CONSENT: &str = "settings.set_telemetry_consent@1";
/// `settings.show_privacy@1` (R0, exposed on MCP as `settings_show_privacy`).
pub const SHOW_PRIVACY: &str = "settings.show_privacy@1";
/// Reason reported by `settings.show_privacy@1` when stored consent lapsed because the shipped
/// telemetry description changed (REQ-CORE.STO.PRIV-005).
pub const REASON_DESCRIPTION_CHANGED: &str = "description_changed";
/// `format` of `telemetry-description.json`.
pub const TELEMETRY_DESCRIPTION_FORMAT: &str = "fid-telemetry-description/1";
/// `format` of `offline_session.json`.
pub const OFFLINE_SESSION_FORMAT: &str = "fid-offline-session/1";

/// Maximum size of a telemetry description JSON document, in bytes.
pub const MAX_DESCRIPTION_BYTES: usize = 64 * 1024;
/// Maximum size of an offline session script JSON document, in bytes.
pub const MAX_SESSION_BYTES: usize = 256 * 1024;
/// Maximum number of steps accepted in an offline session script.
pub const MAX_SESSION_STEPS: usize = 64;
/// Maximum size of parameters JSON accepted by privacy commands.
pub const MAX_PARAMS_BYTES: usize = 4096;
const MAX_JSON_DEPTH: usize = 16;

/// Canonical bytes of the shipped `telemetry-description.json`.
pub const SHIPPED_TELEMETRY_DESCRIPTION_BYTES: &[u8] = include_bytes!("../privacy/telemetry-description.json");

const CONSENT_SCHEMA_JSON: &str = r#"{"additionalProperties":false,"properties":{"changed_at":{"maxLength":64,"minLength":20,"type":"string"},"changed_by_kind":{"enum":["human"],"type":"string"},"consent":{"type":"boolean"},"description_digest":{"maxLength":71,"minLength":71,"type":"string"}},"required":["consent"],"type":"object"}"#;

crate::fid_settings! {
    pub fn privacy_settings;
    {
        key: "privacy.telemetry_consent",
        owner: "privacy",
        schema: r#"{"additionalProperties":false,"properties":{"changed_at":{"maxLength":64,"minLength":20,"type":"string"},"changed_by_kind":{"enum":["human"],"type":"string"},"consent":{"type":"boolean"},"description_digest":{"maxLength":71,"minLength":71,"type":"string"}},"required":["consent"],"type":"object"}"#,
        default: r#"{"consent":false}"#,
        scopes: [User],
        admin: false,
    }
}

// ------------------------------------------------------------------ telemetry description

/// Validated telemetry data description and its RFC 8785 canonical SHA-256 digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelemetryDescription {
    digest: Digest,
    canonical_bytes: Vec<u8>,
    version: u64,
    purpose: String,
}

impl TelemetryDescription {
    /// Parses, validates and canonically hashes a `telemetry-description.json` document.
    pub fn from_json_bytes(bytes: &[u8]) -> Result<TelemetryDescription, SettingsError> {
        let v = parse_with_limits(bytes, MAX_DESCRIPTION_BYTES, MAX_JSON_DEPTH).map_err(|e| {
            SettingsError::Validation(format!("telemetry description is not valid JSON ({})", e.code()))
        })?;
        let CValue::Obj(ref o) = v else {
            return Err(SettingsError::Validation("telemetry description must be a JSON object".into()));
        };
        let keys: Vec<&str> = o.keys().map(String::as_str).collect();
        if keys != ["collected", "excluded", "format", "purpose", "retention_days", "version"] {
            return Err(SettingsError::Validation(
                "telemetry description must have exact fields collected, excluded, format, purpose, retention_days, version"
                    .into(),
            ));
        }
        if o.get("format") != Some(&CValue::Str(TELEMETRY_DESCRIPTION_FORMAT.to_string())) {
            return Err(SettingsError::Validation(format!(
                "telemetry description format must be {TELEMETRY_DESCRIPTION_FORMAT}"
            )));
        }
        let purpose = match o.get("purpose") {
            Some(CValue::Str(s)) if !s.is_empty() && s.len() <= 1024 => s.clone(),
            _ => {
                return Err(SettingsError::Validation(
                    "telemetry description purpose must be a non-empty string".into(),
                ))
            }
        };
        let version = exact_non_neg_int(o.get("version"), "version")?;
        if version < 1 {
            return Err(SettingsError::Validation("telemetry description version must be >= 1".into()));
        }
        let _retention_days = exact_non_neg_int(o.get("retention_days"), "retention_days")?;
        check_string_list(o.get("collected"), "collected")?;
        check_string_list(o.get("excluded"), "excluded")?;

        let canonical_bytes = to_canonical_bytes(&v).map_err(|e| {
            SettingsError::Validation(format!("telemetry description has no canonical form ({})", e.code()))
        })?;
        let raw_digest = canon_digest(&v)
            .map_err(|e| SettingsError::Validation(format!("telemetry description digest failed ({})", e.code())))?;
        let digest = Digest::parse(&raw_digest.to_string())
            .map_err(|e| SettingsError::Validation(format!("invalid telemetry description digest ({e})")))?;
        Ok(TelemetryDescription { digest, canonical_bytes, version, purpose })
    }

    /// Loads the `telemetry-description.json` shipped with this crate.
    pub fn shipped() -> Result<TelemetryDescription, SettingsError> {
        TelemetryDescription::from_json_bytes(SHIPPED_TELEMETRY_DESCRIPTION_BYTES)
    }

    /// RFC 8785 canonical SHA-256 digest (`sha256:<64 hex>`).
    pub fn digest(&self) -> &Digest {
        &self.digest
    }

    /// Canonical RFC 8785 JSON bytes of the description.
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical_bytes
    }

    /// Monotonic description version number.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Human-readable purpose statement.
    pub fn purpose(&self) -> &str {
        &self.purpose
    }
}

fn exact_non_neg_int(v: Option<&CValue>, name: &str) -> Result<u64, SettingsError> {
    match v {
        Some(CValue::Num(n)) if n.is_finite() && n.fract() == 0.0 && *n >= 0.0 && *n <= 9_007_199_254_740_992.0 => {
            // Safe bounded cast: checked finite, integral, non-negative, <= 2^53.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let u = *n as u64;
            Ok(u)
        }
        _ => Err(SettingsError::Validation(format!("telemetry description {name} must be a non-negative integer"))),
    }
}

fn check_string_list(v: Option<&CValue>, name: &str) -> Result<(), SettingsError> {
    let Some(CValue::Arr(items)) = v else {
        return Err(SettingsError::Validation(format!("telemetry description {name} must be an array")));
    };
    if items.len() > 128 {
        return Err(SettingsError::Validation(format!("telemetry description {name} has more than 128 entries")));
    }
    for item in items {
        match item {
            CValue::Str(s) if !s.is_empty() && s.len() <= 256 => {}
            _ => {
                return Err(SettingsError::Validation(format!(
                    "telemetry description {name} entries must be non-empty strings"
                )))
            }
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ telemetry controller & context

/// Hook for stopping any active telemetry sender before `settings.set_telemetry_consent@1`
/// returns with `consent: false` (REQ-CORE.STO.PRIV-006).
pub trait TelemetryController {
    /// Stops any active telemetry synchronously before returning.
    fn stop(&self);
}

/// Default no-op controller when no telemetry sender is attached.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopTelemetry;

impl TelemetryController for NoopTelemetry {
    fn stop(&self) {}
}

/// Context passed to `settings.set_telemetry_consent@1` and `settings.show_privacy@1`.
pub struct PrivacyCtx<'a> {
    pub caller: Caller,
    pub user: &'a FileStore,
    pub journal: &'a dyn SettingsJournal,
    pub log: &'a dyn ActivityLog,
    pub description: &'a TelemetryDescription,
    pub telemetry: &'a dyn TelemetryController,
    /// Explicit RFC 3339 UTC timestamp supplied by the host (D0 determinism: no hidden clock
    /// reads inside handlers).
    pub now: &'a str,
}

// ------------------------------------------------------------------ parameter & result types

/// Parameters of `settings.set_telemetry_consent@1`.
#[derive(FidSchema, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetTelemetryConsentParams {
    /// `true` to opt in under the shipped telemetry description; `false` to opt out and stop
    /// any telemetry before returning.
    pub consent: bool,
}

/// Result of `settings.set_telemetry_consent@1`.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct SetTelemetryConsentResult {
    pub consent: bool,
    #[fid(min_length = 71, max_length = 71)]
    pub description_digest: String,
}

/// Parameters of `settings.show_privacy@1`.
#[derive(FidSchema, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ShowPrivacyParams {}

/// Result of `settings.show_privacy@1`.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct ShowPrivacyResult {
    /// Effective telemetry consent (`true` only when stored consent is `true` and its recorded
    /// description digest equals the shipped description digest).
    pub consent: bool,
    /// Present (`"description_changed"`) when stored consent was `true` for a different
    /// description digest.
    #[fid(max_length = 64)]
    pub reason: Option<String>,
    /// RFC 3339 UTC timestamp of the last recorded consent change, if any.
    #[fid(min_length = 20, max_length = 64)]
    pub changed_at: Option<String>,
    /// Principal kind of the last recorded consent change (`"human"`), if any.
    #[fid(max_length = 32)]
    pub changed_by_kind: Option<String>,
    /// RFC 8785 canonical SHA-256 digest of the shipped telemetry data description in force.
    #[fid(min_length = 71, max_length = 71)]
    pub description_digest: String,
}

fn obj(pairs: Vec<(&str, CValue)>) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn text(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

impl SetTelemetryConsentParams {
    pub fn from_value(v: &CValue) -> Result<SetTelemetryConsentParams, SettingsError> {
        let CValue::Obj(o) = v else {
            return Err(SettingsError::Validation("parameters are an object".into()));
        };
        if o.len() != 1 || !o.contains_key("consent") {
            return Err(SettingsError::Validation("parameters must contain exactly consent".into()));
        }
        match o.get("consent") {
            Some(CValue::Bool(consent)) => Ok(SetTelemetryConsentParams { consent: *consent }),
            _ => Err(SettingsError::Validation("consent must be a boolean".into())),
        }
    }

    pub fn from_json(bytes: &[u8]) -> Result<SetTelemetryConsentParams, SettingsError> {
        let v = parse_with_limits(bytes, MAX_PARAMS_BYTES, MAX_JSON_DEPTH)
            .map_err(|e| SettingsError::Validation(format!("parameters are not JSON ({})", e.code())))?;
        SetTelemetryConsentParams::from_value(&v)
    }
}

impl SetTelemetryConsentResult {
    pub fn to_value(&self) -> CValue {
        obj(vec![("consent", CValue::Bool(self.consent)), ("description_digest", text(&self.description_digest))])
    }
}

impl ShowPrivacyParams {
    pub fn from_value(v: &CValue) -> Result<ShowPrivacyParams, SettingsError> {
        let CValue::Obj(o) = v else {
            return Err(SettingsError::Validation("parameters are an object".into()));
        };
        if !o.is_empty() {
            return Err(SettingsError::Validation("settings.show_privacy@1 takes no parameters".into()));
        }
        Ok(ShowPrivacyParams {})
    }

    pub fn from_json(bytes: &[u8]) -> Result<ShowPrivacyParams, SettingsError> {
        let v = parse_with_limits(bytes, MAX_PARAMS_BYTES, MAX_JSON_DEPTH)
            .map_err(|e| SettingsError::Validation(format!("parameters are not JSON ({})", e.code())))?;
        ShowPrivacyParams::from_value(&v)
    }
}

impl ShowPrivacyResult {
    pub fn to_value(&self) -> CValue {
        let mut pairs =
            vec![("consent", CValue::Bool(self.consent)), ("description_digest", text(&self.description_digest))];
        if let Some(reason) = &self.reason {
            pairs.push(("reason", text(reason)));
        }
        if let Some(changed_at) = &self.changed_at {
            pairs.push(("changed_at", text(changed_at)));
        }
        if let Some(changed_by_kind) = &self.changed_by_kind {
            pairs.push(("changed_by_kind", text(changed_by_kind)));
        }
        obj(pairs)
    }
}

// ------------------------------------------------------------------ stored consent parsing

#[derive(Debug, Clone, PartialEq, Eq)]
struct StoredConsent {
    consent: bool,
    description_digest: Option<String>,
    changed_at: Option<String>,
    changed_by_kind: Option<String>,
}

fn consent_schema() -> Result<CValue, SettingsError> {
    parse_with_limits(CONSENT_SCHEMA_JSON.as_bytes(), 4096, MAX_JSON_DEPTH)
        .map_err(|e| SettingsError::Validation(format!("consent schema invalid ({})", e.code())))
}

fn parse_stored_consent(v: &CValue, schema: &CValue) -> Option<StoredConsent> {
    validate_value(schema, v).ok()?;
    let CValue::Obj(o) = v else { return None };
    let Some(CValue::Bool(consent)) = o.get("consent") else { return None };
    let description_digest = match o.get("description_digest") {
        None => None,
        Some(CValue::Str(s)) if Digest::parse(s).is_ok() => Some(s.clone()),
        Some(_) => return None,
    };
    let changed_at = match o.get("changed_at") {
        None => None,
        Some(CValue::Str(s)) if is_rfc3339_utc(s) => Some(s.clone()),
        Some(_) => return None,
    };
    let changed_by_kind = match o.get("changed_by_kind") {
        None => None,
        Some(CValue::Str(s)) if s == PrincipalKind::Human.as_str() => Some(s.clone()),
        Some(_) => return None,
    };
    Some(StoredConsent { consent: *consent, description_digest, changed_at, changed_by_kind })
}

/// Validates an RFC 3339 UTC timestamp (`YYYY-MM-DDTHH:MM:SS(.frac)?Z`, 20..=64 ASCII bytes).
pub fn is_rfc3339_utc(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() < 20 || b.len() > 64 || !b.is_ascii() {
        return false;
    }
    let digits = |slice: &[u8]| slice.iter().all(u8::is_ascii_digit);
    let pair = |a: u8, c: u8| -> Option<u8> {
        if a.is_ascii_digit() && c.is_ascii_digit() {
            Some((a - b'0') * 10 + (c - b'0'))
        } else {
            None
        }
    };
    if !digits(&b[0..4]) || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' || b[16] != b':' {
        return false;
    }
    let Some(month) = pair(b[5], b[6]) else { return false };
    let Some(day) = pair(b[8], b[9]) else { return false };
    let Some(hour) = pair(b[11], b[12]) else { return false };
    let Some(min) = pair(b[14], b[15]) else { return false };
    let Some(sec) = pair(b[17], b[18]) else { return false };
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || min > 59 || sec > 60 {
        return false;
    }
    let tail = &b[19..];
    if tail == b"Z" {
        return true;
    }
    if tail.len() >= 3 && tail[0] == b'.' && tail.last() == Some(&b'Z') {
        let frac = &tail[1..tail.len() - 1];
        return (1..=9).contains(&frac.len()) && digits(frac);
    }
    false
}

// ------------------------------------------------------------------ commands

#[fid_command(
    id = "settings.set_telemetry_consent",
    major = 1,
    title = "Set telemetry consent",
    description = "Records an explicit human opt-in or opt-out for telemetry tied to the shipped \
                   telemetry description digest. Human principals on CLI or GUI only; never on MCP.",
    side_effect = W3,
    mcp_projection = Never,
    determinism = D0,
    idempotency = Natural,
    fs_effect = None,
    undo = NotUndoable,
    preconditions = [],
    effects = [SET_TELEMETRY_CONSENT],
    algorithms = [],
    exposure = [Cli, Gui],
    lifecycle(state = "active", since = "2026-11-22"),
    human_only = true,
    web = Unavailable,
    web_note = "Desktop only.",
)]
pub fn set_telemetry_consent(
    ctx: &PrivacyCtx<'_>,
    params: SetTelemetryConsentParams,
) -> Result<SetTelemetryConsentResult, SettingsError> {
    // 1. Surface and principal check (REQ-CORE.STO.PRIV-004, AC-3, AC-4).
    if !matches!(ctx.caller.surface, Surface::Cli | Surface::Gui) {
        return Err(SettingsError::Policy(format!(
            "{SET_TELEMETRY_CONSENT} is available only from the CLI and the GUI"
        )));
    }
    if ctx.caller.kind != PrincipalKind::Human {
        return Err(SettingsError::Policy(format!("{SET_TELEMETRY_CONSENT} commits only for a human principal")));
    }
    // 2. Scope check: telemetry consent is a user-scope setting.
    if ctx.user.scope() != Scope::User {
        return Err(SettingsError::ScopeNotAllowed(format!("{TELEMETRY_CONSENT_KEY} is stored only in scope user")));
    }
    // 3. Explicit timestamp validation (D0 determinism).
    if !is_rfc3339_utc(ctx.now) {
        return Err(SettingsError::Validation("changed_at timestamp must be RFC 3339 UTC".into()));
    }
    let schema = consent_schema()?;
    let digest_str = ctx.description.digest().to_string();
    let stored_value = obj(vec![
        ("changed_at", text(ctx.now)),
        ("changed_by_kind", text(PrincipalKind::Human.as_str())),
        ("consent", CValue::Bool(params.consent)),
        ("description_digest", text(&digest_str)),
    ]);
    validate_value(&schema, &stored_value)
        .map_err(|e| SettingsError::Validation(format!("consent record fails schema {e}")))?;

    // 4. Verify the user settings file is writable before committing to the journal.
    let created = ctx.user.check_writable()?;

    // 5. Look up previous valid stored value (if any).
    let loaded = ctx.user.load();
    let previous =
        loaded.values.get(TELEMETRY_CONSENT_KEY).filter(|v| parse_stored_consent(v, &schema).is_some()).cloned();

    // 6. Commit one W3 journal entry to the per-user settings journal.
    let entry = ChangeEntry::new(
        SET_TELEMETRY_CONSENT,
        TELEMETRY_CONSENT_KEY.to_string(),
        Scope::User,
        Some(stored_value.clone()),
        previous,
        PrincipalKind::Human,
        None,
    );
    if let Err(e) = ctx.journal.commit(&JournalTarget::UserSettings, &entry) {
        remove_created(&created);
        return Err(SettingsError::Journal(e));
    }

    // 7. Persist the updated setting atomically after the journal commit.
    ctx.user.write_value(TELEMETRY_CONSENT_KEY, Some(&stored_value))?;

    // 8. Stop any active telemetry synchronously before returning when consent is off (REQ-006).
    if !params.consent {
        ctx.telemetry.stop();
    }

    Ok(SetTelemetryConsentResult { consent: params.consent, description_digest: digest_str })
}

#[fid_command(
    id = "settings.show_privacy",
    major = 1,
    title = "Show privacy and telemetry consent status",
    description = "Reports effective telemetry consent, any lapse reason, last change time and \
                   principal kind, and the digest of the shipped telemetry data description.",
    side_effect = R0,
    mcp_projection = Exposed,
    determinism = D0,
    idempotency = Natural,
    fs_effect = None,
    undo = NotUndoable,
    preconditions = [],
    effects = [],
    algorithms = [],
    exposure = [Cli, Mcp, Python, Gui],
    lifecycle(state = "active", since = "2026-11-22"),
    web = Available,
)]
pub fn show_privacy(ctx: &PrivacyCtx<'_>, _params: ShowPrivacyParams) -> Result<ShowPrivacyResult, SettingsError> {
    let current_digest = ctx.description.digest().to_string();
    let default_off = ShowPrivacyResult {
        consent: false,
        reason: None,
        changed_at: None,
        changed_by_kind: None,
        description_digest: current_digest.clone(),
    };
    let loaded = ctx.user.load();
    if loaded.unreadable {
        ctx.log.warn(&SettingWarning { key: None, scope: ctx.user.scope(), reason: WarningReason::Unreadable });
        return Ok(default_off);
    }
    let Some(raw) = loaded.values.get(TELEMETRY_CONSENT_KEY) else {
        return Ok(default_off);
    };
    let schema = consent_schema()?;
    let Some(stored) = parse_stored_consent(raw, &schema) else {
        ctx.log.warn(&SettingWarning {
            key: Some(TELEMETRY_CONSENT_KEY.to_string()),
            scope: ctx.user.scope(),
            reason: WarningReason::InvalidValue,
        });
        return Ok(default_off);
    };
    if !stored.consent {
        return Ok(ShowPrivacyResult {
            consent: false,
            reason: None,
            changed_at: stored.changed_at,
            changed_by_kind: stored.changed_by_kind,
            description_digest: current_digest,
        });
    }
    if stored.description_digest.as_deref() == Some(current_digest.as_str()) {
        Ok(ShowPrivacyResult {
            consent: true,
            reason: None,
            changed_at: stored.changed_at,
            changed_by_kind: stored.changed_by_kind,
            description_digest: current_digest,
        })
    } else {
        Ok(ShowPrivacyResult {
            consent: false,
            reason: Some(REASON_DESCRIPTION_CHANGED.to_string()),
            changed_at: stored.changed_at,
            changed_by_kind: stored.changed_by_kind,
            description_digest: current_digest,
        })
    }
}

fid_commands![set_telemetry_consent, show_privacy];

// ------------------------------------------------------------------ offline session harness (AC-1)

/// Summary of an executed offline session script (`fixtures/core/sto/priv/offline_session.json`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfflineSessionReport {
    pub project_name: String,
    pub steps_completed: usize,
    pub entries_persisted: i64,
    pub blobs_verified: usize,
    pub last_blob_digest: Option<String>,
    pub initial_consent: bool,
}

struct NullJournal;
impl SettingsJournal for NullJournal {
    fn commit(&self, _target: &JournalTarget, _entry: &ChangeEntry) -> Result<(), String> {
        Err("offline session is read-only for settings".into())
    }
}

struct NullLog;
impl ActivityLog for NullLog {
    fn warn(&self, _warning: &SettingWarning) {}
}

/// Runs an offline session script (`fid-offline-session/1`) end-to-end against local storage
/// under `workspace_dir` and `user_settings_path` with zero network calls and no user account
/// (REQ-CORE.STO.PRIV-001, -002; AC-1).
pub fn run_offline_session(
    script_bytes: &[u8],
    workspace_dir: &Path,
    user_settings_path: &Path,
) -> Result<OfflineSessionReport, SettingsError> {
    let v = parse_with_limits(script_bytes, MAX_SESSION_BYTES, MAX_JSON_DEPTH)
        .map_err(|e| SettingsError::Validation(format!("offline session is not valid JSON ({})", e.code())))?;
    let CValue::Obj(root) = v else {
        return Err(SettingsError::Validation("offline session must be a JSON object".into()));
    };
    if root.get("format") != Some(&CValue::Str(OFFLINE_SESSION_FORMAT.to_string())) {
        return Err(SettingsError::Validation(format!("offline session format must be {OFFLINE_SESSION_FORMAT}")));
    }
    let project_name = match root.get("project_name") {
        Some(CValue::Str(s))
            if !s.is_empty()
                && s.len() <= 64
                && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') =>
        {
            s.clone()
        }
        _ => return Err(SettingsError::Validation("offline session project_name must be safe ASCII".into())),
    };
    let Some(CValue::Arr(steps)) = root.get("steps") else {
        return Err(SettingsError::Validation("offline session steps must be an array".into()));
    };
    if steps.is_empty() || steps.len() > MAX_SESSION_STEPS {
        return Err(SettingsError::Validation(format!(
            "offline session must have between 1 and {MAX_SESSION_STEPS} steps"
        )));
    }

    let project_dir = workspace_dir.join(&project_name);
    let user_store = FileStore::new(Scope::User, user_settings_path.to_path_buf())?;
    let description = TelemetryDescription::shipped()?;
    let mut open_store: Option<Store> = None;
    let mut seq: u64 = 0;
    let mut stored_blobs: Vec<(crate::cas::Address, Vec<u8>)> = Vec::new();
    let mut last_blob_digest: Option<String> = None;
    let mut initial_consent = false;
    let mut entries_persisted: i64 = 0;
    let mut blobs_verified: usize = 0;

    for (idx, step) in steps.iter().enumerate() {
        let CValue::Obj(s) = step else {
            return Err(SettingsError::Validation(format!("step {idx} must be an object")));
        };
        let Some(CValue::Str(op)) = s.get("op") else {
            return Err(SettingsError::Validation(format!("step {idx} missing op")));
        };
        match op.as_str() {
            "show_privacy" => {
                let ctx = PrivacyCtx {
                    caller: Caller { kind: PrincipalKind::Human, surface: Surface::Cli },
                    user: &user_store,
                    journal: &NullJournal,
                    log: &NullLog,
                    description: &description,
                    telemetry: &NoopTelemetry,
                    now: "2026-11-22T00:00:00Z",
                };
                let res = show_privacy(&ctx, ShowPrivacyParams {})?;
                initial_consent = res.consent;
                if let Some(CValue::Bool(expected)) = s.get("expect_consent") {
                    if res.consent != *expected {
                        return Err(SettingsError::Validation(format!(
                            "step {idx}: expected consent {expected}, got {}",
                            res.consent
                        )));
                    }
                }
            }
            "create_project" => {
                let (_loc, lock) = ProjectLocation::create(&project_dir)
                    .map_err(|e| SettingsError::Storage(format!("create_project failed: {e}")))?;
                drop(lock);
                let store = Store::open(&project_dir, OpenMode::Write)
                    .map_err(|e| SettingsError::Storage(format!("open created project failed: {e}")))?;
                open_store = Some(store);
            }
            "import_element" | "edit_element" => {
                let Some(store) = open_store.as_ref() else {
                    return Err(SettingsError::Validation(format!("step {idx}: project is not open")));
                };
                let element_id = req_str(s, "element_id", idx)?;
                let kind = req_str(s, "kind", idx)?;
                let payload = req_str(s, "payload", idx)?;
                seq += 1;
                let entry_hash = sha256_hex(format!("{seq}:{op}:{element_id}:{payload}").as_bytes());
                let header = format!(r#"{{"element_id":"{element_id}","op":"{op}","seq":{seq}}}"#);
                let mut txn = store.transaction();
                let blob_addr = txn
                    .put_blob(payload.as_bytes().to_vec())
                    .map_err(|e| SettingsError::Storage(format!("step {idx} put_blob failed: {e}")))?;
                let blob_digest = blob_addr.digest();
                txn.insert(
                    "entries",
                    &[
                        ("hash", Value::from(entry_hash.as_str())),
                        ("stream", Value::from("record")),
                        ("branch", Value::from("main")),
                        ("kind", Value::from(kind)),
                        ("header", Value::Blob(header.into_bytes())),
                    ],
                );
                txn.insert(
                    "entry_blobs",
                    &[
                        ("entry_hash", Value::from(entry_hash.as_str())),
                        ("role", Value::from(op.as_str())),
                        ("digest", Value::from(blob_digest)),
                    ],
                );
                txn.commit().map_err(|e| SettingsError::Storage(format!("step {idx} commit failed: {e}")))?;
                last_blob_digest = Some(blob_digest.to_string());
                stored_blobs.push((blob_addr, payload.as_bytes().to_vec()));
            }
            "save_project" => {
                let Some(store) = open_store.as_ref() else {
                    return Err(SettingsError::Validation(format!("step {idx}: project is not open")));
                };
                // Verify durability pragmas and readable committed rows on the open store.
                let count: i64 = store
                    .read(|c| c.query_row("SELECT count(*) FROM entries", [], |r| r.get(0)))
                    .map_err(|e| SettingsError::Storage(format!("step {idx} save check failed: {e}")))?;
                entries_persisted = count;
            }
            "close_project" => {
                if open_store.take().is_none() {
                    return Err(SettingsError::Validation(format!("step {idx}: project is already closed")));
                }
            }
            "reopen_project" => {
                if open_store.is_some() {
                    return Err(SettingsError::Validation(format!("step {idx}: close project before reopen")));
                }
                let store = Store::open(&project_dir, OpenMode::Write)
                    .map_err(|e| SettingsError::Storage(format!("step {idx} reopen failed: {e}")))?;
                let count: i64 = store
                    .read(|c| c.query_row("SELECT count(*) FROM entries", [], |r| r.get(0)))
                    .map_err(|e| SettingsError::Storage(format!("step {idx} count entries failed: {e}")))?;
                entries_persisted = count;
                if let Some(expected_n) = s.get("expect_entries") {
                    let expected = exact_non_neg_int(Some(expected_n), "expect_entries")?;
                    if u64::try_from(count).ok() != Some(expected) {
                        return Err(SettingsError::Validation(format!(
                            "step {idx}: expected {expected} entries after reopen, got {count}"
                        )));
                    }
                }
                blobs_verified = 0;
                for (addr, expected_bytes) in &stored_blobs {
                    let actual = store
                        .blobs()
                        .get(addr)
                        .map_err(|e| SettingsError::Storage(format!("step {idx} verify blob failed: {e}")))?;
                    if actual != *expected_bytes {
                        return Err(SettingsError::Storage(format!("step {idx}: blob content mismatch after reopen")));
                    }
                    blobs_verified += 1;
                }
                open_store = Some(store);
            }
            other => {
                return Err(SettingsError::Validation(format!("step {idx}: unknown offline session op {other:?}")));
            }
        }
    }

    drop(open_store);
    Ok(OfflineSessionReport {
        project_name,
        steps_completed: steps.len(),
        entries_persisted,
        blobs_verified,
        last_blob_digest,
        initial_consent,
    })
}

fn req_str<'a>(map: &'a BTreeMap<String, CValue>, field: &str, idx: usize) -> Result<&'a str, SettingsError> {
    match map.get(field) {
        Some(CValue::Str(s)) if !s.is_empty() => Ok(s.as_str()),
        _ => Err(SettingsError::Validation(format!("step {idx}: {field} must be a non-empty string"))),
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut out = String::from("sha256:");
    for b in Sha256::digest(bytes) {
        use std::fmt::Write as _;
        let _ = write!(out, "{b:02x}");
    }
    out
}
