//! Acceptance criteria AC-1..AC-4 of spec node QUAL.VAP.PROTO, run through the
//! `fid-xtask qual vap check-protocols` binary, plus the library rules. The cases and their
//! expected output are written by `fixtures/qual/vap/proto/make_fixtures.py` (an independent
//! stdlib Python implementation of the rules); refusal cases below are hand-labelled.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_qual::{CheckResult, GoldenExceedance, GoldenOutcome, GoldenResult, QualificationRecord, RecordKind, Verdict};
use fid_types::Digest;
use fid_xtask::qual::vap::proto::{self, ManifestCommands, RecordSchema};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

const DIR: &str = "fixtures/qual/vap/proto";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn manifest_arg() -> String {
    format!("{DIR}/manifest.json")
}

fn schema_arg() -> String {
    format!("{DIR}/qual-record-1.json")
}

/// Runs `fid-xtask qual vap check-protocols <args>` from the repository root; returns
/// (exit code, stdout lines, stderr).
fn run(args: &[&str]) -> (i32, Vec<String>, String) {
    let go = || {
        let out = Command::new(env!("CARGO_BIN_EXE_fid-xtask"))
            .current_dir(repo_root())
            .args(["qual", "vap", "check-protocols"])
            .args(args)
            .output()
            .unwrap();
        let stdout = String::from_utf8(out.stdout).unwrap().replace("\r\n", "\n");
        let lines: Vec<String> = stdout.lines().map(str::to_string).collect();
        (out.status.code().unwrap(), lines, String::from_utf8(out.stderr).unwrap())
    };
    let got = go();
    // D0: identical output across runs.
    assert_eq!(go(), got);
    got
}

/// Runs one case of `expected.json` with the fixture manifest and schema.
fn run_case(dir: &str) -> (i32, Vec<String>) {
    let (m, s) = (manifest_arg(), schema_arg());
    let (code, out, _) = run(&["--manifest", &m, "--record-schema", &s, "--dir", dir]);
    (code, out)
}

/// `expected.json`: case name -> (dir, exit code, stdout lines).
fn expected() -> Vec<(String, String, i32, Vec<String>)> {
    let bytes = std::fs::read(repo_root().join(DIR).join("expected.json")).unwrap();
    let CValue::Obj(cases) = fid_canon::parse(&bytes).unwrap() else { panic!("expected.json is not an object") };
    let mut out = Vec::new();
    for (name, v) in cases {
        let CValue::Obj(c) = v else { panic!("{name}") };
        let Some(CValue::Str(dir)) = c.get("dir") else { panic!("{name}: dir") };
        let Some(CValue::Num(code)) = c.get("exit") else { panic!("{name}: exit") };
        let Some(CValue::Arr(lines)) = c.get("stdout") else { panic!("{name}: stdout") };
        let lines = lines
            .iter()
            .map(|l| match l {
                CValue::Str(s) => s.clone(),
                _ => panic!("{name}: stdout line"),
            })
            .collect();
        out.push((name, dir.clone(), *code as i32, lines));
    }
    out
}

fn case(name: &str) -> (String, i32, Vec<String>) {
    let (_, dir, code, lines) = expected().into_iter().find(|c| c.0 == name).unwrap();
    (dir, code, lines)
}

/// AC-1: the shipped protocols, the manifest fixture and the record schema exit 0.
#[test]
fn ac1_shipped_protocols_pass() {
    let (dir, code, lines) = case("shipped");
    assert_eq!(dir, "vap/templates");
    assert_eq!(code, 0);
    assert_eq!(run_case(&dir), (code, lines.clone()));
    // Without --dir the shipped template directory is the default.
    let (m, s) = (manifest_arg(), schema_arg());
    let (c, out, _) = run(&["--manifest", &m, "--record-schema", &s]);
    assert_eq!((c, out), (0, lines));
}

/// AC-2: a step naming `validate.run_iq@2` exits 1 naming the step.
#[test]
fn ac2_unknown_command_major() {
    let (dir, code, lines) = case("bad-command");
    assert_eq!(code, 1);
    assert_eq!(lines[0], "PROTO-COMMAND iq-protocol/iq-signatures: validate.run_iq@2 is not in the registry manifest");
    assert_eq!(run_case(&dir), (code, lines));
}

/// AC-3: a step naming field `checks.verdictt` exits 1 naming the step.
#[test]
fn ac3_unknown_record_field() {
    let (dir, code, lines) = case("bad-field");
    assert_eq!(code, 1);
    assert_eq!(lines[0], "PROTO-FIELD iq-protocol/iq-artefacts: checks.verdictt is not a field of qual-record/1");
    assert_eq!(run_case(&dir), (code, lines));
}

/// AC-4: an OQ protocol without a `validate.run_oq@1` step exits 1.
#[test]
fn ac4_oq_without_run_oq() {
    let (dir, code, lines) = case("no-run-oq");
    assert_eq!(code, 1);
    assert_eq!(lines[0], "PROTO-REQUIRED validate.run_oq@1: oq-protocol has no step that runs it");
    assert_eq!(run_case(&dir), (code, lines));
}

/// Every case of the oracle, including the error paths beyond the four ACs.
#[test]
fn every_oracle_case() {
    let all = expected();
    assert_eq!(all.len(), 10);
    for (name, dir, code, lines) in all {
        assert_eq!(run_case(&dir), (code, lines), "case {name}");
    }
}

/// Usage errors and unreadable or invalid inputs exit 2, never 0 or 1.
#[test]
fn usage_and_input_errors() {
    let (m, s) = (manifest_arg(), schema_arg());
    let two = |args: &[&str], needle: &str| {
        let (code, out, err) = run(args);
        assert_eq!(code, 2, "{args:?}: {out:?} {err}");
        assert!(out.is_empty(), "{args:?}: {out:?}");
        assert!(err.contains(needle), "{args:?}: {err}");
    };
    two(&[], "--manifest and --record-schema are required");
    two(&["--manifest", &m], "--manifest and --record-schema are required");
    two(&["--manifest", &m, "--manifest", &m, "--record-schema", &s], "usage:");
    two(&["--manifest", &m, "--record-schema", &s, "--bogus", "x"], "usage:");
    two(&["--manifest", &m, "--record-schema"], "usage:");
    // Each input swapped for the other is refused, not read as empty.
    two(&["--manifest", &s, "--record-schema", &s], "registry manifest: format must be 1");
    two(&["--manifest", &m, "--record-schema", &m], "record schema: root must have \"type\": \"object\"");
    two(&["--manifest", &format!("{DIR}/absent.json"), "--record-schema", &s], "registry manifest: cannot open");
    two(&["--manifest", &m, "--record-schema", &s, "--dir", &format!("{DIR}/absent")], "template directory");
    two(&["--manifest", &m, "--record-schema", &s, "--catalogue", &m], "intended-use catalogue");
    // A directory is not a regular file.
    two(&["--manifest", DIR, "--record-schema", &s], "is not a regular file");
    two(&["--manifest", &m, "--record-schema", DIR], "is not a regular file");
}

/// A symbolic link is refused for the manifest and the record schema, even when it points at
/// the right file.
#[cfg(unix)]
#[test]
fn symlinked_inputs_refused() {
    let tmp = Path::new(env!("CARGO_TARGET_TMPDIR")).join("qual_vap_proto_symlinks");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let (lm, ls) = (tmp.join("manifest.json"), tmp.join("schema.json"));
    std::os::unix::fs::symlink(repo_root().join(DIR).join("manifest.json"), &lm).unwrap();
    std::os::unix::fs::symlink(repo_root().join(DIR).join("qual-record-1.json"), &ls).unwrap();
    let (m, s) = (manifest_arg(), schema_arg());
    let (lm, ls) = (lm.to_str().unwrap(), ls.to_str().unwrap());
    for (args, needle) in [
        (["--manifest", lm, "--record-schema", s.as_str()], "registry manifest:"),
        (["--manifest", m.as_str(), "--record-schema", ls], "record schema:"),
    ] {
        let (code, out, err) = run(&args);
        assert_eq!(code, 2, "{out:?} {err}");
        assert!(err.contains(needle) && err.contains("is a symbolic link"), "{err}");
    }
}

/// Until QUAL.IQOQ.IQ, QUAL.IQOQ.BUNDLE and QUAL.IQOQ.OQ land, each protocol's purpose text
/// says its commands are specified but not in this release. When one of those leaves records
/// `built_in`, this test fails: drop the sentence (and this pin) for that command then.
#[test]
fn unbuilt_commands_are_stated() {
    let pins = [
        (
            "iq-protocol",
            "Not runnable yet: the commands validate.run_iq@1 and validate.export_bundle@1 are specified but are not in this release, so this protocol cannot be executed until a release ships them.",
            ["spec/qual/iqoq/iq/NODE.md", "spec/qual/iqoq/bundle/NODE.md"].as_slice(),
        ),
        (
            "oq-protocol",
            "Not runnable yet: the command validate.run_oq@1 is specified but is not in this release, so this protocol cannot be executed until a release ships it.",
            ["spec/qual/iqoq/oq/NODE.md"].as_slice(),
        ),
    ];
    let cat = fid_qual::vap::tpl::Catalogue::from_path(&repo_root().join("vap/intended-uses.toml")).unwrap();
    for (id, sentence, nodes) in pins {
        let bytes = std::fs::read(repo_root().join(format!("vap/templates/{id}.json"))).unwrap();
        let t = fid_qual::vap::tpl::Template::from_json(id, &bytes, &cat).unwrap();
        let purpose = t.sections().iter().find(|s| s.id == "purpose").and_then(|s| s.text.clone()).unwrap();
        assert!(purpose.contains(sentence), "{id}: purpose text lacks the not-in-this-release sentence");
        for node in nodes {
            let text = std::fs::read_to_string(repo_root().join(node)).unwrap();
            // Only the frontmatter has a line starting with `built_in:`.
            assert!(
                !text.lines().any(|l| l.starts_with("built_in:")),
                "{node} is now built: remove the not-in-this-release sentence from {id} and update this test"
            );
        }
    }
}

// ------------------------------------------------------------------ library rules

#[test]
fn manifest_refusals() {
    let ok = br#"{"format":1,"commands":[{"id":"validate.run_iq","major":1},{"id":"validate.run_iq","major":2}]}"#;
    let m = ManifestCommands::from_json(ok).unwrap();
    assert!(m.contains("validate.run_iq@1") && m.contains("validate.run_iq@2"));
    assert!(!m.contains("validate.run_iq@01") && !m.contains("validate.run_iq"));
    assert_eq!(m.len(), 2);
    let refuse = |b: &[u8], needle: &str| {
        let e = ManifestCommands::from_json(b).unwrap_err();
        assert!(e.contains(needle), "{e}");
    };
    refuse(br#"{"format":2,"commands":[]}"#, "format must be 1");
    refuse(br#"{"commands":[]}"#, "format must be 1");
    refuse(br#"{"format":1}"#, "commands must be an array");
    refuse(br#"[]"#, "must be a JSON object");
    refuse(br#"{"format":1,"commands":[1]}"#, "commands[0] must be an object");
    refuse(br#"{"format":1,"commands":[{"id":"Bad Name","major":1}]}"#, "commands[0].id");
    refuse(br#"{"format":1,"commands":[{"id":"a.b","major":1.5}]}"#, "commands[0].major");
    refuse(br#"{"format":1,"commands":[{"id":"a.b","major":-1}]}"#, "commands[0].major");
    refuse(br#"{"format":1,"commands":[{"id":"a.b","major":100000}]}"#, "commands[0].major");
    refuse(br#"{"format":1,"commands":[{"id":"a.b","major":"1"}]}"#, "commands[0].major");
    refuse(br#"{"format":1,"commands":[{"id":"a.b","major":1},{"id":"a.b","major":1}]}"#, "a second time");
    refuse(br#"{"format":1,"format":1,"commands":[]}"#, "not valid JSON");
    // An empty command list is a valid (if useless) manifest: every step then fails.
    assert!(ManifestCommands::from_json(br#"{"format":1,"commands":[]}"#).unwrap().is_empty());
}

fn fixture_schema() -> RecordSchema {
    RecordSchema::from_json(&std::fs::read(repo_root().join(DIR).join("qual-record-1.json")).unwrap()).unwrap()
}

#[test]
fn schema_resolution() {
    let s = fixture_schema();
    for ok in ["verdict", "checks", "checks.verdict", "checks.id", "goldens.outcome", "goldens.exceedances.delta"] {
        assert!(s.resolves(ok), "{ok}");
    }
    for bad in
        ["", "verdictt", "checks.verdictt", "checks.", ".verdict", "Verdict", "verdict.x", "goldens.items", "$defs"]
    {
        assert!(!s.resolves(bad), "{bad:?}");
    }
    let refuse = |b: &[u8], needle: &str| {
        let e = RecordSchema::from_json(b).unwrap_err();
        assert!(e.contains(needle), "{e}");
    };
    refuse(br#"{"type":"object","properties":{}}"#, "properties.schema must be");
    refuse(br#"{"type":"object","properties":{"schema":{"const":"qual-record/2"}}}"#, "properties.schema must be");
    refuse(br#"{"properties":{"schema":{"const":"qual-record/1"}}}"#, "root must have \"type\"");
    refuse(br#"{"type":"object"}"#, "root must have properties");
    refuse(br#"{"type":"object","type":"object"}"#, "not valid JSON");
    // A $ref outside #/$defs, a missing definition and a $ref cycle do not resolve.
    let s = RecordSchema::from_json(
        br##"{"type":"object","properties":{"schema":{"const":"qual-record/1"},
        "a":{"$ref":"http://example.invalid/x"},"b":{"$ref":"#/$defs/none"},"c":{"$ref":"#/$defs/loop"},
        "d":{"type":"array","items":{"type":"array","items":{"properties":{"e":{}}}}}},
        "$defs":{"loop":{"$ref":"#/$defs/loop"}}}"##,
    )
    .unwrap();
    for bad in ["a", "b", "c", "a.x", "b.x", "c.x"] {
        assert!(!s.resolves(bad), "{bad}");
    }
    assert_eq!(s.field_paths(), ["d", "d.e", "schema"]);
    assert!(s.resolves("d.e"));
}

fn digest(c: char) -> Digest {
    Digest::parse(&format!("sha256:{}", c.to_string().repeat(64))).unwrap()
}

/// Every key path of a JSON value, arrays standing for their elements.
fn value_paths(v: &CValue, prefix: &str, out: &mut BTreeSet<String>) {
    match v {
        CValue::Obj(m) => {
            for (k, child) in m {
                let p = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
                out.insert(p.clone());
                value_paths(child, &p, out);
            }
        }
        CValue::Arr(a) => {
            for item in a {
                value_paths(item, prefix, out);
            }
        }
        _ => {}
    }
}

/// The fixture schema has exactly the fields fid-qual writes in `qual-record/1`: every key
/// path of a fully populated IQ and OQ record resolves, and every schema path occurs in one of
/// them. A field added to or removed from the record type fails this test until the schema
/// (and the protocols) follow.
#[test]
fn record_schema_matches_fid_qual_records() {
    let check = |id: &str| CheckResult { id: id.to_string(), verdict: Verdict::Pass, detail: "MATCH".to_string() };
    let base = |kind| QualificationRecord {
        schema: fid_qual::RECORD_SCHEMA.to_string(),
        kind,
        product_version: "1.4.0".to_string(),
        kernel_manifest_digest: digest('1'),
        evidence_statement_digest: digest('2'),
        env_digest: digest('3'),
        checks: vec![check("artefact:fid-cli")],
        verdict: Verdict::Pass,
        rdl_subset_digest: None,
        goldens: None,
    };
    let iq = base(RecordKind::Iq);
    let mut oq = base(RecordKind::Oq);
    oq.rdl_subset_digest = Some(digest('4'));
    oq.goldens = Some(vec![GoldenResult {
        golden_id: "synthetic-001".to_string(),
        outcome: GoldenOutcome::Divergent,
        reasons: vec!["tolerance".to_string()],
        exceedances: vec![GoldenExceedance {
            characteristic: "c1".to_string(),
            delta: "0.002".to_string(),
            decision_changed: false,
        }],
    }]);
    let mut record_paths = BTreeSet::new();
    for r in [&iq, &oq] {
        r.validate().unwrap();
        value_paths(&r.to_value(), "", &mut record_paths);
    }
    let schema = fixture_schema();
    for p in &record_paths {
        assert!(schema.resolves(p), "record field {p} is missing from the schema");
    }
    let schema_paths: BTreeSet<String> = schema.field_paths().into_iter().collect();
    assert_eq!(schema_paths, record_paths);
}

/// The required command IDs are the ones fid-qual registers or names.
#[test]
fn required_commands_match_fid_qual() {
    assert_eq!(proto::RUN_IQ, fid_qual::RUN_IQ_ID);
    assert_eq!(proto::RUN_OQ, fid_qual::RUN_OQ_ID);
    assert_eq!(proto::REGISTER_GOLDEN, fid_qual::REGISTER_GOLDEN_ID);
    assert_eq!(proto::Protocol::Iq.required(), [proto::RUN_IQ, proto::EXPORT_BUNDLE]);
    assert_eq!(proto::Protocol::Oq.required(), [proto::RUN_OQ]);
}

/// The step table read from the shipped IQ protocol, with the commands REQ-003 names.
#[test]
fn shipped_iq_steps() {
    let cat = fid_qual::vap::tpl::Catalogue::from_path(&repo_root().join("vap/intended-uses.toml")).unwrap();
    let bytes = std::fs::read(repo_root().join("vap/templates/iq-protocol.json")).unwrap();
    let t = fid_qual::vap::tpl::Template::from_json("iq-protocol", &bytes, &cat).unwrap();
    let steps = proto::steps(&t).unwrap();
    let ids: Vec<&str> = steps.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "iq-version",
            "iq-artefacts",
            "iq-signatures",
            "iq-kernel-manifest",
            "iq-environment",
            "iq-verdict",
            "iq-bundle"
        ]
    );
    assert!(steps.iter().all(|s| !s.purpose.is_empty() && !s.acceptance.is_empty()));
    assert_eq!(steps.last().unwrap().command, proto::EXPORT_BUNDLE);
}
