//! Acceptance criteria of spec node PLAT.HW.PROFILE (AC-1 .. AC-5) and the requirement
//! edge cases. Every expected value is hand-labelled from the spec's rules (the fixtures in
//! `fixtures/plat/hw/` state their expected outcome in their header comments); none is
//! produced by running this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_platform::hw_profile::{parse, validate, CpuLevel, GpuClass, HwError, MAX_BYTES};
use fid_platform::matrix::Arch;
use std::path::PathBuf;
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture(name: &str) -> PathBuf {
    root().join("fixtures/plat/hw").join(name)
}

/// Runs `fid-platform plat-hw validate [--file <path>]` from the repository root (what
/// `cargo xtask plat-hw validate` runs; the wrapper is exercised by the rust.yml step).
fn cli(file: Option<PathBuf>) -> (i32, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_fid-platform"));
    cmd.current_dir(root()).args(["plat-hw", "validate"]);
    if let Some(f) = file {
        cmd.arg("--file").arg(f);
    }
    let out = cmd.output().unwrap();
    (out.status.code().unwrap(), String::from_utf8(out.stdout).unwrap())
}

const MIN: &str = r#"
[profiles.minimum]
cores = 4
ram_gib = 16
gpu_class = "integrated"
vram_gib = 2
storage_free_gib = 20
display = [1920, 1080]
cpu_level = { x86_64 = "x86-64-v2", aarch64 = "armv8.0" }
"#;

fn rec(body: &str) -> String {
    format!("schema_version = 1\n{MIN}\n[profiles.recommended]\n{body}\n")
}

const REC_OK: &str = r#"cores = 8
ram_gib = 32
gpu_class = "discrete_mid"
vram_gib = 8
storage_free_gib = 100
display = [2560, 1440]
cpu_level = { x86_64 = "x86-64-v3", aarch64 = "armv8.2" }"#;

fn errs(text: &str) -> Vec<HwError> {
    validate(text.as_bytes()).unwrap_err()
}

fn below(field: &str) -> HwError {
    HwError::RecommendedBelowMinimum { field: field.into() }
}

#[test]
fn ac1_valid_fixture_prints_both_names() {
    let (code, out) = cli(Some(fixture("profiles-valid.toml")));
    assert_eq!(code, 0, "{out}");
    assert_eq!(out, "minimum\nrecommended\n");
    let p = parse(&std::fs::read(fixture("profiles-valid.toml")).unwrap()).unwrap();
    let min = p.minimum().unwrap();
    assert_eq!(min.cores, 4);
    assert_eq!(min.ram_gib, 16);
    assert_eq!(min.gpu_class, GpuClass::Integrated);
    assert_eq!(min.vram_gib, 2);
    assert_eq!(min.storage_free_gib, 20);
    assert_eq!(min.display, (1920, 1080));
    assert_eq!(min.cpu_level.get(&Arch::X86_64), Some(&CpuLevel::X86_64V2));
    assert_eq!(min.cpu_level.get(&Arch::Aarch64), Some(&CpuLevel::Armv8_0));
    let r = p.recommended().unwrap();
    assert_eq!(r.gpu_class, GpuClass::DiscreteMid);
    assert_eq!(r.display, (2560, 1440));
    assert_eq!(r.cpu_level.get(&Arch::Aarch64), Some(&CpuLevel::Armv8_2));
}

#[test]
fn ac2_inverted_ram_names_the_field() {
    let (code, out) = cli(Some(fixture("profiles-inverted.toml")));
    assert_eq!(code, 1, "{out}");
    assert_eq!(out, "RecommendedBelowMinimum { field: ram_gib }\n");
}

#[test]
fn ac3_no_minimum() {
    let (code, out) = cli(Some(fixture("profiles-no-minimum.toml")));
    assert_eq!(code, 1, "{out}");
    assert_eq!(out, "MissingProfile { name: minimum }\n");
}

#[test]
fn ac4_extra_field() {
    let (code, out) = cli(Some(fixture("profiles-extra-field.toml")));
    assert_eq!(code, 1, "{out}");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "UnknownField { field: ram_gb } in profiles.minimum");
    assert_eq!(lines[1], "Parse: profiles.minimum.ram_gib is missing");
    assert_eq!(lines.len(), 2);
    let e = errs(&std::fs::read_to_string(fixture("profiles-extra-field.toml")).unwrap());
    assert_eq!(e[0], HwError::UnknownField { field: "ram_gb".into(), at: "profiles.minimum".into() });
}

#[test]
fn ac5_shipped_file_validates_with_default_path() {
    let (code, out) = cli(None);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out, "minimum\nrecommended\n");
    // The shipped values are the spec's initial proposal (PLAT.HW.PROFILE Design).
    let p = parse(&std::fs::read(root().join("platform/hw-profiles.toml")).unwrap()).unwrap();
    let (min, r) = (p.minimum().unwrap(), p.recommended().unwrap());
    assert_eq!((min.cores, min.ram_gib, min.vram_gib, min.storage_free_gib), (4, 16, 2, 20));
    assert_eq!((r.cores, r.ram_gib, r.vram_gib, r.storage_free_gib), (8, 32, 8, 100));
    assert_eq!((min.gpu_class, r.gpu_class), (GpuClass::Integrated, GpuClass::DiscreteMid));
    assert_eq!((min.display, r.display), ((1920, 1080), (2560, 1440)));
}

#[test]
fn req002_extra_named_profiles_allowed() {
    let text = format!("{}\n[profiles.lab]\n{REC_OK}\n", rec(REC_OK));
    let p = validate(text.as_bytes()).unwrap();
    assert_eq!(p.profiles.keys().map(String::as_str).collect::<Vec<_>>(), ["lab", "minimum", "recommended"]);
}

#[test]
fn req002_duplicate_profile_is_a_parse_error() {
    let text = format!("{}\n[profiles.minimum]\ncores = 4\n", rec(REC_OK));
    assert!(matches!(errs(&text)[..], [HwError::Parse { .. }]));
}

#[test]
fn req002_both_missing_in_order() {
    let e = errs("schema_version = 1\n[profiles]\n");
    assert_eq!(e, [HwError::MissingProfile { name: "minimum" }, HwError::MissingProfile { name: "recommended" }]);
}

#[test]
fn req003_every_field_below_in_schema_order() {
    let body = r#"cores = 2
ram_gib = 8
gpu_class = "software"
vram_gib = 1
storage_free_gib = 10
display = [1280, 1080]
cpu_level = { x86_64 = "x86-64-v2" }"#;
    let e = errs(&rec(body));
    // aarch64 is listed by minimum but not by recommended: counts as below.
    let want: Vec<HwError> =
        ["cpu_level.aarch64", "cores", "ram_gib", "gpu_class", "vram_gib", "storage_free_gib", "display"]
            .into_iter()
            .map(below)
            .collect();
    assert_eq!(e, want);
}

#[test]
fn req003_equal_is_allowed_and_each_axis_checked() {
    // Recommended equal to minimum on every field is valid.
    validate(rec(&MIN.replace("[profiles.minimum]", "")).as_bytes()).unwrap();
    // A lower GPU class that still ranks at least the minimum is valid.
    validate(rec(&REC_OK.replace("discrete_mid", "discrete_entry")).as_bytes()).unwrap();
    // Height alone lower fails on display.
    let e = errs(&rec(&REC_OK.replace("[2560, 1440]", "[2560, 1000]")));
    assert_eq!(e, [below("display")]);
    // Minimum x86-64-v3 with recommended x86-64-v2 fails on cpu_level.x86_64 only.
    let text = rec(&REC_OK.replace("x86-64-v3", "x86-64-v2")).replacen("x86-64-v2", "x86-64-v3", 1);
    assert_eq!(errs(&text), [below("cpu_level.x86_64")]);
    // An architecture only recommended lists is fine.
    validate(rec(REC_OK).replacen(", aarch64 = \"armv8.0\"", "", 1).as_bytes()).unwrap();
}

#[test]
fn req004_gpu_class_rank_and_names() {
    assert!(GpuClass::Software < GpuClass::Integrated);
    assert!(GpuClass::Integrated < GpuClass::DiscreteEntry);
    assert!(GpuClass::DiscreteEntry < GpuClass::DiscreteMid);
    let text = rec(REC_OK).replacen("gpu_class = \"integrated\"", "gpu_class = \"software\"", 1);
    validate(text.as_bytes()).unwrap();
    let e = errs(&rec(&REC_OK.replace("discrete_mid", "discrete_high")));
    assert_eq!(
        e,
        [HwError::Parse {
            line: None,
            msg: "profiles.recommended.gpu_class is not one of software, integrated, discrete_entry, discrete_mid"
                .into()
        }]
    );
}

#[test]
fn req001_cpu_level_must_match_its_arch() {
    assert_eq!(CpuLevel::from_name(Arch::X86_64, "armv8.0"), None);
    assert_eq!(CpuLevel::from_name(Arch::Aarch64, "armv8.2"), Some(CpuLevel::Armv8_2));
    let e = errs(&rec(&REC_OK.replace("x86_64 = \"x86-64-v3\"", "x86_64 = \"armv8.2\"")));
    assert_eq!(
        e,
        [HwError::Parse {
            line: None,
            msg: "profiles.recommended.cpu_level.x86_64 is not one of x86-64-v2, x86-64-v3".into()
        }]
    );
    let e = errs(&rec(&REC_OK.replace("aarch64 =", "riscv64 =")));
    assert_eq!(e, [HwError::UnknownField { field: "riscv64".into(), at: "profiles.recommended.cpu_level".into() }]);
}

#[test]
fn req001_bad_values() {
    let e = errs(&rec(&REC_OK.replace("cores = 8", "cores = 0")));
    assert_eq!(
        e,
        [HwError::Parse {
            line: None,
            msg: "profiles.recommended.cores is not an integer from 1 to 4294967295".into()
        }]
    );
    let e = errs(&rec(&REC_OK.replace("ram_gib = 32", "ram_gib = -1")));
    assert_eq!(e.len(), 1);
    let e = errs(&rec(&REC_OK.replace("ram_gib = 32", "ram_gib = 32.0")));
    assert_eq!(e.len(), 1);
    for d in ["[2560]", "[2560, 1440, 1]", "[2560, 0]", "\"2560x1440\""] {
        let e = errs(&rec(&REC_OK.replace("[2560, 1440]", d)));
        assert_eq!(
            e,
            [HwError::Parse {
                line: None,
                msg: "profiles.recommended.display is not [width, height] in pixels, each at least 1".into()
            }],
            "{d}"
        );
    }
}

#[test]
fn req005_unknown_top_level_key() {
    let text = rec(REC_OK).replacen("schema_version = 1\n", "schema_version = 1\nnotes = \"x\"\n", 1);
    assert_eq!(errs(&text), [HwError::UnknownField { field: "notes".into(), at: String::new() }]);
}

#[test]
fn req005_size_cap_is_64_kib() {
    let base = rec(REC_OK);
    // Pad with a comment so the file is exactly MAX_BYTES: accepted.
    let pad = MAX_BYTES - base.len() - 2;
    let exact = format!("{base}#{}\n", "x".repeat(pad));
    assert_eq!(exact.len(), MAX_BYTES);
    validate(exact.as_bytes()).unwrap();
    let over = format!("{base}#{}\n", "x".repeat(pad + 1));
    assert_eq!(errs(&over), [HwError::TooLarge { found: MAX_BYTES + 1, max: MAX_BYTES }]);
}

#[test]
fn schema_version_rules() {
    let text = rec(REC_OK).replacen("schema_version = 1", "schema_version = 2", 1);
    assert_eq!(errs(&text), [HwError::UnsupportedSchema { found: 2 }]);
    let text = rec(REC_OK).replacen("schema_version = 1\n", "", 1);
    assert_eq!(errs(&text), [HwError::Parse { line: None, msg: "schema_version is missing".into() }]);
}

#[test]
fn not_toml_or_not_utf8() {
    assert!(matches!(errs("schema_version = = 1")[..], [HwError::Parse { line: Some(1), .. }]));
    assert!(matches!(validate(&[0xff, 0xfe]).unwrap_err()[..], [HwError::Parse { line: None, .. }]));
}

#[test]
fn cli_usage_and_unreadable_file_exit_2() {
    let out = Command::new(env!("CARGO_BIN_EXE_fid-platform")).args(["plat-hw", "check"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let (code, _) = cli(Some(fixture("no-such-file.toml")));
    assert_eq!(code, 2);
}
