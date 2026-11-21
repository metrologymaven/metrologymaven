//! ALN.QA.HIST manifest check and fragment tests. The manifests in
//! `fixtures/aln/qa/hist-manifest/` are hand-authored; the expected reports and fragment come
//! from the independent Python reference `make_expected.py` in that folder, never from running
//! this crate. The refusal cases below are hand-labelled from the spec's rules.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::parse;
use fid_xtask::convergence::{check, load, render_fragment, report};
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

const DIR: &str = "fixtures/aln/qa/hist-manifest";

fn fixture(name: &str) -> PathBuf {
    root().join(DIR).join(name)
}

fn expected(name: &str) -> String {
    std::fs::read_to_string(fixture(name)).unwrap()
}

fn xtask(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_fid-xtask")).current_dir(root()).args(args).output().unwrap();
    (out.status.code().unwrap(), String::from_utf8(out.stdout).unwrap(), String::from_utf8(out.stderr).unwrap())
}

/// AC-1: the defective manifest fails naming the command and `default`; the fixed one passes.
#[test]
fn ac1_check_names_command_and_field() {
    let c = check(&load(&fixture("defective.json")).unwrap()).unwrap();
    assert_eq!(report(&c), expected("expected-check-defective.txt"));
    assert_eq!(c.findings.len(), 1);
    assert_eq!(c.findings[0].command.text(), "align.fit_features@1");
    assert!(c.findings[0].field.ends_with("/default"));
    assert_eq!(c.findings[0].code, "manifest_field_missing");
    let c = check(&load(&fixture("fixed.json")).unwrap()).unwrap();
    assert_eq!(report(&c), expected("expected-check-fixed.txt"));
    assert!(c.findings.is_empty());

    let (code, out, _) = xtask(&["convergence-check", "--manifest", &format!("{DIR}/defective.json")]);
    assert_eq!((code, out), (1, expected("expected-check-defective.txt")));
    let (code, out, _) = xtask(&["convergence-check", "--manifest", &format!("{DIR}/fixed.json")]);
    assert_eq!((code, out), (0, expected("expected-check-fixed.txt")));
}

/// AC-4: the fragment is byte-identical on two runs and has one table per iterative command.
#[test]
fn ac4_fragment_deterministic() {
    let a = render_fragment(&check(&load(&fixture("fixed.json")).unwrap()).unwrap()).unwrap();
    let b = render_fragment(&check(&load(&fixture("fixed.json")).unwrap()).unwrap()).unwrap();
    assert_eq!(a, b);
    assert_eq!(a, expected("expected-fragment.md"));
    assert_eq!(a.matches("| Criterion | Unit | Default | Meaning |").count(), 3);
    assert_eq!(a.matches("\n## `").count(), 3);
    assert!(!a.contains("align.rps"), "direct commands have no table");

    let m = format!("{DIR}/fixed.json");
    let (c1, o1, _) = xtask(&["convergence-doc", "--manifest", &m]);
    let (c2, o2, _) = xtask(&["convergence-doc", "--manifest", &m]);
    assert_eq!((c1, c2), (0, 0));
    assert_eq!(o1, o2);
    assert_eq!(o1, a);
    let (code, _, _) = xtask(&["convergence-doc", "--manifest", &m, "--check", &format!("{DIR}/expected-fragment.md")]);
    assert_eq!(code, 0);
    // A hand edit of the committed fragment is caught.
    let (code, _, err) = xtask(&["convergence-doc", "--manifest", &m, "--check", &format!("{DIR}/fixed.json")]);
    assert_eq!(code, 1, "{err}");
    // A manifest that fails the check produces no fragment.
    let (code, out, _) = xtask(&["convergence-doc", "--manifest", &format!("{DIR}/defective.json")]);
    assert_eq!((code, out.as_str()), (1, ""));
    assert!(render_fragment(&check(&load(&fixture("defective.json")).unwrap()).unwrap()).is_none());
}

fn lines(json: &str) -> Vec<String> {
    let c = check(&parse(json.as_bytes()).unwrap()).unwrap();
    c.findings.iter().map(|f| format!("{} {} {}", f.command.text(), f.field, f.code)).collect()
}

const OK: &str = r#"{"name":"increment","unit":"m","default":"1e-9","meaning":"Step below this."}"#;

fn one(cmd_fields: &str) -> String {
    format!(r#"{{"format":1,"commands":[{{"id":"align.x","major":1{cmd_fields}}}]}}"#)
}

/// Hand-labelled refusals (REQ-004 and the field rules).
#[test]
fn refusals() {
    assert_eq!(lines(&one(r#","iterative":true"#)), ["align.x@1 /convergence manifest_convergence_missing"]);
    assert_eq!(
        lines(&one(r#","iterative":true,"convergence":[]"#)),
        ["align.x@1 /convergence manifest_convergence_missing"]
    );
    assert_eq!(lines(&one(r#","iterative":true,"convergence":{}"#)), ["align.x@1 /convergence manifest_field_invalid"]);
    assert_eq!(lines(&one(r#","iterative":"yes""#)), ["align.x@1 /iterative manifest_field_invalid"]);
    assert!(lines(&one(r#","iterative":false"#)).is_empty());
    assert!(lines(&one("")).is_empty());
    assert_eq!(
        lines(&one(r#","iterative":true,"convergence":[{"name":"","default":"1"}]"#)),
        [
            "align.x@1 /convergence/0/meaning manifest_field_missing",
            "align.x@1 /convergence/0/name manifest_field_missing",
            "align.x@1 /convergence/0/unit manifest_field_missing",
        ]
    );
    let bad = r#"{"name":"Inc","unit":"m s","default":"1.","meaning":"two\nlines"}"#;
    assert_eq!(
        lines(&one(&format!(r#","iterative":true,"convergence":[{bad},1,{OK}]"#))),
        [
            "align.x@1 /convergence/0/default manifest_field_invalid",
            "align.x@1 /convergence/0/meaning manifest_field_invalid",
            "align.x@1 /convergence/0/name manifest_field_invalid",
            "align.x@1 /convergence/0/unit manifest_field_invalid",
            "align.x@1 /convergence/1 manifest_field_invalid",
        ]
    );
    let num = r#"{"name":"a","unit":"m","default":1e-9,"meaning":"x"}"#;
    assert_eq!(
        lines(&one(&format!(r#","iterative":true,"convergence":[{num}]"#))),
        ["align.x@1 /convergence/0/default manifest_field_invalid"]
    );
    let long = format!(r#"{{"name":"a","unit":"m","default":"1","meaning":"{}"}}"#, "x".repeat(121));
    assert_eq!(
        lines(&one(&format!(r#","iterative":true,"convergence":[{long}]"#))),
        ["align.x@1 /convergence/0/meaning manifest_field_invalid"]
    );
    assert_eq!(
        lines(&one(&format!(r#","iterative":true,"convergence":[{OK},{OK}]"#))),
        ["align.x@1 /convergence/1/name manifest_field_invalid"]
    );
    let twice = format!(
        r#"{{"commands":[{{"id":"a.b","major":1,"iterative":true,"convergence":[{OK}]}},{{"id":"a.b","major":1}}]}}"#
    );
    assert_eq!(lines(&twice), ["a.b@1  manifest_duplicate_command"]);
    // Sorted by ID bytes then major (2 before 10), whatever the input order.
    let order = r#"{"commands":[{"id":"b","major":10,"iterative":true},{"id":"b","major":2,"iterative":true},{"id":"a","major":1,"iterative":true}]}"#;
    assert_eq!(
        lines(order),
        [
            "a@1 /convergence manifest_convergence_missing",
            "b@2 /convergence manifest_convergence_missing",
            "b@10 /convergence manifest_convergence_missing",
        ]
    );
}

/// Input that is not a manifest is `manifest_invalid` (exit 2), never a pass.
#[test]
fn invalid_manifests() {
    for doc in [
        "[]",
        r#"{"format":1}"#,
        r#"{"commands":{}}"#,
        r#"{"commands":[1]}"#,
        r#"{"commands":[{"major":1}]}"#,
        r#"{"commands":[{"id":"a"}]}"#,
        r#"{"commands":[{"id":"a","major":1.5}]}"#,
        r#"{"commands":[{"id":"a","major":100000}]}"#,
        r#"{"commands":[{"id":"a","major":-1}]}"#,
    ] {
        assert!(check(&parse(doc.as_bytes()).unwrap()).is_err(), "{doc}");
    }
    let many: Vec<String> =
        (0..65).map(|i| format!(r#"{{"name":"c{i}","unit":"m","default":"1","meaning":"x"}}"#)).collect();
    let doc = one(&format!(r#","iterative":true,"convergence":[{}]"#, many.join(",")));
    assert!(check(&parse(doc.as_bytes()).unwrap()).is_err());
    // Duplicate JSON keys are refused by the parser.
    assert!(parse(br#"{"commands":[],"commands":[]}"#).is_err());
    let (code, _, err) = xtask(&["convergence-check", "--manifest", &format!("{DIR}/trace.txt")]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("manifest_invalid"));
    let (code, _, _) = xtask(&["convergence-check"]);
    assert_eq!(code, 2);
}

/// No iterative command: the check passes and the fragment says so.
#[test]
fn no_iterative_commands() {
    let c = check(&parse(br#"{"commands":[{"id":"a","major":1}]}"#).unwrap()).unwrap();
    assert_eq!(report(&c), "convergence-check: ok (0 iterative commands)\n");
    let f = render_fragment(&c).unwrap();
    assert!(f.ends_with("\n_No iterative commands._\n"));
}
