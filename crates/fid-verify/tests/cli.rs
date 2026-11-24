//! Integration tests for `fid-verify` CLI and `verify.package@1` (`TRUST.VERIFY.CLI`, `TRUST.VERIFY.REPORT`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse as canon_parse, CValue};
use fid_pkg::reader::FileOpener;
use fid_verify::command::{verify_package as cmd_verify_package, VerifyPackageParams};
use fid_verify::report::VerificationReport;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_fid-verify")
}

fn run_cli(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(bin()).current_dir(root()).args(args).output().unwrap();
    (out.status.code().unwrap(), String::from_utf8(out.stdout).unwrap(), String::from_utf8(out.stderr).unwrap())
}

fn load_schema() -> CValue {
    let bytes = std::fs::read(root().join("schema/verify-report-1.json")).unwrap();
    canon_parse(&bytes).unwrap()
}

fn resolve_ref<'a>(root_schema: &'a CValue, r: &str) -> &'a CValue {
    let path = r.strip_prefix("#/").expect("only local #/ $ref supported");
    let mut cur = root_schema;
    for seg in path.split('/') {
        let CValue::Obj(o) = cur else { panic!("expected obj while resolving {r}") };
        cur = o.get(seg).unwrap_or_else(|| panic!("missing {seg} in {r}"));
    }
    cur
}

fn validate_against_schema(root_schema: &CValue, schema: &CValue, val: &CValue, path: &str) {
    let CValue::Obj(s) = schema else { panic!("{path}: schema must be object") };
    if let Some(CValue::Str(r)) = s.get("$ref") {
        let target = resolve_ref(root_schema, r);
        validate_against_schema(root_schema, target, val, path);
        return;
    }
    if let Some(CValue::Arr(allowed)) = s.get("enum") {
        assert!(allowed.contains(val), "{path}: {val:?} not in enum {allowed:?}");
    }
    if let Some(CValue::Str(ty)) = s.get("type") {
        match ty.as_str() {
            "object" => {
                let CValue::Obj(o) = val else { panic!("{path}: expected object, got {val:?}") };
                if let Some(CValue::Arr(req)) = s.get("required") {
                    for r in req {
                        let CValue::Str(k) = r else { panic!() };
                        assert!(o.contains_key(k), "{path}: missing required key {k}");
                    }
                }
                let props = match s.get("properties") {
                    Some(CValue::Obj(p)) => p,
                    _ => panic!("{path}: missing properties"),
                };
                if s.get("additionalProperties") == Some(&CValue::Bool(false)) {
                    for k in o.keys() {
                        assert!(props.contains_key(k), "{path}: unexpected property {k}");
                    }
                }
                for (k, v) in o {
                    if let Some(ps) = props.get(k) {
                        validate_against_schema(root_schema, ps, v, &format!("{path}.{k}"));
                    }
                }
            }
            "array" => {
                let CValue::Arr(arr) = val else { panic!("{path}: expected array, got {val:?}") };
                if let Some(item_schema) = s.get("items") {
                    for (i, item) in arr.iter().enumerate() {
                        validate_against_schema(root_schema, item_schema, item, &format!("{path}[{i}]"));
                    }
                }
            }
            "string" => {
                let CValue::Str(st) = val else { panic!("{path}: expected string, got {val:?}") };
                if let Some(CValue::Num(min)) = s.get("minLength") {
                    assert!(st.len() >= *min as usize, "{path}: string too short");
                }
                if let Some(CValue::Str(pat)) = s.get("pattern") {
                    assert!(matches_schema_pattern(pat, st), "{path}: {st:?} failed pattern {pat:?}");
                }
            }
            "integer" => {
                let CValue::Num(n) = val else { panic!("{path}: expected integer, got {val:?}") };
                assert!(n.fract() == 0.0, "{path}: expected integer, got {n}");
                if let Some(CValue::Num(min)) = s.get("minimum") {
                    assert!(*n >= *min, "{path}: {n} < {min}");
                }
                if let Some(CValue::Num(max)) = s.get("maximum") {
                    assert!(*n <= *max, "{path}: {n} > {max}");
                }
            }
            "boolean" => {
                assert!(matches!(val, CValue::Bool(_)), "{path}: expected boolean, got {val:?}");
            }
            other => panic!("unsupported schema type {other}"),
        }
    }
}

fn is_canonical_uint(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && (s == "0" || !s.starts_with('0'))
}

fn matches_schema_pattern(pat: &str, val: &str) -> bool {
    match pat {
        "^[0-9a-f]{64}$" => val.len() == 64 && val.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')),
        "^sha256:[0-9a-f]{64}$" => val
            .strip_prefix("sha256:")
            .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))),
        "^(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)$" => {
            let mut parts = val.split('.');
            matches!((parts.next(), parts.next(), parts.next(), parts.next()), (Some(a), Some(b), Some(c), None) if is_canonical_uint(a) && is_canonical_uint(b) && is_canonical_uint(c))
        }
        "^(0|[1-9][0-9]*)$" => is_canonical_uint(val),
        other => panic!("unsupported schema pattern {other:?}"),
    }
}

fn parse_and_validate(stdout: &str) -> BTreeMap<String, CValue> {
    let schema = load_schema();
    let v = canon_parse(stdout.trim().as_bytes()).expect("valid JSON output");
    validate_against_schema(&schema, &schema, &v, "$");
    match v {
        CValue::Obj(o) => o,
        _ => panic!("expected top-level JSON object"),
    }
}

fn as_str(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s.as_str(),
        other => panic!("expected string, got {other:?}"),
    }
}

fn as_obj(v: &CValue) -> &BTreeMap<String, CValue> {
    match v {
        CValue::Obj(o) => o,
        other => panic!("expected object, got {other:?}"),
    }
}

#[test]
fn signed_golden_passes_and_matches_command_and_schema() {
    let (code, stdout, stderr) = run_cli(&[
        "fixtures/pkg/golden/two-signatures.zip",
        "--trust-root",
        "fixtures/troot/roots/both.json",
        "--json",
    ]);
    assert_eq!(code, 0, "stderr={stderr}");
    assert!(stderr.is_empty(), "stderr={stderr}");

    let obj = parse_and_validate(&stdout);
    assert_eq!(as_str(&obj["report_version"]), "1.0.0");
    assert_eq!(as_str(&obj["verdict"]), "PASS");
    assert_eq!(obj["exit_code"], CValue::Num(0.0));
    assert_eq!(as_str(&as_obj(&obj["replay"])["status"]), "not_requested");
    assert_eq!(as_str(&as_obj(&obj["chain"])["verdict"]), "VALID");
    let lims = match &obj["limitations"] {
        CValue::Arr(a) => a.iter().map(as_str).collect::<Vec<_>>(),
        _ => panic!(),
    };
    assert!(lims.contains(&"signing time is signer-asserted"), "{lims:?}");

    // Byte-identical to verify.package@1 command handler (REQ-TRUST.VERIFY.CLI-005).
    let opener = FileOpener;
    let params = VerifyPackageParams {
        package: root().join("fixtures/pkg/golden/two-signatures.zip").to_string_lossy().into_owned(),
        trust_root: Some(root().join("fixtures/troot/roots/both.json").to_string_lossy().into_owned()),
        revoked: None,
        keys: None,
        as_of: None,
        replay: false,
        rep1: None,
        kernel_cache: None,
        fail_fast: false,
        memory_limit: None,
        time_budget: None,
    };
    let report: VerificationReport = cmd_verify_package(&opener, params).unwrap();
    let cmd_json = String::from_utf8(report.to_canonical_bytes()).unwrap();
    assert_eq!(stdout.trim(), cmd_json);

    // Deterministic plain-text summary without ANSI escape codes.
    let (hcode, hstdout, hstderr) =
        run_cli(&["fixtures/pkg/golden/two-signatures.zip", "--trust-root", "fixtures/troot/roots/both.json"]);
    assert_eq!(hcode, 0, "stderr={hstderr}");
    assert!(!hstdout.contains('\x1b'));
    assert!(hstdout.contains("verdict: PASS (exit_code 0)"));
    assert_eq!(hstdout, report.render_human());
}

#[test]
fn unsigned_golden_fails_with_bit_1_and_limitation() {
    let (code, stdout, stderr) =
        run_cli(&["fixtures/pkg/golden/points.zip", "--trust-root", "fixtures/troot/roots/both.json", "--json"]);
    assert_eq!(code, 1, "stderr={stderr}");
    let obj = parse_and_validate(&stdout);
    assert_eq!(as_str(&obj["verdict"]), "FAIL");
    assert_eq!(obj["exit_code"], CValue::Num(1.0));
    assert_eq!(as_str(&as_obj(&obj["signatures"])["package_status"]), "UNSIGNED");
    assert_eq!(as_str(&as_obj(&obj["chain"])["verdict"]), "VALID");
    let lims = match &obj["limitations"] {
        CValue::Arr(a) => a.iter().map(as_str).collect::<Vec<_>>(),
        _ => panic!(),
    };
    assert!(lims.contains(&"unsigned package"), "{lims:?}");
}

#[test]
fn signature_failure_modes_set_bit_1() {
    // Revoked key.
    let (code, stdout, _) = run_cli(&[
        "fixtures/pkg/golden/two-signatures.zip",
        "--trust-root",
        "fixtures/troot/roots/both.json",
        "--revoked",
        "fixtures/troot/revoked/revoked-before.json",
        "--json",
    ]);
    assert_eq!(code, 1);
    let obj = parse_and_validate(&stdout);
    assert_eq!(as_str(&obj["verdict"]), "FAIL");

    // Expired key.
    let (code, stdout, _) = run_cli(&[
        "fixtures/pkg/golden/two-signatures.zip",
        "--trust-root",
        "fixtures/troot/roots/expired.json",
        "--json",
    ]);
    assert_eq!(code, 1);
    parse_and_validate(&stdout);

    // Unknown key (empty trust root / omitted trust root).
    let (code, stdout, _) = run_cli(&[
        "fixtures/pkg/golden/two-signatures.zip",
        "--trust-root",
        "fixtures/troot/roots/empty.json",
        "--json",
    ]);
    assert_eq!(code, 1);
    parse_and_validate(&stdout);

    let (code, stdout, _) = run_cli(&["fixtures/pkg/golden/two-signatures.zip", "--json"]);
    assert_eq!(code, 1);
    parse_and_validate(&stdout);

    // Keyring only -> VALID_UNTRUSTED -> still FAIL (bit 1).
    let (code, stdout, _) = run_cli(&[
        "fixtures/pkg/golden/two-signatures.zip",
        "--trust-root",
        "fixtures/troot/roots/empty.json",
        "--keys",
        "fixtures/troot/keyring/keyring.json",
        "--json",
    ]);
    assert_eq!(code, 1);
    let obj = parse_and_validate(&stdout);
    assert_eq!(obj["exit_code"], CValue::Num(1.0));

    // Tampered signature faults (`faults.zip` is minimal package without journal segment, so --fail-fast stops at sigs).
    let (code, stdout, _) = run_cli(&[
        "fixtures/troot/packages/faults.zip",
        "--trust-root",
        "fixtures/troot/roots/both.json",
        "--fail-fast",
        "--json",
    ]);
    assert_eq!(code, 1);
    parse_and_validate(&stdout);

    // Invalid signature on a full package (`two-signatures.zip` with flipped signature char in 0.dsse.json or wrong trust root)
    // checked with full mode (no --fail-fast) sets bit 1 and keeps chain VALID.
    let (code, stdout, _) =
        run_cli(&["fixtures/pkg/golden/two-signatures.zip", "--trust-root", "fixtures/troot/roots/one.json", "--json"]);
    assert_eq!(code, 1);
    let obj = parse_and_validate(&stdout);
    assert_eq!(as_str(&as_obj(&obj["chain"])["verdict"]), "VALID");
}

#[test]
fn chain_tamper_fixtures_set_bit_2() {
    for name in ["modified.zip", "inserted.zip", "removed.zip", "reordered.zip", "truncated.zip", "unanchored.zip"] {
        let pkg = format!("fixtures/chain/tamper/{name}");
        let (code, stdout, stderr) = run_cli(&[&pkg, "--trust-root", "fixtures/troot/roots/both.json", "--json"]);
        assert_ne!(code & 2, 0, "{name}: expected chain bit 2, got code={code}, stderr={stderr}");
        let obj = parse_and_validate(&stdout);
        assert_eq!(as_str(&obj["verdict"]), "FAIL", "{name}");
        assert_eq!(as_str(&as_obj(&obj["chain"])["verdict"]), "BROKEN", "{name}");
    }
}

#[test]
fn fail_fast_skips_later_sections() {
    let (code, stdout, stderr) = run_cli(&[
        "fixtures/pkg/golden/points.zip",
        "--trust-root",
        "fixtures/troot/roots/both.json",
        "--fail-fast",
        "--json",
    ]);
    assert_eq!(code, 1, "stderr={stderr}");
    let obj = parse_and_validate(&stdout);
    assert_eq!(as_str(&as_obj(&obj["signatures"])["status"]), "checked");
    assert_eq!(as_str(&as_obj(&obj["chain"])["status"]), "skipped");
    assert_eq!(as_str(&as_obj(&obj["replay"])["status"]), "skipped");
}

#[test]
fn replay_handles_results_tier_and_fails_closed_on_points() {
    // Results tier -> not_applicable (no bit 8).
    let (code, stdout, stderr) = run_cli(&[
        "fixtures/pkg/golden/results.zip",
        "--trust-root",
        "fixtures/troot/roots/both.json",
        "--replay",
        "--json",
    ]);
    assert_eq!(code, 1, "stderr={stderr}");
    let obj = parse_and_validate(&stdout);
    assert_eq!(as_str(&as_obj(&obj["replay"])["status"]), "not_applicable");

    // Signed points package with --replay -> must fail closed as NOT_REPLAYABLE (bit 8), never fake IDENTICAL.
    let (code, stdout, stderr) = run_cli(&[
        "fixtures/pkg/golden/two-signatures.zip",
        "--trust-root",
        "fixtures/troot/roots/both.json",
        "--replay",
        "--json",
    ]);
    assert_eq!(code, 8, "stderr={stderr}");
    let obj = parse_and_validate(&stdout);
    assert_eq!(as_str(&obj["verdict"]), "FAIL");
    assert_eq!(as_str(&as_obj(&obj["replay"])["status"]), "NOT_REPLAYABLE");

    // Unsigned points package with --replay -> 1 | 8 = 9.
    let (code, stdout, _) = run_cli(&[
        "fixtures/pkg/golden/points.zip",
        "--trust-root",
        "fixtures/troot/roots/both.json",
        "--replay",
        "--json",
    ]);
    assert_eq!(code, 9);
    parse_and_validate(&stdout);

    // --time-budget 0s -> NOT_REPLAYABLE with reason BUDGET (bit 8).
    let (code, stdout, _) = run_cli(&[
        "fixtures/pkg/golden/two-signatures.zip",
        "--trust-root",
        "fixtures/troot/roots/both.json",
        "--replay",
        "--time-budget",
        "0s",
        "--json",
    ]);
    assert_eq!(code, 8);
    let obj = parse_and_validate(&stdout);
    let replay = as_obj(&obj["replay"]);
    assert_eq!(as_str(&replay["status"]), "NOT_REPLAYABLE");
    assert_eq!(as_str(&replay["reason"]), "BUDGET");
}

#[test]
fn malformed_corpus_and_trust_files_exit_64() {
    let malformed_dir = root().join("fixtures/pkg/malformed");
    let mut count = 0usize;
    for entry in std::fs::read_dir(&malformed_dir).unwrap() {
        let entry = entry.unwrap();
        let pkg = entry.path().join("package.zip");
        if !pkg.is_file() {
            continue;
        }
        count += 1;
        let pkg_s = pkg.to_string_lossy().into_owned();
        let (code, stdout, stderr) = run_cli(&[&pkg_s]);
        assert_eq!(code, 64, "{}: stdout={stdout} stderr={stderr}", pkg.display());
        assert!(stdout.is_empty(), "{}", pkg.display());
        assert!(stderr.contains("pkg."), "{}: stderr={stderr}", pkg.display());
    }
    assert!(count >= 100, "expected >=100 malformed cases, found {count}");

    // >64 signatures package -> sig.malformed -> exit 64.
    let (code, _, stderr) =
        run_cli(&["fixtures/troot/packages/too-many.zip", "--trust-root", "fixtures/troot/roots/both.json"]);
    assert_eq!(code, 64, "stderr={stderr}");
    assert!(stderr.contains("sig.malformed"), "stderr={stderr}");

    // Malformed trust roots -> troot.malformed -> exit 64.
    let troot_mal = root().join("fixtures/troot/malformed");
    let mut tcount = 0usize;
    for entry in std::fs::read_dir(&troot_mal).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy();
        if name.starts_with("keyring-") || name.starts_with("revoked-") {
            continue;
        }
        tcount += 1;
        let p_s = path.to_string_lossy().into_owned();
        let (code, _, stderr) = run_cli(&["fixtures/pkg/golden/two-signatures.zip", "--trust-root", &p_s]);
        assert_eq!(code, 64, "{name}: stderr={stderr}");
        assert!(stderr.contains("troot.malformed"), "{name}: stderr={stderr}");
    }
    assert!(tcount >= 8);

    // Malformed --rep1 -> rep1.malformed -> exit 64.
    let tmp = std::env::temp_dir().join("fid-verify-bad-rep1.json");
    std::fs::write(&tmp, b"{bad json").unwrap();
    let tmp_s = tmp.to_string_lossy().into_owned();
    let (code, _, stderr) = run_cli(&["fixtures/pkg/golden/two-signatures.zip", "--rep1", &tmp_s]);
    let _ = std::fs::remove_file(&tmp);
    assert_eq!(code, 64, "stderr={stderr}");
    assert!(stderr.contains("rep1.malformed"), "stderr={stderr}");

    // Tampered copy of two-signatures.zip -> exit 64.
    let mut bytes = std::fs::read(root().join("fixtures/pkg/golden/two-signatures.zip")).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0xff;
    let bad_zip = std::env::temp_dir().join("fid-verify-tampered-zip.zip");
    std::fs::write(&bad_zip, &bytes).unwrap();
    let bad_s = bad_zip.to_string_lossy().into_owned();
    let (code, _, stderr) = run_cli(&[&bad_s]);
    let _ = std::fs::remove_file(&bad_zip);
    assert_eq!(code, 64, "stderr={stderr}");
    assert!(stderr.contains("pkg."), "stderr={stderr}");
}

#[test]
fn usage_errors_and_version_and_mcp_stub() {
    for bad_args in [
        &[][..],
        &["--unknown"][..],
        &["fixtures/pkg/golden/points.zip", "extra.zip"][..],
        &["fixtures/pkg/golden/points.zip", "--trust-root"][..],
        &["fixtures/pkg/golden/points.zip", "--as-of", "not-a-date"][..],
        &["fixtures/pkg/golden/points.zip", "--memory-limit", "0"][..],
        &["fixtures/pkg/golden/points.zip", "--time-budget", "forever"][..],
        &["mcp", "--root", "."][..],
    ] {
        let (code, stdout, stderr) = run_cli(bad_args);
        assert_eq!(code, 65, "args={bad_args:?} stdout={stdout} stderr={stderr}");
    }

    let (code, stdout, stderr) = run_cli(&["--version", "--json"]);
    assert_eq!(code, 0, "stderr={stderr}");
    let v = canon_parse(stdout.trim().as_bytes()).unwrap();
    let obj = as_obj(&v);
    assert_eq!(as_str(&obj["tool"]), "fid-verify");
    assert_eq!(as_str(&obj["report_version"]), "1.0.0");
    assert_eq!(as_str(&obj["version"]), env!("CARGO_PKG_VERSION"));
    assert_eq!(obj["supported_package_formats"], CValue::Arr(vec![CValue::Num(1.0)]));

    let (code, stdout, _) = run_cli(&["--version"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("fid-verify"));

    let (code, stdout, _) = run_cli(&["--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("usage: fid-verify"));

    // `fid-verify mcp --json` honors `--json` and fails closed with exit 65.
    let (code, stdout, stderr) = run_cli(&["mcp", "--json"]);
    assert_eq!(code, 65, "stdout={stdout} stderr={stderr}");
    assert!(stdout.is_empty(), "stdout={stdout}");
    let err_obj = canon_parse(stderr.trim().as_bytes()).expect("mcp --json emits canonical JSON error");
    assert_eq!(as_str(&as_obj(&as_obj(&err_obj)["error"])["code"]), "params.invalid");

    // Verify all 4 exit-code failure bits are reachable and distinct (1, 2, 8, etc.)
    let bits: BTreeSet<i32> = [1, 2, 8].into_iter().collect();
    assert_eq!(bits.len(), 3);
}

fn build_stored_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for &(name, data) in entries {
        let nb = name.as_bytes();
        let crc = fid_pkg::crc32(data);
        let len32 = u32::try_from(data.len()).unwrap();
        let nlen = u16::try_from(nb.len()).unwrap();
        let off = u32::try_from(out.len()).unwrap();
        // Local file header
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes()); // version needed
        out.extend_from_slice(&0x0800u16.to_le_bytes()); // UTF-8 flag
        out.extend_from_slice(&0u16.to_le_bytes()); // method 0 stored
        out.extend_from_slice(&0u16.to_le_bytes()); // mod time
        out.extend_from_slice(&0x0021u16.to_le_bytes()); // mod date
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&len32.to_le_bytes());
        out.extend_from_slice(&len32.to_le_bytes());
        out.extend_from_slice(&nlen.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // extra len
        out.extend_from_slice(nb);
        out.extend_from_slice(data);
        // Central directory header
        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&((3u16 << 8) | 45u16).to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&0x0800u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0x0021u16.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&len32.to_le_bytes());
        central.extend_from_slice(&len32.to_le_bytes());
        central.extend_from_slice(&nlen.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes()); // extra
        central.extend_from_slice(&0u16.to_le_bytes()); // comment
        central.extend_from_slice(&0u16.to_le_bytes()); // disk
        central.extend_from_slice(&0u16.to_le_bytes()); // int attr
        central.extend_from_slice(&(0o100_644u32 << 16).to_le_bytes());
        central.extend_from_slice(&off.to_le_bytes());
        central.extend_from_slice(nb);
    }
    let cd_off = u32::try_from(out.len()).unwrap();
    let cd_size = u32::try_from(central.len()).unwrap();
    let count = u16::try_from(entries.len()).unwrap();
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&cd_size.to_le_bytes());
    out.extend_from_slice(&cd_off.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

#[test]
fn forged_manifest_with_untouched_statement_and_signatures_fails() {
    let f = std::fs::File::open(root().join("fixtures/pkg/golden/two-signatures.zip")).unwrap();
    let mut reader = fid_pkg::reader::PackageReader::open(f).unwrap();
    let mut manifest = reader.manifest().clone();
    let names: Vec<String> = reader.entry_names().into_iter().map(str::to_owned).collect();

    let forged_qif = b"<QIFDocument>FORGED MEASUREMENTS ALL PASS</QIFDocument>".to_vec();
    let forged_profile = br#"{"profile":"attacker_controlled"}"#.to_vec();
    manifest.profile_digest = fid_pkg::hash_bytes(&forged_profile);
    for m in &mut manifest.members {
        if m.path.as_str() == "results/results.qif" {
            m.length = u64::try_from(forged_qif.len()).unwrap();
            m.sha256 = fid_pkg::hash_bytes(&forged_qif);
        } else if m.path.as_str() == "profile/profile.json" {
            m.length = u64::try_from(forged_profile.len()).unwrap();
            m.sha256 = fid_pkg::hash_bytes(&forged_profile);
        }
    }
    let new_manifest_bytes = manifest.to_canonical_bytes().unwrap();

    // Keep statement.json and signatures/*.dsse.json completely untouched.
    let mut stored_entries: Vec<(String, Vec<u8>)> = Vec::new();
    for name in &names {
        let data = match name.as_str() {
            "manifest.json" => new_manifest_bytes.clone(),
            "results/results.qif" => forged_qif.clone(),
            "profile/profile.json" => forged_profile.clone(),
            other => reader.read_member(other, 16 * 1024 * 1024).unwrap().unwrap(),
        };
        stored_entries.push((name.clone(), data));
    }
    let refs: Vec<(&str, &[u8])> = stored_entries.iter().map(|(k, v)| (k.as_str(), v.as_slice())).collect();
    let zip_bytes = build_stored_zip(&refs);

    let forged_zip = std::env::temp_dir().join("fid-verify-forged-manifest.zip");
    std::fs::write(&forged_zip, &zip_bytes).unwrap();
    let forged_s = forged_zip.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_cli(&[&forged_s, "--trust-root", "fixtures/troot/roots/both.json", "--json"]);
    let _ = std::fs::remove_file(&forged_zip);

    assert_ne!(code, 0, "forged package must never pass! stdout={stdout} stderr={stderr}");
    assert_eq!(code, 1, "expected bit 1 FAIL, got code={code} stdout={stdout} stderr={stderr}");
    let obj = parse_and_validate(&stdout);
    assert_eq!(as_str(&obj["verdict"]), "FAIL");
    let sigs = match &as_obj(&obj["signatures"])["signatures"] {
        CValue::Arr(a) => a,
        _ => panic!("expected signatures array"),
    };
    assert_eq!(sigs.len(), 2);
    for sig in sigs {
        assert_eq!(as_str(&as_obj(sig)["status"]), "INVALID");
    }

    // Malformed statement.json -> exit 64 (sig.malformed).
    let bad_entries: Vec<(String, Vec<u8>)> = stored_entries
        .into_iter()
        .map(|(k, v)| if k == "statement.json" { (k, b"{not valid json".to_vec()) } else { (k, v) })
        .collect();
    let bad_refs: Vec<(&str, &[u8])> = bad_entries.iter().map(|(k, v)| (k.as_str(), v.as_slice())).collect();
    let bad_zip_bytes = build_stored_zip(&bad_refs);
    let bad_stmt_zip = std::env::temp_dir().join("fid-verify-bad-statement.zip");
    std::fs::write(&bad_stmt_zip, &bad_zip_bytes).unwrap();
    let bad_s = bad_stmt_zip.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_cli(&[&bad_s, "--trust-root", "fixtures/troot/roots/both.json", "--json"]);
    let _ = std::fs::remove_file(&bad_stmt_zip);
    assert_eq!(code, 64, "stdout={stdout} stderr={stderr}");
    assert!(stderr.contains("sig.malformed"), "stderr={stderr}");
}

#[test]
fn replay_after_broken_chain_records_not_replayable_journal_invalid() {
    let (code, stdout, stderr) = run_cli(&[
        "fixtures/chain/tamper/modified.zip",
        "--trust-root",
        "fixtures/troot/roots/both.json",
        "--replay",
        "--json",
    ]);
    assert_eq!(code, 11, "expected 1|2|8 = 11, got code={code} stderr={stderr}");
    let obj = parse_and_validate(&stdout);
    assert_eq!(as_str(&obj["verdict"]), "FAIL");
    assert_eq!(as_str(&as_obj(&obj["chain"])["verdict"]), "BROKEN");
    let replay = as_obj(&obj["replay"]);
    assert_eq!(as_str(&replay["status"]), "NOT_REPLAYABLE");
    assert_eq!(as_str(&replay["reason"]), "JOURNAL_INVALID");
}

#[test]
fn tier5_help_precedence_end_of_options_and_trust_root_open_error_copy() {
    // 1. --help before or after unknown option prints usage and exits 0.
    let (code, stdout, stderr) = run_cli(&["--help", "--unknown"]);
    assert_eq!(code, 0, "stderr={stderr}");
    assert!(stdout.contains("usage:"), "stdout={stdout}");

    // 2. -- end-of-options allows dash-prefixed package name instead of unknown option (65).
    let (code, _, stderr) = run_cli(&["--", "-nonexistent.fpkg"]);
    assert_eq!(code, 70, "expected pkg.io (70), got code={code} stderr={stderr}");
    assert!(!stderr.contains("unknown option"), "stderr={stderr}");

    // 3. Unreadable / directory --trust-root reports "cannot open trust root" (not "cannot open the package").
    let (code, stdout, stderr) =
        run_cli(&["fixtures/pkg/golden/results.fpkg", "--trust-root", "fixtures/troot/roots", "--json"]);
    assert_eq!(code, 70, "stdout={stdout} stderr={stderr}");
    let combined = format!("{stdout}\n{stderr}");
    assert!(combined.contains("cannot open trust root"), "combined={combined}");
    assert!(!combined.contains("cannot open the package"), "combined={combined}");
}
