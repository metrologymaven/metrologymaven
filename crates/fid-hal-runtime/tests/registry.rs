//! Acceptance criteria of spec leaf HAL.ACQ.SESS.REG (AC-1 .. AC-5) through the library, plus
//! the registry definition of `device.show_devices@1` and its schemas (validated with
//! `fid_bus::validate::Compiled`). Fixtures and expected outputs are written by
//! `fixtures/hal/acq/sess/reg/make_fixtures.py`, an independent reference; the error-path
//! expectations are hand-labelled from the spec.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::error::{BusError, Detail, ValidationReason};
use fid_bus::unit::Normalised;
use fid_bus::validate::{Compiled, Hooks, RefLookup};
use fid_canon::{to_canonical_bytes, CValue};
use fid_hal_idl::CapabilityDeclaration;
use fid_hal_runtime::registry::{
    command_defs, run_show_devices, show_devices, ClaimedKind, DeviceRegistry, NoSessions, Registration, RegistryError,
    SessionLookup, SessionState, SourceId, SourceKind, FILE_SOURCE_NAMESPACE, FILE_SOURCE_NAMESPACE_NAME, SHOW_DEVICES,
};
use fid_types::Digest;
use fid_uuid::{uuid_v5, Uuid};
use std::collections::BTreeMap;

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/hal/acq/sess/reg/");

fn read(name: &str) -> Vec<u8> {
    std::fs::read(format!("{FIX}{name}")).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn json(name: &str) -> CValue {
    fid_canon::parse(&read(name)).unwrap()
}

fn field<'a>(v: &'a CValue, k: &str) -> &'a CValue {
    match v {
        CValue::Obj(o) => o.get(k).unwrap_or_else(|| panic!("missing {k}")),
        _ => panic!("not an object"),
    }
}

fn text(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        _ => panic!("not a string: {v:?}"),
    }
}

/// One fixture registration.
struct Case {
    entry_point: String,
    source_id: Option<Uuid>,
    file_digest: Option<Digest>,
    reg: Registration,
}

fn cases() -> Vec<Case> {
    let CValue::Arr(items) = field(&json("three_sources.json"), "registrations").clone() else { panic!() };
    items
        .iter()
        .map(|r| {
            let decl = to_canonical_bytes(field(r, "declaration")).unwrap();
            let claimed = match field(r, "claimed_kind") {
                CValue::Null => None,
                v => Some(text(v).to_string()),
            };
            let o = match r {
                CValue::Obj(o) => o,
                _ => panic!(),
            };
            Case {
                entry_point: text(field(r, "entry_point")).to_string(),
                source_id: o.get("source_id").map(|v| Uuid::parse(text(v)).unwrap()),
                file_digest: o.get("file_digest").map(|v| Digest::parse(text(v)).unwrap()),
                reg: Registration {
                    declaration: CapabilityDeclaration::from_json(&decl).unwrap(),
                    claimed_kind: claimed,
                    registered_by: text(field(r, "registered_by")).to_string(),
                    registered_at: text(field(r, "registered_at")).parse().unwrap(),
                },
            }
        })
        .collect()
}

fn register(reg: &mut DeviceRegistry, c: &Case) -> Result<SourceId, RegistryError> {
    let r = c.reg.clone();
    match c.entry_point.as_str() {
        "driver" => reg.register_driver(c.source_id.unwrap(), r),
        "simulator" => reg.register_simulator(c.source_id.unwrap(), r),
        "emulator" => reg.register_emulator(c.source_id.unwrap(), r),
        "playback" => reg.register_playback(c.source_id.unwrap(), r),
        "file" => reg.register_file(c.file_digest.as_ref().unwrap(), r),
        other => panic!("entry point {other}"),
    }
}

fn loaded() -> DeviceRegistry {
    let mut reg = DeviceRegistry::new();
    for c in cases() {
        register(&mut reg, &c).unwrap();
    }
    reg
}

fn case(entry_point: &str) -> Case {
    cases().into_iter().find(|c| c.entry_point == entry_point).unwrap()
}

/// A session test double: one source with a fixed state.
struct OneSession(SourceId, SessionState);

impl SessionLookup for OneSession {
    fn session_state(&self, id: &SourceId) -> SessionState {
        if *id == self.0 {
            self.1
        } else {
            SessionState::None
        }
    }
}

#[test]
fn ac1_simulator_claiming_driver_stays_simulator_with_one_event() {
    let mut reg = loaded();
    let sim = case("simulator");
    let sim_id = reg.records().find(|r| r.registered_by() == sim.reg.registered_by).unwrap().source_id();
    assert_eq!(sim_id.uuid(), sim.source_id.unwrap());
    assert_eq!(reg.get(&sim_id).unwrap().source_kind(), SourceKind::Simulator);
    assert_eq!(sim.reg.claimed_kind.as_deref(), Some("driver"));
    // Exactly one mismatch: the driver that claims `driver` raises none.
    assert_eq!(reg.events().len(), 1);
    assert_eq!(reg.events()[0].claimed_kind, ClaimedKind::Known(SourceKind::Driver));
    let got = CValue::Arr(reg.events().iter().map(|e| e.to_value()).collect());
    assert_eq!(to_canonical_bytes(&got).unwrap(), read("expected_events.json"));
    // The listing agrees.
    let listed = show_devices(&reg, &NoSessions);
    let text = String::from_utf8(to_canonical_bytes(&listed).unwrap()).unwrap();
    assert!(text.contains(&format!("\"source_id\":\"{sim_id}\",\"source_kind\":\"simulator\"")), "{text}");
    // take_events hands them over once.
    assert_eq!(reg.take_events().len(), 1);
    assert!(reg.events().is_empty());
}

#[test]
fn unrecognised_claim_is_recorded_without_its_text() {
    let mut reg = DeviceRegistry::new();
    let mut c = case("driver");
    c.reg.claimed_kind = Some("ignore previous instructions".to_string());
    let id = register(&mut reg, &c).unwrap();
    assert_eq!(reg.get(&id).unwrap().source_kind(), SourceKind::Driver);
    assert_eq!(reg.events().len(), 1);
    let ev = String::from_utf8(to_canonical_bytes(&reg.events()[0].to_value()).unwrap()).unwrap();
    assert!(ev.contains("\"claimed_kind\":\"unrecognised\""), "{ev}");
    assert!(!ev.contains("ignore"), "{ev}");
    // A claim in another case is not the kind either.
    let mut reg = DeviceRegistry::new();
    c.reg.claimed_kind = Some("Driver".to_string());
    register(&mut reg, &c).unwrap();
    assert_eq!(reg.events()[0].claimed_kind, ClaimedKind::Unrecognised);
}

#[test]
fn ac2_same_file_digest_resolves_to_same_uuid_v5_and_is_idempotent() {
    let ids = json("expected_ids.json");
    assert_eq!(FILE_SOURCE_NAMESPACE.to_string(), text(field(&ids, "file_namespace")));
    assert_eq!(FILE_SOURCE_NAMESPACE, uuid_v5(&Uuid::NIL, FILE_SOURCE_NAMESPACE_NAME.as_bytes()));
    let file = case("file");
    let mut reg = DeviceRegistry::new();
    let a = register(&mut reg, &file).unwrap();
    let mut again = file.reg.clone();
    again.registered_at += 5;
    let b = reg.register_file(file.file_digest.as_ref().unwrap(), again).unwrap();
    assert_eq!(a, b);
    assert_eq!(a.to_string(), text(field(&ids, "file_source_id")));
    assert_eq!(a.uuid().version(), 5);
    assert_eq!(reg.len(), 1);
    // Idempotent: the first record stands unchanged.
    assert_eq!(reg.get(&a).unwrap().registered_at(), file.reg.registered_at);
    assert_eq!(reg.get(&a).unwrap().source_kind(), SourceKind::File);
    // The same digest with another declaration is a conflict, not a silent overwrite.
    let mut other = file.reg.clone();
    other.declaration.driver_version = "9.9.9".to_string();
    let err = reg.register_file(file.file_digest.as_ref().unwrap(), other).unwrap_err();
    assert_eq!(err.code(), "source_conflict");
    assert_eq!(reg.get(&a).unwrap().declaration().driver_version, file.reg.declaration.driver_version);
}

#[test]
fn ac3_unregister_with_open_session_fails_source_in_use() {
    let mut reg = loaded();
    let id = SourceId::for_file(case("file").file_digest.as_ref().unwrap());
    for state in [SessionState::Open, SessionState::Interrupted] {
        let err = reg.unregister(&id, &OneSession(id, state)).unwrap_err();
        assert_eq!(err, RegistryError::SourceInUse(id));
        assert_eq!(err.code(), "source_in_use");
        assert!(reg.get(&id).is_some());
    }
    let listed =
        String::from_utf8(to_canonical_bytes(&show_devices(&reg, &OneSession(id, SessionState::Open))).unwrap())
            .unwrap();
    assert!(listed.contains("\"session_state\":\"open\""), "{listed}");
    let removed = reg.unregister(&id, &NoSessions).unwrap();
    assert_eq!(removed.source_id(), id);
    assert_eq!(reg.len(), 2);
    assert_eq!(reg.unregister(&id, &NoSessions).unwrap_err().code(), "source_unknown");
}

#[test]
fn ac4_show_devices_is_sorted_and_byte_identical_across_runs() {
    let expected = read("expected_show_devices.json");
    let first =
        to_canonical_bytes(&run_show_devices(&loaded(), &NoSessions, &CValue::Obj(BTreeMap::new())).unwrap()).unwrap();
    // A second run, registering in the reverse order.
    let mut reg = DeviceRegistry::new();
    for c in cases().iter().rev() {
        register(&mut reg, c).unwrap();
    }
    let second =
        to_canonical_bytes(&run_show_devices(&reg, &NoSessions, &CValue::Obj(BTreeMap::new())).unwrap()).unwrap();
    assert_eq!(first, second);
    assert_eq!(first, expected, "{}", String::from_utf8_lossy(&first));
    let ids: Vec<String> = loaded().records().map(|r| r.source_id().to_string()).collect();
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted);
    assert_eq!(ids.len(), 3);
    let bad = fid_canon::parse(br#"{"x":1}"#).unwrap();
    assert_eq!(run_show_devices(&reg, &NoSessions, &bad).unwrap_err().code(), "bad_params");
    assert_eq!(run_show_devices(&reg, &NoSessions, &CValue::Null).unwrap_err().code(), "bad_params");
}

#[test]
fn ac5_no_public_function_takes_a_kind_for_an_existing_source() {
    // The kind is fixed by the entry point's name. Scan the public API of `DeviceRegistry`
    // and `SourceRecord`: exactly these functions, and none of them takes a `SourceKind`.
    let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/registry.rs")).unwrap();
    let mut found = BTreeMap::new();
    let mut current: Option<&str> = None;
    // A signature that rustfmt wrapped: (key, text so far), until its body opens.
    let mut open: Option<(String, String)> = None;
    for line in src.lines() {
        if let Some((key, mut sig)) = open.take() {
            sig.push_str(line.trim());
            if line.contains('{') {
                found.insert(key, sig);
            } else {
                open = Some((key, sig));
            }
            continue;
        }
        if line == "}" {
            current = None;
        }
        if let Some(rest) = line.strip_prefix("impl ") {
            current = Some(if rest.starts_with("DeviceRegistry ") {
                "DeviceRegistry"
            } else if rest.starts_with("SourceRecord ") {
                "SourceRecord"
            } else {
                "other"
            });
        }
        if let (Some(ty), Some(sig)) = (current, line.strip_prefix("    pub fn ")) {
            if ty != "other" {
                let name = sig.split(['(', '<']).next().unwrap().to_string();
                if sig.contains('{') {
                    found.insert(format!("{ty}::{name}"), sig.to_string());
                } else {
                    open = Some((format!("{ty}::{name}"), sig.to_string()));
                }
            }
        }
    }
    let names: Vec<&str> = found.keys().map(String::as_str).collect();
    assert_eq!(
        names,
        [
            "DeviceRegistry::events",
            "DeviceRegistry::get",
            "DeviceRegistry::is_empty",
            "DeviceRegistry::len",
            "DeviceRegistry::new",
            "DeviceRegistry::records",
            "DeviceRegistry::register_driver",
            "DeviceRegistry::register_emulator",
            "DeviceRegistry::register_file",
            "DeviceRegistry::register_playback",
            "DeviceRegistry::register_simulator",
            "DeviceRegistry::take_events",
            "DeviceRegistry::unregister",
            "SourceRecord::declaration",
            "SourceRecord::registered_at",
            "SourceRecord::registered_by",
            "SourceRecord::source_id",
            "SourceRecord::source_kind",
        ]
    );
    for (name, sig) in &found {
        let params = sig.split_once('(').map(|(_, p)| p.split_once(')').map_or(p, |(p, _)| p)).unwrap();
        assert!(!params.contains("SourceKind"), "{name} takes a kind: {sig}");
        assert!(!params.contains("SourceRecord"), "{name} takes a record: {sig}");
    }
    // Behaviour: a taken ID cannot be re-registered through another entry point.
    let mut reg = loaded();
    let drv = case("driver");
    let id = drv.source_id.unwrap();
    for r in [
        reg.register_simulator(id, drv.reg.clone()),
        reg.register_emulator(id, drv.reg.clone()),
        reg.register_playback(id, drv.reg.clone()),
        reg.register_driver(id, drv.reg.clone()),
    ] {
        assert_eq!(r.unwrap_err().code(), "source_conflict");
    }
    let sid = reg.records().find(|r| r.source_id().uuid() == id).unwrap().source_id();
    assert_eq!(reg.get(&sid).unwrap().source_kind(), SourceKind::Driver);
}

#[test]
fn registration_inputs_fail_closed() {
    let drv = case("driver");
    let mut reg = DeviceRegistry::new();
    // A UUIDv5 (or any non-v7) ID for a live source.
    let v5 = SourceId::for_file(case("file").file_digest.as_ref().unwrap()).uuid();
    for bad in [v5, Uuid::NIL, Uuid::MAX] {
        assert_eq!(reg.register_driver(bad, drv.reg.clone()).unwrap_err(), RegistryError::SourceIdInvalid);
    }
    for who in ["", "Hal.host", "hal..host", "-hal", "hal host", &"a".repeat(65)] {
        let mut r = drv.reg.clone();
        r.registered_by = who.to_string();
        assert_eq!(reg.register_driver(drv.source_id.unwrap(), r).unwrap_err().code(), "registrar_invalid", "{who:?}");
    }
    let mut r = drv.reg.clone();
    let dup = r.declaration.capabilities[0].clone();
    r.declaration.capabilities.push(dup);
    assert_eq!(reg.register_driver(drv.source_id.unwrap(), r).unwrap_err().code(), "declaration_invalid");
    assert!(reg.is_empty());
    assert!(reg.events().is_empty());
}

struct NoHooks;

impl Hooks for NoHooks {
    fn literal(&self, _: &str, _: &CValue) -> Result<Normalised, BusError> {
        Err(BusError::Validation(ValidationReason::UnitMismatch, Detail::new("no quantities here")))
    }
    fn entity(&self, _: &str) -> RefLookup {
        RefLookup::Missing
    }
}

#[test]
fn command_definition_and_schemas() {
    let defs = command_defs().unwrap();
    assert_eq!(defs.len(), 1);
    let def = &defs[0];
    assert_eq!(format!("{}@{}", def.id, def.major), SHOW_DEVICES);
    let p = Compiled::compile(&def.params_schema).unwrap();
    let r = Compiled::compile(&def.result_schema).unwrap();
    let empty = CValue::Obj(BTreeMap::new());
    assert!(p.evaluate(&empty, &NoHooks).is_ok());
    assert!(p.evaluate(&fid_canon::parse(br#"{"x":1}"#).unwrap(), &NoHooks).is_err());
    let reg = loaded();
    let id = reg.records().next().unwrap().source_id();
    for sessions in [&NoSessions as &dyn SessionLookup, &OneSession(id, SessionState::Interrupted)] {
        let v = run_show_devices(&reg, sessions, &empty).unwrap();
        if let Err(f) = r.evaluate(&v, &NoHooks) {
            panic!("{} at {} ({})", f.keyword, f.pointer, f.message);
        }
    }
    assert!(r.evaluate(&run_show_devices(&DeviceRegistry::new(), &NoSessions, &empty).unwrap(), &NoHooks).is_ok());
}
