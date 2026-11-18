//! Acceptance criteria of spec node ECO.REL.SIGN.SBOM (AC-1 .. AC-5). Expected documents come
//! from `fixtures/rel/sbom/make_fixtures.py` (an independent Python reference that evaluates the
//! fixture's dependency graph by hand) or are hand-labelled from the spec's rules.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, CValue};
use fid_xtask::rel::sbom::{run, Options, SbomError, SBOM_SUFFIX};
use fid_xtask::rel::sums::{unsummed_kind, KIND_SBOM};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

const LINUX: &str = "fid-cli-x86_64-linux.tar.gz.spdx.json";
const WINDOWS: &str = "fid-cli-x86_64-windows.zip.spdx.json";

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/rel/sbom").join(name)
}

/// A fresh, empty output directory under the target directory (never inside the fixtures).
fn out_dir(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join("rel-sbom").join(name);
    let _ = std::fs::remove_dir_all(&d);
    d
}

/// Runs `fid-xtask rel sbom ...` (what `cargo xtask rel sbom ...` runs) from the fixture root
/// with the given arguments; returns (exit code, stdout).
fn cli(args: &[&str], out: &Path) -> (i32, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_fid-xtask"))
        .current_dir(fixture(""))
        .args(["rel", "sbom"])
        .args(args)
        .arg("--out")
        .arg(out)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&o.stderr);
    assert!(o.status.code().is_some(), "killed: {stderr}");
    (o.status.code().unwrap(), String::from_utf8(o.stdout).unwrap())
}

fn ac1_args() -> Vec<&'static str> {
    vec!["--plan", "plan.json", "--workspace", "ws", "--native", "native-components.toml"]
}

fn obj(v: &CValue) -> &BTreeMap<String, CValue> {
    match v {
        CValue::Obj(o) => o,
        _ => panic!("not an object"),
    }
}

fn arr(v: &CValue) -> &[CValue] {
    match v {
        CValue::Arr(a) => a,
        _ => panic!("not an array"),
    }
}

fn st(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        _ => panic!("not a string"),
    }
}

/// The documents' packages as name -> package object.
fn packages(doc: &CValue) -> BTreeMap<String, BTreeMap<String, CValue>> {
    arr(&obj(doc)["packages"]).iter().map(|p| (st(&obj(p)["name"]).to_string(), obj(p).clone())).collect()
}

fn read_doc(dir: &Path, name: &str) -> CValue {
    parse(&std::fs::read(dir.join(name)).unwrap()).unwrap()
}

/// AC-1: the Linux document lists exactly the five expected packages and equals the reference
/// document byte for byte (as does the Windows one).
#[test]
fn ac1_documents_match_reference() {
    let out = out_dir("ac1");
    let (code, stdout) = cli(&ac1_args(), &out);
    assert_eq!((code, stdout.as_str()), (0, "sbom: ok 2 documents (1.4.0)\n"));
    for name in [LINUX, WINDOWS] {
        assert_eq!(
            std::fs::read(out.join(name)).unwrap(),
            std::fs::read(fixture("expected").join(name)).unwrap(),
            "{name} differs from the Python reference"
        );
    }
    let names: Vec<String> = packages(&read_doc(&out, LINUX)).into_keys().collect();
    assert_eq!(
        names,
        ["Fixture Sans", "fid-cli", "fid-cli-x86_64-linux.tar.gz", "fid-core", "fid-sbom-fixture-dep"]
    );
    // Only the two documents are written: no temporary files are left behind.
    let mut files: Vec<String> =
        std::fs::read_dir(&out).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    files.sort();
    assert_eq!(files, [LINUX, WINDOWS]);
}

/// AC-1: the document fields REQ-002/003/006 require, labelled by hand.
#[test]
fn ac1_required_fields() {
    let out = out_dir("ac1-fields");
    assert_eq!(cli(&ac1_args(), &out).0, 0);
    let doc = read_doc(&out, LINUX);
    let top = obj(&doc);
    assert_eq!(st(&top["spdxVersion"]), "SPDX-2.3");
    assert_eq!(st(&top["dataLicense"]), "CC0-1.0");
    assert_eq!(st(&obj(&top["creationInfo"])["created"]), "2026-11-17T09:30:00Z");
    let pk = packages(&doc);
    let dep = &pk["fid-sbom-fixture-dep"];
    assert_eq!(st(&dep["versionInfo"]), "0.1.0");
    assert_eq!(st(&dep["licenseDeclared"]), "MIT OR Apache-2.0");
    assert_eq!(st(&dep["downloadLocation"]), "https://crates.io/api/v1/crates/fid-sbom-fixture-dep/0.1.0/download");
    let purl = obj(&arr(&dep["externalRefs"])[0]);
    assert_eq!(st(&purl["referenceType"]), "purl");
    assert_eq!(st(&purl["referenceLocator"]), "pkg:cargo/fid-sbom-fixture-dep@0.1.0");
    // The lockfile checksum of the vendored crate (fixtures/rel/sbom/ws/Cargo.lock).
    let lock = std::fs::read_to_string(fixture("ws/Cargo.lock")).unwrap();
    let sum = st(&obj(&arr(&dep["checksums"])[0])["checksumValue"]).to_string();
    assert!(lock.contains(&format!("checksum = \"{sum}\"")));
    // Path crates have no lockfile checksum and no download location.
    assert!(!pk["fid-core"].contains_key("checksums"));
    assert_eq!(st(&pk["fid-core"]["downloadLocation"]), "NOASSERTION");
    // The font carries its SHA-256 from native-components.toml.
    let toml = std::fs::read_to_string(fixture("native-components.toml")).unwrap();
    let font = st(&obj(&arr(&pk["Fixture Sans"]["checksums"])[0])["checksumValue"]).to_string();
    assert!(toml.contains(&format!("sha256 = \"{font}\"")));
    // REQ-006: the root is the artefact with its SHA-256, described by the document and
    // containing every component.
    let root = &pk["fid-cli-x86_64-linux.tar.gz"];
    assert_eq!(st(&root["SPDXID"]), "SPDXRef-Artefact");
    let body = std::fs::read(fixture("artefacts/fid-cli-x86_64-linux.tar.gz")).unwrap();
    let root_sum = st(&obj(&arr(&root["checksums"])[0])["checksumValue"]).to_string();
    let hex: String = Sha256::digest(body.as_slice()).iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(root_sum, hex);
    let rels = arr(&top["relationships"]);
    assert_eq!(rels.len(), 1 + 4);
    assert_eq!(st(&obj(&rels[0])["relationshipType"]), "DESCRIBES");
    for r in &rels[1..] {
        assert_eq!(st(&obj(r)["spdxElementId"]), "SPDXRef-Artefact");
        assert_eq!(st(&obj(r)["relationshipType"]), "CONTAINS");
    }
}

/// AC-1 with `--artefacts`: hashing the artefact files gives the same documents as the
/// plan's `sha256` values.
#[test]
fn ac1_artefact_digests_from_files() {
    let out = out_dir("ac1-files");
    let mut args = ac1_args();
    args.extend(["--artefacts", "artefacts"]);
    assert_eq!(cli(&args, &out).0, 0);
    for name in [LINUX, WINDOWS] {
        assert_eq!(std::fs::read(out.join(name)).unwrap(), std::fs::read(fixture("expected").join(name)).unwrap());
    }
}

/// AC-2: a crate whose licence is `GPL-2.0 OR` exits 1 with `SBOM-LICENCE <crate>`; nothing is
/// written.
#[test]
fn ac2_invalid_licence() {
    let out = out_dir("ac2");
    let args = [
        "--plan",
        "bad-licence/plan.json",
        "--workspace",
        "bad-licence/ws",
        "--native",
        "bad-licence/native-components.toml",
    ];
    let (code, stdout) = cli(&args, &out);
    assert_eq!(code, 1, "{stdout}");
    assert!(stdout.starts_with("SBOM-LICENCE bad-lic"), "{stdout}");
    assert_eq!(stdout, "SBOM-LICENCE bad-lic@0.1.0: expression ends after OR\n");
    assert!(!out.exists());
}

/// AC-2 companion: a native component with a malformed SHA-256 exits 1 with
/// SBOM-NATIVE-DIGEST; nothing is written.
#[test]
fn native_digest_error() {
    let out = out_dir("native-digest");
    let args = ["--plan", "plan.json", "--workspace", "ws", "--native", "native-bad-digest.toml"];
    assert_eq!(cli(&args, &out), (1, "SBOM-NATIVE-DIGEST Fixture Sans@1.0\n".to_string()));
    assert!(!out.exists());
}

/// Bad usage and unusable inputs exit 2.
#[test]
fn input_errors_exit_2() {
    let out = out_dir("input-errors");
    let (code, stdout) = cli(&["--plan", "native-components.toml", "--native", "native-components.toml"], &out);
    assert_eq!(code, 2);
    assert!(stdout.starts_with("SBOM-PLAN-PARSE: "), "{stdout}");
    let (code, stdout) =
        cli(&["--plan", "plan.json", "--workspace", "ws", "--native", "native-components.toml", "--commit-time", "now"], &out);
    assert_eq!(code, 2);
    assert!(stdout.starts_with("SBOM-PLAN-PARSE: commit time"), "{stdout}");
    let o = Command::new(env!("CARGO_BIN_EXE_fid-xtask")).args(["rel", "sbom", "--plan"]).output().unwrap();
    assert_eq!(o.status.code(), Some(2));
    assert!(!out.exists());
}

/// AC-3: the Windows-only dependency appears in the Windows document and not in the Linux one.
#[test]
fn ac3_target_specific_dependency() {
    let out = out_dir("ac3");
    assert_eq!(cli(&ac1_args(), &out).0, 0);
    assert!(!packages(&read_doc(&out, LINUX)).contains_key("fid-winio"));
    let win = packages(&read_doc(&out, WINDOWS));
    assert_eq!(st(&win["fid-winio"]["licenseDeclared"]), "Apache-2.0");
    assert_eq!(win.len(), 6);
}

/// AC-4: two runs give byte-identical documents; across OSes the documents equal the committed
/// reference (checked by AC-1 on every CI OS). The commit-time override is honoured.
#[test]
fn ac4_deterministic() {
    let opts = Options {
        plan: &fixture("plan.json"),
        workspace: &fixture("ws"),
        native: &fixture("native-components.toml"),
        artefacts: None,
        commit_time: None,
    };
    let (a, b) = (out_dir("ac4-a"), out_dir("ac4-b"));
    run(&opts, &a).unwrap();
    run(&opts, &b).unwrap();
    for name in [LINUX, WINDOWS] {
        assert_eq!(std::fs::read(a.join(name)).unwrap(), std::fs::read(b.join(name)).unwrap());
    }
    let c = out_dir("ac4-c");
    let later = Options { commit_time: Some("2027-01-02T03:04:05Z"), ..opts };
    run(&later, &c).unwrap();
    assert_eq!(st(&obj(&obj(&read_doc(&c, LINUX))["creationInfo"])["created"]), "2027-01-02T03:04:05Z");
}

/// AC-5 (partial, QUAL.SBOM.COMPLETE is not built yet): every component of the AC-1 documents
/// carries the fields that leaf's policy requires (name, version, download location, licence
/// expression, package URL, and a checksum whenever the lockfile or native file has one), and
/// exactly one document exists per planned artefact.
#[test]
fn ac5_policy_fields_present() {
    let out = out_dir("ac5");
    assert_eq!(cli(&ac1_args(), &out).0, 0);
    let lock = std::fs::read_to_string(fixture("ws/Cargo.lock")).unwrap();
    for name in [LINUX, WINDOWS] {
        for (pname, p) in packages(&read_doc(&out, name)) {
            if st(&p["SPDXID"]) == "SPDXRef-Artefact" {
                continue;
            }
            for f in ["name", "versionInfo", "downloadLocation", "licenseDeclared", "externalRefs"] {
                assert!(p.contains_key(f), "{name}: {pname} lacks {f}");
            }
            let locked_with_sum = lock.contains(&format!("name = \"{pname}\"\nversion = \"{}\"\nsource", st(&p["versionInfo"])));
            if locked_with_sum || st(&p["SPDXID"]).starts_with("SPDXRef-Native-") {
                assert!(p.contains_key("checksums"), "{name}: {pname} lacks checksums");
            }
        }
    }
}

/// Coordination with `rel sums`: documents are named `<artefact>.spdx.json`, the name family that
/// `rel sums` leaves out as kind `sbom`.
#[test]
fn names_match_sums_exclusion() {
    assert_eq!(SBOM_SUFFIX, ".spdx.json");
    assert_eq!(unsummed_kind(LINUX), Some(KIND_SBOM));
    assert_eq!(unsummed_kind(WINDOWS), Some(KIND_SBOM));
    assert_eq!(SbomError::Licence { component: "x@1".into(), reason: "r".into() }.exit_code(), 1);
}
