//! Spec leaf UX.SCREENS.INV: `fid-design screens-check` and `fid-design screens-gate` on
//! the shipped design inventory and spec tree, on fixtures `fixtures/ux/screens/inv/`
//! (AC-1 .. AC-6), and on synthetic edge cases (unknown trace, missing approval, claimed
//! uncited screen, frontmatter wireframes, D0 determinism, exit-2 bad usage).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_design::screens::{
    check_gate, check_screens, APPROVAL_MISSING, APPROVAL_STALE, GATE_UNAPPROVED, GATE_UNCITED, GA_MOCKUP_MISSING,
    ID_SYNTAX, TRACE_UNKNOWN, UNTRACED,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

const FX: &str = "fixtures/ux/screens/inv";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fid-design")).current_dir(root()).args(args).output().expect("run fid-design")
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn fx(p: &str) -> String {
    format!("{FX}/{p}")
}

fn tempdir(tag: &str) -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    let id = N.fetch_add(1, Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!("fid-design-screens-{}-{tag}-{id}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn write_file(p: &Path, content: &str) {
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(p, content).unwrap();
}

/// AC-1: ok/ fixture (4 screens, 2 journey maps, matching approvals) exits 0 and is D0 deterministic.
#[test]
fn ac1_ok_fixture_passes_and_is_d0() {
    let args = ["screens-check", "--root", &fx("ok/design"), "--journeys", &fx("ok/journeys"), "--json"];
    let o1 = run(&args);
    let o2 = run(&args);
    assert_eq!(o1.status.code(), Some(0), "{}{}", out(&o1), String::from_utf8_lossy(&o1.stderr));
    assert_eq!(o1.stdout, o2.stdout, "screens-check --json must be byte-identical (D0)");
    assert!(out(&o1).contains("\"ok\":true"), "{}", out(&o1));

    let r = check_screens(&root().join(fx("ok/design")), &root().join(fx("ok/journeys")), None).unwrap();
    assert!(r.findings.is_empty(), "{:?}", r.findings);
    assert_eq!(r.screens, 4);
    assert_eq!(r.journeys, 2);
    assert_eq!(r.approvals, 6);
}

/// AC-2: untraced/ exits 1 with `SCREEN-UNTRACED SCR.RPT.FAIFORM`.
#[test]
fn ac2_untraced_fails() {
    let o = run(&["screens-check", "--root", &fx("untraced/design"), "--journeys", &fx("untraced/journeys")]);
    assert_eq!(o.status.code(), Some(1), "{}", out(&o));
    assert!(out(&o).contains("SCREEN-UNTRACED SCR.RPT.FAIFORM"), "{}", out(&o));

    let j = run(&["screens-check", "--root", &fx("untraced/design"), "--journeys", &fx("untraced/journeys"), "--json"]);
    assert_eq!(j.status.code(), Some(1));
    assert!(out(&j).contains("\"code\":\"SCREEN-UNTRACED\""), "{}", out(&j));

    let r = check_screens(&root().join(fx("untraced/design")), &root().join(fx("untraced/journeys")), None).unwrap();
    assert_eq!(r.findings.len(), 1);
    assert_eq!(r.findings[0].code, UNTRACED);
    assert_eq!(r.findings[0].subject, "SCR.RPT.FAIFORM");
}

/// AC-3: stale/ (journey map changed after approval) exits 1 with
/// `SCREEN-APPROVAL-STALE SCR.RPT.FAIFORM WF.RPT.FAIFORM@1`.
#[test]
fn ac3_stale_approval_fails() {
    let o = run(&["screens-check", "--root", &fx("stale/design"), "--journeys", &fx("stale/journeys")]);
    assert_eq!(o.status.code(), Some(1), "{}", out(&o));
    assert!(out(&o).contains("SCREEN-APPROVAL-STALE SCR.RPT.FAIFORM WF.RPT.FAIFORM@1"), "{}", out(&o));

    let j = run(&["screens-check", "--root", &fx("stale/design"), "--journeys", &fx("stale/journeys"), "--json"]);
    assert_eq!(j.status.code(), Some(1));
    assert!(out(&j).contains("\"code\":\"SCREEN-APPROVAL-STALE\""), "{}", out(&j));

    let r = check_screens(&root().join(fx("stale/design")), &root().join(fx("stale/journeys")), None).unwrap();
    assert_eq!(r.findings.len(), 1);
    assert_eq!(r.findings[0].code, APPROVAL_STALE);
    assert_eq!(r.findings[0].subject, "SCR.RPT.FAIFORM WF.RPT.FAIFORM@1");
}

/// AC-4: tree/ leaf citing WF.RPT.FAIFORM@3 when only @2 is approved exits 1 with SCREEN-GATE-UNAPPROVED.
#[test]
fn ac4_gate_unapproved_fails() {
    let args = ["screens-gate", "--spec", &fx("tree"), "--root", &fx("ok/design"), "--json"];
    let o1 = run(&args);
    let o2 = run(&args);
    assert_eq!(o1.status.code(), Some(1), "{}{}", out(&o1), String::from_utf8_lossy(&o1.stderr));
    assert_eq!(o1.stdout, o2.stdout, "screens-gate --json must be byte-identical (D0)");
    assert!(out(&o1).contains("\"code\":\"SCREEN-GATE-UNAPPROVED\""), "{}", out(&o1));

    let text = run(&args[..5]);
    assert_eq!(text.status.code(), Some(1));
    assert!(out(&text).contains("SCREEN-GATE-UNAPPROVED RPT.FAIFORM WF.RPT.FAIFORM@3"), "{}", out(&text));

    let r = check_gate(&root().join(fx("tree")), &root().join(fx("ok/design"))).unwrap();
    assert_eq!(r.leaves_scanned, 2);
    assert_eq!(r.findings.len(), 1);
    assert_eq!(r.findings[0].code, GATE_UNAPPROVED);
    assert_eq!(r.findings[0].subject, "RPT.FAIFORM WF.RPT.FAIFORM@3");
}

/// AC-5: ok/ with --ga report flags the Report screen lacking a mockup; --ga inspect passes.
#[test]
fn ac5_ga_mockup_completeness() {
    let o = run(&[
        "screens-check",
        "--root",
        &fx("ok/design"),
        "--journeys",
        &fx("ok/journeys"),
        "--ga",
        "report",
        "--json",
    ]);
    assert_eq!(o.status.code(), Some(1), "{}", out(&o));
    assert!(out(&o).contains("\"code\":\"SCREEN-GA-MOCKUP-MISSING\""), "{}", out(&o));
    assert!(out(&o).contains("\"subject\":\"SCR.RPT.BALLOON\""), "{}", out(&o));

    let r = check_screens(&root().join(fx("ok/design")), &root().join(fx("ok/journeys")), Some("report")).unwrap();
    assert_eq!(r.findings.len(), 1);
    assert_eq!(r.findings[0].code, GA_MOCKUP_MISSING);
    assert_eq!(r.findings[0].subject, "SCR.RPT.BALLOON");

    let ok_insp = run(&[
        "screens-check",
        "--root",
        &fx("ok/design"),
        "--journeys",
        &fx("ok/journeys"),
        "--ga",
        "inspect",
        "--json",
    ]);
    assert_eq!(ok_insp.status.code(), Some(0), "{}", out(&ok_insp));
}

/// AC-6: bad-id/ (`SCR.rpt.form`) exits 1 with `SCREEN-ID-SYNTAX`.
#[test]
fn ac6_bad_id_fails() {
    let o = run(&["screens-check", "--root", &fx("bad-id/design"), "--journeys", &fx("bad-id/journeys"), "--json"]);
    assert_eq!(o.status.code(), Some(1), "{}", out(&o));
    assert!(out(&o).contains("\"code\":\"SCREEN-ID-SYNTAX\""), "{}", out(&o));

    let r = check_screens(&root().join(fx("bad-id/design")), &root().join(fx("bad-id/journeys")), None).unwrap();
    assert!(r.findings.iter().any(|f| f.code == ID_SYNTAX && f.subject == "SCR.rpt.form"), "{:?}", r.findings);
}

/// Shipped repository inventory, README and spec tree pass both commands.
#[test]
fn shipped_inventory_and_spec_tree_pass() {
    let o = run(&["screens-check", "--root", "design", "--journeys", "acceptance/journeys/persona", "--json"]);
    assert_eq!(o.status.code(), Some(0), "{}{}", out(&o), String::from_utf8_lossy(&o.stderr));
    assert!(out(&o).contains("\"ok\":true"), "{}", out(&o));

    let r = check_screens(&root().join("design"), &root().join("acceptance/journeys/persona"), None).unwrap();
    assert!(r.findings.is_empty(), "{:?}", r.findings);
    assert!(r.screens >= 8, "expected primary screens across all 8 apps, got {}", r.screens);
    assert!(r.journeys >= 9, "expected shipped persona journey maps, got {}", r.journeys);

    let g = run(&["screens-gate", "--spec", "spec", "--root", "design", "--json"]);
    assert_eq!(g.status.code(), Some(0), "{}{}", out(&g), String::from_utf8_lossy(&g.stderr));
    assert!(out(&g).contains("\"ok\":true"), "{}", out(&g));

    let inv_text = fs::read_to_string(root().join("design/screens/inventory.json")).unwrap();
    for app in
        ["\"shell\"", "\"setup\"", "\"inspect\"", "\"probe\"", "\"network\"", "\"report\"", "\"kiosk\"", "\"station\""]
    {
        assert!(inv_text.contains(app), "shipped inventory missing app {app}");
    }

    let readme = fs::read_to_string(root().join("design/screens/README.md")).unwrap();
    assert!(readme.contains("SCR.<APP>.<NAME>"));
    assert!(readme.contains("WF.<APP>.<NAME>@<rev>"));
    assert!(readme.contains("MK.<APP>.<NAME>@<rev>"));
    assert!(readme.contains("Wireframes:"));
    assert!(readme.contains("wireframes:"));
}

/// Unknown trace map/step, missing approval, claimed uncited screen, and frontmatter wireframes.
#[test]
fn trace_unknown_missing_approval_and_uncited_gate() {
    let dir = tempdir("edges");
    let design = dir.join("design");
    let journeys = dir.join("journeys");
    let spec = dir.join("spec");

    write_file(&journeys.join("per-01.json"), r#"{"format":"journey-map@1","map_id":"per-01","steps":[{"n":1}]}"#);
    write_file(
        &design.join("screens/inventory.json"),
        r#"{
  "format": "screen-inventory@1",
  "screens": [
    {
      "id": "SCR.RPT.FAIFORM",
      "app": "report",
      "title": "FAI Form",
      "role_layout": "table",
      "phase": "P1a",
      "traces": [
        {"map": "per-01", "step": 99},
        {"map": "per-99", "step": 1}
      ],
      "keyboard_path": "Tab",
      "wireframe": "WF.RPT.FAIFORM@1",
      "mockup": "none"
    }
  ]
}"#,
    );

    let r = check_screens(&design, &journeys, None).unwrap();
    assert!(r.findings.iter().any(|f| f.code == TRACE_UNKNOWN && f.detail.contains("per-99")), "{:?}", r.findings);
    assert!(r.findings.iter().any(|f| f.code == TRACE_UNKNOWN && f.detail.contains("step `99`")), "{:?}", r.findings);
    assert!(
        r.findings.iter().any(|f| f.code == APPROVAL_MISSING && f.subject == "SCR.RPT.FAIFORM WF.RPT.FAIFORM@1"),
        "{:?}",
        r.findings
    );

    // Spec tree with:
    // 1. claimed leaf naming SCR.RPT.FAIFORM without citing a WF -> SCREEN-GATE-UNCITED
    // 2. unclaimed leaf naming SCR.RPT.FAIFORM without citing a WF -> allowed
    // 3. leaf citing WF.RPT.FAIFORM@2 in frontmatter `wireframes:` -> SCREEN-GATE-UNAPPROVED
    write_file(
        &spec.join("a/NODE.md"),
        "---\nid: RPT.CLAIMED\nstatus: leaf\nclaimed_by: bob\n---\n\n## Design\n\nBuilds SCR.RPT.FAIFORM.\n",
    );
    write_file(
        &spec.join("b/NODE.md"),
        "---\nid: RPT.UNCLAIMED\nstatus: leaf\nclaimed_by:\n---\n\n## Design\n\nBuilds SCR.RPT.FAIFORM.\n",
    );
    write_file(
        &spec.join("c/NODE.md"),
        "---\nid: RPT.FM\nstatus: leaf\nclaimed_by: carol\nwireframes: [WF.RPT.FAIFORM@2]\n---\n\n## Design\n\nBuilds SCR.RPT.FAIFORM.\n",
    );

    let g = check_gate(&spec, &design).unwrap();
    assert_eq!(g.leaves_scanned, 3);
    assert!(
        g.findings.iter().any(|f| f.code == GATE_UNCITED && f.subject == "RPT.CLAIMED SCR.RPT.FAIFORM"),
        "{:?}",
        g.findings
    );
    assert!(
        !g.findings.iter().any(|f| f.subject.starts_with("RPT.UNCLAIMED")),
        "unclaimed leaf must not trigger SCREEN-GATE-UNCITED: {:?}",
        g.findings
    );
    assert!(
        g.findings.iter().any(|f| f.code == GATE_UNAPPROVED && f.subject == "RPT.FM WF.RPT.FAIFORM@2"),
        "{:?}",
        g.findings
    );
    assert!(
        !g.findings.iter().any(|f| f.code == GATE_UNCITED && f.subject.starts_with("RPT.FM")),
        "frontmatter wireframe satisfies WF citation for SCR.RPT.FAIFORM: {:?}",
        g.findings
    );

    let _ = fs::remove_dir_all(&dir);
}

/// Exit 2 on bad CLI usage, unknown --ga app, or missing paths.
#[test]
fn exit_two_on_bad_usage_and_missing_inputs() {
    for bad in [
        &["screens-check", "--root", &fx("ok/design")][..],
        &["screens-check", "--root", &fx("ok/design"), "--journeys", &fx("ok/journeys"), "--ga", "bogus"][..],
        &["screens-check", "--root", &fx("missing"), "--journeys", &fx("ok/journeys")][..],
        &["screens-check", "--root", &fx("ok/design"), "--journeys", &fx("missing")][..],
        &["screens-gate", "--root", &fx("ok/design")][..],
        &["screens-gate", "--spec", &fx("missing"), "--root", &fx("ok/design")][..],
        &["screens-gate", "--spec", &fx("tree"), "--root", &fx("ok/design"), "--ga", "report"][..],
        &["pkg-check", "--root", "design", "--journeys", &fx("ok/journeys")][..],
    ] {
        let o = run(bad);
        assert_eq!(o.status.code(), Some(2), "expected exit 2 for {bad:?}, got {:?}", o.status.code());
    }
}
