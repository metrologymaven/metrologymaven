//! Acceptance criteria of spec node UX.TOKENS.CHECK (AC-1 .. AC-6): the `color-check` release
//! gate. Expected results are in `fixtures/ux/tokens/check/`, written by `make_fixtures.py`: the
//! whole release report of the shipped tree (`expected-report.json`) computed by an independent
//! Python reading of the files, and the hand-labelled findings of each fixture (`expected.json`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_design::status::ciede2000::delta_e2000;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fix(rel: &str) -> PathBuf {
    repo().join("fixtures/ux/tokens/check").join(rel)
}

fn design() -> PathBuf {
    repo().join("design")
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fid-design")).args(args).output().expect("run fid-design")
}

fn gate(root: &Path, extra: &[&str]) -> Output {
    let r = root.to_string_lossy().into_owned();
    let mut args = vec!["color-check", "--root", r.as_str()];
    args.extend_from_slice(extra);
    run(&args)
}

fn stdout(o: &Output) -> String {
    String::from_utf8(o.stdout.clone()).unwrap()
}

fn parse(bytes: &[u8]) -> CValue {
    fid_canon::parse(bytes).unwrap()
}

fn get<'a>(v: &'a CValue, path: &[&str]) -> &'a CValue {
    let mut cur = v;
    for k in path {
        match cur {
            CValue::Obj(m) => cur = m.get(*k).unwrap_or_else(|| panic!("missing key {k}")),
            _ => panic!("not an object at {k}"),
        }
    }
    cur
}

fn arr(v: &CValue) -> &[CValue] {
    match v {
        CValue::Arr(a) => a,
        other => panic!("not an array: {other:?}"),
    }
}

fn s(v: &CValue) -> &str {
    match v {
        CValue::Str(x) => x,
        other => panic!("not a string: {other:?}"),
    }
}

fn num(v: &CValue) -> f64 {
    match v {
        CValue::Num(n) => *n,
        other => panic!("not a number: {other:?}"),
    }
}

fn finding_keys(report: &CValue) -> Vec<String> {
    let keys: BTreeSet<String> = arr(get(report, &["findings"]))
        .iter()
        .map(|f| format!("{} {}", s(get(f, &["code"])), s(get(f, &["subject"]))))
        .collect();
    keys.into_iter().collect()
}

fn fixture_case(name: &str) -> String {
    let e = parse(&std::fs::read(fix("expected.json")).unwrap());
    let want = get(&e, &["fixtures", name]);
    let o = gate(&fix(name), &["--json"]);
    assert_eq!(o.status.code(), Some(num(get(want, &["exit"])) as i32), "{name}: {}", stdout(&o));
    let r = parse(stdout(&o).trim_end().as_bytes());
    let labels: Vec<String> = arr(get(want, &["findings"])).iter().map(|x| s(x).to_string()).collect();
    assert_eq!(finding_keys(&r), labels, "{name}");
    stdout(&gate(&fix(name), &[]))
}

/// AC-1 and REQ-006: the shipped package passes, and the report (every pair in all four themes,
/// the status distances, every stepped palette under four vision conditions, rounded to 0.01)
/// is byte-for-byte the independent oracle's report in canonical form. `--out` writes the same
/// bytes.
#[test]
fn ac1_shipped_package_passes_and_the_report_equals_the_oracle() {
    let dir = std::env::temp_dir().join(format!("fid-design-color-check-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join(fid_design::color::REPORT_FILE);
    let o = gate(&design(), &["--out", out.to_str().unwrap(), "--json"]);
    assert_eq!(o.status.code(), Some(0), "{}", stdout(&o));
    let want = fid_canon::to_canonical_bytes(&parse(&std::fs::read(fix("expected-report.json")).unwrap())).unwrap();
    assert_eq!(stdout(&o).trim_end().as_bytes(), want.as_slice());
    let written = std::fs::read(&out).unwrap();
    assert_eq!(written, o.stdout);
    let _ = std::fs::remove_dir_all(&dir);

    let r = parse(&want);
    let tokens = fid_design::tokens::check(&design()).unwrap();
    assert_eq!(arr(get(&r, &["pairs"])).len(), 4 * tokens.pairs.len());
    let themes: BTreeSet<&str> = arr(get(&r, &["pairs"])).iter().map(|p| s(get(p, &["theme"]))).collect();
    assert_eq!(themes.len(), 4);
    let pals = arr(get(&r, &["palettes"]));
    assert_eq!(pals.len(), 2);
    for p in pals {
        let n = arr(get(p, &["lstar"])).len();
        assert_eq!(arr(get(p, &["adjacent"])).len(), (n - 1) * 4, "{}", s(get(p, &["ref"])));
    }
    assert_eq!(arr(get(&r, &["status"])).len(), 4 * 2 * 4);
    assert_eq!(s(get(&r, &["cvd", "model"])), fid_design::color::CVD_MODEL);
    assert_eq!(s(get(&r, &["cvd", "table"])), fid_design::color::CVD_TABLE);
    let t = stdout(&gate(&design(), &[]));
    assert!(t.contains("ok: no findings"), "{t}");
}

#[test]
fn ac1_valid_fixture_passes() {
    fixture_case("ok");
}

#[test]
fn ac2_low_contrast() {
    let t = fixture_case("low-contrast");
    assert!(t.contains("CONTRAST light sem.text.muted/sem.surface.base 4.20"), "{t}");
}

#[test]
fn ac3_hc_below_7_to_1() {
    let t = fixture_case("hc-6-to-1");
    assert!(t.contains("CONTRAST-HC"), "{t}");
}

#[test]
fn ac4_deuteranopia_clash() {
    let t = fixture_case("deutan-clash");
    assert!(t.contains("STATUS-DISTANCE deuteranopia"), "{t}");
}

/// AC-5 and REQ-005: CIEDE2000 reproduces the published test data within 1e-4.
#[test]
fn ac5_sharma_test_data() {
    let text = std::fs::read_to_string(fix("ciede2000-sharma.csv")).unwrap();
    let mut n = 0;
    for line in text.lines().skip(1) {
        let v: Vec<f64> = line.split(',').map(|x| x.parse().unwrap()).collect();
        assert_eq!(v.len(), 7, "{line}");
        let (p, q) = ([v[0], v[1], v[2]], [v[3], v[4], v[5]]);
        assert!((delta_e2000(p, q) - v[6]).abs() < 1e-4, "{line}: {}", delta_e2000(p, q));
        assert!((delta_e2000(q, p) - v[6]).abs() < 1e-4, "{line}");
        n += 1;
    }
    assert_eq!(n, 34);
}

/// AC-6: same inputs, byte-identical reports (stdout and --out).
#[test]
fn ac6_reports_are_byte_identical() {
    let a = gate(&design(), &["--json"]);
    let b = gate(&design(), &["--json"]);
    assert_eq!(a.stdout, b.stdout);
    let canon = fid_canon::to_canonical_bytes(&parse(stdout(&a).trim_end().as_bytes())).unwrap();
    assert_eq!(stdout(&a).trim_end().as_bytes(), canon.as_slice());
    let c = gate(&fix("deutan-clash"), &["--json"]);
    let d = gate(&fix("deutan-clash"), &["--json"]);
    assert_eq!(c.stdout, d.stdout);
}

/// One gate: a finding of any sub-check fails color-check (here a DATAPAL error fixture).
#[test]
fn sub_check_findings_fail_the_gate() {
    let o = gate(&repo().join("fixtures/ux/tokens/datapal/delta"), &["--json"]);
    assert_eq!(o.status.code(), Some(1));
    let r = parse(stdout(&o).trim_end().as_bytes());
    assert!(finding_keys(&r).contains(&"PAL-DELTA pal.step.cvd@1 5 6".to_string()), "{:?}", finding_keys(&r));
    let gates: BTreeMap<String, f64> = match get(&r, &["gates"]) {
        CValue::Obj(m) => m.iter().map(|(k, v)| (k.clone(), num(v))).collect(),
        _ => panic!(),
    };
    assert!(gates["palettes-check"] >= 1.0);
    let o = gate(&repo().join("fixtures/ux/tokens/color/missing-dark"), &[]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stdout(&o).contains("TOK-UNRESOLVED dark sem.text.muted"), "{}", stdout(&o));
}

#[test]
fn bad_usage_and_unreadable_input_exit_2() {
    assert_eq!(run(&["color-check"]).status.code(), Some(2));
    assert_eq!(run(&["color-check", "--root"]).status.code(), Some(2));
    assert_eq!(run(&["color-check", "--root", "x", "--out"]).status.code(), Some(2));
    let d = design().to_string_lossy().into_owned();
    assert_eq!(run(&["tokens-check", "--root", &d, "--out", "x"]).status.code(), Some(2));
    assert_eq!(gate(&fix("no-such-dir"), &[]).status.code(), Some(2));
    let bad_out = fix("no-such-dir/sub/report.json");
    assert_eq!(gate(&design(), &["--out", bad_out.to_str().unwrap()]).status.code(), Some(2));
}

#[test]
fn every_fixture_is_labelled() {
    let e = parse(&std::fs::read(fix("expected.json")).unwrap());
    let CValue::Obj(m) = get(&e, &["fixtures"]) else { panic!() };
    assert_eq!(m.len(), 4);
    for n in m.keys() {
        assert!(fix(n).join("tokens").is_dir(), "{n}");
    }
}
