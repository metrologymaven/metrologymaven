//! Acceptance criteria of spec node PLAT.DOCTOR.LOOKUP (AC-1 .. AC-6), the independent Python
//! oracle cases (`fixtures/plat/doctor/lookup/cases.json`, written by `make_cases.py`) and edge
//! cases. Expected values come from the spec text (ACs) or the oracle, never from this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_doctor::lookup::{
    assess, gpu_class, Assessment, Field, GpuClassFound, Outcome, Value, Verdict, VerdictReason, DISCRETE_MID_VRAM_GIB,
};
use fid_doctor::probe::{EnvSnapshot, GpuSelection, Probed, RamInfo, UnknownReason};
use fid_platform_core::hw_profile::{self, GpuClass, HwProfiles};
use fid_platform_core::matrix::{self, EffectiveTier, Matrix, Phase, Tier};
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> Vec<u8> {
    std::fs::read(root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn snapshot(name: &str) -> EnvSnapshot {
    EnvSnapshot::from_json(&read(&format!("fixtures/plat/doctor/{name}"))).unwrap()
}

fn matrix() -> Matrix {
    matrix::parse(&read("fixtures/plat/matrix/valid.toml")).unwrap()
}

fn profiles() -> HwProfiles {
    hw_profile::parse(&read("fixtures/plat/hw/profiles-valid.toml")).unwrap()
}

fn run(snap: &str, with_profiles: bool, phase: Phase) -> Assessment {
    let p = profiles();
    assess(&snapshot(snap), &matrix(), if with_profiles { Some(&p) } else { None }, phase)
}

/// AC-1
#[test]
fn ac1_win11_discrete_meets_recommended() {
    let a = run("env-win11-discrete.json", true, Phase::P1a);
    assert_eq!(a.tier, Probed::Known(EffectiveTier::Tier(Tier::Tier1)));
    assert_eq!(a.profiles.len(), 2);
    for p in &a.profiles {
        assert_eq!(p.verdict, Verdict::Meets, "{}", p.name);
        assert!(p.shortfalls.is_empty() && p.unknowns.is_empty() && p.reason.is_none());
    }
    assert_eq!(a.outcome, Outcome::MeetsRecommended);
    assert_eq!(a.exit_code(), 0);
}

/// AC-2
#[test]
fn ac2_ubuntu_vm_software_below_minimum() {
    let a = run("env-ubuntu-vm-software.json", true, Phase::P1a);
    let min = a.profile("minimum").unwrap();
    assert_eq!(min.verdict, Verdict::Below);
    let got: Vec<(Field, Value, Value)> =
        min.shortfalls.iter().map(|s| (s.field, s.required.clone(), s.found.clone())).collect();
    assert_eq!(
        got,
        vec![
            (Field::RamGib, Value::Count(16), Value::Gib(8.0)),
            (Field::GpuClass, Value::Name("integrated".into()), Value::Name("software".into())),
        ]
    );
    assert_eq!(min.shortfalls[0].required.to_string(), "16");
    assert_eq!(min.shortfalls[0].found.to_string(), "8");
    assert_eq!(a.outcome, Outcome::BelowMinimum);
    assert_eq!(a.exit_code(), 2);
}

/// AC-3
#[test]
fn ac3_freebsd_unsupported_with_verdicts() {
    let a = run("env-freebsd.json", true, Phase::P1a);
    assert_eq!(a.tier, Probed::Known(EffectiveTier::NotListed));
    assert_eq!(a.outcome, Outcome::UnsupportedPlatform);
    assert_eq!(a.exit_code(), 3);
    assert_eq!(a.profiles.len(), 2);
    for p in &a.profiles {
        assert_eq!(p.verdict, Verdict::Meets, "{}: verdicts are still computed", p.name);
        assert!(p.reason.is_none());
    }
}

/// AC-4
#[test]
fn ac4_unknown_ram_indeterminate() {
    let a = run("env-unknown-ram.json", true, Phase::P1a);
    let min = a.profile("minimum").unwrap();
    assert_eq!(min.verdict, Verdict::Indeterminate);
    assert!(min.shortfalls.is_empty());
    assert_eq!(min.unknowns, vec![Field::RamGib]);
    assert_eq!(a.outcome, Outcome::Indeterminate);
    assert_eq!(a.exit_code(), 4);
}

/// AC-5
#[test]
fn ac5_no_profile_file() {
    let a = run("env-win11-discrete.json", false, Phase::P1a);
    assert_eq!(a.tier, Probed::Known(EffectiveTier::Tier(Tier::Tier1)));
    let names: Vec<&str> = a.profiles.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["minimum", "recommended"]);
    for p in &a.profiles {
        assert_eq!(p.verdict, Verdict::Indeterminate);
        assert_eq!(p.reason, Some(VerdictReason::NoProfiles));
    }
}

/// AC-6
#[test]
fn ac6_serialised_twice_byte_identical() {
    let first = run("env-ubuntu-vm-software.json", true, Phase::P1a).to_json().unwrap();
    let second = run("env-ubuntu-vm-software.json", true, Phase::P1a).to_json().unwrap();
    assert_eq!(first, second);
    assert_eq!(fid_canon::canonicalize(&first).unwrap(), first, "output is canonical JSON");
}

/// Every case of the independent Python oracle, compared byte for byte.
#[test]
fn oracle_cases() {
    let all = fid_canon::parse(&read("fixtures/plat/doctor/lookup/cases.json")).unwrap();
    let CValue::Arr(cases) = all else { panic!("cases.json: not an array") };
    assert!(cases.len() >= 40, "only {} cases", cases.len());
    let m = matrix();
    let p = profiles();
    for case in &cases {
        let CValue::Obj(c) = case else { panic!("case: not an object") };
        let Some(CValue::Str(name)) = c.get("name") else { panic!("case without name") };
        let Some(CValue::Str(phase)) = c.get("phase") else { panic!("{name}: phase") };
        let phase = Phase::from_name(phase).unwrap();
        let profiles = match c.get("profiles") {
            Some(CValue::Null) => None,
            Some(CValue::Str(s)) if s == "fixtures/plat/hw/profiles-valid.toml" => Some(&p),
            other => panic!("{name}: unexpected profiles {other:?}"),
        };
        let snap_bytes = fid_canon::to_canonical_bytes(c.get("snapshot").unwrap()).unwrap();
        let snap = EnvSnapshot::from_json(&snap_bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        let want = fid_canon::to_canonical_bytes(c.get("expected").unwrap()).unwrap();
        let got = assess(&snap, &m, profiles, phase).to_json().unwrap();
        assert_eq!(
            String::from_utf8(got).unwrap(),
            String::from_utf8(want).unwrap(),
            "case {name}: Rust assessment differs from the Python oracle"
        );
    }
}

fn gpu(device_type: Option<&str>, vram: Option<f64>, fallback: bool) -> GpuSelection {
    GpuSelection {
        backend: "vulkan".into(),
        adapter: None,
        driver_version: None,
        device_type: device_type.map(String::from),
        vram_gib: vram,
        fallback,
        fallback_cause: "none".into(),
        reason: String::new(),
    }
}

/// REQ-PLAT.DOCTOR.LOOKUP-004, including the VRAM boundary and non-finite VRAM.
#[test]
fn gpu_class_mapping() {
    use GpuClassFound::{AtLeast, Exact, Unknown};
    assert_eq!(DISCRETE_MID_VRAM_GIB, 6.0);
    let cases: &[(Option<&str>, Option<f64>, bool, GpuClassFound)] = &[
        (Some("discrete"), Some(12.0), true, Exact(GpuClass::Software)),
        (Some("cpu"), None, false, Exact(GpuClass::Software)),
        (Some("integrated"), Some(1.0), false, Exact(GpuClass::Integrated)),
        (Some("integrated"), None, false, Exact(GpuClass::Integrated)),
        (Some("discrete"), Some(0.0), false, Exact(GpuClass::DiscreteEntry)),
        (Some("discrete"), Some(5.999), false, Exact(GpuClass::DiscreteEntry)),
        (Some("discrete"), Some(6.0), false, Exact(GpuClass::DiscreteMid)),
        (Some("discrete"), Some(48.0), false, Exact(GpuClass::DiscreteMid)),
        (Some("discrete"), None, false, AtLeast(GpuClass::DiscreteEntry)),
        (Some("discrete"), Some(f64::NAN), false, AtLeast(GpuClass::DiscreteEntry)),
        (Some("discrete"), Some(f64::INFINITY), false, AtLeast(GpuClass::DiscreteEntry)),
        (Some("discrete"), Some(-1.0), false, AtLeast(GpuClass::DiscreteEntry)),
        (Some("virtual"), Some(8.0), false, Unknown),
        (Some("other"), None, false, Unknown),
        (None, None, false, Unknown),
    ];
    for (dt, vram, fb, want) in cases {
        assert_eq!(gpu_class(&gpu(*dt, *vram, *fb)), *want, "{dt:?} {vram:?} fallback={fb}");
    }
}

/// A profile file that lacks a required profile (only possible for hand-built inputs) makes
/// that verdict `indeterminate(missing_profile)` and never a `meets_*` outcome.
#[test]
fn missing_required_profile() {
    let mut p = profiles();
    p.profiles.remove("recommended");
    let a = assess(&snapshot("env-win11-discrete.json"), &matrix(), Some(&p), Phase::P1a);
    let rec = a.profile("recommended").unwrap();
    assert_eq!((rec.verdict, rec.reason), (Verdict::Indeterminate, Some(VerdictReason::MissingProfile)));
    assert_eq!(a.outcome, Outcome::MeetsMinimum);

    let mut p = profiles();
    p.profiles.remove("minimum");
    let a = assess(&snapshot("env-win11-discrete.json"), &matrix(), Some(&p), Phase::P1a);
    assert_eq!(a.profile("minimum").unwrap().reason, Some(VerdictReason::MissingProfile));
    assert_eq!(a.outcome, Outcome::Indeterminate);
    let names: Vec<&str> = a.profiles.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["minimum", "recommended"], "name order");
}

/// Extra profiles are assessed too, in name order, but do not change the outcome.
#[test]
fn extra_profile_in_name_order() {
    let mut p = profiles();
    let mut kiosk = p.profiles["minimum"].clone();
    kiosk.ram_gib = 64;
    p.profiles.insert("kiosk".into(), kiosk);
    let a = assess(&snapshot("env-win11-discrete.json"), &matrix(), Some(&p), Phase::P1a);
    let names: Vec<&str> = a.profiles.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["kiosk", "minimum", "recommended"]);
    assert_eq!(a.profile("kiosk").unwrap().verdict, Verdict::Below);
    assert_eq!(a.outcome, Outcome::MeetsRecommended);
}

/// Hand-built snapshots with non-finite numbers never panic: the item is unknown and the
/// assessment still serialises.
#[test]
fn non_finite_ram_is_unknown() {
    let mut snap = snapshot("env-win11-discrete.json");
    snap.ram = Probed::Known(RamInfo { total_gib: f64::NAN, available_gib: 1.0 });
    let a = assess(&snap, &matrix(), Some(&profiles()), Phase::P1a);
    assert_eq!(a.profile("minimum").unwrap().unknowns, vec![Field::RamGib]);
    assert_eq!(a.outcome, Outcome::Indeterminate);
    assert!(a.to_json().is_ok());
}

/// An unknown OS gives an unknown tier carrying the probe's reason, and never unsupported.
#[test]
fn unknown_os_tier() {
    let mut snap = snapshot("env-win11-discrete.json");
    snap.os = Probed::unknown(UnknownReason::Timeout, "probe exceeded deadline");
    let a = assess(&snap, &matrix(), Some(&profiles()), Phase::P1a);
    assert_eq!(
        a.tier,
        Probed::Unknown { reason: UnknownReason::Timeout, detail: "os: probe exceeded deadline".into() }
    );
    assert_eq!(a.outcome, Outcome::Indeterminate);
}

#[test]
fn exit_codes_are_distinct() {
    let all = [
        Outcome::UnsupportedPlatform,
        Outcome::BelowMinimum,
        Outcome::Indeterminate,
        Outcome::MeetsMinimum,
        Outcome::MeetsRecommended,
    ];
    let codes: Vec<i32> = all.iter().map(|o| o.exit_code()).collect();
    assert_eq!(codes, [3, 2, 4, 1, 0]);
    let names: Vec<&str> = all.iter().map(|o| o.as_str()).collect();
    assert_eq!(names, ["unsupported_platform", "below_minimum", "indeterminate", "meets_minimum", "meets_recommended"]);
}
