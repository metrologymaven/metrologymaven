//! Spec leaf UX.ICONS.PROV: `fid-design icons-prov` and `fid-design icons-registry` on the
//! shipped design package (AC-1) and on the hand-labelled fixtures of
//! `fixtures/ux/icons/prov/` (AC-2 .. AC-4), plus the record, attribution and index rules.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_design::icons_prov::{
    attribution_headings, check_prov, check_registry, parse_index, parse_record, spdx_ok, ThirdParty,
};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const FX: &str = "fixtures/ux/icons/prov";

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

/// AC-1: the shipped package and ATTRIBUTION.md pass (the package ships no icon yet, so the
/// report lists none; the check still runs over `icons/` and `gdt/`).
#[test]
fn ac1_shipped_icons_pass() {
    let o = run(&["icons-prov", "--root", "design", "--attribution", "ATTRIBUTION.md", "--json"]);
    assert_eq!(o.status.code(), Some(0), "{}{}", out(&o), String::from_utf8_lossy(&o.stderr));
    assert!(out(&o).contains("\"ok\":true"), "{}", out(&o));
    let r = check_prov(&root().join("design"), &root().join("ATTRIBUTION.md")).unwrap();
    assert!(r.findings.is_empty(), "{:?}", r.findings);
}

/// A complete set: beside and central declarations, a listed third-party icon, a labelless
/// icon with an `a11y_id`.
#[test]
fn ok_fixture_passes() {
    let o = run(&["icons-prov", "--root", &fx("ok"), "--attribution", &fx("ATTRIBUTION.md")]);
    assert_eq!(o.status.code(), Some(0), "{}", out(&o));
    let r = check_prov(&root().join(fx("ok")), &root().join(fx("ATTRIBUTION.md"))).unwrap();
    assert_eq!(r.icons, ["gdt/flatness.svg", "icons/save.svg", "icons/thirdp.svg"]);
    assert_eq!(r.index_entries, 3);
}

/// AC-2: an icon without a provenance record.
#[test]
fn ac2_missing_record() {
    let o = run(&["icons-prov", "--root", &fx("no-record"), "--attribution", &fx("ATTRIBUTION.md")]);
    assert_eq!(o.status.code(), Some(1));
    assert!(out(&o).contains("PROV-MISSING icons/x.svg"), "{}", out(&o));
    let o = run(&["icons-prov", "--root", &fx("no-record"), "--attribution", &fx("ATTRIBUTION.md"), "--json"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(out(&o).contains("\"code\":\"PROV-MISSING\""), "{}", out(&o));
}

/// AC-3: a third-party icon whose source is not in the attribution file.
#[test]
fn ac3_third_party_unlisted() {
    let o = run(&["icons-prov", "--root", &fx("third-party-unlisted"), "--attribution", &fx("ATTRIBUTION.md")]);
    assert_eq!(o.status.code(), Some(1));
    assert!(out(&o).contains("PROV-ATTRIBUTION icons/t.svg"), "{}", out(&o));
    // The same record passes against the repository's file only if the source is a heading
    // there; it is not.
    let r = check_prov(&root().join(fx("third-party-unlisted")), &root().join("ATTRIBUTION.md")).unwrap();
    assert_eq!(r.findings.len(), 1);
}

/// REQ-003: a labelless index entry without an `a11y_id`.
#[test]
fn labelless_without_a11y_id() {
    let o = run(&["icons-prov", "--root", &fx("labelless-no-a11y"), "--attribution", &fx("ATTRIBUTION.md")]);
    assert_eq!(o.status.code(), Some(1));
    assert!(out(&o).contains("PROV-A11Y-MISSING save"), "{}", out(&o));
}

/// AC-4: a command naming an icon that is not in the index; commands without the field are
/// informational.
#[test]
fn ac4_registry_missing_icon() {
    let args = ["icons-registry", "--manifest", &fx("manifest-icons.json"), "--root", &fx("ok"), "--json"];
    let o = run(&args);
    assert_eq!(o.status.code(), Some(1), "{}", out(&o));
    let text = run(&args[..5]);
    assert!(out(&text).contains("ICON-REGISTRY-MISSING align.run_bestfit@1 align.bestfit-x"), "{}", out(&text));
    let r = check_registry(&root().join(fx("manifest-icons.json")), &root().join(fx("ok"))).unwrap();
    assert_eq!(r.findings.len(), 1);
    assert_eq!(r.findings[0].subject, "align.run_bestfit@1 align.bestfit-x");
    assert_eq!(r.with_icon, 3);
    assert_eq!(r.no_icon_field, ["view.reset@1"]);
    assert!(out(&o).contains("\"no_icon_field\":[\"view.reset@1\"]"), "{}", out(&o));
}

#[test]
fn bad_usage_and_unreadable_inputs_exit_2() {
    for args in [
        &["icons-prov", "--root", "design"][..],
        &["icons-prov", "--attribution", "ATTRIBUTION.md"],
        &["icons-prov", "--root", "design", "--attribution", "ATTRIBUTION.md", "--manifest", "x"],
        &["icons-registry", "--root", "design"],
        &["icons-registry", "--manifest", "m", "--root", "design", "--all"],
        &["pkg-check", "--root", "design", "--attribution", "ATTRIBUTION.md"],
    ] {
        assert_eq!(run(args).status.code(), Some(2), "{args:?}");
    }
    assert_eq!(run(&["icons-prov", "--root", "design", "--attribution", "no/such.md"]).status.code(), Some(2));
    assert_eq!(run(&["icons-registry", "--manifest", "no/such.json", "--root", "design"]).status.code(), Some(2));
}

fn temp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fid-icons-prov-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let t = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &t);
        } else {
            std::fs::copy(e.path(), t).unwrap();
        }
    }
}

/// Mutations of the ok fixture: each breaks exactly one rule.
#[test]
fn record_rules_on_a_copy() {
    let attr = root().join(fx("ATTRIBUTION.md"));
    let codes = |d: &Path| -> Vec<(String, String)> {
        check_prov(d, &attr).unwrap().findings.into_iter().map(|f| (f.code.to_string(), f.subject)).collect()
    };
    let fresh = |tag: &str| {
        let d = temp(tag);
        copy_dir(&root().join(fx("ok")), &d);
        d
    };
    // No declaration at all.
    let d = fresh("nodecl");
    std::fs::remove_file(d.join("icons/save.svg.origin.json")).unwrap();
    assert_eq!(codes(&d), [("PROV-MISSING".into(), "icons/save.svg".into())]);
    // Declared both beside and centrally.
    let d = fresh("twice");
    std::fs::create_dir_all(d.join("review/origin/icons")).unwrap();
    std::fs::copy(d.join("icons/save.svg.origin.json"), d.join("review/origin/icons/save.svg.origin.json")).unwrap();
    assert_eq!(codes(&d), [("PROV-SCHEMA".into(), "icons/save.svg".into())]);
    // A declaration for another artefact.
    let d = fresh("wrongdecl");
    std::fs::copy(d.join("icons/thirdp.svg.origin.json"), d.join("icons/save.svg.origin.json")).unwrap();
    assert_eq!(codes(&d), [("PROV-SCHEMA".into(), "icons/save.svg".into())]);
    // A bad licence in the provenance file.
    let d = fresh("licence");
    let p = d.join("icons/save.svg.prov.json");
    let t = std::fs::read_to_string(&p).unwrap().replace("CC-BY-4.0", "CC BY 4.0 (c)");
    std::fs::write(&p, t).unwrap();
    assert_eq!(codes(&d), [("PROV-SCHEMA".into(), "icons/save.svg".into())]);
    // A provenance file whose icon is gone (and the icon's declaration with it).
    let d = fresh("orphan");
    std::fs::remove_file(d.join("icons/save.svg")).unwrap();
    std::fs::remove_file(d.join("icons/save.svg.origin.json")).unwrap();
    assert_eq!(codes(&d), [("PROV-SCHEMA".into(), "icons/save.svg.prov.json".into())]);
    // A malformed index.
    let d = fresh("index");
    std::fs::write(d.join("icons/index.json"), r#"{"icons":[{"name":"a"},{"name":"a"}]}"#).unwrap();
    assert_eq!(codes(&d), [("PROV-SCHEMA".into(), "icons/index.json".into())]);
    // The registry check cannot run on a malformed index.
    assert!(check_registry(&root().join(fx("manifest-icons.json")), &d).is_err());
    // No index: every named icon is missing.
    std::fs::remove_file(d.join("icons/index.json")).unwrap();
    let r = check_registry(&root().join(fx("manifest-icons.json")), &d).unwrap();
    assert_eq!(r.findings.len(), 3);
    // A malformed manifest is exit 2 (an error), never a pass.
    let m = d.join("m.json");
    for bad in [
        r#"[]"#,
        r#"{"commands":{}}"#,
        r#"{"commands":[{"id":"a"}]}"#,
        r#"{"commands":[{"id":"a","major":1,"icon":7}]}"#,
    ] {
        std::fs::write(&m, bad).unwrap();
        assert!(check_registry(&m, &d).is_err(), "{bad}");
    }
}

#[test]
fn spdx_and_headings_and_records() {
    for ok in ["MIT", "CC-BY-4.0", "Apache-2.0 WITH LLVM-exception", "MIT OR Apache-2.0", "LicenseRef-Own"] {
        assert!(spdx_ok(ok), "{ok}");
    }
    for bad in ["", " MIT", "MIT AND", "AND", "MIT  OR Apache-2.0", "(MIT)", "MIT/X11", "-MIT"] {
        assert!(!spdx_ok(bad), "{bad:?}");
    }
    let h = attribution_headings("# A\n## B c \n####### Too deep\n#NoSpace\ntext\n### \n");
    assert_eq!(h.into_iter().collect::<Vec<_>>(), ["A", "B c"]);
    let rec = |tp: &str| {
        format!(r#"{{"artefact":"icons/a.svg","format":"icon-provenance@1","licence":"MIT","third_party":{tp}}}"#)
    };
    assert_eq!(parse_record(&rec("false"), "icons/a.svg").unwrap().third_party, ThirdParty::No);
    let y = parse_record(&rec(r#"{"licence":"MIT","source":"Set"}"#), "icons/a.svg").unwrap();
    assert_eq!(y.third_party, ThirdParty::Yes { source: "Set".into(), licence: "MIT".into() });
    for bad in ["true", r#"{"source":"Set"}"#, r#"{"licence":"MIT","source":"  "}"#, "null"] {
        assert!(parse_record(&rec(bad), "icons/a.svg").is_err(), "{bad}");
    }
    assert!(parse_record(&rec("false"), "icons/b.svg").is_err(), "artefact must match");
    let dup = r#"{"artefact":"icons/a.svg","artefact":"icons/a.svg","format":"icon-provenance@1","licence":"MIT","third_party":false}"#;
    assert!(parse_record(dup, "icons/a.svg").is_err(), "duplicate keys are refused");
    let extra =
        r#"{"artefact":"icons/a.svg","author":"x","format":"icon-provenance@1","licence":"MIT","third_party":false}"#;
    assert!(parse_record(extra, "icons/a.svg").is_err());
    let bidi = rec("false").replace("MIT", "MIT\u{202e}");
    assert!(parse_record(&bidi, "icons/a.svg").is_err());
    assert_eq!(parse_index(r#"{"icons":[]}"#).unwrap(), []);
    assert!(parse_index(r#"{"icons":[{"name":"a","labelless":"yes"}]}"#).is_err());
    assert!(parse_index(r#"{"icons":[{"name":" a"}]}"#).is_err());
    let e = parse_index(r#"{"icons":[{"name":"a","labelless":true,"a11y_id":" "}]}"#).unwrap();
    assert_eq!(e[0].a11y_id, None, "a blank a11y_id counts as missing");
}
