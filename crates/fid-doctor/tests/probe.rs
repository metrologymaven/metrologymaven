//! Acceptance criteria of spec node PLAT.DOCTOR.PROBE (AC-1 .. AC-6) and edge cases.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_doctor::probe::{
    bound_detail, probe_all, Arch, CpuInfo, Env, EnvSnapshot, FakeEnv, GpuSelection, OsInfo, Probed, RamInfo, RealEnv,
    UnknownReason, VolumeInfo, VolumeRole, DEFAULT_DEADLINE, MAX_DETAIL_CHARS, SCHEMA_VERSION,
};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fix(name: &str) -> PathBuf {
    repo_root().join("fixtures/plat/doctor").join(name)
}

#[test]
fn ac1_real_env_probes_core_items_and_validates_against_schema() {
    let snap = probe_all(RealEnv::new(), DEFAULT_DEADLINE);
    assert_eq!(snap.schema_version, SCHEMA_VERSION);

    let os = snap.os.known().expect("OS should be known on Tier 1 runner");
    assert!(!os.family.is_empty());
    assert!(!os.version.is_empty());

    let arch = snap.arch.known().expect("arch should be known on Tier 1 runner");
    assert!(!arch.as_str().is_empty());

    let cpu = snap.cpu.known().expect("CPU should be known on Tier 1 runner");
    assert!(cpu.physical_cores >= 1);
    assert!(!cpu.instruction_set_level.is_empty());

    let ram = snap.ram.known().expect("RAM should be known on Tier 1 runner");
    assert!(ram.total_gib > 0.0);

    // Serialises to canonical JSON and validates cleanly through fid-doctor's snapshot schema.
    let bytes = snap.to_json().unwrap();
    let parsed = EnvSnapshot::from_json(&bytes).unwrap();
    assert_eq!(parsed, snap);
    assert_eq!(parsed.to_json().unwrap(), bytes);

    // Schema file itself is valid JSON.
    let schema_bytes = std::fs::read(repo_root().join("platform/env-snapshot.schema.json")).unwrap();
    fid_canon::parse(&schema_bytes).unwrap();
}

/// Custom fake `Env` for AC-2: RAM returns an error, display never returns, everything else known.
struct BrokenAndHungEnv;

impl Env for BrokenAndHungEnv {
    fn probe_os(&self) -> Probed<OsInfo> {
        Probed::Known(OsInfo { family: "ubuntu".into(), version: "24.04".into() })
    }

    fn probe_arch(&self) -> Probed<Arch> {
        Probed::Known(Arch::X86_64)
    }

    fn probe_cpu(&self) -> Probed<CpuInfo> {
        Probed::Known(CpuInfo {
            model: "Synthetic CPU".into(),
            physical_cores: 4,
            instruction_set_level: "x86-64-v3".into(),
        })
    }

    fn probe_ram(&self) -> Probed<RamInfo> {
        Probed::unknown(UnknownReason::Error, "simulated RAM probe failure")
    }

    fn probe_storage(&self) -> Probed<Vec<VolumeInfo>> {
        Probed::Known(vec![
            VolumeInfo { role: VolumeRole::Install, free_gib: 50.0, total_gib: 100.0 },
            VolumeInfo { role: VolumeRole::Data, free_gib: 50.0, total_gib: 100.0 },
        ])
    }

    fn probe_display(&self) -> Probed<(u32, u32)> {
        // Never returns: blocks forever on an orphan channel.
        let (_tx, rx) = mpsc::channel::<()>();
        let _ = rx.recv();
        Probed::Known((1920, 1080))
    }

    fn probe_gpu(&self, _deadline: Duration) -> Probed<GpuSelection> {
        Probed::Known(GpuSelection {
            backend: "vulkan".into(),
            adapter: Some("Synthetic GPU".into()),
            driver_version: Some("24.0.0".into()),
            device_type: Some("discrete".into()),
            vram_gib: Some(8.0),
            fallback: false,
            fallback_cause: "none".into(),
            reason: String::new(),
        })
    }
}

#[test]
fn ac2_error_and_timeout_become_unknown_within_deadline_without_panic() {
    let started = Instant::now();
    let snap = probe_all(BrokenAndHungEnv, Duration::from_secs(2));
    let elapsed = started.elapsed();

    assert_eq!(snap.ram.unknown_reason(), Some(UnknownReason::Error));
    assert_eq!(snap.display.unknown_reason(), Some(UnknownReason::Timeout));
    assert!(snap.os.is_known());
    assert!(snap.arch.is_known());
    assert!(snap.cpu.is_known());
    assert!(snap.storage.is_known());
    assert!(snap.gpu.is_known());
    assert!(elapsed < Duration::from_secs(3), "probe_all took {elapsed:?}, expected < 3 s");
}

#[test]
fn ac3_fake_env_round_trips_fixtures_to_byte_identical_json() {
    for name in [
        "env-win11-discrete.json",
        "env-ubuntu-vm-software.json",
        "env-macos-arm.json",
        "env-unknowns.json",
        "env-freebsd.json",
        "env-unknown-ram.json",
    ] {
        let path = fix(name);
        let original = std::fs::read(&path).unwrap();
        let fake = FakeEnv::from_file(&path).unwrap();
        let snap = probe_all(fake, DEFAULT_DEADLINE);
        let round_tripped = snap.to_json().unwrap();
        assert_eq!(round_tripped, original, "{name} did not round-trip byte-identically");
    }
}

#[test]
fn ac4_headless_linux_reports_no_display() {
    #[cfg(target_os = "linux")]
    {
        let has_display = std::env::var_os("DISPLAY").is_some_and(|v| !v.is_empty())
            || std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty());
        if !has_display {
            let snap = probe_all(RealEnv::new(), DEFAULT_DEADLINE);
            assert_eq!(snap.display.unknown_reason(), Some(UnknownReason::NoDisplay));
        }
    }
}

#[test]
#[cfg(not(feature = "gpu"))]
fn ac5_without_gpu_feature_reports_not_built() {
    let snap = probe_all(RealEnv::new(), DEFAULT_DEADLINE);
    assert_eq!(snap.gpu.unknown_reason(), Some(UnknownReason::NotBuilt));
}

#[test]
#[cfg(feature = "gpu")]
fn gpu_feature_returns_known_selection_from_fid_gpu_select() {
    let mut env = RealEnv::new();
    env.gpu_policy_path = Some(repo_root().join("platform/gpu-policy.toml"));
    env.gpu_known_bad_path = Some(repo_root().join("platform/gpu-known-bad.toml"));
    let snap = probe_all(env, Duration::from_secs(5));
    assert!(snap.gpu.is_known(), "expected known GpuSelection with gpu feature, got {:?}", snap.gpu);
}

fn read_os_hostname() -> Option<String> {
    if let Ok(h) = std::env::var("HOSTNAME").or_else(|_| std::env::var("COMPUTERNAME")) {
        let t = h.trim().to_string();
        if !t.is_empty() {
            return Some(t);
        }
    }
    if let Ok(h) = std::fs::read_to_string("/etc/hostname") {
        let t = h.trim().to_string();
        if !t.is_empty() {
            return Some(t);
        }
    }
    std::process::Command::new("hostname")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
}

fn read_os_username() -> Option<String> {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .or_else(|_| std::env::var("LOGNAME"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[test]
fn ac6_real_snapshot_contains_no_hostname_or_username() {
    let snap = probe_all(RealEnv::new(), DEFAULT_DEADLINE);
    let json = String::from_utf8(snap.to_json().unwrap()).unwrap().to_ascii_lowercase();

    if let Some(host) = read_os_hostname() {
        // Guard against trivial 1-2 char hostnames that could match a normal word.
        if host.len() >= 3 {
            assert!(!json.contains(&host.to_ascii_lowercase()), "hostname {host:?} leaked into snapshot: {json}");
        }
    }
    if let Some(user) = read_os_username() {
        if user.len() >= 3 {
            assert!(!json.contains(&user.to_ascii_lowercase()), "username {user:?} leaked into snapshot: {json}");
        }
    }
}

struct PanickingEnv;

impl Env for PanickingEnv {
    fn probe_os(&self) -> Probed<OsInfo> {
        panic!("simulated driver panic");
    }
    fn probe_arch(&self) -> Probed<Arch> {
        Probed::Known(Arch::X86_64)
    }
    fn probe_cpu(&self) -> Probed<CpuInfo> {
        Probed::Known(CpuInfo { model: "CPU".into(), physical_cores: 2, instruction_set_level: "x86-64-v2".into() })
    }
    fn probe_ram(&self) -> Probed<RamInfo> {
        Probed::Known(RamInfo { total_gib: 16.0, available_gib: 8.0 })
    }
    fn probe_storage(&self) -> Probed<Vec<VolumeInfo>> {
        Probed::Known(vec![VolumeInfo { role: VolumeRole::Install, free_gib: 20.0, total_gib: 100.0 }])
    }
    fn probe_display(&self) -> Probed<(u32, u32)> {
        Probed::unknown(UnknownReason::NoDisplay, "none")
    }
    fn probe_gpu(&self, _deadline: Duration) -> Probed<GpuSelection> {
        Probed::unknown(UnknownReason::NotBuilt, "no gpu")
    }
}

#[test]
fn worker_panic_and_detail_bounds_and_schema_rejections() {
    let snap = probe_all(PanickingEnv, DEFAULT_DEADLINE);
    assert_eq!(snap.os.unknown_reason(), Some(UnknownReason::Error));
    assert!(snap.arch.is_known());

    // Detail truncation to 200 chars and control-character sanitisation.
    let long = format!("line1\nline2\t{}", "x".repeat(500));
    let bounded = bound_detail(&long);
    assert_eq!(bounded.chars().count(), MAX_DETAIL_CHARS);
    assert!(!bounded.chars().any(char::is_control));

    // Reject unknown keys, wrong schema_version, and oversized detail in JSON.
    let valid_bytes = std::fs::read(fix("env-unknowns.json")).unwrap();
    let valid_str = String::from_utf8(valid_bytes).unwrap();
    assert!(
        EnvSnapshot::from_json(valid_str.replace("\"schema_version\":1", "\"schema_version\":2").as_bytes()).is_err()
    );
    assert!(EnvSnapshot::from_json(b"{}").is_err());
    let over_detail = valid_str.replace("cannot read meminfo", &"a".repeat(201));
    assert!(EnvSnapshot::from_json(over_detail.as_bytes()).is_err());
}

#[test]
fn real_env_probe_all_holds_the_per_probe_deadline() {
    let started = Instant::now();
    let _snap = probe_all(RealEnv::new(), DEFAULT_DEADLINE);
    let elapsed = started.elapsed();
    assert!(elapsed < DEFAULT_DEADLINE + Duration::from_secs(1), "probe_all took {elapsed:?}");
}
