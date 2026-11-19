//! Acceptance criteria of spec node OUTR.LAUNCH.GATE (AC-1 .. AC-7), fail-closed and escape
//! paths. Expected states and reasons are hand-labelled from the spec's rules against the
//! fixtures under `fixtures/outr/launch/gate/` (see its LICENSE.txt), not produced by this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_outreach::launch_gate::{md_escape, meets_pct, parse_frontmatter};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const RELEASE: &str = "v0.5.0-beta.1";

fn fix(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/outr/launch/gate").join(rel)
}

/// `name/file` if the fixture has it, else `ready/file`.
fn pick(name: &str, file: &str) -> String {
    let own = fix(name).join(file);
    let p = if own.exists() { own } else { fix("ready").join(file) };
    p.to_string_lossy().into_owned()
}

/// A fresh, not yet existing output directory.
fn out_dir(tag: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("gate-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn run_with(name: &str, milestone: &str, release: &str, out: &Path, json: bool) -> Output {
    let mut args: Vec<String> = vec![
        "gate-check".into(),
        "--milestone".into(),
        milestone.into(),
        "--release".into(),
        release.into(),
        "--author".into(),
        "bob".into(),
    ];
    for (flag, file) in [
        ("--criteria", "criteria.toml"),
        ("--tree", "spec"),
        ("--trace", "trace.json"),
        ("--status", "status.json"),
        ("--tests", "tests.json"),
        ("--signoffs", "signoffs"),
        ("--roster", "roster.toml"),
    ] {
        args.push(flag.into());
        args.push(pick(name, file));
    }
    args.push("--out".into());
    args.push(out.to_string_lossy().into_owned());
    if json {
        args.push("--json".into());
    }
    Command::new(env!("CARGO_BIN_EXE_fid-outreach")).args(&args).output().unwrap()
}

fn run(name: &str, tag: &str) -> (Output, PathBuf) {
    let out = out_dir(tag);
    let o = run_with(name, "inspect-beta", RELEASE, &out, true);
    (o, out)
}

fn obj(v: &CValue) -> &BTreeMap<String, CValue> {
    match v {
        CValue::Obj(o) => o,
        other => panic!("not an object: {other:?}"),
    }
}

fn s<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> &'a str {
    match o.get(k) {
        Some(CValue::Str(s)) => s,
        other => panic!("{k} is {other:?}"),
    }
}

/// The written JSON report; it must equal the `--json` stdout.
fn report(o: &Output, out: &Path, milestone: &str) -> CValue {
    let file = std::fs::read(out.join(format!("gate-{milestone}-{RELEASE}.json"))).unwrap();
    assert_eq!(o.stdout.strip_suffix(b"\n").unwrap(), &file[..]);
    fid_canon::parse(&file).unwrap()
}

/// (state, [(reason, subject)]) of one criterion.
type St = (String, Vec<(String, String)>);

/// criterion id -> its state and problems.
fn states(v: &CValue) -> BTreeMap<String, St> {
    let CValue::Arr(cs) = &obj(v)["criteria"] else { panic!("no criteria array") };
    cs.iter()
        .map(|c| {
            let c = obj(c);
            let CValue::Arr(ps) = &c["problems"] else { panic!("no problems array") };
            let ps: Vec<(String, String)> =
                ps.iter().map(|p| (s(obj(p), "reason").to_string(), s(obj(p), "subject").to_string())).collect();
            (s(c, "id").to_string(), (s(c, "state").to_string(), ps))
        })
        .collect()
}

fn met() -> St {
    ("met".into(), vec![])
}

fn st(state: &str, problems: &[(&str, &str)]) -> St {
    (state.into(), problems.iter().map(|(r, s)| (r.to_string(), s.to_string())).collect())
}

/// The AC-1 states with one criterion replaced.
fn ready_but(id: &str, state: St) -> BTreeMap<String, St> {
    let mut m: BTreeMap<_, _> =
        ["INS-B-01", "INS-B-02", "INS-B-03", "INS-B-04", "INS-B-05"].iter().map(|i| (i.to_string(), met())).collect();
    m.insert(id.to_string(), state);
    m
}

#[test]
fn ac1_ready() {
    let (o, out) = run("ready", "ac1");
    assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stderr));
    let v = report(&o, &out, "inspect-beta");
    assert_eq!(s(obj(&v), "result"), "ready");
    assert_eq!(s(obj(&v), "milestone"), "inspect-beta");
    assert_eq!(s(obj(&v), "release"), RELEASE);
    assert_eq!(s(obj(&v), "author"), "bob");
    let got = states(&v);
    assert_eq!(got, ready_but("INS-B-01", met()));
    // Criteria are reported in criteria-file order with their kinds.
    let CValue::Arr(cs) = &obj(&v)["criteria"] else { panic!() };
    let kinds: Vec<&str> = cs.iter().map(|c| s(obj(c), "kind")).collect();
    assert_eq!(kinds, ["node_status", "phase_done", "parity", "parity", "signoff"]);
    // phase_done evidence: SCAN, SCAN.CMP, SCAN.VIEW (P2 SCAN.LIVE and the SCANX sibling are out).
    let CValue::Arr(ev) = &obj(&cs[1])["evidence"] else { panic!() };
    let nodes: Vec<&str> = ev.iter().map(|e| s(obj(e), "node")).collect();
    assert_eq!(nodes, ["SCAN", "SCAN.CMP", "SCAN.VIEW"]);
    assert_eq!(s(obj(&ev[1]), "ac_total"), "2");
    // Quotes are stripped like tools/scope.py does.
    assert_eq!(s(obj(&ev[2]), "status"), "leaf");
    // Parity evidence: 41 of 50 against 82 percent (exactly at the threshold).
    let CValue::Arr(ev) = &obj(&cs[2])["evidence"] else { panic!() };
    assert_eq!(s(obj(&ev[0]), "live"), "41");
    assert_eq!(s(obj(&ev[0]), "min_pct"), "82");
    let md = std::fs::read_to_string(out.join(format!("gate-inspect-beta-{RELEASE}.md"))).unwrap();
    assert!(md.contains("| Result | **ready** |"), "{md}");
    assert!(md.contains("| INS\\-B\\-05 | signoff | met |  |"), "{md}");
}

#[test]
fn ac1_text_mode() {
    let out = out_dir("text");
    let o = run_with("ready", "inspect-beta", RELEASE, &out, false);
    assert_eq!(o.status.code(), Some(0));
    let text = String::from_utf8(o.stdout).unwrap();
    assert!(text.starts_with("INS-B-01 node_status met\n"), "{text}");
    assert!(text.ends_with("result: ready\n"), "{text}");
}

#[test]
fn ac2_open_node() {
    let (o, out) = run("open-node", "ac2");
    assert_eq!(o.status.code(), Some(1));
    let v = report(&o, &out, "inspect-beta");
    assert_eq!(s(obj(&v), "result"), "not_ready");
    assert_eq!(states(&v), ready_but("INS-B-02", st("not_met", &[("node_open", "SCAN.VIEW")])));
}

#[test]
fn ac3_failing_ac() {
    let (o, out) = run("failing-ac", "ac3");
    assert_eq!(o.status.code(), Some(1));
    let v = report(&o, &out, "inspect-beta");
    assert_eq!(states(&v), ready_but("INS-B-02", st("not_met", &[("ac_failed", "SCAN.CMP AC-2")])));
}

#[test]
fn leaf_without_results_is_untested() {
    let (o, out) = run("untested", "untested");
    assert_eq!(o.status.code(), Some(1));
    let v = report(&o, &out, "inspect-beta");
    assert_eq!(states(&v), ready_but("INS-B-02", st("not_met", &[("leaf_untested", "SCAN.VIEW")])));
}

#[test]
fn ac4_seed_not_live() {
    let (o, out) = run("seed-not-live", "ac4");
    assert_eq!(o.status.code(), Some(1));
    let v = report(&o, &out, "inspect-beta");
    assert_eq!(states(&v), ready_but("INS-B-04", st("not_met", &[("seed_not_live", "SEED-INS-002")])));
}

#[test]
fn parity_below_threshold() {
    // 40 of 50 is 80 percent, below 82.
    let (o, out) = run("below-threshold", "below");
    assert_eq!(o.status.code(), Some(1));
    let v = report(&o, &out, "inspect-beta");
    assert_eq!(states(&v), ready_but("INS-B-03", st("not_met", &[("below_threshold", "inspect")])));
}

#[test]
fn ac5_self_signoff_and_wrong_role() {
    let (o, out) = run("self-signoff", "ac5a");
    assert_eq!(o.status.code(), Some(1));
    let v = report(&o, &out, "inspect-beta");
    assert_eq!(states(&v), ready_but("INS-B-05", st("needs_signoff", &[("self_signoff", "bob")])));

    let (o, out) = run("wrong-role", "ac5b");
    assert_eq!(o.status.code(), Some(1));
    let v = report(&o, &out, "inspect-beta");
    assert_eq!(states(&v), ready_but("INS-B-05", st("needs_signoff", &[("role_missing", "dave")])));
}

#[test]
fn signoff_release_mismatch_and_missing() {
    let (o, out) = run("release-mismatch", "relmis");
    assert_eq!(o.status.code(), Some(1));
    let v = report(&o, &out, "inspect-beta");
    assert_eq!(states(&v), ready_but("INS-B-05", st("needs_signoff", &[("release_mismatch", "alice")])));

    let (o, out) = run("no-signoff", "nosign");
    assert_eq!(o.status.code(), Some(1));
    let v = report(&o, &out, "inspect-beta");
    assert_eq!(
        states(&v),
        ready_but("INS-B-05", st("needs_signoff", &[("signoff_missing", "inspect-beta/INS-B-05.toml")]))
    );
}

#[test]
fn ac6_byte_identical() {
    let a = out_dir("ac6a");
    let b = out_dir("ac6b");
    let oa = run_with("ready", "inspect-beta", RELEASE, &a, true);
    let ob = run_with("ready", "inspect-beta", RELEASE, &b, true);
    assert_eq!(oa.status.code(), Some(0));
    assert_eq!(oa.stdout, ob.stdout);
    for ext in ["json", "md"] {
        let f = format!("gate-inspect-beta-{RELEASE}.{ext}");
        assert_eq!(std::fs::read(a.join(&f)).unwrap(), std::fs::read(b.join(&f)).unwrap(), "{f}");
    }
}

#[test]
fn ac7_unknown_milestone_writes_nothing() {
    let out = out_dir("ac7");
    let o = run_with("ready", "ghost", RELEASE, &out, true);
    assert_eq!(o.status.code(), Some(2));
    assert!(!out.exists());
    let v = fid_canon::parse(o.stdout.strip_suffix(b"\n").unwrap()).unwrap();
    assert_eq!(s(obj(&obj(&v)["error"]), "code"), "MilestoneUnknown");
}

#[test]
fn milestone_without_criteria_is_not_ready() {
    let out = out_dir("empty");
    let o = run_with("ready", "inspect-alpha", RELEASE, &out, true);
    assert_eq!(o.status.code(), Some(1));
    let v = report(&o, &out, "inspect-alpha");
    assert_eq!(s(obj(&v), "result"), "not_ready");
    assert!(states(&v).is_empty());
}

#[test]
fn only_the_named_milestone_is_evaluated() {
    let out = out_dir("other");
    let o = run_with("ready", "report-alpha", RELEASE, &out, true);
    assert_eq!(o.status.code(), Some(1));
    let v = report(&o, &out, "report-alpha");
    let want: BTreeMap<_, _> = [("RPT-A-01".to_string(), st("not_met", &[("node_missing", "RPT")]))].into();
    assert_eq!(states(&v), want);
}

#[test]
fn fatal_inputs_exit_2_and_write_nothing() {
    for (name, code) in [
        ("bad-criteria", "InputParse"),
        ("bad-status", "InputParse"),
        ("bad-node", "InputParse"),
        ("no-such-fixture-dir", "InputUnreadable"),
    ] {
        let out = out_dir(name);
        let o = if name == "no-such-fixture-dir" {
            // Every input exists except the tree.
            let mut args = vec!["gate-check".to_string()];
            for (k, v) in [
                ("--milestone", "inspect-beta".to_string()),
                ("--release", RELEASE.to_string()),
                ("--author", "bob".to_string()),
                ("--criteria", pick("ready", "criteria.toml")),
                ("--tree", fix("ready").join("no-such-tree").to_string_lossy().into_owned()),
                ("--trace", pick("ready", "trace.json")),
                ("--status", pick("ready", "status.json")),
                ("--tests", pick("ready", "tests.json")),
                ("--signoffs", pick("ready", "signoffs")),
                ("--roster", pick("ready", "roster.toml")),
                ("--out", out.to_string_lossy().into_owned()),
            ] {
                args.push(k.to_string());
                args.push(v);
            }
            args.push("--json".into());
            Command::new(env!("CARGO_BIN_EXE_fid-outreach")).args(&args).output().unwrap()
        } else {
            run_with(name, "inspect-beta", RELEASE, &out, true)
        };
        assert_eq!(o.status.code(), Some(2), "{name}");
        assert!(!out.exists(), "{name}");
        let v = fid_canon::parse(o.stdout.strip_suffix(b"\n").unwrap()).unwrap();
        assert_eq!(s(obj(&obj(&v)["error"]), "code"), code, "{name}");
    }
}

#[test]
fn release_tag_cannot_leave_the_output_directory() {
    let out = out_dir("badrel");
    let o = run_with("ready", "inspect-beta", "../escape", &out, true);
    assert_eq!(o.status.code(), Some(2));
    assert!(!out.exists());
}

#[test]
fn bad_usage_exits_2() {
    let o = Command::new(env!("CARGO_BIN_EXE_fid-outreach"))
        .args(["gate-check", "--milestone", "inspect-beta"])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(2));
}

#[test]
fn markdown_escapes_input_text() {
    let (o, out) = run("escape", "escape");
    assert_eq!(o.status.code(), Some(1));
    let v = report(&o, &out, "inspect-beta");
    let signer = "e|vil <b>*x*</b>";
    assert_eq!(states(&v), ready_but("INS-B-05", st("needs_signoff", &[("role_missing", signer)])));
    let md = std::fs::read_to_string(out.join(format!("gate-inspect-beta-{RELEASE}.md"))).unwrap();
    assert!(md.contains("e\\|vil &lt;b&gt;\\*x\\*&lt;\\/b&gt;"), "{md}");
    assert!(!md.contains("<b>"), "{md}");
}

#[test]
fn md_escape_rules() {
    assert_eq!(md_escape("a|b"), "a\\|b");
    assert_eq!(md_escape("[x](y)"), "\\[x\\]\\(y\\)");
    assert_eq!(md_escape("`c` _u_ #h"), "\\`c\\` \\_u\\_ \\#h");
    assert_eq!(md_escape("<&>\"'"), "&lt;&amp;&gt;&quot;&#39;");
    assert_eq!(md_escape("a\nb\r\tc"), "a b  c");
    assert_eq!(md_escape("plain AC-2"), "plain AC\\-2");
}

#[test]
fn pct_is_exact() {
    // live * 100 >= min_pct * in_scope, with min_pct's exact binary value.
    // 4100 >= 4100, and 4000 < 4100.
    assert!(meets_pct(41, 50, 82.0));
    assert!(!meets_pct(40, 50, 82.0));
    assert!(meets_pct(0, 10, 0.0));
    assert!(meets_pct(10, 10, 100.0));
    // The f64 nearest 33.3 is below it: 100 >= 99.89999...
    assert!(meets_pct(1, 3, 33.3));
    // The f64 nearest 100/3 is 33.33333333333333570..., so 3 times it exceeds 100.
    assert!(!meets_pct(1, 3, 100.0 / 3.0));
    // The f64 nearest 0.1 is above 0.1: 1000 times it exceeds 100 (floating point says equal).
    assert!(!meets_pct(1, 1000, 0.1));
    assert!(meets_pct(1, 1000, 0.09));
    assert!(!meets_pct(1, 2, f64::NAN));
    assert!(!meets_pct(1, 2, 100.5));
    assert!(!meets_pct(1, 2, -1.0));
}

#[test]
fn frontmatter_follows_scope_py() {
    let m = parse_frontmatter("---\nid: A.B\n# c\n\nstatus: 'leaf'\nlist: [x, y]\n---\nbody\n").unwrap();
    assert_eq!(m["id"].as_deref(), Some("A.B"));
    assert_eq!(m["status"].as_deref(), Some("leaf"));
    assert_eq!(m["list"], None);
    assert!(parse_frontmatter("id: A\n").is_err());
    assert!(parse_frontmatter("---\nid: A\n").is_err());
    assert!(parse_frontmatter("---\nno colon\n---\n").is_err());
    let crlf = parse_frontmatter("---\r\nid: A\r\nphase: P0\r\n---\r\n").unwrap();
    assert_eq!(crlf["phase"].as_deref(), Some("P0"));
}
