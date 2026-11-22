//! Acceptance criteria of spec node CORE.STO.PRIV (AC-1 .. AC-6) and its requirements
//! (REQ-CORE.STO.PRIV-001 .. -007).
//!
//! Expected digests come from `fixtures/core/sto/priv/make_fixtures.py` (Python stdlib
//! `json` + `hashlib`), never from this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, CValue};
use fid_registry::validate::mcp_projection;
use fid_registry::{validate, Projection, SideEffect, Surface};
use fid_store::privacy::{
    declarations, privacy_settings, run_offline_session, set_telemetry_consent, show_privacy, NoopTelemetry,
    PrivacyCtx, SetTelemetryConsentParams, ShowPrivacyParams, TelemetryController, TelemetryDescription,
    REASON_DESCRIPTION_CHANGED, SET_TELEMETRY_CONSENT, SHOW_PRIVACY, TELEMETRY_CONSENT_KEY,
};
use fid_store::settings::{
    list_values, reset_value, set_admin_value, set_value, ActivityLog, Caller, ChangeEntry, FileStore, JournalTarget,
    ListValuesParams, PrincipalKind, ResetValueParams, Scope, SetValueParams, SettingValue, SettingWarning,
    SettingsCtx, SettingsError, SettingsJournal, SettingsRegistry, Source, WarningReason,
};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::Mutex;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/core/sto/priv");

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        static N: AtomicU32 = AtomicU32::new(0);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos();
        let name = format!("fid-priv-{tag}-{}-{nanos}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst));
        let p = std::path::absolute(std::env::temp_dir().join(name)).unwrap();
        fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }

    fn user_store(&self) -> FileStore {
        FileStore::new(Scope::User, self.0.join("user").join("fiducial").join("settings.json")).unwrap()
    }

    fn machine_store(&self) -> FileStore {
        FileStore::new(Scope::Machine, self.0.join("machine").join("fiducial").join("settings.json")).unwrap()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Default)]
struct RecordingJournal {
    entries: Mutex<Vec<(JournalTarget, ChangeEntry)>>,
}

impl RecordingJournal {
    fn snapshot(&self) -> Vec<(JournalTarget, ChangeEntry)> {
        self.entries.lock().unwrap().clone()
    }
}

impl SettingsJournal for RecordingJournal {
    fn commit(&self, target: &JournalTarget, entry: &ChangeEntry) -> Result<(), String> {
        self.entries.lock().unwrap().push((target.clone(), entry.clone()));
        Ok(())
    }
}

#[derive(Default)]
struct RecordingLog {
    warnings: Mutex<Vec<SettingWarning>>,
}

impl RecordingLog {
    fn snapshot(&self) -> Vec<SettingWarning> {
        self.warnings.lock().unwrap().clone()
    }
}

impl ActivityLog for RecordingLog {
    fn warn(&self, warning: &SettingWarning) {
        self.warnings.lock().unwrap().push(warning.clone());
    }
}

#[derive(Default)]
struct RecordingTelemetry {
    stops: AtomicUsize,
}

impl RecordingTelemetry {
    fn stop_count(&self) -> usize {
        self.stops.load(Ordering::SeqCst)
    }
}

impl TelemetryController for RecordingTelemetry {
    fn stop(&self) {
        self.stops.fetch_add(1, Ordering::SeqCst);
    }
}

fn read_fixture(name: &str) -> Vec<u8> {
    fs::read(Path::new(FIXTURES).join(name)).unwrap_or_else(|e| panic!("read fixture {name}: {e}"))
}

fn expected_json() -> std::collections::BTreeMap<String, CValue> {
    let bytes = read_fixture("expected.json");
    let CValue::Obj(o) = parse(&bytes).unwrap() else { panic!("expected.json must be an object") };
    o
}

fn expected_str(map: &std::collections::BTreeMap<String, CValue>, key: &str) -> String {
    match map.get(key) {
        Some(CValue::Str(s)) => s.clone(),
        other => panic!("expected string for {key}, got {other:?}"),
    }
}

fn expected_num(map: &std::collections::BTreeMap<String, CValue>, key: &str) -> i64 {
    match map.get(key) {
        Some(CValue::Num(n)) => *n as i64,
        other => panic!("expected number for {key}, got {other:?}"),
    }
}

// ------------------------------------------------------------------ AC-1: offline operation

/// AC-1: runs `fixtures/core/sto/priv/offline_session.json` (show privacy, create project,
/// import a test-kit element, edit, save, close, reopen) with no network and no account.
/// When executed inside a Linux network namespace (`FID_IN_OFFLINE_NETNS=1`), also asserts
/// that no active network interfaces exist and an outbound TCP connect fails.
#[test]
fn ac1_offline_session_runs_to_completion() {
    #[cfg(target_os = "linux")]
    if std::env::var_os("FID_IN_OFFLINE_NETNS").is_some() {
        let route_table = fs::read_to_string("/proc/net/route").unwrap();
        let active_routes: Vec<&str> = route_table.lines().skip(1).map(str::trim).filter(|l| !l.is_empty()).collect();
        assert!(active_routes.is_empty(), "expected zero routes inside isolated netns, found: {active_routes:?}");
        assert!(
            std::net::TcpStream::connect("192.0.2.1:80").is_err(),
            "outbound network connect must fail inside isolated netns"
        );
    }

    let tmp = TempDir::new("ac1");
    let expected = expected_json();
    let script = read_fixture("offline_session.json");
    let report = run_offline_session(&script, &tmp.0, &tmp.0.join("cfg").join("settings.json")).unwrap();

    assert_eq!(report.project_name, "offline_gauge_block");
    assert!(!report.initial_consent);
    assert_eq!(report.steps_completed as i64, expected_num(&expected, "offline_steps"));
    assert_eq!(report.entries_persisted, expected_num(&expected, "offline_entries"));
    assert_eq!(report.blobs_verified as i64, expected_num(&expected, "offline_blobs"));
    assert_eq!(report.last_blob_digest.as_deref(), Some(expected_str(&expected, "edit_blob_digest").as_str()));
}

/// AC-1 on Linux: re-executes `ac1_offline_session_runs_to_completion` inside an unprivileged
/// `unshare -r -n` network namespace whenever the host kernel permits unprivileged user
/// namespaces (CI also runs `bash crates/fid-store/tests/offline.sh` using `sudo unshare --net`).
#[cfg(target_os = "linux")]
#[test]
fn ac1_offline_session_in_linux_netns() {
    if std::env::var_os("FID_IN_OFFLINE_NETNS").is_some() {
        return;
    }
    let exe = std::env::current_exe().unwrap();
    let out = std::process::Command::new("unshare")
        .args(["-r", "-n", "--"])
        .arg(&exe)
        .args(["--exact", "ac1_offline_session_runs_to_completion", "--nocapture"])
        .env("FID_IN_OFFLINE_NETNS", "1")
        .output();
    match out {
        Ok(o) if o.status.success() => {}
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            if stderr.contains("Operation not permitted") || stderr.contains("Permission denied") {
                // Restricted unprivileged userns (e.g. AppArmor on Ubuntu 24.04 runners);
                // Linux CI exercises this path with `sudo unshare --net` via tests/offline.sh.
                return;
            }
            panic!(
                "offline netns test failed (status {}):\nstdout:\n{}\nstderr:\n{}",
                o.status,
                String::from_utf8_lossy(&o.stdout),
                stderr
            );
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => panic!("failed to spawn unshare: {e}"),
    }
}

// ------------------------------------------------------------------ AC-2: fresh install off

/// AC-2: on a fresh installation, `settings.show_privacy@1` reports consent `false` (off) with
/// the shipped telemetry description digest.
#[test]
fn ac2_fresh_install_show_privacy_reports_off() {
    let tmp = TempDir::new("ac2");
    let user = tmp.user_store();
    let journal = RecordingJournal::default();
    let log = RecordingLog::default();
    let desc = TelemetryDescription::shipped().unwrap();
    let expected = expected_json();
    let v1_digest = expected_str(&expected, "v1_digest");

    let ctx = PrivacyCtx {
        caller: Caller { kind: PrincipalKind::Human, surface: Surface::Cli },
        user: &user,
        journal: &journal,
        log: &log,
        description: &desc,
        telemetry: &NoopTelemetry,
        now: "2026-11-22T00:00:00Z",
    };
    let res = show_privacy(&ctx, ShowPrivacyParams {}).unwrap();
    assert!(!res.consent);
    assert_eq!(res.reason, None);
    assert_eq!(res.changed_at, None);
    assert_eq!(res.changed_by_kind, None);
    assert_eq!(res.description_digest, v1_digest);
    assert!(journal.snapshot().is_empty());
    assert!(log.snapshot().is_empty());

    let CValue::Obj(obj) = res.to_value() else { panic!("object") };
    assert_eq!(obj.get("consent"), Some(&CValue::Bool(false)));
    assert_eq!(obj.get("description_digest"), Some(&CValue::Str(v1_digest)));
    assert!(!obj.contains_key("reason"));
}

// ------------------------------------------------------------------ AC-3: agent / MCP refused

/// AC-3: `settings.set_telemetry_consent@1` is never exposed as an MCP tool, and any direct
/// call from an `agent` principal (or over MCP/Python/Control) fails with `policy` and journals
/// nothing.
#[test]
fn ac3_agent_over_mcp_has_no_tool_and_direct_call_returns_policy() {
    let decls = declarations();
    let cmd_decls: Vec<_> = decls.iter().map(|d| fid_registry::CommandDecl::from_def(&d.def)).collect();
    let errors = validate(&cmd_decls);
    assert!(errors.is_empty(), "privacy declarations failed CORE.REG.VAL: {errors:?}");

    let set_cmd = decls.iter().find(|d| d.def.id == "settings.set_telemetry_consent").unwrap();
    assert_eq!(set_cmd.def.side_effect, SideEffect::W3);
    assert!(set_cmd.def.human_only);
    assert_eq!(set_cmd.def.mcp_projection, Projection::Never);
    assert_eq!(set_cmd.def.exposure, BTreeSet::from([Surface::Cli, Surface::Gui]));
    assert_eq!(mcp_projection(&set_cmd.def), Projection::Never);

    let show_cmd = decls.iter().find(|d| d.def.id == "settings.show_privacy").unwrap();
    assert_eq!(show_cmd.def.side_effect, SideEffect::R0);
    assert_eq!(show_cmd.def.mcp_projection, Projection::Exposed);
    assert!(show_cmd.def.exposure.contains(&Surface::Mcp));
    assert_eq!(mcp_projection(&show_cmd.def), Projection::Exposed);

    let tmp = TempDir::new("ac3");
    let user = tmp.user_store();
    let journal = RecordingJournal::default();
    let log = RecordingLog::default();
    let telemetry = RecordingTelemetry::default();
    let desc = TelemetryDescription::shipped().unwrap();

    for caller in [
        Caller { kind: PrincipalKind::Agent, surface: Surface::Mcp },
        Caller { kind: PrincipalKind::Agent, surface: Surface::Cli },
        Caller { kind: PrincipalKind::Agent, surface: Surface::Gui },
        Caller { kind: PrincipalKind::Human, surface: Surface::Mcp },
        Caller { kind: PrincipalKind::Human, surface: Surface::Python },
        Caller { kind: PrincipalKind::Human, surface: Surface::Control },
    ] {
        let ctx = PrivacyCtx {
            caller,
            user: &user,
            journal: &journal,
            log: &log,
            description: &desc,
            telemetry: &telemetry,
            now: "2026-11-22T01:00:00Z",
        };
        let err = set_telemetry_consent(&ctx, SetTelemetryConsentParams { consent: true }).unwrap_err();
        assert_eq!(err.code(), "policy", "caller {caller:?} must fail with policy");
        assert!(matches!(err, SettingsError::Policy(_)));
    }
    assert!(journal.snapshot().is_empty(), "refused calls must journal nothing");
    assert!(!user.path().exists(), "refused calls must not create settings.json");
    assert_eq!(telemetry.stop_count(), 0);
}

// ------------------------------------------------------------------ AC-4: script principal refused

/// AC-4: a `script` principal calling `settings.set_telemetry_consent@1` from CLI fails with
/// `policy` and journals nothing.
#[test]
fn ac4_script_principal_on_cli_fails_with_policy() {
    let tmp = TempDir::new("ac4");
    let user = tmp.user_store();
    let journal = RecordingJournal::default();
    let log = RecordingLog::default();
    let telemetry = RecordingTelemetry::default();
    let desc = TelemetryDescription::shipped().unwrap();

    let ctx = PrivacyCtx {
        caller: Caller { kind: PrincipalKind::Script, surface: Surface::Cli },
        user: &user,
        journal: &journal,
        log: &log,
        description: &desc,
        telemetry: &telemetry,
        now: "2026-11-22T02:00:00Z",
    };
    for consent in [true, false] {
        let err = set_telemetry_consent(&ctx, SetTelemetryConsentParams { consent }).unwrap_err();
        assert_eq!(err.code(), "policy");
        assert!(matches!(err, SettingsError::Policy(_)));
    }
    assert!(journal.snapshot().is_empty());
    assert!(!user.path().exists());
    assert_eq!(telemetry.stop_count(), 0);
}

// ------------------------------------------------------------------ AC-5: description digest change lapses consent

/// AC-5: after a human enables consent in the GUI under description v1, switching the shipped
/// description to v2 causes `settings.show_privacy@1` to report consent `false` with reason
/// `description_changed`, until a human consents again under v2.
#[test]
fn ac5_description_change_lapses_consent_with_reason() {
    let tmp = TempDir::new("ac5");
    let user = tmp.user_store();
    let journal = RecordingJournal::default();
    let log = RecordingLog::default();
    let telemetry = RecordingTelemetry::default();

    let expected = expected_json();
    let v1_digest = expected_str(&expected, "v1_digest");
    let v2_digest = expected_str(&expected, "v2_digest");
    assert_ne!(v1_digest, v2_digest);

    let desc_v1 = TelemetryDescription::from_json_bytes(&read_fixture("telemetry-description.v1.json")).unwrap();
    let desc_v2 = TelemetryDescription::from_json_bytes(&read_fixture("telemetry-description.v2.json")).unwrap();
    assert_eq!(desc_v1.digest().as_str(), v1_digest);
    assert_eq!(desc_v2.digest().as_str(), v2_digest);

    // Human consents in GUI under v1.
    let ctx_v1 = PrivacyCtx {
        caller: Caller { kind: PrincipalKind::Human, surface: Surface::Gui },
        user: &user,
        journal: &journal,
        log: &log,
        description: &desc_v1,
        telemetry: &telemetry,
        now: "2026-11-22T03:04:05Z",
    };
    let set_res = set_telemetry_consent(&ctx_v1, SetTelemetryConsentParams { consent: true }).unwrap();
    assert!(set_res.consent);
    assert_eq!(set_res.description_digest, v1_digest);
    assert_eq!(telemetry.stop_count(), 0);

    let status_v1 = show_privacy(&ctx_v1, ShowPrivacyParams {}).unwrap();
    assert!(status_v1.consent);
    assert_eq!(status_v1.reason, None);
    assert_eq!(status_v1.changed_at.as_deref(), Some("2026-11-22T03:04:05Z"));
    assert_eq!(status_v1.changed_by_kind.as_deref(), Some("human"));
    assert_eq!(status_v1.description_digest, v1_digest);

    // Description changes to v2 -> consent lapses with reason `description_changed`.
    let ctx_v2 = PrivacyCtx {
        caller: Caller { kind: PrincipalKind::Human, surface: Surface::Gui },
        user: &user,
        journal: &journal,
        log: &log,
        description: &desc_v2,
        telemetry: &telemetry,
        now: "2026-11-22T04:00:00Z",
    };
    let status_v2 = show_privacy(&ctx_v2, ShowPrivacyParams {}).unwrap();
    assert!(!status_v2.consent);
    assert_eq!(status_v2.reason.as_deref(), Some(REASON_DESCRIPTION_CHANGED));
    assert_eq!(status_v2.changed_at.as_deref(), Some("2026-11-22T03:04:05Z"));
    assert_eq!(status_v2.changed_by_kind.as_deref(), Some("human"));
    assert_eq!(status_v2.description_digest, v2_digest);

    // Human re-consents under v2 -> effective consent is true again.
    let reconsent = set_telemetry_consent(&ctx_v2, SetTelemetryConsentParams { consent: true }).unwrap();
    assert!(reconsent.consent);
    assert_eq!(reconsent.description_digest, v2_digest);
    let status_v2_after = show_privacy(&ctx_v2, ShowPrivacyParams {}).unwrap();
    assert!(status_v2_after.consent);
    assert_eq!(status_v2_after.reason, None);
    assert_eq!(status_v2_after.changed_at.as_deref(), Some("2026-11-22T04:00:00Z"));
}

// ------------------------------------------------------------------ AC-6: turn off journals W3 + stops telemetry

/// AC-6 & REQ-006: when a human turns consent off, telemetry is stopped synchronously before
/// returning, one W3 `settings.set_telemetry_consent@1` entry is committed, and `show_privacy`
/// reports consent `false`.
#[test]
fn ac6_human_turns_consent_off_journals_w3_and_stops_telemetry() {
    let tmp = TempDir::new("ac6");
    let user = tmp.user_store();
    let journal = RecordingJournal::default();
    let log = RecordingLog::default();
    let telemetry = RecordingTelemetry::default();
    let desc = TelemetryDescription::shipped().unwrap();

    // First turn on.
    let ctx_on = PrivacyCtx {
        caller: Caller { kind: PrincipalKind::Human, surface: Surface::Cli },
        user: &user,
        journal: &journal,
        log: &log,
        description: &desc,
        telemetry: &telemetry,
        now: "2026-11-22T05:00:00Z",
    };
    set_telemetry_consent(&ctx_on, SetTelemetryConsentParams { consent: true }).unwrap();
    assert_eq!(telemetry.stop_count(), 0);
    assert_eq!(journal.snapshot().len(), 1);

    // Clear journal recording to isolate the turn-off event for AC-6.
    journal.entries.lock().unwrap().clear();

    // Turn off.
    let ctx_off = PrivacyCtx {
        caller: Caller { kind: PrincipalKind::Human, surface: Surface::Cli },
        user: &user,
        journal: &journal,
        log: &log,
        description: &desc,
        telemetry: &telemetry,
        now: "2026-11-22T05:10:00Z",
    };
    let res = set_telemetry_consent(&ctx_off, SetTelemetryConsentParams { consent: false }).unwrap();
    assert!(!res.consent);
    assert_eq!(telemetry.stop_count(), 1, "telemetry must stop before command returns");

    let recorded = journal.snapshot();
    assert_eq!(recorded.len(), 1);
    let (target, entry) = &recorded[0];
    assert_eq!(*target, JournalTarget::UserSettings);
    assert_eq!(entry.command(), SET_TELEMETRY_CONSENT);
    assert_eq!(entry.key(), TELEMETRY_CONSENT_KEY);
    assert_eq!(entry.scope(), Scope::User);
    assert_eq!(entry.principal(), PrincipalKind::Human);
    assert!(entry.previous().is_some());

    let shown = show_privacy(&ctx_off, ShowPrivacyParams {}).unwrap();
    assert!(!shown.consent);
    assert_eq!(shown.reason, None);
    assert_eq!(shown.changed_at.as_deref(), Some("2026-11-22T05:10:00Z"));
    assert_eq!(shown.changed_by_kind.as_deref(), Some("human"));
    assert_eq!(shown.description_digest, desc.digest().as_str());
}

// ------------------------------------------------------------------ generic settings guard & fail-closed

/// `privacy.telemetry_consent` registers cleanly in `SettingsRegistry`, defaults to off, is
/// readable through `settings.list_values@1`, and refuses writes through `settings.set_value@1`,
/// `settings.reset_value@1`, and `settings.set_admin_value@1`.
#[test]
fn generic_settings_commands_refuse_mutating_telemetry_consent() {
    let registry = SettingsRegistry::build(&privacy_settings()).unwrap();
    let decl = registry.get(TELEMETRY_CONSENT_KEY).unwrap();
    assert_eq!(decl.owner(), "privacy");
    assert!(!decl.admin());
    assert_eq!(decl.scopes().collect::<Vec<_>>(), vec![Scope::User]);

    let tmp = TempDir::new("guard");
    let user = tmp.user_store();
    let machine = tmp.machine_store();
    let journal = RecordingJournal::default();
    let log = RecordingLog::default();

    let ctx = SettingsCtx {
        registry: &registry,
        caller: Caller { kind: PrincipalKind::Human, surface: Surface::Cli },
        project: None,
        user: &user,
        machine: &machine,
        journal: &journal,
        log: &log,
    };

    let listed = list_values(&ctx, ListValuesParams::default()).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].key, TELEMETRY_CONSENT_KEY);
    assert_eq!(listed[0].source_scope, Source::Default);
    let CValue::Obj(ref def_obj) = listed[0].value.0 else { panic!("default must be object") };
    assert_eq!(def_obj.get("consent"), Some(&CValue::Bool(false)));

    let val = SettingValue(parse(br#"{"consent":true}"#).unwrap());
    let err_set =
        set_value(&ctx, SetValueParams { key: TELEMETRY_CONSENT_KEY.into(), scope: Scope::User, value: val.clone() })
            .unwrap_err();
    assert_eq!(err_set.code(), "policy");

    let err_reset =
        reset_value(&ctx, ResetValueParams { key: TELEMETRY_CONSENT_KEY.into(), scope: Scope::User }).unwrap_err();
    assert_eq!(err_reset.code(), "policy");

    let err_admin =
        set_admin_value(&ctx, SetValueParams { key: TELEMETRY_CONSENT_KEY.into(), scope: Scope::User, value: val })
            .unwrap_err();
    assert_eq!(err_admin.code(), "policy");

    assert!(journal.snapshot().is_empty());
}

/// Corrupt or tampered stored consent values fail closed (reported off) and emit a warning.
#[test]
fn corrupt_stored_consent_fails_closed_with_warning() {
    let tmp = TempDir::new("corrupt");
    let user = tmp.user_store();
    fs::create_dir_all(user.path().parent().unwrap()).unwrap();
    // Tampered file: changed_by_kind is "agent" instead of "human".
    fs::write(
        user.path(),
        br#"{"format":"fid-settings/1","values":{"privacy.telemetry_consent":{"changed_at":"2026-11-22T00:00:00Z","changed_by_kind":"agent","consent":true,"description_digest":"sha256:0000000000000000000000000000000000000000000000000000000000000000"}}}"#,
    )
    .unwrap();

    let journal = RecordingJournal::default();
    let log = RecordingLog::default();
    let desc = TelemetryDescription::shipped().unwrap();
    let ctx = PrivacyCtx {
        caller: Caller { kind: PrincipalKind::Human, surface: Surface::Cli },
        user: &user,
        journal: &journal,
        log: &log,
        description: &desc,
        telemetry: &NoopTelemetry,
        now: "2026-11-22T00:00:00Z",
    };

    let res = show_privacy(&ctx, ShowPrivacyParams {}).unwrap();
    assert!(!res.consent);
    assert_eq!(res.reason, None);
    let warnings = log.snapshot();
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].key.as_deref(), Some(TELEMETRY_CONSENT_KEY));
    assert_eq!(warnings[0].reason, WarningReason::InvalidValue);
}

/// Parameter decoders and description validator reject malformed inputs cleanly.
#[test]
fn param_decoders_and_description_validation_reject_malformed_inputs() {
    assert!(SetTelemetryConsentParams::from_json(br#"{"consent":true}"#).unwrap().consent);
    assert!(SetTelemetryConsentParams::from_json(br#"{"consent":"yes"}"#).is_err());
    assert!(SetTelemetryConsentParams::from_json(br#"{"consent":true,"extra":1}"#).is_err());
    assert!(ShowPrivacyParams::from_json(br#"{}"#).is_ok());
    assert!(ShowPrivacyParams::from_json(br#"{"unexpected":1}"#).is_err());
    assert!(TelemetryDescription::from_json_bytes(br#"{"format":"wrong"}"#).is_err());
    assert_eq!(SHOW_PRIVACY, "settings.show_privacy@1");
    assert_eq!(SET_TELEMETRY_CONSENT, "settings.set_telemetry_consent@1");
}
