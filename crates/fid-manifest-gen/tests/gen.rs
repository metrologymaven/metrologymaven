//! Acceptance criteria of spec node KRN.MAN.MANI.GEN (AC-1, AC-3, AC-4 and the typed errors)
//! at library level. Every expected file comes from `fixtures/man/gen-toy-v1/make_fixtures.py`
//! (Python hashlib, json and tomllib, and the pure-Python KRN.MAN.MANI.IMPL oracle), never from
//! this crate. The runner path (`fid-xart` and the pinned WASM runner, AC-2) runs in CI through
//! `scripts/manifest-gen-ci.sh`; here the self-report is supplied by fakes that behave like the
//! runner's table.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};

use fid_kci::kci::sha256_digest;
use fid_kci::Digest;
use fid_manifest_gen::gen::{
    self, expected_answer, field, parse_artifacts, parse_xart_table, render_constant, self_report_call_set,
    ArtifactSpec, Config, GenError, SelfReport, SelfReporter,
};

fn toy() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/man/gen-toy-v1")
}

fn read(rel: &str) -> Vec<u8> {
    fs::read(toy().join(rel)).unwrap()
}

fn core_digest() -> Digest {
    Digest::parse(String::from_utf8(read("expected-core-digest.txt")).unwrap().trim_end()).unwrap()
}

/// A fresh copy of the toy fixture under the test temp directory.
fn copy(name: &str) -> PathBuf {
    fn walk(from: &Path, to: &Path) {
        fs::create_dir_all(to).unwrap();
        for e in fs::read_dir(from).unwrap() {
            let e = e.unwrap();
            let target = to.join(e.file_name());
            if e.file_type().unwrap().is_dir() {
                walk(&e.path(), &target);
            } else {
                fs::copy(e.path(), &target).unwrap();
            }
        }
    }
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("gen-toy").join(name);
    if dir.exists() {
        fs::remove_dir_all(&dir).unwrap();
    }
    walk(&toy(), &dir);
    dir
}

fn core_in(dir: &Path) -> Result<gen::StageOne, GenError> {
    gen::core(&Config::load(&dir.join("manifest-gen.toml")).unwrap())
}

#[test]
fn ac1_stage_one_equals_the_expected_core_and_constant() {
    let stage = core_in(&toy()).unwrap();
    assert_eq!(stage.manifest.canonical_json(), read("expected-core.json").as_slice());
    assert_eq!(stage.manifest.core_digest(), &core_digest());
    assert_eq!(stage.constant, read("expected-constant.rs"));
    assert_eq!(stage.constant_path, "shim/src/manifest_core_digest.rs");
    assert!(stage.manifest.artifacts().is_empty());
}

/// AC-4 (first half) for stage one, and no circularity: two copies in different directories
/// give the same bytes, and writing the generated constant into the workspace (as a release
/// build does) changes nothing in the core.
#[test]
fn ac4_stage_one_is_identical_across_directories_and_after_embedding() {
    let a = copy("ac4-a");
    let b = copy("ac4-b");
    let sa = core_in(&a).unwrap();
    let sb = core_in(&b).unwrap();
    assert_eq!(sa.manifest.canonical_json(), sb.manifest.canonical_json());
    assert_eq!(sa.constant, sb.constant);
    fs::write(a.join("ws").join(&sa.constant_path), sa.constant.as_slice()).unwrap();
    let again = core_in(&a).unwrap();
    assert_eq!(again.manifest.canonical_json(), read("expected-core.json").as_slice());
}

/// AC-4 (error path): a missing RDL release file is `MissingSource{reference_suite_digest}`;
/// every other source names its own field.
#[test]
fn ac4_missing_sources_name_their_field() {
    let cases = [
        ("rdl-release.json", field::REFERENCE_SUITE),
        ("solver-limits.json", field::SOLVER_LIMITS),
        ("toolchain-record.json", field::TOOLCHAIN),
        ("wasm-runtime.toml", field::REPLAY_RUNTIME),
        ("native-gate.toml", field::NATIVE_GATE),
        ("cards/geo-point-dist.toml", field::ALGORITHMS),
        ("metadata.json", field::ALGORITHMS),
        ("tracked-files.txt", field::SOURCE_TREE),
    ];
    for (i, (file, f)) in cases.iter().enumerate() {
        let dir = copy(&format!("missing-{i}"));
        fs::remove_file(dir.join(file)).unwrap();
        let e = core_in(&dir).unwrap_err();
        assert_eq!(e, GenError::MissingSource { field: *f }, "{file}");
        assert_eq!(e.code(), "missing_source");
    }
    let dir = copy("missing-rdl-key");
    let cfg = String::from_utf8(read("manifest-gen.toml")).unwrap().replace("rdl_release = \"rdl-release.json\"\n", "");
    fs::write(dir.join("manifest-gen.toml"), cfg).unwrap();
    assert_eq!(core_in(&dir).unwrap_err().to_string(), "missing_source: reference_suite_digest");
}

fn edit_config(name: &str, from: &str, to: &str) -> PathBuf {
    let dir = copy(name);
    let cfg = String::from_utf8(read("manifest-gen.toml")).unwrap();
    assert!(cfg.contains(from), "{from}");
    fs::write(dir.join("manifest-gen.toml"), cfg.replace(from, to)).unwrap();
    dir
}

#[test]
fn constant_inside_an_implementation_closure_is_refused() {
    let dir = edit_config(
        "constant-in-closure",
        "constant = \"shim/src/manifest_core_digest.rs\"",
        "constant = \"crates/knum/src/lib.rs\"",
    );
    assert_eq!(
        core_in(&dir).unwrap_err(),
        GenError::ConstantInClosure { constant: "crates/knum/src/lib.rs".to_string() }
    );
}

#[test]
fn malformed_sources_are_refused() {
    // A solver-limits file that is not canonical (a trailing newline).
    let dir = copy("limits-newline");
    let mut limits = read("solver-limits.json");
    limits.push(b'\n');
    fs::write(dir.join("solver-limits.json"), limits).unwrap();
    assert!(matches!(core_in(&dir).unwrap_err(), GenError::SourceInvalid { field: "solver_limits_digest", .. }));
    // A toolchain record for another lockfile.
    let dir = copy("record-lockfile");
    let lock = sha256_digest(&read("ws/Cargo.lock"));
    let other = sha256_digest(b"another Cargo.lock");
    let record = String::from_utf8(read("toolchain-record.json")).unwrap().replace(lock.as_str(), other.as_str());
    fs::write(dir.join("toolchain-record.json"), record).unwrap();
    assert!(matches!(core_in(&dir).unwrap_err(), GenError::SourceInvalid { field: "toolchain_digest", .. }));
    // An RDL release file without results_digest.
    let dir = copy("rdl-no-results");
    fs::write(
        dir.join("rdl-release.json"),
        br#"{"suite_digest":"sha256:a30442beb62ae2865d28bbad88d059682d6c77ec797c4dbad2ec56f1de9aded4"}"#,
    )
    .unwrap();
    assert!(matches!(core_in(&dir).unwrap_err(), GenError::SourceInvalid { field: "reference_results_digest", .. }));
    // An unknown native_gate value.
    let dir = copy("gate-unknown");
    fs::write(dir.join("native-gate.toml"), "native_gate = \"always\"\n").unwrap();
    assert!(matches!(core_in(&dir).unwrap_err(), GenError::SourceInvalid { field: "native_gate", .. }));
    // A card for another algorithm.
    let dir = copy("card-other-id");
    fs::copy(dir.join("cards/fit-plane-lsq.toml"), dir.join("cards/geo-point-dist.toml")).unwrap();
    assert!(matches!(core_in(&dir).unwrap_err(), GenError::SourceInvalid { field: "algorithms", .. }));
    // A name the schema refuses.
    let dir = edit_config("bad-name", "name = \"toy-kernel\"", "name = \"Toy\"");
    assert!(matches!(core_in(&dir).unwrap_err(), GenError::SchemaInvalid(_)));
    // Unknown configuration keys.
    let dir = edit_config("unknown-key", "schema = \"1\"", "schema = \"1\"\nextra = \"x\"");
    let e = Config::load(&dir.join("manifest-gen.toml")).unwrap_err();
    assert!(matches!(e, GenError::SourceInvalid { field: "config", .. }), "{e}");
}

#[test]
fn committed_constants_are_the_generated_none_form() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let kernel = fs::read(repo.join("crates/fid-kernel-wasm/src/manifest_core_digest.rs")).unwrap();
    assert_eq!(kernel, render_constant(None), "edit by fid-manifest-gen only");
    assert_eq!(read("ws/shim/src/manifest_core_digest.rs"), render_constant(None));
    assert_eq!(render_constant(Some(&core_digest())), read("expected-constant.rs"));
}

#[test]
fn expected_answer_is_the_kernel_envelope() {
    assert_eq!(expected_answer(&core_digest()).unwrap(), read("expected-self-report.json"));
    assert_eq!(
        self_report_call_set(),
        b"# KRN.MAN.MANI.GEN self-report\nmanifest_core_digest\t{\"algorithm\":\"kernel.manifest.core_digest@1.0.0\",\"inputs\":[],\"params\":\"\"}\n"
    );
}

/// Behaves like the `fid-xart` table: the artifact file's SHA-256 and the SHA-256 of `answer`.
struct Fake {
    answer: Result<(u8, Vec<u8>), String>,
    file_digest: Option<Digest>,
    calls: usize,
}

impl SelfReporter for Fake {
    fn report(&mut self, artifact: &ArtifactSpec, call_set: &[u8]) -> Result<SelfReport, String> {
        assert_eq!(call_set, self_report_call_set());
        self.calls += 1;
        let (exit, stdout) = self.answer.clone()?;
        let artifact_sha256 =
            self.file_digest.clone().unwrap_or_else(|| sha256_digest(&fs::read(&artifact.file).unwrap()));
        Ok(SelfReport { artifact_sha256, exit, result_digest: sha256_digest(&stdout) })
    }
}

fn artifacts(dir: &Path, list: &str) -> Vec<ArtifactSpec> {
    parse_artifacts(&fs::read(dir.join(list)).unwrap())
        .unwrap()
        .into_iter()
        .map(|a| ArtifactSpec { file: dir.join(&a.file), ..a })
        .collect()
}

#[test]
fn ac1_finish_equals_the_expected_manifest() {
    let list = artifacts(&toy(), "artifacts.toml");
    assert_eq!(list.len(), 1);
    assert_eq!((list[0].label().as_str(), list[0].kind()), ("wasm-wasm32-wasip1", "wasm"));
    let mut fake = Fake { answer: Ok((0, read("expected-self-report.json"))), file_digest: None, calls: 0 };
    let m = gen::finish(&read("expected-core.json"), &list, &mut fake).unwrap();
    assert_eq!(fake.calls, 1);
    assert_eq!(m.canonical_json(), read("expected-manifest.json").as_slice());
    let digest = String::from_utf8(read("expected-manifest-digest.txt")).unwrap();
    assert_eq!(m.manifest_digest().as_str(), digest.trim_end());
}

/// AC-3: an artifact built from a different core (its answer is the toy module `other-core`'s
/// payload), an artifact without a digest (a kernel error) and an artifact that cannot answer.
#[test]
fn ac3_a_different_core_is_core_digest_mismatch() {
    let list = artifacts(&toy(), "artifacts-other-core.toml");
    let core = read("expected-core.json");
    // The other-core module's stdout: the self-report envelope of a different digest.
    let other = Digest::parse("sha256:0000000000000000000000000000000000000000000000000000000000000001").unwrap();
    let mut fake = Fake { answer: Ok((0, expected_answer(&other).unwrap())), file_digest: None, calls: 0 };
    let e = gen::finish(&core, &list, &mut fake).unwrap_err();
    assert_eq!(e, GenError::CoreDigestMismatch { artifact: "wasm-wasm32-wasip1".to_string() });
    assert_eq!(e.to_string(), "core_digest_mismatch: wasm-wasm32-wasip1");
    let mut unset =
        Fake { answer: Ok((1, br#"{"error":"manifest_core_digest_unset"}"#.to_vec())), file_digest: None, calls: 0 };
    assert_eq!(gen::finish(&core, &list, &mut unset).unwrap_err().code(), "core_digest_mismatch");
    // The right bytes with a kernel-error exit are still a mismatch.
    let mut exit1 = Fake { answer: Ok((1, read("expected-self-report.json"))), file_digest: None, calls: 0 };
    assert_eq!(gen::finish(&core, &list, &mut exit1).unwrap_err().code(), "core_digest_mismatch");
    let mut failed = Fake { answer: Err("timeout".to_string()), file_digest: None, calls: 0 };
    assert_eq!(
        gen::finish(&core, &list, &mut failed).unwrap_err(),
        GenError::SelfReportFailed { artifact: "wasm-wasm32-wasip1".to_string(), reason: "timeout".to_string() }
    );
    // The runner hashed a file other than the one finish hashed.
    let mut swapped = Fake { answer: Ok((0, read("expected-self-report.json"))), file_digest: Some(other), calls: 0 };
    assert_eq!(gen::finish(&core, &list, &mut swapped).unwrap_err().code(), "self_report_failed");
}

#[test]
fn finish_refuses_bad_stage_one_and_artifact_lists() {
    let list = artifacts(&toy(), "artifacts.toml");
    let mut fake = Fake { answer: Ok((0, read("expected-self-report.json"))), file_digest: None, calls: 0 };
    // Not canonical (a trailing newline).
    let mut core = read("expected-core.json");
    core.push(b'\n');
    assert!(matches!(gen::finish(&core, &list, &mut fake).unwrap_err(), GenError::SchemaInvalid(_)));
    // A full manifest is not a stage-one manifest.
    assert!(matches!(
        gen::finish(&read("expected-manifest.json"), &list, &mut fake).unwrap_err(),
        GenError::SourceInvalid { field: "core", .. }
    ));
    assert!(matches!(
        gen::finish(&read("expected-core.json"), &[], &mut fake).unwrap_err(),
        GenError::SourceInvalid { field: "artifacts", .. }
    ));
    // The same artifact twice is refused by the schema (artifacts strictly sorted).
    let twice = vec![list[0].clone(), list[0].clone()];
    assert!(matches!(
        gen::finish(&read("expected-core.json"), &twice, &mut fake).unwrap_err(),
        GenError::SchemaInvalid(_)
    ));
    for bad in [
        "",
        "[[artifact]]\nrole = \"wasm\"\ntarget = \"t\"\nfile = \"f\"\ncommand = []\n",
        "[[artifact]]\nrole = \"wasm\"\ntarget = \"t\"\nfile = \"f\"\ncommand = [\"x\"]\nextra = 1\n",
        "other = 1\n",
    ] {
        assert!(matches!(parse_artifacts(bad.as_bytes()).unwrap_err(), GenError::SourceInvalid { .. }), "{bad:?}");
    }
}

#[test]
fn xart_table_rows_are_read_strictly() {
    let ok = format!(
        "{{\"artifact\":\"wasm-wasm32-wasip1\",\"artifact_sha256\":\"{a}\",\"call_set_digest\":\"{a}\",\"host\":\"manifest-gen\",\
         \"kind\":\"wasm\",\"rows\":[{{\"algorithm\":\"kernel.manifest.core_digest@1.0.0\",\"call\":\"0\",\"exit\":\"0\",\
         \"name\":\"manifest_core_digest\",\"result_digest\":\"{r}\"}}],\"schema\":\"fid-xart-table@1\"}}",
        a = core_digest(),
        r = sha256_digest(b"x"),
    );
    let report = parse_xart_table(ok.as_bytes()).unwrap();
    assert_eq!(report, SelfReport { artifact_sha256: core_digest(), exit: 0, result_digest: sha256_digest(b"x") });
    for (from, to) in [
        ("fid-xart-table@1", "fid-xart-table@2"),
        ("\"exit\":\"0\"", "\"exit\":\"2\""),
        ("manifest_core_digest\"", "other\""),
        ("\"rows\":[", "\"rows\":[{},"),
    ] {
        assert!(parse_xart_table(ok.replace(from, to).as_bytes()).is_err(), "{to}");
    }
}
