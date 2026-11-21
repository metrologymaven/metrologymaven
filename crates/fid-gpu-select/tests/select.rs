//! Spec leaf PLAT.GPU.DETECT: AC-1 .. AC-7 over the recorded adapter lists and policy and
//! known-bad fixtures of `fixtures/plat/gpu/` (hand-written, invented IDs above the PCI range;
//! the expected outcomes below are hand-labelled from the spec's rules). In
//! `adapters-win-discrete.json` the discrete adapter is on DX12 and Vulkan and the integrated
//! one on Vulkan only.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_gpu_policy::policy::{parse_known_bad, parse_policy, Backend, GpuOs, KnownBad, PolicyRow};
use fid_gpu_policy::{Arch, Version};
use fid_gpu_select::{
    parse_adapters, probe, select, AdapterChoice, AdapterInfo, Cause, DeviceType, Prober, Rejection,
    DEFAULT_PROBE_TIMEOUT,
};
use std::path::PathBuf;
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
    assert_eq!(s.known_bad, ["detect-force-vulkan"]);
    assert!(s.reason.contains("detect-force-vulkan"), "{}", s.reason);
    // Without the override, DX12 comes first and keeps the adapter with its workaround.
    let s = select(&adapters("adapters-win-discrete.json"), &[], &windows_row(), &kb, None);
    assert_eq!((s.backend, s.fallback), (Backend::Dx12, false));
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
    // One the row does not list (or `software`) is ignored and noted.
    for o in [Backend::Metal, Backend::Software] {
        let s = select(&a, &[], &windows_row(), &none(), Some(o));
        assert_eq!(s.backend, Backend::Dx12);
        assert_eq!(s.unverified.len(), 1, "{:?}", s.unverified);
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
    };
    let s = select(std::slice::from_ref(&cpu), &[], &row, &none(), None);
    assert_eq!((s.backend, s.fallback), (Backend::Software, true));
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

/// A prober scripted per backend: an answer, a failure, or a probe that never returns.
struct Stub;

impl Prober for Stub {
    fn enumerate(&self, backend: Backend) -> Result<Vec<AdapterInfo>, String> {
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
    let r = probe(Arc::new(Stub), &row, DEFAULT_PROBE_TIMEOUT);
    let took = t0.elapsed();
    assert!(took >= DEFAULT_PROBE_TIMEOUT && took < DEFAULT_PROBE_TIMEOUT + Duration::from_secs(3), "{took:?}");
    assert_eq!(r.rejections, [Rejection { adapter: None, backend: Backend::Vulkan, cause: Cause::ProbeTimeout }]);
    assert_eq!(r.adapters.len(), 1);
    let s = select(&r.adapters, &r.rejections, &row, &none(), None);
    assert_eq!(s.backend, Backend::Dx12);
    assert!(s.reason.contains("vulkan: probe_timeout"), "{}", s.reason);
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
    let r = probe(Arc::new(Failing), &row, Duration::from_secs(2));
    assert!(r.adapters.is_empty());
    assert_eq!(
        r.rejections[0],
        Rejection { adapter: None, backend: Backend::Dx12, cause: Cause::ProbeFailed("DXGI unavailable".into()) }
    );
    assert!(matches!(&r.rejections[1], Rejection { backend: Backend::Vulkan, cause: Cause::ProbeFailed(_), .. }));
    let s = select(&r.adapters, &r.rejections, &row, &none(), None);
    assert_eq!((s.backend, s.fallback), (Backend::Software, true));
}

/// AC-7 and REQ-007: the serialised selection is byte-identical across runs and has the fields.
#[test]
fn ac7_json_is_deterministic() {
    let a = adapters("adapters-win-discrete.json");
    let one = select(&a, &[], &windows_row(), &none(), None).to_json().unwrap();
    let two = select(&a, &[], &windows_row(), &none(), None).to_json().unwrap();
    assert_eq!(one, two);
    let text = String::from_utf8(one).unwrap();
    for k in [
        "\"backend\":\"dx12\"",
        "\"name\":\"Fixture Discrete GPU\"",
        "\"vendor_id\":\"0x7e570010\"",
        "\"device_id\":\"0x00000200\"",
        "\"driver_version\":\"31.0.1\"",
        "\"fallback\":false",
        "\"reason\":",
        "\"known_bad\":[]",
        "\"workarounds\":[]",
    ] {
        assert!(text.contains(k), "{k} in {text}");
    }
    let sw = select(&adapters("adapters-vm-none.json"), &[], &windows_row(), &none(), None).to_json().unwrap();
    assert!(String::from_utf8(sw).unwrap().contains("\"adapter\":null"));
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
    assert_eq!(fid_gpu_select::driver_version_of("31.0.15.4601"), Version::parse("31.0.15.4601"));
    assert_eq!(fid_gpu_select::driver_version_of("Mesa 24.0.5 (git-abc)"), Version::parse("24.0.5"));
    assert_eq!(fid_gpu_select::driver_version_of("no digits"), None);
    assert_eq!(fid_gpu_select::driver_version_of("v7"), None, "a lone number is not a driver version");
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
        // Package IDs are `path+file:///.../crates/<name>#<version>` or `registry+...#<name>@<version>`.
        let tail = id.rsplit('/').next().unwrap_or(id);
        tail.split(['#', '@']).next().unwrap_or(tail).to_string()
    };
    for root_id in deps.keys().filter(|id| kernel.contains(&name_of(id))) {
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
