//! Acceptance criteria of spec node PLAT.GPU.FALLBK.INVAR (AC-1 .. AC-4). Recorded digest
//! lists and the expected reports come from `fixtures/plat/gpu/make_invar_fixtures.py`, an
//! independent Python reference of the report rules.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_xtask::plat_gpu_invar::{
    compare, parse_digest_list, parse_sequence, read_recorded, run, Backend, DigestList, Driver, InvarError,
    RunOptions, StepKind, DEFAULT_SEQUENCE,
};
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture(name: &str) -> PathBuf {
    root().join("fixtures/plat/gpu").join(name)
}

/// A fresh scratch folder for one test.
fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fid-invar-test-{name}-{}", std::process::id()));
    if d.exists() {
        std::fs::remove_dir_all(&d).unwrap();
    }
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// An examples root holding the sequence's project with one file.
fn examples(name: &str) -> PathBuf {
    let d = scratch(name);
    let seq = parse_sequence(&std::fs::read_to_string(root().join(DEFAULT_SEQUENCE)).unwrap()).unwrap();
    let p = d.join("examples").join(&seq.project);
    std::fs::create_dir_all(p.join("data")).unwrap();
    std::fs::write(p.join("data/scan.txt"), b"0 0 0\n").unwrap();
    d
}

/// `fid-xtask plat-gpu invariance <args>` from the workspace root; (exit code, stdout).
fn cli(args: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_fid-xtask"))
        .current_dir(root())
        .args(["plat-gpu", "invariance"])
        .args(args)
        .output()
        .unwrap();
    (out.status.code().unwrap(), String::from_utf8(out.stdout).unwrap())
}

/// A driver that answers with the recorded lists of a fixture folder, after checking that it
/// got a fresh project copy; `pretend` makes it report another backend than requested.
struct Recorded {
    dir: PathBuf,
    pretend: Option<Backend>,
    drop_command: Option<&'static str>,
    calls: Vec<Backend>,
}

impl Recorded {
    fn new(dir: &str) -> Recorded {
        Recorded { dir: fixture(dir), pretend: None, drop_command: None, calls: Vec::new() }
    }
}

impl Driver for Recorded {
    fn run(&mut self, backend: Backend, project: &Path, sequence: &Path) -> Result<DigestList, InvarError> {
        assert!(project.join("data/scan.txt").is_file(), "project copied");
        assert!(project.ends_with(Path::new(backend.name()).join("ex-inspect-scan-to-cad")));
        assert!(sequence.is_file());
        self.calls.push(backend);
        let b = self.pretend.unwrap_or(backend);
        let mut list = parse_digest_list(&std::fs::read(self.dir.join(format!("{}.json", b.name()))).unwrap())?;
        if let Some(c) = self.drop_command {
            list.entries.retain(|e| e.command.as_str() != c);
        }
        Ok(list)
    }
}

fn opts<'a>(d: &'a Path, seq: &'a Path, backends: &'a [Backend], os: &'a str, work: &'a Path) -> RunOptions<'a> {
    RunOptions { sequence: seq, examples: d, work, backends, host_os: os }
}

/// REQ-001: the shipped sequence has a pick, a lasso, a fit, an evaluation and a report.
#[test]
fn req1_shipped_sequence() {
    let seq = parse_sequence(&std::fs::read_to_string(root().join(DEFAULT_SEQUENCE)).unwrap()).unwrap();
    for k in StepKind::ALL {
        assert!(seq.steps.iter().any(|s| s.kind == k), "{k:?}");
    }
    assert_eq!(seq.project, "ex-inspect-scan-to-cad");
}

/// REQ-001: a sequence without a lasso (or with a mislabelled command) is refused.
#[test]
fn req1_incomplete_sequence_is_refused() {
    let full = std::fs::read_to_string(root().join(DEFAULT_SEQUENCE)).unwrap();
    let no_lasso = full.replace("kind = \"lasso\"", "kind = \"pick\"");
    let e = parse_sequence(&no_lasso).unwrap_err();
    assert!(matches!(&e, InvarError::Input(m) if m.contains("pick")), "{e}");
    let no_fit = full.replace("kind = \"fit\"", "kind = \"report\"");
    assert!(parse_sequence(&no_fit).is_err());
}

/// AC-1 (stand-in until a GPU runner and the app exist): three backends that journal equal
/// digests pass, each in its own fresh project copy, in the order given.
#[test]
fn ac1_equal_lists_pass() {
    let d = examples("ac1");
    let seq = root().join(DEFAULT_SEQUENCE);
    let backends = [Backend::Dx12, Backend::Vulkan, Backend::Software];
    let work = d.join("work");
    let mut driver = Recorded::new("invar-digests-equal");
    let report = run(&opts(&d.join("examples"), &seq, &backends, "windows", &work), &mut driver).unwrap();
    assert!(report.equal, "{}", report.render());
    assert_eq!(driver.calls, backends);
    assert!(report.error().is_none());
    // The recorded form of the same lists, through the command.
    let (code, out) = cli(&["--recorded", "fixtures/plat/gpu/invar-digests-equal"]);
    let expected = std::fs::read_to_string(fixture("invar-digests-equal/expected-report.txt")).unwrap();
    assert_eq!((code, out.as_str()), (0, expected.as_str()));
}

/// AC-2: three recorded lists, one differing at entry 4: exit 1 naming entry 4 and its command.
#[test]
fn ac2_diverged_lists_fail_naming_entry() {
    let (code, out) = cli(&["--recorded", "fixtures/plat/gpu/invar-digests-diff"]);
    let expected = std::fs::read_to_string(fixture("invar-digests-diff/expected-report.txt")).unwrap();
    assert_eq!(code, 1);
    assert_eq!(out, expected);
    assert!(out.contains("first difference: entry 4 command fit.ls_cylinder@1\n"));
    assert!(out.contains("differing backends: software\n"));
    let lists = read_recorded(&fixture("invar-digests-diff")).unwrap();
    assert_eq!(
        compare(&lists).error(),
        Some(InvarError::Diverged { index: 4, command: "fit.ls_cylinder@1".into(), backends: vec!["software".into()] })
    );
}

/// AC-3: `metal` on Windows is BackendUnavailable, before anything runs; never substituted.
#[test]
fn ac3_metal_on_windows_is_unavailable() {
    let d = examples("ac3");
    let seq = root().join(DEFAULT_SEQUENCE);
    let mut driver = Recorded::new("invar-digests-equal");
    let backends = [Backend::Metal, Backend::Software];
    let e = run(&opts(&d.join("examples"), &seq, &backends, "windows", &d.join("work")), &mut driver).unwrap_err();
    assert!(e.to_string().starts_with("BackendUnavailable { backend: metal }"), "{e}");
    assert!(driver.calls.is_empty());
    // Through the command on this host: a backend the host cannot offer.
    let foreign = if cfg!(target_os = "macos") { "dx12" } else { "metal" };
    let (code, out) = cli(&["--backends", &format!("{foreign},software")]);
    assert_eq!(code, 1, "{out}");
    assert!(out.starts_with(&format!("BackendUnavailable {{ backend: {foreign} }}")), "{out}");
}

/// AC-4: the AC-2 report is byte-identical across runs.
#[test]
fn ac4_report_is_byte_identical() {
    let a = cli(&["--recorded", "fixtures/plat/gpu/invar-digests-diff"]);
    let b = cli(&["--recorded", "fixtures/plat/gpu/invar-digests-diff"]);
    assert_eq!(a, b);
    let lists = read_recorded(&fixture("invar-digests-diff")).unwrap();
    assert_eq!(compare(&lists).render(), compare(&lists).render());
}

/// REQ-004: a run that reports another backend than requested is refused, not compared.
#[test]
fn req4_substitution_is_refused() {
    let d = examples("subst");
    let seq = root().join(DEFAULT_SEQUENCE);
    let mut driver = Recorded::new("invar-digests-equal");
    driver.pretend = Some(Backend::Software);
    let backends = [Backend::Vulkan, Backend::Software];
    let e = run(&opts(&d.join("examples"), &seq, &backends, "linux", &d.join("work")), &mut driver).unwrap_err();
    assert!(matches!(&e, InvarError::BackendUnavailable { backend, .. } if backend == "vulkan"), "{e}");
}

/// Fail closed today: the current `fid` has no `--gpu-backend` and exits 2, which is
/// BackendUnavailable, not a pass. (The xtask binary itself stands in for such a `fid`.)
#[test]
fn driver_without_gpu_backend_fails_closed() {
    let d = examples("nofid");
    let ex = d.join("examples");
    let work = d.join("work");
    let fid = env!("CARGO_BIN_EXE_fid-xtask");
    let (code, out) = cli(&[
        "--backends",
        "software,reduced",
        "--examples",
        ex.to_str().unwrap(),
        "--work",
        work.to_str().unwrap(),
        "--fid",
        fid,
    ]);
    assert_eq!(code, 1, "{out}");
    assert!(out.starts_with("BackendUnavailable { backend: software }: fid exited 2"), "{out}");
}

/// No vacuous pass: a missing example project, a journal lacking a step, a reused work
/// folder and a single backend all fail.
#[test]
fn no_vacuous_pass() {
    let seq = root().join(DEFAULT_SEQUENCE);
    let backends = [Backend::Software, Backend::Reduced];
    let empty = scratch("noproject");
    let mut driver = Recorded::new("invar-digests-equal");
    let e = run(&opts(&empty, &seq, &backends, "linux", &empty.join("work")), &mut driver).unwrap_err();
    assert!(matches!(e, InvarError::SequenceFailed { backend: None, .. }), "{e}");

    let d = examples("nostep");
    let backends = [Backend::Dx12, Backend::Software];
    let mut driver = Recorded::new("invar-digests-equal");
    driver.drop_command = Some("check.eval_size@1");
    let e = run(&opts(&d.join("examples"), &seq, &backends, "windows", &d.join("work")), &mut driver).unwrap_err();
    assert!(e.to_string().contains("check.eval_size@1"), "{e}");
    assert!(matches!(e, InvarError::SequenceFailed { backend: Some(_), .. }));

    // The work folder from the previous run is not fresh any more.
    let mut driver = Recorded::new("invar-digests-equal");
    let e = run(&opts(&d.join("examples"), &seq, &backends, "windows", &d.join("work")), &mut driver).unwrap_err();
    assert!(matches!(e, InvarError::Input(_)), "{e}");

    let (code, _) = cli(&["--backends", "software"]);
    assert_eq!(code, 2);
    let (code, _) = cli(&["--recorded", "fixtures/plat/gpu/invar-digests-diff", "--backends", "dx12,software"]);
    assert_eq!(code, 2);
}
