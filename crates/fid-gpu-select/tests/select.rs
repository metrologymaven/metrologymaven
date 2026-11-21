//! Spec leaf PLAT.GPU.DETECT: AC-1 .. AC-7 over the recorded adapter lists and policy and
//! known-bad fixtures of `fixtures/plat/gpu/` (hand-written, invented IDs above the PCI range;
//! the expected outcomes below are hand-labelled from the spec's rules). In
//! `adapters-win-discrete.json` the discrete adapter is on DX12 and Vulkan and the integrated
//! one on Vulkan only.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_gpu_policy::policy::{parse_known_bad, parse_policy, Backend, GpuOs, KnownBad, Minimum, PolicyRow};
use fid_gpu_policy::{Arch, Version};
use fid_gpu_select::{
    driver_version_of, parse_adapters, select, AdapterChoice, AdapterInfo, Cause, DeviceType, FallbackCause, Probe,
    Prober, Rejection, DEFAULT_PROBE_TIMEOUT, USER_OVERRIDE,
};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(root().join("fixtures/plat/gpu").join(name)).unwrap()
}

fn windows_row() -> PolicyRow {
    parse_policy(&read("policy-valid.toml")).unwrap().row(GpuOs::Windows, Arch::X86_64).unwrap().clone()
}

fn none() -> KnownBad {
    parse_known_bad(b"schema_version = 1\n").unwrap()
}

fn adapters(name: &str) -> Vec<AdapterInfo> {
    parse_adapters(&read(name)).unwrap()
}

#[test]
fn ac1_discrete_dx12() {
    let s = select(&adapters("adapters-win-discrete.json"), &[], &windows_row(), &none(), None);
    assert_eq!(s.backend, Backend::Dx12);
    assert!(!s.fallback);
    assert_eq!(s.fallback_cause, FallbackCause::None);
    let a = s.adapter.as_ref().unwrap();
    assert_eq!((a.name.as_str(), a.vendor_id, a.device_id), ("Fixture Discrete GPU", 0x7E57_0010, 0x200));
    assert_eq!(a.device_type, DeviceType::Discrete);
    assert!(s.known_bad.is_empty() && s.workarounds.is_empty() && s.rejections.is_empty());
    assert_eq!(
        s.adapter_choice(),
        AdapterChoice {
            backend: "dx12".into(),
            vendor: Some(0x7E57_0010),
            device: Some(0x200),
            name: Some("Fixture Discrete GPU".into())
        }
    );
}

#[test]
fn ac2_vm_below_every_minimum_is_software() {
    let s = select(&adapters("adapters-vm-none.json"), &[], &windows_row(), &none(), None);
    assert_eq!(s.backend, Backend::Software);
    assert!(s.fallback);
    assert_eq!(s.fallback_cause, FallbackCause::NoQualifyingAdapter);
    assert!(s.adapter.is_none());
    assert!(s.reason.contains("Fixture Basic Display Adapter"), "{}", s.reason);
    assert!(s.reason.contains("below_minimum"), "{}", s.reason);
    assert_eq!(s.rejections.len(), 2);
    assert!(s.rejections.iter().all(|r| r.cause == Cause::BelowMinimum));
    assert_eq!(s.adapter_choice().backend, "software");
}

#[test]
fn ac3_avoid_backend_moves_to_vulkan_on_the_same_adapter() {
    let kb = parse_known_bad(&read("known-bad-detect.toml")).unwrap();
    let s = select(&adapters("adapters-win-discrete.json"), &[], &windows_row(), &kb, None);
    assert_eq!(s.backend, Backend::Vulkan);
    assert!(!s.fallback);
    let a = s.adapter.as_ref().unwrap();
    assert_eq!((a.vendor_id, a.device_id), (0x7E57_0010, 0x200), "the same discrete adapter");
    assert_eq!(s.known_bad, ["detect-avoid-dx12"]);
    assert_eq!(
        s.rejections,
        [Rejection {
            adapter: Some("Fixture Discrete GPU".into()),
            backend: Backend::Dx12,
            cause: Cause::KnownBad("detect-avoid-dx12".into())
        }]
    );
}

#[test]
fn ac4_force_software_beats_the_override() {
    let kb = parse_known_bad(&read("known-bad-detect-force.toml")).unwrap();
    let s = select(&adapters("adapters-win-discrete.json"), &[], &windows_row(), &kb, Some(Backend::Vulkan));
    assert_eq!(s.backend, Backend::Software);
    assert!(s.fallback);
    assert_eq!(s.fallback_cause, FallbackCause::KnownBad, "the override is beaten, not the cause");
    assert_eq!(s.known_bad, ["detect-force-vulkan"]);
    assert!(s.reason.contains("detect-force-vulkan"), "{}", s.reason);
    // Without the override, DX12 comes first and keeps the adapter with its workaround.
    let s = select(&adapters("adapters-win-discrete.json"), &[], &windows_row(), &kb, None);
    assert_eq!((s.backend, s.fallback, s.fallback_cause), (Backend::Dx12, false, FallbackCause::None));
    assert_eq!(s.workarounds, ["no_msaa"]);
    assert_eq!(s.known_bad, ["detect-workaround-dx12"]);
}

#[test]
fn override_rules() {
    let a = adapters("adapters-win-discrete.json");
    // An allowed override goes first.
    let s = select(&a, &[], &windows_row(), &none(), Some(Backend::Vulkan));
    assert_eq!(s.backend, Backend::Vulkan);
    assert_eq!(s.adapter.unwrap().device_type, DeviceType::Discrete, "device-type order within the backend");
    // One the row does not list is ignored and noted.
    let s = select(&a, &[], &windows_row(), &none(), Some(Backend::Metal));
    assert_eq!(s.backend, Backend::Dx12);
    assert_eq!(s.unverified.len(), 1, "{:?}", s.unverified);
    assert!(s.unverified[0].contains("override metal"), "{:?}", s.unverified);
}

/// REQ-004: an allowed `software` override is the escape from a crashing driver: `software`
/// at once, reason `user_override`, no adapter evaluated, and nothing probed.
#[test]
fn software_override_selects_software() {
    let a = adapters("adapters-win-discrete.json");
    let kb = parse_known_bad(&read("known-bad-detect-force.toml")).unwrap();
    for k in [none(), kb] {
        let s = select(&a, &[], &windows_row(), &k, Some(Backend::Software));
        assert_eq!((s.backend, s.fallback), (Backend::Software, true));
        // Structured, so PLAT.DOCTOR does not read this as a machine of GPU class `software`.
        assert_eq!(s.fallback_cause, FallbackCause::UserOverride);
        assert!(s.adapter.is_none());
        assert!(s.reason.starts_with(USER_OVERRIDE), "{}", s.reason);
        assert!(s.known_bad.is_empty() && s.workarounds.is_empty(), "no adapter is evaluated");
        assert!(s.unverified.is_empty() && s.rejections.is_empty(), "{s:?}");
        assert_eq!(s.adapter_choice().backend, "software");
    }
    // Probe rejections already recorded are carried, not lost.
    let hung = [Rejection { adapter: None, backend: Backend::Vulkan, cause: Cause::ProbeTimeout }];
    let s = select(&a, &hung, &windows_row(), &none(), Some(Backend::Software));
    assert_eq!(s.reason, "vulkan: probe_timeout; user_override: software requested by the caller");
    assert_eq!(s.probe_timeout, [Backend::Vulkan]);
    assert_eq!(s.fallback_cause, FallbackCause::UserOverride, "a probe timeout does not change the cause");
    let json = String::from_utf8(s.to_json().unwrap()).unwrap();
    assert!(json.contains("\"fallback\":true,\"fallback_cause\":\"user_override\""), "{json}");
    // The probe never touches the driver under a software override.
    let calls = Arc::new(AtomicUsize::new(0));
    let p = Probe::new(Counting(Arc::clone(&calls)));
    let r = p.run_with_override(&windows_row(), Some(Backend::Software), DEFAULT_PROBE_TIMEOUT);
    assert!(r.adapters.is_empty() && r.rejections.is_empty());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let r = p.run_with_override(&windows_row(), Some(Backend::Vulkan), DEFAULT_PROBE_TIMEOUT);
    assert!(r.rejections.is_empty());
    assert_eq!(calls.load(Ordering::SeqCst), 2, "a hardware override still probes every backend");
}

/// Counts enumerate calls; answers with no adapters.
struct Counting(Arc<AtomicUsize>);

impl Prober for Counting {
    fn enumerate(&self, _: Backend) -> Result<Vec<AdapterInfo>, String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(Vec::new())
    }
}

#[test]
fn cpu_adapters_and_unknown_versions() {
    let row = windows_row();
    let cpu = AdapterInfo {
        backend: Backend::Dx12,
        name: "Fixture Software Rasteriser".into(),
        vendor_id: 0x7E57_0030,
        device_id: 1,
        device_type: DeviceType::Cpu,
        driver_version: Some(Version::parse("10.0").unwrap()),
        api_version: Some(Version::parse("12.0").unwrap()),
        api_floor: None,
    };
    let s = select(std::slice::from_ref(&cpu), &[], &row, &none(), None);
    assert_eq!((s.backend, s.fallback), (Backend::Software, true));
    assert_eq!(s.fallback_cause, FallbackCause::NoQualifyingAdapter);
    assert_eq!(s.rejections[0].cause, Cause::NotHardware);
    // Unknown API and driver versions do not fail a minimum by themselves; they are listed.
    let unknown = AdapterInfo {
        device_type: DeviceType::Integrated,
        backend: Backend::Vulkan,
        api_version: None,
        driver_version: None,
        ..cpu
    };
    let s = select(std::slice::from_ref(&unknown), &[], &row, &none(), None);
    assert_eq!((s.backend, s.fallback), (Backend::Vulkan, false));
    assert_eq!(s.unverified.len(), 2, "{:?}", s.unverified);
}

/// The API minimum on real hardware: wgpu gives only a guaranteed floor. A floor at or above
/// the minimum verifies it; below it the minimum is unverified (listed), never a rejection;
/// an exact version below the minimum still rejects; known-bad entries still apply.
#[test]
fn api_floor_checks_the_minimum_where_it_can() {
    let a = AdapterInfo {
        backend: Backend::Dx12,
        name: "Fixture Discrete GPU".into(),
        vendor_id: 0x7E57_0010,
        device_id: 0x200,
        device_type: DeviceType::Discrete,
        driver_version: Some(Version::parse("31.0.1").unwrap()),
        api_version: None,
        api_floor: Version::parse("11.0"),
    };
    let mut row = windows_row();
    // Row minimum dx12 11.0, floor 11.0: verified.
    let s = select(std::slice::from_ref(&a), &[], &row, &none(), None);
    assert_eq!((s.backend, s.fallback), (Backend::Dx12, false));
    assert!(s.unverified.is_empty(), "{:?}", s.unverified);
    // Minimum 12.0 above the floor: selected, the minimum listed as unverified.
    row.min.insert(Backend::Dx12, Minimum { api: Version::parse("12.0").unwrap(), driver: None });
    let s = select(std::slice::from_ref(&a), &[], &row, &none(), None);
    assert_eq!(s.backend, Backend::Dx12);
    assert_eq!(
        s.unverified,
        ["Fixture Discrete GPU on dx12: API version unknown (at least 11.0); minimum 12.0 unverified"]
    );
    // No floor at all: also unverified.
    let s = select(&[AdapterInfo { api_floor: None, ..a.clone() }], &[], &row, &none(), None);
    assert_eq!(s.unverified, ["Fixture Discrete GPU on dx12: API version unknown; minimum 12.0 unverified"]);
    // An exact version below the minimum rejects, whatever the floor.
    let exact = AdapterInfo { api_version: Version::parse("11.1"), ..a.clone() };
    let s = select(&[exact], &[], &row, &none(), None);
    assert_eq!((s.backend, s.rejections[0].cause.clone()), (Backend::Software, Cause::BelowMinimum));
    // Known-bad entries apply to an adapter whose minimum is unverified.
    let kb = parse_known_bad(&read("known-bad-detect.toml")).unwrap();
    let s = select(&[a], &[], &row, &kb, None);
    assert_eq!(s.known_bad, ["detect-avoid-dx12"]);
    assert_eq!(s.backend, Backend::Software);
}

/// The driver-string parser reads the first dotted numeric token only, and gives `None`
/// (unknown, the safe side) when that token is not a clean version.
#[test]
fn driver_strings() {
    let v = |s: &str| Version::parse(s);
    assert_eq!(driver_version_of("31.0.15.4601"), v("31.0.15.4601"));
    assert_eq!(driver_version_of("Mesa 24.0.5 (git-abc)"), v("24.0.5"));
    assert_eq!(driver_version_of("560.94"), v("560.94"));
    // GL-style: the first token is taken, as the rule says (the GL API version here).
    assert_eq!(driver_version_of("4.6.0 NVIDIA 560.94"), v("4.6.0"));
    // AMDVLK: the first dotted token touches a letter: unknown, not 2.1.
    assert_eq!(driver_version_of("2024.Q2.1 (LLPC)"), None);
    // Five components: unknown, not the LLVM version 17.0.6.
    assert_eq!(driver_version_of("Mesa 24.0.5.1.2 (LLVM 17.0.6)"), None);
    assert_eq!(driver_version_of("v1.2"), None, "a token touching a letter");
    assert_eq!(driver_version_of("1.2a 3.4"), None, "a token touching a letter, no later token");
    assert_eq!(driver_version_of("Driver 1.2."), v("1.2"), "a sentence-ending dot");
    assert_eq!(driver_version_of("no digits"), None);
    assert_eq!(driver_version_of("v7"), None, "a lone number is not a driver version");
    assert_eq!(driver_version_of(""), None);
}

/// A prober scripted per backend: an answer, a failure, or a probe that never returns. Counts
/// the Vulkan probes started.
#[derive(Default)]
struct Stub(Arc<AtomicUsize>);

impl Prober for Stub {
    fn enumerate(&self, backend: Backend) -> Result<Vec<AdapterInfo>, String> {
        if backend == Backend::Vulkan {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
        match backend {
            Backend::Dx12 => Ok(parse_adapters(&read("adapters-win-discrete.json"))
                .unwrap()
                .into_iter()
                .filter(|a| a.backend == Backend::Dx12)
                .collect()),
            Backend::Vulkan => loop {
                std::thread::park();
            },
            _ => Err("not scripted".into()),
        }
    }
}

/// AC-5: a probe that never returns for Vulkan: probing completes at the 5 s deadline, Vulkan
/// is `probe_timeout`, selection completes, and nothing panics.
#[test]
fn ac5_probe_timeout() {
    let row = windows_row();
    let t0 = Instant::now();
    let r = Probe::new(Stub::default()).run(&row, DEFAULT_PROBE_TIMEOUT);
    let took = t0.elapsed();
    assert!(took >= DEFAULT_PROBE_TIMEOUT && took < DEFAULT_PROBE_TIMEOUT + Duration::from_secs(3), "{took:?}");
    assert_eq!(r.rejections, [Rejection { adapter: None, backend: Backend::Vulkan, cause: Cause::ProbeTimeout }]);
    assert_eq!(r.adapters.len(), 1);
    let s = select(&r.adapters, &r.rejections, &row, &none(), None);
    assert_eq!(s.backend, Backend::Dx12);
    assert!(s.reason.contains("vulkan: probe_timeout"), "{}", s.reason);
    assert_eq!(r.timed_out(), [Backend::Vulkan]);
    assert_eq!((s.probe_timeout, s.probe_failed), (vec![Backend::Vulkan], vec![]));
}

/// A hung driver is contained: while the Vulkan probe is still inside the driver, a new probe
/// reports Vulkan `probe_timeout` at once and starts no second thread for it.
#[test]
fn a_still_hung_backend_is_not_probed_again() {
    let row = windows_row();
    let vulkan_probes = Arc::new(AtomicUsize::new(0));
    let p = Probe::new(Stub(Arc::clone(&vulkan_probes)));
    let r = p.run(&row, Duration::from_millis(300));
    assert_eq!(r.timed_out(), [Backend::Vulkan]);
    assert_eq!(p.in_flight(), [Backend::Vulkan]);
    for _ in 0..3 {
        let t0 = Instant::now();
        let r = p.run(&row, Duration::from_secs(30));
        assert!(t0.elapsed() < Duration::from_secs(10), "no wait on the hung backend: {:?}", t0.elapsed());
        assert_eq!(r.timed_out(), [Backend::Vulkan]);
        assert_eq!(r.adapters.len(), 1, "DX12 is still probed");
    }
    assert_eq!(vulkan_probes.load(Ordering::SeqCst), 1, "one thread for the hung backend");
}

struct Failing;

impl Prober for Failing {
    fn enumerate(&self, backend: Backend) -> Result<Vec<AdapterInfo>, String> {
        match backend {
            Backend::Dx12 => Err("DXGI unavailable".into()),
            _ => panic!("a probe panics inside the graphics layer"),
        }
    }
}

#[test]
fn failing_and_panicking_probes_are_rejections() {
    let row = windows_row();
    let p = Probe::new(Failing);
    let r = p.run(&row, Duration::from_secs(2));
    assert!(r.adapters.is_empty());
    assert_eq!(
        r.rejections[0],
        Rejection { adapter: None, backend: Backend::Dx12, cause: Cause::ProbeFailed("DXGI unavailable".into()) }
    );
    assert!(matches!(&r.rejections[1], Rejection { backend: Backend::Vulkan, cause: Cause::ProbeFailed(_), .. }));
    let s = select(&r.adapters, &r.rejections, &row, &none(), None);
    assert_eq!((s.backend, s.fallback), (Backend::Software, true));
    assert_eq!(s.fallback_cause, FallbackCause::NoQualifyingAdapter, "every probe failed");
    assert_eq!(r.failed(), [Backend::Dx12, Backend::Vulkan]);
    assert_eq!((s.probe_failed, s.probe_timeout), (vec![Backend::Dx12, Backend::Vulkan], vec![]));
    // A failed or panicked probe is not left in flight: the next probe runs it again.
    assert!(p.in_flight().is_empty(), "{:?}", p.in_flight());
}

/// AC-7 and REQ-007: the serialised selection is byte-identical across runs and equal, byte
/// for byte, to the hand-written golden file `selection-win-discrete.golden.json` (written
/// from the spec's rules and checked as RFC 8785 key order by Python's `json.dumps`, not by
/// running this crate).
#[test]
fn ac7_json_is_deterministic() {
    let a = adapters("adapters-win-discrete.json");
    let one = select(&a, &[], &windows_row(), &none(), None).to_json().unwrap();
    let two = select(&a, &[], &windows_row(), &none(), None).to_json().unwrap();
    assert_eq!(one, two);
    let golden = read("selection-win-discrete.golden.json");
    assert_eq!(one, golden, "\n got: {}\nwant: {}", String::from_utf8_lossy(&one), String::from_utf8_lossy(&golden));
    let sw = select(&adapters("adapters-vm-none.json"), &[], &windows_row(), &none(), None).to_json().unwrap();
    let sw = String::from_utf8(sw).unwrap();
    assert!(sw.contains("\"adapter\":null"), "{sw}");
    assert!(sw.contains("\"fallback_cause\":\"no_qualifying_adapter\""), "{sw}");
}

/// `fallback_cause` names round-trip, and `fallback` is set exactly when the cause is not `none`.
#[test]
fn fallback_cause_names() {
    for (c, name) in [
        (FallbackCause::None, "none"),
        (FallbackCause::NoQualifyingAdapter, "no_qualifying_adapter"),
        (FallbackCause::KnownBad, "known_bad"),
        (FallbackCause::UserOverride, "user_override"),
    ] {
        assert_eq!(c.as_str(), name);
        assert_eq!(FallbackCause::parse(name), Some(c));
    }
    assert_eq!(FallbackCause::parse("software"), None);
    let a = adapters("adapters-win-discrete.json");
    let force = parse_known_bad(&read("known-bad-detect-force.toml")).unwrap();
    for (kb, o) in
        [(none(), None), (none(), Some(Backend::Software)), (force.clone(), Some(Backend::Vulkan)), (force, None)]
    {
        let s = select(&a, &[], &windows_row(), &kb, o);
        assert_eq!(s.fallback, s.fallback_cause != FallbackCause::None, "{s:?}");
    }
    let s = select(&adapters("adapters-vm-none.json"), &[], &windows_row(), &none(), None);
    assert_eq!((s.fallback, s.fallback_cause), (true, FallbackCause::NoQualifyingAdapter));
}

#[test]
fn fixtures_and_parsing() {
    // The detect known-bad fixtures pass the policy validator.
    for f in ["known-bad-detect.toml", "known-bad-detect-force.toml"] {
        let v = fid_gpu_policy::policy::validate_known_bad(&read(f));
        assert!(v.errors.is_empty(), "{f}: {:?}", v.errors);
    }
    assert!(parse_adapters(b"{}").is_err());
    assert!(parse_adapters(br#"[{"backend":"dx12"}]"#).is_err());
    let dup = br#"[{"api_version":null,"api_version":null,"backend":"dx12","device_id":"0x1","device_type":"other","driver_version":null,"name":"a","vendor_id":"0x1"}]"#;
    assert!(parse_adapters(dup).is_err(), "duplicate keys are refused");
    let bad_id = br#"[{"api_version":null,"backend":"dx12","device_id":"1","device_type":"other","driver_version":null,"name":"a","vendor_id":"0x1"}]"#;
    assert!(parse_adapters(bad_id).is_err());
    let unknown_key = br#"[{"api_version":null,"backend":"dx12","device_id":"0x1","device_type":"other","driver_version":null,"extra":null,"name":"a","vendor_id":"0x1"}]"#;
    assert!(parse_adapters(unknown_key).is_err());
    let floor = br#"[{"api_floor":"11.0","api_version":null,"backend":"dx12","device_id":"0x1","device_type":"other","driver_version":null,"name":"a","vendor_id":"0x1"}]"#;
    assert_eq!(parse_adapters(floor).unwrap()[0].api_floor, Version::parse("11.0"));
    assert_eq!(adapters("adapters-win-discrete.json")[0].api_floor, None, "api_floor is optional");
}

/// AC-6: no crate of the kernel graph (layers `num` and `kernel`) reaches fid-gpu-select in the
/// resolved dependency graph (any dependency kind); the layer gate (`cargo xtask layers`)
/// enforces the same from layers.toml, where the crate is `viewer`.
#[test]
fn ac6_not_in_the_kernel_graph() {
    let layers = std::fs::read_to_string(root().join("layers.toml")).unwrap();
    assert!(layers.contains("\"fid-gpu-select\" = \"viewer\""));
    let kernel: Vec<String> = layers
        .lines()
        .filter(|l| l.ends_with("= \"kernel\"") || l.ends_with("= \"num\""))
        .filter_map(|l| l.split('"').nth(1).map(str::to_string))
        .collect();
    assert!(kernel.iter().any(|k| k == "fid-num"), "{kernel:?}");
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let out = std::process::Command::new(cargo)
        .current_dir(root())
        .args(["metadata", "--format-version", "1", "--offline", "--locked"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let meta = fid_canon::parse(&out.stdout).unwrap();
    use fid_canon::CValue;
    let obj = |v: &CValue| match v {
        CValue::Obj(m) => m.clone(),
        _ => panic!("object"),
    };
    let m = obj(&meta);
    let CValue::Arr(nodes) = &obj(&m["resolve"])["nodes"] else { panic!("nodes") };
    let mut deps: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for n in nodes {
        let n = obj(n);
        let CValue::Str(id) = &n["id"] else { panic!("id") };
        let CValue::Arr(ds) = &n["dependencies"] else { panic!("deps") };
        deps.insert(
            id.clone(),
            ds.iter().filter_map(|d| if let CValue::Str(s) = d { Some(s.clone()) } else { None }).collect(),
        );
    }
    let name_of = |id: &str| -> String {
        // Package IDs are `<source>#<name>@<version>`, or `path+file:///.../<name>#<version>`
        // when the directory has the package's name.
        let (path, frag) = id.rsplit_once('#').unwrap_or((id, ""));
        match frag.split_once('@') {
            Some((name, _)) => name.to_string(),
            None => path.rsplit('/').next().unwrap_or(path).to_string(),
        }
    };
    let roots: Vec<&String> = deps.keys().filter(|id| kernel.contains(&name_of(id))).collect();
    let mut found: Vec<String> = roots.iter().map(|id| name_of(id)).collect();
    found.sort();
    found.dedup();
    let mut want = kernel.clone();
    want.sort();
    // Every kernel-graph crate of layers.toml was found in the resolve graph (a name the
    // walk cannot match would otherwise pass unchecked).
    assert_eq!(roots.len(), kernel.len(), "roots {found:?}, kernel crates {want:?}");
    assert_eq!(found, want);
    for root_id in roots {
        let mut stack = vec![root_id.clone()];
        let mut seen = std::collections::BTreeSet::new();
        while let Some(id) = stack.pop() {
            if !seen.insert(id.clone()) {
                continue;
            }
            assert_ne!(name_of(&id), "fid-gpu-select", "{} reaches fid-gpu-select", name_of(root_id));
            stack.extend(deps.get(&id).cloned().unwrap_or_default());
        }
    }
}
