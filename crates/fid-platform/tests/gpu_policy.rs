//! Acceptance criteria of spec node PLAT.GPU.POLICY (AC-1 .. AC-6) and the requirement edge
//! cases. Expected diagnostics are hand-labelled from the spec's rules (each fixture in
//! `fixtures/plat/gpu/` states its expected outcome in its header comment); none is produced
//! by running this crate. Known-bad data is invented test data, never real driver claims.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_platform::gpu_policy::{
    backends_or_software, check_coverage, known_bad_or_empty, parse_known_bad, parse_policy, validate_known_bad,
    validate_policy, Backend, BadAction, GpuOs, GpuPolicyError, IdRange, Limit, Probe, VersionRange, MAX_BYTES,
    MAX_ENTRIES,
};
use fid_platform::matrix::{self, Arch, OsFamily, Phase, Version};
use std::path::PathBuf;
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn gpu(name: &str) -> String {
    format!("fixtures/plat/gpu/{name}")
}

const MATRIX_FIXTURE: &str = "fixtures/plat/matrix/valid.toml";

/// Runs `fid-platform plat-gpu validate <args>` from the repository root.
fn cli(args: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_fid-platform"))
        .current_dir(root())
        .args(["plat-gpu", "validate"])
        .args(args)
        .output()
        .unwrap();
    (out.status.code().unwrap(), String::from_utf8(out.stdout).unwrap())
}

fn read(rel: &str) -> Vec<u8> {
    std::fs::read(root().join(rel)).unwrap()
}

fn policy_errors(text: &str) -> Vec<GpuPolicyError> {
    validate_policy(text.as_bytes()).errors
}

fn bad_errors(text: &str) -> Vec<GpuPolicyError> {
    validate_known_bad(text.as_bytes()).errors
}

fn v(s: &str) -> Version {
    Version::parse(s).unwrap()
}

// ---------------------------------------------------------------- acceptance criteria

#[test]
fn ac1_valid_policy_against_matrix_fixture_exits_0() {
    let (code, out) = cli(&["--policy", &gpu("policy-valid.toml"), "--matrix", MATRIX_FIXTURE, "--phase", "P1a"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out, "policy: 4 rows, 0 errors\nknown-bad: 0 entries, 0 errors\n");

    let p = parse_policy(&read(&gpu("policy-valid.toml"))).unwrap();
    let m = matrix::parse(&read(MATRIX_FIXTURE)).unwrap();
    assert!(check_coverage(&p, &m, Phase::P1a).is_empty());
    let win = p.row(GpuOs::Windows, Arch::X86_64).unwrap();
    assert_eq!(win.backends, vec![Backend::Dx12, Backend::Vulkan, Backend::Software]);
    assert_eq!(win.min[&Backend::Dx12].api, v("11.0"));
    assert_eq!(win.min[&Backend::Dx12].driver, None);
    assert_eq!(win.min[&Backend::Vulkan].driver, Some(v("1.0")));
    assert!(!win.min.contains_key(&Backend::Software));
}

#[test]
fn ac2_windows_without_vulkan() {
    let (code, out) = cli(&["--policy", &gpu("policy-win-no-vulkan.toml"), "--matrix", MATRIX_FIXTURE]);
    assert_eq!(code, 1, "{out}");
    assert_eq!(
        out,
        "MissingMandatoryBackend { os: windows, backend: vulkan }: row 1 (windows x86_64)\n\
         policy: 3 rows, 1 errors\nknown-bad: 0 entries, 0 errors\n"
    );
    assert_eq!(
        validate_policy(&read(&gpu("policy-win-no-vulkan.toml"))).errors,
        vec![GpuPolicyError::MissingMandatoryBackend {
            row: 1,
            os: GpuOs::Windows,
            arch: Arch::X86_64,
            backend: Backend::Vulkan
        }]
    );
}

#[test]
fn ac3_missing_macos_row_at_p1a() {
    let (code, out) = cli(&["--policy", &gpu("policy-missing-row.toml"), "--matrix", MATRIX_FIXTURE, "--phase", "P1a"]);
    assert_eq!(code, 1, "{out}");
    assert_eq!(
        out,
        "NoPolicyRow { os: macos, arch: aarch64 }\npolicy: 2 rows, 1 errors\nknown-bad: 0 entries, 0 errors\n"
    );

    let p = parse_policy(&read(&gpu("policy-missing-row.toml"))).unwrap();
    let m = matrix::parse(&read(MATRIX_FIXTURE)).unwrap();
    assert_eq!(
        check_coverage(&p, &m, Phase::P1a),
        vec![GpuPolicyError::NoPolicyRow { os: GpuOs::Macos, arch: Arch::Aarch64 }]
    );
    // At P0 the macOS row (tier1 since P1a) is only planned, so nothing is missing.
    assert!(check_coverage(&p, &m, Phase::P0).is_empty());
    // At P1b ubuntu aarch64 (tier2 since P1b) becomes effective too; sorted (os, arch).
    assert_eq!(
        check_coverage(&p, &m, Phase::P1b),
        vec![
            GpuPolicyError::NoPolicyRow { os: GpuOs::Linux, arch: Arch::Aarch64 },
            GpuPolicyError::NoPolicyRow { os: GpuOs::Macos, arch: Arch::Aarch64 },
        ]
    );
}

#[test]
fn ac4_no_software_fallback() {
    let (code, out) = cli(&["--policy", &gpu("policy-no-software.toml"), "--matrix", MATRIX_FIXTURE]);
    assert_eq!(code, 1, "{out}");
    assert_eq!(out.lines().count(), 3, "{out}");
    assert!(out.starts_with("NoSoftwareFallback: row 2 (linux x86_64)"), "{out}");
    assert_eq!(
        validate_policy(&read(&gpu("policy-no-software.toml"))).errors,
        vec![GpuPolicyError::NoSoftwareFallback { row: 2, os: Some(GpuOs::Linux), arch: Some(Arch::X86_64) }]
    );
}

#[test]
fn ac5_known_bad_valid_and_noref() {
    let (code, out) = cli(&["--known-bad", &gpu("known-bad-valid.toml")]);
    assert_eq!(code, 0, "{out}");
    assert!(out.ends_with("known-bad: 3 entries, 0 errors\n"), "{out}");

    let k = parse_known_bad(&read(&gpu("known-bad-valid.toml"))).unwrap();
    assert_eq!(k.entries.len(), 3);
    let e = &k.entries[0];
    assert_eq!(e.id, "fixture-avoid-1");
    assert_eq!(e.vendor_id, 0x7E57_0001);
    assert_eq!(e.device_ids, IdRange::Range { min: 0x0100, max: 0x01FF });
    assert_eq!(e.driver, VersionRange { min: Some(v("1.2")), max: Some(v("1.4")) });
    assert_eq!(e.action, BadAction::AvoidBackend);
    assert_eq!(k.entries[1].device_ids, IdRange::Any);
    assert_eq!(k.entries[1].driver, VersionRange { min: None, max: Some(v("2.0")) });
    assert_eq!(k.entries[1].action, BadAction::ForceSoftware);
    assert_eq!(k.entries[2].driver, VersionRange { min: None, max: None });
    assert_eq!(k.entries[2].action, BadAction::Workaround("no_msaa".into()));

    let (code, out) = cli(&["--known-bad", &gpu("known-bad-noref.toml")]);
    assert_eq!(code, 1, "{out}");
    assert!(out.ends_with("MissingReference { id: fixture-noref }\nknown-bad: 2 entries, 1 errors\n"), "{out}");
    assert_eq!(
        validate_known_bad(&read(&gpu("known-bad-noref.toml"))).errors,
        vec![GpuPolicyError::MissingReference { id: "fixture-noref".into() }]
    );
}

#[test]
fn ac6_shipped_files_validate() {
    let (code, out) = cli(&[]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out, "policy: 4 rows, 0 errors\nknown-bad: 0 entries, 0 errors\n");
    // The proposal rows of the spec's Design.
    let p = parse_policy(&read("platform/gpu-policy.toml")).unwrap();
    let rows: Vec<(GpuOs, Arch, Vec<Backend>)> = p.rows.iter().map(|r| (r.os, r.arch, r.backends.clone())).collect();
    use Backend::{Dx12, Metal, Software, Vulkan};
    assert_eq!(
        rows,
        vec![
            (GpuOs::Windows, Arch::X86_64, vec![Dx12, Vulkan, Software]),
            (GpuOs::Linux, Arch::X86_64, vec![Vulkan, Software]),
            (GpuOs::Linux, Arch::Aarch64, vec![Vulkan, Software]),
            (GpuOs::Macos, Arch::Aarch64, vec![Metal, Software]),
        ]
    );
    assert!(parse_known_bad(&read("platform/gpu-known-bad.toml")).unwrap().entries.is_empty());
}

// ---------------------------------------------------------------- policy rules

const ROW: &str = "schema_version = 1\n[[row]]\nos = \"linux\"\narch = \"x86_64\"\n";

#[test]
fn req001_unknown_keys_are_rejected() {
    let text = format!(
        "{ROW}backends = [\"vulkan\", \"software\"]\nmin.vulkan = {{ api = \"1.1\", extra = 1 }}\ncolour = 1\n"
    );
    let errs = policy_errors(&text);
    assert_eq!(errs.len(), 2, "{errs:?}");
    assert_eq!(errs[0].to_string(), "BadField: row 1: colour is not a known key");
    assert_eq!(errs[1].to_string(), "BadField: row 1: min.vulkan.extra is not a known key");
    let errs = policy_errors("schema_version = 1\nrows = []\n");
    assert_eq!(
        errs,
        vec![GpuPolicyError::BadField { item: None, field: "rows".into(), msg: "is not a known key".into() }]
    );
}

#[test]
fn req001_unknown_backend_and_os() {
    let errs = policy_errors(
        "schema_version = 1\n[[row]]\nos = \"bsd\"\narch = \"x86_64\"\nbackends = [\"opengl\", \"software\"]\n",
    );
    assert_eq!(errs.len(), 2, "{errs:?}");
    assert!(errs[0].to_string().starts_with("BadField: row 1: os is not one of windows, linux, macos"));
    assert!(errs[1].to_string().starts_with("BadField: row 1: backends has an element"));
}

#[test]
fn req001_hardware_backend_needs_a_minimum_and_minimums_need_a_backend() {
    let errs = policy_errors(&format!("{ROW}backends = [\"vulkan\", \"software\"]\n"));
    assert_eq!(errs[0].to_string(), "BadField: row 1: min.vulkan is missing");
    let errs = policy_errors(&format!(
        "{ROW}backends = [\"vulkan\", \"software\"]\nmin.vulkan = {{ api = \"1.1\" }}\nmin.metal = {{ api = \"2\" }}\n"
    ));
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert_eq!(errs[0].to_string(), "BadField: row 1: min.metal names a backend that is not in backends");
    let errs =
        policy_errors(&format!("{ROW}backends = [\"vulkan\", \"software\"]\nmin.vulkan = {{ api = \"1.x\" }}\n"));
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert!(errs[0].to_string().starts_with("BadField: row 1: min.vulkan.api is not a dotted numeric version"));
    let errs =
        policy_errors(&format!("{ROW}backends = [\"vulkan\", \"software\"]\nmin.vulkan = {{ driver = \"1\" }}\n"));
    assert_eq!(
        errs,
        vec![GpuPolicyError::BadField {
            item: Some("row 1".into()),
            field: "min.vulkan.api".into(),
            msg: "is missing".into()
        }]
    );
}

#[test]
fn req003_mandatory_backends_per_os() {
    // Windows without both: two diagnostics in Backend::ALL order (vulkan, dx12).
    let errs =
        policy_errors("schema_version = 1\n[[row]]\nos = \"windows\"\narch = \"x86_64\"\nbackends = [\"software\"]\n");
    assert_eq!(
        errs,
        vec![
            GpuPolicyError::MissingMandatoryBackend {
                row: 1,
                os: GpuOs::Windows,
                arch: Arch::X86_64,
                backend: Backend::Vulkan
            },
            GpuPolicyError::MissingMandatoryBackend {
                row: 1,
                os: GpuOs::Windows,
                arch: Arch::X86_64,
                backend: Backend::Dx12
            },
        ]
    );
    let errs = policy_errors("schema_version = 1\n[[row]]\nos = \"macos\"\narch = \"aarch64\"\nbackends = [\"vulkan\", \"software\"]\nmin.vulkan = { api = \"1.2\" }\n");
    assert_eq!(
        errs,
        vec![GpuPolicyError::MissingMandatoryBackend {
            row: 1,
            os: GpuOs::Macos,
            arch: Arch::Aarch64,
            backend: Backend::Metal
        }]
    );
    let errs = policy_errors(&format!("{ROW}backends = [\"software\"]\n"));
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert!(errs[0].to_string().starts_with("MissingMandatoryBackend { os: linux, backend: vulkan }"));
}

#[test]
fn req004_software_must_be_last() {
    let errs =
        policy_errors(&format!("{ROW}backends = [\"software\", \"vulkan\"]\nmin.vulkan = {{ api = \"1.1\" }}\n"));
    assert_eq!(
        errs,
        vec![GpuPolicyError::NoSoftwareFallback { row: 1, os: Some(GpuOs::Linux), arch: Some(Arch::X86_64) }]
    );
}

#[test]
fn duplicate_backend_and_duplicate_row() {
    let errs = policy_errors(&format!(
        "{ROW}backends = [\"vulkan\", \"vulkan\", \"vulkan\", \"software\"]\nmin.vulkan = {{ api = \"1.1\" }}\n"
    ));
    assert_eq!(errs, vec![GpuPolicyError::DuplicateBackend { row: 1, backend: Backend::Vulkan }]);
    let row = "[[row]]\nos = \"linux\"\narch = \"x86_64\"\nbackends = [\"vulkan\", \"software\"]\nmin.vulkan = { api = \"1.1\" }\n";
    let errs = policy_errors(&format!("schema_version = 1\n{row}{row}{row}"));
    assert_eq!(
        errs,
        vec![
            GpuPolicyError::DuplicateRow { a: 1, b: 2, os: GpuOs::Linux, arch: Arch::X86_64 },
            GpuPolicyError::DuplicateRow { a: 1, b: 3, os: GpuOs::Linux, arch: Arch::X86_64 },
        ]
    );
}

#[test]
fn coverage_maps_every_linux_family_to_linux() {
    assert_eq!(GpuOs::from_family(OsFamily::Windows), GpuOs::Windows);
    for f in [OsFamily::Ubuntu, OsFamily::Debian, OsFamily::Fedora, OsFamily::Rhel] {
        assert_eq!(GpuOs::from_family(f), GpuOs::Linux);
    }
    assert_eq!(GpuOs::from_family(OsFamily::Macos), GpuOs::Macos);
    // Tier 3 rows never need a policy row.
    let m = matrix::parse(
        b"schema_version = 1\n[[row]]\nos = \"fedora\"\nversion_min = \"40\"\narch = \"aarch64\"\ntier = \"tier3\"\nsince_phase = \"P0\"\nnotes = \"t\"\n",
    )
    .unwrap();
    let p = parse_policy(b"schema_version = 1\n").unwrap();
    assert!(check_coverage(&p, &m, Phase::Later).is_empty());
}

#[test]
fn schema_version_rules() {
    let v2 = validate_policy(b"schema_version = 2\n");
    assert_eq!(v2.errors, vec![GpuPolicyError::UnsupportedSchema { found: 2 }]);
    let k2 = validate_known_bad(b"schema_version = 2\n");
    assert_eq!(k2.errors, vec![GpuPolicyError::UnsupportedSchema { found: 2 }]);
    let errs = policy_errors("");
    assert_eq!(errs[0].to_string(), "BadField: schema_version is missing");
    let errs = bad_errors("schema_version = 0\n");
    assert_eq!(errs[0].to_string(), "BadField: schema_version is not a positive integer");
}

#[test]
fn parse_errors_are_fatal_with_a_line() {
    let errs = policy_errors("schema_version = 1\n[[row]]\nos = \n");
    assert_eq!(errs.len(), 1);
    assert!(matches!(errs[0], GpuPolicyError::Parse { line: Some(3), .. }), "{errs:?}");
    let errs = bad_errors("schema_version = 1\nschema_version = 1\n");
    assert!(matches!(errs[0], GpuPolicyError::Parse { .. }), "{errs:?}");
    let errs = validate_known_bad(&[0xFF, 0xFE]).errors;
    assert_eq!(errs, vec![GpuPolicyError::Parse { line: None, msg: "file is not UTF-8".into() }]);
}

// ---------------------------------------------------------------- size limits (REQ-007)

#[test]
fn req007_too_many_bytes() {
    let mut text = String::from("schema_version = 1\n");
    while text.len() <= MAX_BYTES {
        text.push_str("# padding padding padding padding padding padding padding padding\n");
    }
    let found = text.len();
    let expected = vec![GpuPolicyError::TooLarge { limit: Limit::Bytes, found, max: MAX_BYTES }];
    assert_eq!(policy_errors(&text), expected);
    assert_eq!(bad_errors(&text), expected);
    // Exactly MAX_BYTES is accepted.
    let mut ok = String::from("schema_version = 1\n");
    ok.push_str(&"#".repeat(MAX_BYTES - ok.len() - 1));
    ok.push('\n');
    assert_eq!(ok.len(), MAX_BYTES);
    assert!(policy_errors(&ok).is_empty());
}

#[test]
fn req007_too_many_entries() {
    let mut text = String::from("schema_version = 1\n");
    for _ in 0..=MAX_ENTRIES {
        text.push_str("[[entry]]\n");
    }
    assert!(text.len() < MAX_BYTES);
    assert_eq!(bad_errors(&text), vec![GpuPolicyError::TooLarge { limit: Limit::Entries, found: 10_001, max: 10_000 }]);
    let rows = text.replace("[[entry]]", "[[row]]");
    assert_eq!(
        policy_errors(&rows),
        vec![GpuPolicyError::TooLarge { limit: Limit::Entries, found: 10_001, max: 10_000 }]
    );
    // Inline arrays escape the header prescan but not the post-parse count.
    let inline = format!("schema_version = 1\nentry = [{}{{}}]\n", "{},".repeat(MAX_ENTRIES));
    assert!(inline.len() < MAX_BYTES);
    assert_eq!(
        bad_errors(&inline),
        vec![GpuPolicyError::TooLarge { limit: Limit::Entries, found: 10_001, max: 10_000 }]
    );
}

// ---------------------------------------------------------------- known-bad rules

const ENTRY: &str = r#"schema_version = 1
[[entry]]
id = "t-1"
os = "linux"
backend = "vulkan"
vendor_id = 0x7E570009
device_ids = "any"
driver = "any"
action = "avoid_backend"
reason = "invented"
reference = "https://example.invalid/t-1"
"#;

/// `ENTRY` with the line starting with `key =` replaced by `line` (or removed if empty).
fn entry_with(key: &str, line: &str) -> String {
    let prefix = format!("{key} =");
    let mut out = String::new();
    for l in ENTRY.lines() {
        if l.starts_with(&prefix) {
            if !line.is_empty() {
                out.push_str(line);
                out.push('\n');
            }
        } else {
            out.push_str(l);
            out.push('\n');
        }
    }
    out
}

#[test]
fn base_entry_is_valid() {
    assert!(bad_errors(ENTRY).is_empty());
}

#[test]
fn req006_reason_and_reference_are_mandatory() {
    assert_eq!(bad_errors(&entry_with("reason", "")), vec![GpuPolicyError::MissingReason { id: "t-1".into() }]);
    assert_eq!(
        bad_errors(&entry_with("reason", "reason = \"   \"")),
        vec![GpuPolicyError::MissingReason { id: "t-1".into() }]
    );
    assert_eq!(bad_errors(&entry_with("reference", "")), vec![GpuPolicyError::MissingReference { id: "t-1".into() }]);
    for bad in ["\"\"", "\"see the forum\"", "\"http://example.invalid/x\"", "\"https://\"", "\"https://a b\"", "7"] {
        assert_eq!(
            bad_errors(&entry_with("reference", &format!("reference = {bad}"))),
            vec![GpuPolicyError::MissingReference { id: "t-1".into() }],
            "{bad}"
        );
    }
    // Without a usable id the entry is named by its position.
    let errs = bad_errors(&entry_with("id", "").replace("reason = \"invented\"\n", ""));
    assert_eq!(errs[0].to_string(), "BadField: entry 1: id is missing");
    assert_eq!(errs[1], GpuPolicyError::MissingReason { id: "entry 1".into() });
}

#[test]
fn req005_actions() {
    for (text, action) in [
        ("force_software", BadAction::ForceSoftware),
        ("workaround:a", BadAction::Workaround("a".into())),
        ("workaround:no_msaa_2", BadAction::Workaround("no_msaa_2".into())),
    ] {
        let k = parse_known_bad(entry_with("action", &format!("action = \"{text}\"")).as_bytes()).unwrap();
        assert_eq!(k.entries[0].action, action);
        assert_eq!(k.entries[0].action.to_string(), text);
    }
    let long = format!("workaround:{}", "a".repeat(65));
    for bad in ["workaround:", "workaround:Upper", "workaround:1st", "workaround:a-b", "avoid", long.as_str()] {
        let errs = bad_errors(&entry_with("action", &format!("action = \"{bad}\"")));
        assert_eq!(errs.len(), 1, "{bad}: {errs:?}");
        assert!(errs[0].to_string().starts_with("BadField: entry 1: action is not"), "{bad}");
    }
}

#[test]
fn req005_bad_ranges() {
    let errs = bad_errors(&entry_with("device_ids", "device_ids = { min = 0x0200, max = 0x0100 }"));
    assert_eq!(errs, vec![GpuPolicyError::BadRange { id: "t-1".into(), field: "device_ids" }]);
    let errs = bad_errors(&entry_with("driver", "driver = { min = \"2.0\", max = \"2\" }"));
    assert_eq!(errs, vec![GpuPolicyError::BadRange { id: "t-1".into(), field: "driver" }]);
    let errs = bad_errors(&entry_with("driver", "driver = {}"));
    assert_eq!(errs.len(), 1);
    assert!(errs[0].to_string().starts_with("BadField: entry 1: driver has neither min nor max"));
    let errs = bad_errors(&entry_with("device_ids", "device_ids = { min = 1 }"));
    assert_eq!(errs[0].to_string(), "BadField: entry 1: device_ids.max is missing");
    let errs = bad_errors(&entry_with("vendor_id", "vendor_id = -1"));
    assert_eq!(errs[0].to_string(), "BadField: entry 1: vendor_id is not an integer in 0..=0xFFFFFFFF");
    let errs = bad_errors(&entry_with("vendor_id", "vendor_id = 0x100000000"));
    assert_eq!(errs.len(), 1);
    let errs = bad_errors(&entry_with("device_ids", ""));
    assert!(errs[0].to_string().starts_with("BadField: entry 1: device_ids is missing"));
    let errs = bad_errors(&entry_with("driver", "driver = { min = \"1\", newest = \"2\" }"));
    assert_eq!(errs[0].to_string(), "BadField: entry 1: driver.newest is not a known key");
}

#[test]
fn known_bad_unknown_keys_and_duplicate_ids() {
    let errs = bad_errors(&format!("{ENTRY}severity = 3\n"));
    assert_eq!(
        errs,
        vec![GpuPolicyError::BadField {
            item: Some("entry 1".into()),
            field: "severity".into(),
            msg: "is not a known key".into()
        }]
    );
    let second = ENTRY.replace("schema_version = 1\n", "");
    let errs = bad_errors(&format!("{ENTRY}{second}"));
    assert_eq!(errs, vec![GpuPolicyError::DuplicateId { id: "t-1".into() }]);
    let errs = bad_errors(&entry_with("id", "id = \"has space\""));
    assert!(errs[0].to_string().starts_with("BadField: entry 1: id is not an id"));
}

// ---------------------------------------------------------------- matching

#[test]
fn matching_is_inclusive_on_devices_and_half_open_on_drivers() {
    let k = parse_known_bad(&read(&gpu("known-bad-valid.toml"))).unwrap();
    let probe = |os, backend, vendor_id, device_id, driver: &str| Probe {
        os,
        backend,
        vendor_id,
        device_id,
        driver: v(driver),
    };
    let ids = |p: &Probe| k.matching(p).iter().map(|e| e.id.clone()).collect::<Vec<_>>();
    let w = |device, driver| probe(GpuOs::Windows, Backend::Vulkan, 0x7E57_0001, device, driver);

    // Device range 0x0100..=0x01FF, driver [1.2, 1.4).
    assert_eq!(ids(&w(0x0100, "1.2")), vec!["fixture-avoid-1"]);
    assert_eq!(ids(&w(0x01FF, "1.3.99")), vec!["fixture-avoid-1"]);
    assert_eq!(ids(&w(0x0100, "1.2.0.0")), vec!["fixture-avoid-1"]);
    assert!(ids(&w(0x00FF, "1.3")).is_empty());
    assert!(ids(&w(0x0200, "1.3")).is_empty());
    assert!(ids(&w(0x0150, "1.1.9")).is_empty());
    assert!(ids(&w(0x0150, "1.4")).is_empty());
    assert!(ids(&w(0x0150, "1.4.0")).is_empty());
    // Other OS, backend or vendor never match.
    assert!(ids(&probe(GpuOs::Linux, Backend::Vulkan, 0x7E57_0001, 0x0150, "1.3")).is_empty());
    assert!(ids(&probe(GpuOs::Windows, Backend::Dx12, 0x7E57_0001, 0x0150, "1.3")).is_empty());
    assert!(ids(&probe(GpuOs::Windows, Backend::Vulkan, 0x7E57_0002, 0x0150, "1.3")).is_empty());

    // Any device, driver below 2.0 (open lower bound).
    let l = |driver| probe(GpuOs::Linux, Backend::Vulkan, 0x7E57_0002, 0xFFFF_FFFF, driver);
    assert_eq!(ids(&l("0")), vec!["fixture-software-2"]);
    assert_eq!(ids(&l("1.99.99.99")), vec!["fixture-software-2"]);
    assert!(ids(&l("2")).is_empty());

    // Single device, any driver.
    let m = |device| probe(GpuOs::Macos, Backend::Metal, 0x7E57_0003, device, "4294967295.0.0.0");
    assert_eq!(ids(&m(0x0042)), vec!["fixture-workaround-3"]);
    assert!(ids(&m(0x0041)).is_empty());
    assert!(ids(&m(0x0043)).is_empty());
}

#[test]
fn matching_returns_every_match_in_file_order() {
    let second =
        ENTRY.replace("schema_version = 1\n", "").replace("t-1", "t-0").replace("avoid_backend", "force_software");
    let k = parse_known_bad(format!("{ENTRY}{second}").as_bytes()).unwrap();
    let p = Probe { os: GpuOs::Linux, backend: Backend::Vulkan, vendor_id: 0x7E57_0009, device_id: 7, driver: v("1") };
    let got: Vec<&str> = k.matching(&p).iter().map(|e| e.id.as_str()).collect();
    assert_eq!(got, vec!["t-1", "t-0"]);
}

// ---------------------------------------------------------------- determinism and CLI

#[test]
fn rendering_is_byte_identical() {
    let text = "schema_version = 1\n[[row]]\nos = \"windows\"\narch = \"x86_64\"\nbackends = [\"vulkan\", \"vulkan\"]\nzz = 1\naa = 2\n";
    let a = validate_policy(text.as_bytes()).render("policy", "rows");
    let b = validate_policy(text.as_bytes()).render("policy", "rows");
    assert_eq!(a, b);
    assert_eq!(
        a,
        "BadField: row 1: aa is not a known key\n\
         BadField: row 1: zz is not a known key\n\
         DuplicateBackend { backend: vulkan }: row 1\n\
         BadField: row 1: min.vulkan is missing\n\
         MissingMandatoryBackend { os: windows, backend: dx12 }: row 1 (windows x86_64)\n\
         NoSoftwareFallback: row 1 (windows x86_64) does not end with software\n\
         policy: 1 rows, 6 errors\n"
    );
}

#[test]
fn cli_bad_usage_and_unreadable_file_exit_2() {
    assert_eq!(cli(&["--policy"]).0, 2);
    assert_eq!(cli(&["--colour", "red"]).0, 2);
    assert_eq!(cli(&["--phase", "P9"]).0, 2);
    assert_eq!(cli(&["--policy", "a", "--policy", "b"]).0, 2);
    assert_eq!(cli(&["--policy", &gpu("no-such-file.toml")]).0, 2);
}

#[test]
fn cli_reports_an_invalid_matrix() {
    let (code, out) = cli(&["--matrix", "fixtures/plat/matrix/bad-phase.toml"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.starts_with("matrix: "), "{out}");
    assert!(out.contains("policy: 4 rows, 0 errors\n"), "{out}");
}

// ---------------------------------------------------------------- review decisions (PR #151)

/// `ENTRY` renamed to `id` with `action`, without the schema line.
fn entry_as(id: &str, action: &str) -> String {
    entry_with("action", &format!("action = \"{action}\"")).replace("schema_version = 1\n", "").replace("t-1", id)
}

#[test]
fn precedence_force_software_then_avoid_then_workaround_ties_in_file_order() {
    let p = Probe { os: GpuOs::Linux, backend: Backend::Vulkan, vendor_id: 0x7E57_0009, device_id: 7, driver: v("1") };
    let win = |entries: &[(&str, &str)]| {
        let mut text = String::from("schema_version = 1\n");
        for (id, action) in entries {
            text.push_str(&entry_as(id, action));
        }
        let k = parse_known_bad(text.as_bytes()).unwrap();
        k.winning(&p).map(|e| e.id.clone())
    };
    assert_eq!(win(&[("w", "workaround:a"), ("a", "avoid_backend"), ("f", "force_software")]), Some("f".into()));
    assert_eq!(win(&[("w", "workaround:a"), ("a", "avoid_backend")]), Some("a".into()));
    assert_eq!(win(&[("w1", "workaround:a"), ("w2", "workaround:b")]), Some("w1".into()));
    assert_eq!(win(&[("a1", "avoid_backend"), ("a2", "avoid_backend")]), Some("a1".into()));
    assert_eq!(win(&[("f1", "force_software"), ("w", "workaround:a"), ("f2", "force_software")]), Some("f1".into()));
    assert_eq!(win(&[]), None);
    let other = Probe { vendor_id: 1, ..p };
    let k = parse_known_bad(format!("schema_version = 1\n{}", entry_as("x", "force_software")).as_bytes()).unwrap();
    assert!(k.winning(&other).is_none());
    assert_eq!(BadAction::ForceSoftware.precedence(), 0);
    assert_eq!(BadAction::AvoidBackend.precedence(), 1);
    assert_eq!(BadAction::Workaround("a".into()).precedence(), 2);
}

#[test]
fn safe_default_policy_is_software_only() {
    assert_eq!(backends_or_software(None, GpuOs::Windows, Arch::X86_64), vec![Backend::Software]);
    let invalid = validate_policy(&read(&gpu("policy-win-no-vulkan.toml")));
    assert_eq!(backends_or_software(invalid.value.as_ref(), GpuOs::Linux, Arch::X86_64), vec![Backend::Software]);
    let p = parse_policy(&read(&gpu("policy-valid.toml"))).unwrap();
    assert_eq!(backends_or_software(Some(&p), GpuOs::Linux, Arch::X86_64), vec![Backend::Vulkan, Backend::Software]);
    // A valid policy without a row for the platform also falls back to software only.
    assert_eq!(backends_or_software(Some(&p), GpuOs::Macos, Arch::X86_64), vec![Backend::Software]);
}

#[test]
fn safe_default_invalid_known_bad_is_empty_with_warnings() {
    let (k, warnings) = known_bad_or_empty(validate_known_bad(&read(&gpu("known-bad-noref.toml"))));
    assert!(k.entries.is_empty());
    assert_eq!(warnings, vec![GpuPolicyError::MissingReference { id: "fixture-noref".into() }]);
    let (k, warnings) = known_bad_or_empty(validate_known_bad(&read(&gpu("known-bad-valid.toml"))));
    assert_eq!(k.entries.len(), 3);
    assert!(warnings.is_empty());
}
