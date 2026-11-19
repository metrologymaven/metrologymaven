//! Acceptance criteria of spec node KRN.NUM.LINT.FLAGS (AC-1 .. AC-5) and its error paths.
//! Fixtures and expected results under `fixtures/knp1/flags/` are written by the independent
//! Python reference `make_fixtures.py` and hand-labelled from the spec's rules (see its README),
//! not produced by this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_knp1_lint::find::{apply_waivers, lines_digest, ErrorCode, Finding, Rule, Waiver};
use fid_knp1_lint::flags::wasm::SimdPolicy;
use fid_knp1_lint::flags::{
    check_wasm, codegen_options, parse_config, run, tokens, FlagsErrorCode, BUILD_CONFIG_CRATE, BUILD_CONFIG_VERSION,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn fix(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/knp1/flags").join(rel)
}

fn read(rel: &str) -> Vec<u8> {
    std::fs::read(fix(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

/// One line per finding: `RULE file:line:column`.
fn lines(findings: &[Finding]) -> Vec<String> {
    findings.iter().map(|f| format!("{} {}:{}:{}", f.rule, f.file, f.line, f.column)).collect()
}

fn expected(rel: &str) -> Vec<String> {
    String::from_utf8(read(rel)).unwrap().lines().filter(|l| !l.is_empty()).map(str::to_string).collect()
}

fn run_dir(dir: &str) -> Vec<Finding> {
    let f = run(&fix(&format!("{dir}/knp1-lint.toml"))).unwrap_or_else(|e| panic!("{dir}: {e}"));
    for x in &f {
        x.validate().unwrap();
        assert_eq!(x.crate_name, BUILD_CONFIG_CRATE);
        assert_eq!(x.crate_version, BUILD_CONFIG_VERSION);
        assert!(x.excerpt_digest.starts_with("sha256:"));
    }
    f
}

fn assert_dir(dir: &str) -> Vec<Finding> {
    let f = run_dir(dir);
    assert_eq!(lines(&f), expected(&format!("{dir}/expected-findings.txt")), "{dir}");
    f
}

#[test]
fn ac1_native_names_file_and_line() {
    let f = assert_dir("native");
    assert_eq!(f.len(), 1);
    assert_eq!(f[0].rule, Rule::Flag("NATIVE".into()));
    assert_eq!(f[0].file, ".cargo/config.toml");
    assert_eq!(f[0].line, 2);
    assert!(f[0].message.contains("target-cpu=native"));
}

#[test]
fn ac2_fma_target_features() {
    let f = assert_dir("fma");
    assert!(f.iter().all(|x| x.rule.id() == "KNP1-FLAG-FMA"));
    assert!(f[0].message.contains("+fma"));
    assert!(f[1].message.contains("+relaxed-simd"));
    // `-fma` disables a feature; only the configured `+fma4` is reported.
    assert!(f[2].message.ends_with("(+fma4)"), "{}", f[2].message);
}

#[test]
fn ac3_codegen_units_including_defaults() {
    let f = assert_dir("cgu");
    let release = f.iter().find(|x| x.file == "Cargo.toml").unwrap();
    assert!(release.message.contains("`release` defaults to codegen-units = 16"), "{}", release.message);
}

#[test]
fn ac3_toolchain_channel_stable() {
    let f = assert_dir("toolchain");
    assert!(f[0].message.contains("\"stable\""));
}

#[test]
fn req004_toolchain_missing_and_unpinned_components() {
    assert_dir("toolchain-missing");
    assert_dir("toolchain-nocomp");
}

#[test]
fn lto_profile_difference_and_allowance() {
    assert_dir("lto");
    assert!(run(&fix("lto/knp1-lint-allowed.toml")).unwrap().is_empty());
}

#[test]
fn ci_env_rustflags_and_encoded_rustflags() {
    assert_dir("ci");
}

#[test]
fn ac4_wasm_scan_expectations() {
    let only12 = SimdPolicy::Only(vec![12]);
    let mut got = Vec::new();
    for (file, policy_name, policy) in [
        ("relaxed.wasm", "default", &SimdPolicy::Default),
        ("swizzle.wasm", "default", &SimdPolicy::Default),
        ("clean.wasm", "only-12", &only12),
        ("relaxed.wasm", "only-12", &only12),
    ] {
        for x in check_wasm(&read(&format!("wasm/{file}")), file, policy) {
            x.validate().unwrap();
            got.push(format!("{file} {policy_name} {} {}:{}", x.rule, x.line, x.column));
        }
    }
    assert_eq!(got, expected("wasm/expected-scan.txt"));
}

#[test]
fn ac4_clean_module_has_no_findings() {
    assert!(check_wasm(&read("wasm/clean.wasm"), "clean.wasm", &SimdPolicy::Default).is_empty());
}

#[test]
fn ac4_truncated_module_fails_closed() {
    let f = check_wasm(&read("wasm/truncated.wasm"), "truncated.wasm", &SimdPolicy::Default);
    assert_eq!(f.len(), 1);
    assert_eq!(f[0].rule.id(), "KNP1-FLAG-WASMPARSE");
    f[0].validate().unwrap();
}

#[test]
fn ac4_wasm_through_config_and_missing_module() {
    assert_dir("wasm");
    let f = run(&fix("wasm/knp1-lint-missing.toml")).unwrap();
    assert_eq!(lines(&f), ["KNP1-FLAG-WASMPARSE absent.wasm:1:1"]);
}

#[test]
fn wasm_parse_errors_on_malformed_modules() {
    let parse = |b: &[u8]| {
        let f = check_wasm(b, "m.wasm", &SimdPolicy::Default);
        f.len() == 1 && f[0].rule.id() == "KNP1-FLAG-WASMPARSE"
    };
    assert!(parse(b""));
    assert!(parse(b"\0asm"));
    assert!(parse(b"\0asm\x02\0\0\0"));
    assert!(parse(b"XXXX\x01\0\0\0"));
    // Unknown section id 13.
    assert!(parse(b"\0asm\x01\0\0\0\x0d\x00"));
    // Code section with one body holding the unsupported opcode 0x06 (exception `try`).
    assert!(parse(b"\0asm\x01\0\0\0\x0a\x05\x01\x03\x00\x06\x0b"));
    // Body that continues after its final `end`.
    assert!(parse(b"\0asm\x01\0\0\0\x0a\x05\x01\x03\x00\x0b\x01"));
    // Body without a final `end`.
    assert!(parse(b"\0asm\x01\0\0\0\x0a\x04\x01\x02\x00\x01"));
    // Over-long u32 LEB in a section size.
    assert!(parse(b"\0asm\x01\0\0\0\x00\x80\x80\x80\x80\x80\x00"));
    // A bare module header is valid and clean.
    assert!(check_wasm(b"\0asm\x01\0\0\0", "m.wasm", &SimdPolicy::Default).is_empty());
}

#[test]
fn ac5_ok_has_no_findings() {
    assert!(assert_dir("ok").is_empty());
}

#[test]
fn deterministic_runs() {
    for d in ["native", "fma", "cgu", "ci", "wasm", "ok"] {
        assert_eq!(run_dir(d), run_dir(d), "{d}");
    }
}

#[test]
fn rustflags_token_forms() {
    let t = tokens("-C target-cpu=native -Ctarget-feature=+fma --codegen codegen-units=4 --codegen=opt-level=3 -C");
    let o: Vec<String> = codegen_options(&t).iter().map(|c| c.needle.clone()).collect();
    assert_eq!(o, ["target-cpu=native", "target-feature=+fma", "codegen-units=4", "opt-level=3"]);
    let enc = tokens("-C\u{1f}target-cpu=native\u{1f}-C\\x1fcodegen-units=1");
    let o: Vec<String> = codegen_options(&enc).iter().map(|c| c.needle.clone()).collect();
    assert_eq!(o, ["target-cpu=native", "codegen-units=1"]);
}

#[test]
fn config_errors() {
    let dir = Path::new("/ws");
    let bad = |t: &str| parse_config(t, dir).unwrap_err().code;
    assert_eq!(bad(""), FlagsErrorCode::ConfigInvalid);
    assert_eq!(bad("[flags]\nbogus = 1\n"), FlagsErrorCode::ConfigInvalid);
    assert_eq!(bad("[flags]\nprofiles = []\n"), FlagsErrorCode::ConfigInvalid);
    assert_eq!(bad("[flags]\nallowed_simd = [-1]\n"), FlagsErrorCode::ConfigInvalid);
    assert_eq!(bad("[flags]\nallowed_profile_differences = [\"codegen-units\"]\n"), FlagsErrorCode::ConfigInvalid);
    assert_eq!(bad("[flags]\nwasm_module = \"../outside.wasm\"\n"), FlagsErrorCode::ConfigInvalid);
    let c = parse_config("[flags]\nfma_features = [\"+fma4\", \"avx512f\"]\n", dir).unwrap();
    assert_eq!(c.profiles, ["release"]);
    assert_eq!(c.fma_features, ["avx512f", "fma4"]);
    assert_eq!(c.simd_policy, SimdPolicy::Default);
    // A config without its Cargo.toml is a fatal error, not a finding.
    let missing = run(&fix("does-not-exist/knp1-lint.toml")).unwrap_err();
    assert_eq!(missing.code, FlagsErrorCode::ConfigInvalid);
}

#[test]
fn ci_array_commas_and_env_prefixes_do_not_hide_flags() {
    let t = tokens(r#"  KERNEL_FLAGS: ["-C", "target-cpu=native"]"#);
    assert!(!t.iter().any(|x| x.is_empty() || x == ","), "{t:?}");
    let needles = |s: &str| -> Vec<String> { codegen_options(&tokens(s)).into_iter().map(|c| c.needle).collect() };
    assert_eq!(needles(r#"["-C", "target-cpu=native"]"#), ["target-cpu=native"]);
    assert_eq!(needles("RUSTFLAGS=-Ctarget-cpu=native cargo build"), ["target-cpu=native"]);
    assert_eq!(needles("export RUSTFLAGS=-C target-cpu=native"), ["target-cpu=native"]);
    assert_eq!(needles(r#"export RUSTFLAGS="-C target-cpu=native""#), ["target-cpu=native"]);
    // A codegen value that looks like an assignment is not split.
    assert_eq!(needles("-C lto=fat -C panic=abort"), ["lto=fat", "panic=abort"]);
    // A bare LLVM argument token is kept as llvm-args.
    assert_eq!(needles("-C llvm-args=-x -fp-contract=fast"), ["llvm-args=-x", "-fp-contract=fast"]);
}

#[test]
fn target_cpu_outside_allowlist_is_flagged() {
    let f = assert_dir("cpu");
    assert_eq!(f[0].rule.id(), "KNP1-FLAG-CPU");
    assert!(f[0].message.contains("haswell"));
}

#[test]
fn cargo_toml_profile_rustflags_and_inherited_package_overrides() {
    let f = assert_dir("profile-flags");
    assert!(f[1].message.contains("inherited from `release`"), "{}", f[1].message);
}

#[test]
fn legacy_rust_toolchain_file_is_reported() {
    assert_dir("toolchain-legacy");
}

/// A module with one function body: no locals, `0xfd <op as two-byte LEB>`, `end`.
fn simd_module(op: u32) -> Vec<u8> {
    let body = [0x00, 0xfd, u8::try_from((op & 0x7f) | 0x80).unwrap(), u8::try_from(op >> 7).unwrap(), 0x0b];
    let mut m = b"\0asm\x01\0\0\0\x0a".to_vec();
    m.push(u8::try_from(body.len() + 2).unwrap());
    m.extend([0x01, u8::try_from(body.len()).unwrap()]);
    m.extend(body);
    m
}

#[test]
fn unassigned_simd_opcodes_fail_closed() {
    let unassigned = [
        0x9a_u32, 0xa2, 0xa5, 0xa6, 0xaf, 0xb0, 0xb2, 0xb3, 0xb4, 0xbb, 0xc2, 0xc5, 0xc6, 0xcf, 0xd0, 0xd2, 0xd3, 0xd4,
        0xe2, 0xee,
    ];
    for op in unassigned {
        let f = check_wasm(&simd_module(op), "m.wasm", &SimdPolicy::Default);
        assert_eq!(f.len(), 1, "{op:#x}");
        assert_eq!(f[0].rule.id(), "KNP1-FLAG-WASMPARSE", "{op:#x}");
    }
    // Assigned neighbours (f64x2.add 0xf0, i64x2.mul 0xd5) decode cleanly.
    for op in [0xf0_u32, 0xd5] {
        assert!(check_wasm(&simd_module(op), "m.wasm", &SimdPolicy::Default).is_empty(), "{op:#x}");
    }
}

#[test]
fn wasm_findings_are_unwaivable() {
    let file = "relaxed.wasm";
    let bytes = read("wasm/relaxed.wasm");
    let findings = check_wasm(&bytes, file, &SimdPolicy::Default);
    assert_eq!(findings.len(), 1);
    let mut src: BTreeMap<(String, String, String), Vec<u8>> = BTreeMap::new();
    src.insert((BUILD_CONFIG_CRATE.into(), BUILD_CONFIG_VERSION.into(), file.into()), bytes.clone());
    let waiver = Waiver {
        crate_name: BUILD_CONFIG_CRATE.into(),
        file: file.into(),
        lines: (1, 1),
        rule: Rule::Flag("WASMFMA".into()),
        justification: "try to hide it".into(),
        reviewer: "someone".into(),
        date: "2026-11-18".into(),
        lines_sha256: lines_digest(&bytes, 1, 1).unwrap(),
    };
    assert!(!waiver.rule.is_waivable());
    let errs = apply_waivers(findings.clone(), std::slice::from_ref(&waiver), &src).unwrap_err();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, ErrorCode::WaiverSchema);
    // With no waiver the finding stays unwaived.
    let applied = apply_waivers(findings, &[], &src).unwrap();
    assert_eq!(applied.findings.len(), 1);
    // The same waiver shape on a waivable flag rule does waive (control).
    let native = run_dir("native");
    let cfg = read("native/.cargo/config.toml");
    let mut src2: BTreeMap<(String, String, String), Vec<u8>> = BTreeMap::new();
    src2.insert((BUILD_CONFIG_CRATE.into(), BUILD_CONFIG_VERSION.into(), ".cargo/config.toml".into()), cfg.clone());
    let w2 = Waiver {
        file: ".cargo/config.toml".into(),
        lines: (2, 2),
        rule: Rule::Flag("NATIVE".into()),
        lines_sha256: lines_digest(&cfg, 2, 2).unwrap(),
        ..waiver
    };
    let applied = apply_waivers(native, &[w2], &src2).unwrap();
    assert!(applied.findings.is_empty());
    assert_eq!(applied.waived.len(), 1);
}

#[test]
fn allowed_target_cpus_rejects_native() {
    let dir = Path::new("/ws");
    let e = parse_config("[flags]\nallowed_target_cpus = [\"native\"]\n", dir).unwrap_err();
    assert_eq!(e.code, FlagsErrorCode::ConfigInvalid);
    let c = parse_config("[flags]\nallowed_target_cpus = [\"x86-64\", \"generic\"]\n", dir).unwrap();
    assert_eq!(c.allowed_target_cpus, ["generic", "x86-64"]);
}
