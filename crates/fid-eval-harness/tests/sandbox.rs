#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Acceptance tests of spec node AGENT.HARNESS.SANDBOX. Expected digests and file lists come
//! from fixtures/agent/harness/sandbox/make_fixtures.py (hashlib and tarfile), expected error
//! codes are hand-labelled there; nothing expected is computed by this crate.

use fid_canon::{parse, CValue};
use fid_eval_harness::*;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

fn root(rel: &str) -> PathBuf {
    Path::new(ROOT).join(rel)
}

fn suite_task(id: &str) -> PathBuf {
    root("fixtures/agent/eval/suite-mini/tasks").join(id)
}

fn case_dir(name: &str) -> PathBuf {
    root("fixtures/agent/harness/sandbox/cases").join(name)
}

fn expected() -> CValue {
    parse(&std::fs::read(root("fixtures/agent/harness/sandbox/expected.json")).unwrap()).unwrap()
}

fn obj(v: &CValue) -> &BTreeMap<String, CValue> {
    match v {
        CValue::Obj(m) => m,
        other => panic!("expected object, got {other:?}"),
    }
}

fn st(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        other => panic!("expected string, got {other:?}"),
    }
}

fn strs(v: &CValue) -> Vec<String> {
    match v {
        CValue::Arr(a) => a.iter().map(|x| st(x).to_string()).collect(),
        other => panic!("expected array, got {other:?}"),
    }
}

fn pairs(v: &CValue) -> Vec<(String, String)> {
    match v {
        CValue::Arr(a) => a
            .iter()
            .map(|p| {
                let p = strs(p);
                (p[0].clone(), p[1].clone())
            })
            .collect(),
        other => panic!("expected array, got {other:?}"),
    }
}

/// A unique work folder per test, removed on drop.
struct TempWork(PathBuf);

impl TempWork {
    fn new(tag: &str) -> TempWork {
        static N: AtomicUsize = AtomicUsize::new(0);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let n = N.fetch_add(1, Ordering::SeqCst);
        let p = std::env::temp_dir().join(format!("fid-eval-harness-{tag}-{}-{n}-{nanos}", std::process::id()));
        std::fs::create_dir(&p).unwrap();
        TempWork(p)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn run_dirs(&self) -> Vec<String> {
        match std::fs::read_dir(self.0.join("run")) {
            Ok(rd) => rd.map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect(),
            Err(_) => Vec::new(),
        }
    }
}

impl Drop for TempWork {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn file_list(s: &FsSnapshot) -> Vec<(String, String)> {
    s.files().into_iter().map(|(p, d)| (p.to_string(), d.as_str().to_string())).collect()
}

#[test]
fn ac1_project_matches_start_state_and_roots_are_exactly_the_tasks() {
    let exp = expected();
    for (tid, e) in obj(&obj(&exp)["tasks"]) {
        let e = obj(e);
        let work = TempWork::new("ac1");
        let task = TaskSpec::read(&suite_task(tid)).unwrap();
        assert_eq!(task.start_digest.as_str(), st(&e["task_digest"]), "{tid}");
        let sb = Sandbox::build(work.path(), &task).unwrap();

        // Start state: verified digest and the unpacked files (Python tarfile listing).
        assert_eq!(sb.start_digest().as_str(), st(&e["start_digest"]), "{tid}");
        assert_eq!(sb.start_digest(), task.start_digest);
        assert_eq!(file_list(&FsSnapshot::take(sb.project()).unwrap()), pairs(&e["files"]), "{tid}");

        // Run directory layout and UUIDv7 run ID.
        assert_eq!(sb.run_id().version(), 7);
        assert_eq!(sb.run_id().variant_bits(), 0b10);
        assert_eq!(sb.dir(), work.path().join("run").join(sb.run_id().to_string()));
        assert_eq!(work.run_dirs(), vec![sb.run_id().to_string()]);
        for d in ["project", "home", "artifacts", "roots/read", "roots/write"] {
            assert!(sb.dir().join(d).is_dir(), "{tid}: {d}");
        }

        // Roots: exactly the task's, each inside the run directory.
        let read = strs(&e["read"]);
        let write = strs(&e["write"]);
        let decl = |v: &[String], kind: &str| -> Vec<RootDecl> {
            v.iter().map(|r| RootDecl { root: r.clone(), dir: format!("roots/{kind}/{r}") }).collect()
        };
        assert_eq!(sb.read_roots(), decl(&read, "read").as_slice());
        assert_eq!(sb.write_roots(), decl(&write, "write").as_slice());
        for w in &write {
            let p = sb.write_root(w).unwrap();
            assert!(p.starts_with(sb.dir()));
            assert_eq!(p.read_dir().unwrap().count(), 0, "{tid}: write root {w} starts empty");
        }
        for r in &read {
            assert!(sb.read_root(r).unwrap().is_dir());
        }
        let list = |v: &[String], kind: &str| {
            v.iter().map(|r| format!("{{\"dir\":\"roots/{kind}/{r}\",\"root\":\"{r}\"}}")).collect::<Vec<_>>().join(",")
        };
        let json = format!(
            "{{\"format\":\"fid-sandbox-roots@1\",\"read\":[{}],\"write\":[{}]}}",
            list(&read, "read"),
            list(&write, "write")
        );
        assert_eq!(std::fs::read_to_string(sb.dir().join("roots.json")).unwrap(), json);

        // Granted inputs, and nothing else from the suite, are in the run directory.
        let all = sb.snapshot().unwrap();
        let in_roots: Vec<(String, String)> =
            file_list(&all).into_iter().filter(|(p, _)| p.starts_with("roots/")).collect();
        assert_eq!(in_roots, pairs(&e["read_root_files"]), "{tid}");
        for (p, _) in file_list(&all) {
            assert!(p.starts_with("project/") || p.starts_with("roots/read/") || p == "roots.json", "{tid}: {p}");
        }
        sb.remove().unwrap();
        assert!(work.run_dirs().is_empty());
    }
}

#[test]
fn ac2_and_fail_closed_cases_create_no_run_directory() {
    let exp = expected();
    let cases = obj(&obj(&exp)["cases"]);
    assert!(cases.len() >= 20);
    assert_eq!(st(&cases["altered"]), "start_state_mismatch");
    for (name, code) in cases {
        let work = TempWork::new("case");
        let task = TaskSpec::read(&case_dir(name)).unwrap();
        let e = Sandbox::build(work.path(), &task).expect_err(name);
        assert_eq!(e.code.as_str(), st(code), "{name}: {e}");
        assert!(work.run_dirs().is_empty(), "{name}: run directory left behind");
    }
}

#[test]
fn req1_fresh_run_ids_and_no_reuse() {
    let work = TempWork::new("req1");
    let task = TaskSpec::read(&suite_task("mini-inspect-01")).unwrap();
    let a = Sandbox::build(work.path(), &task).unwrap();
    let b = Sandbox::build(work.path(), &task).unwrap();
    assert_ne!(a.run_id(), b.run_id());
    assert_ne!(a.dir(), b.dir());
    let before = a.snapshot().unwrap();
    let e = Sandbox::build_with_run_id(work.path(), &task, a.run_id()).unwrap_err();
    assert_eq!(e.code, ErrorCode::RunDirExists);
    assert_eq!(a.snapshot().unwrap(), before, "the existing run directory is untouched");
    assert_eq!(work.run_dirs().len(), 2);
}

#[test]
fn bad_work_folders_are_refused() {
    let task = TaskSpec::read(&suite_task("mini-inspect-01")).unwrap();
    let e = Sandbox::build(Path::new("relative/work"), &task).unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidWorkDir);
    let work = TempWork::new("badwork");
    let missing = work.path().join("missing");
    assert_eq!(Sandbox::build(&missing, &task).unwrap_err().code, ErrorCode::InvalidWorkDir);
    let outside = TempWork::new("badwork-target");
    make_dir_link(&work.path().join("run"), outside.path());
    assert_eq!(Sandbox::build(work.path(), &task).unwrap_err().code, ErrorCode::InvalidWorkDir);
    assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0, "nothing written through the link");
}

/// Stands in for `fid` reading its user configuration: looks in the platform's standard
/// configuration folders named by its own environment and prints what it finds. It runs only
/// as the child process of `ac3_private_home_hides_user_config`.
#[test]
#[ignore = "child process of ac3_private_home_hides_user_config"]
fn child_config_probe() {
    if std::env::var_os("FID_HARNESS_PROBE").is_none() {
        return;
    }
    let mut candidates = Vec::new();
    if let Some(x) = std::env::var_os("XDG_CONFIG_HOME") {
        candidates.push(PathBuf::from(x).join("fid").join("config.toml"));
    }
    for key in ["HOME", "USERPROFILE"] {
        if let Some(h) = std::env::var_os(key) {
            let h = PathBuf::from(h);
            candidates.push(h.join(".config").join("fid").join("config.toml"));
            candidates.push(h.join("Library").join("Application Support").join("fid").join("config.toml"));
        }
    }
    if let Some(a) = std::env::var_os("APPDATA") {
        candidates.push(PathBuf::from(a).join("fid").join("config.toml"));
    }
    for c in candidates {
        println!("CANDIDATE {}", c.display());
        if let Ok(text) = std::fs::read_to_string(c.as_path()) {
            println!("READ {}", text.trim());
        }
    }
    for (k, v) in std::env::vars() {
        println!("ENV {k}={v}");
    }
}

fn run_probe(mut cmd: Command) -> String {
    let out = cmd
        .args(["child_config_probe", "--exact", "--ignored", "--nocapture", "--test-threads=1"])
        .env("FID_HARNESS_PROBE", "1")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(out.status.success(), "probe failed: {text}\n{}", String::from_utf8_lossy(&out.stderr));
    assert!(text.contains("CANDIDATE "), "probe did not run: {text}");
    text
}

#[test]
fn ac3_private_home_hides_user_config() {
    // A "real" home folder with a user config that would add a root.
    let real = TempWork::new("realhome");
    let marker = "roots = [\"leaked-user-root\"]";
    for rel in [".config/fid", "AppData/Roaming/fid", "Library/Application Support/fid"] {
        let d = real.path().join(rel);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("config.toml"), marker).unwrap();
    }
    let work = TempWork::new("ac3");
    let task = TaskSpec::read(&suite_task("mini-inspect-01")).unwrap();
    let sb = Sandbox::build(work.path(), &task).unwrap();
    let exe = std::env::current_exe().unwrap();

    // Control: the probe does find the config when pointed at the real home.
    let mut control = sb.command(exe.as_os_str());
    control
        .env("HOME", real.path())
        .env("USERPROFILE", real.path())
        .env("XDG_CONFIG_HOME", real.path().join(".config"))
        .env("APPDATA", real.path().join("AppData").join("Roaming"));
    assert!(run_probe(control).contains("leaked-user-root"), "control probe must see the user config");

    // Inside the sandbox environment it does not.
    let text = run_probe(sb.command(exe.as_os_str()));
    assert!(!text.contains("leaked-user-root"), "{text}");
    let dir = sb.dir().to_str().unwrap();
    for line in text.lines() {
        if let Some(c) = line.strip_prefix("CANDIDATE ") {
            assert!(c.starts_with(dir), "config candidate outside the run directory: {c}");
        }
    }
}

#[test]
fn env_is_exactly_private_folders_and_start_variables() {
    let work = TempWork::new("env");
    let task = TaskSpec::read(&suite_task("mini-probe-01")).unwrap();
    let sb = Sandbox::build(work.path(), &task).unwrap();
    let private = [
        "APPDATA",
        "HOME",
        "LOCALAPPDATA",
        "TEMP",
        "TMP",
        "TMPDIR",
        "USERPROFILE",
        "XDG_CACHE_HOME",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_STATE_HOME",
    ];
    let start = ["PATH", "SystemRoot", "SystemDrive", "windir", "ComSpec", "PATHEXT"];
    let keys: Vec<&str> = sb.env().iter().map(|(k, _)| k.as_str()).collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted, "sorted by name");
    for k in private {
        assert!(keys.contains(&k), "{k} missing");
    }
    for (k, v) in sb.env() {
        if private.contains(&k.as_str()) {
            let p = Path::new(v);
            assert!(p.starts_with(sb.home()), "{k}={v}");
            assert!(p.is_dir(), "{k}={v}");
        } else {
            assert!(start.contains(&k.as_str()), "unexpected variable {k}");
        }
    }
}

#[test]
fn ac4_snapshot_diff_lists_exactly_the_written_file() {
    let work = TempWork::new("ac4");
    let task = TaskSpec::read(&suite_task("mini-inspect-01")).unwrap();
    let sb = Sandbox::build(work.path(), &task).unwrap();
    let before = sb.snapshot().unwrap();
    assert!(!before.entries.is_empty());
    // The fixture agent writes out/report.csv into its write root.
    std::fs::write(sb.write_root("out").unwrap().join("report.csv"), "characteristic,outcome\n").unwrap();
    let after = sb.snapshot().unwrap();
    assert_eq!(
        before.diff(&after),
        vec![Change { path: "roots/write/out/report.csv".to_string(), kind: ChangeKind::Added }]
    );
    // Changed and removed files are reported too.
    std::fs::write(sb.project().join("README.txt"), "changed\n").unwrap();
    std::fs::remove_file(sb.read_root("in").unwrap().join("part.qif")).unwrap();
    let later = sb.snapshot().unwrap();
    assert_eq!(
        after.diff(&later),
        vec![
            Change { path: "project/README.txt".to_string(), kind: ChangeKind::Modified },
            Change { path: "roots/read/in/part.qif".to_string(), kind: ChangeKind::Removed },
        ]
    );
    assert!(later.diff(&later).is_empty());
}

/// A directory symbolic link (Unix) or junction (Windows, needs no privilege).
fn make_dir_link(link: &Path, target: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).unwrap();
    #[cfg(windows)]
    {
        // cmd reads `/x` inside a path as a switch, so pass backslash-only paths.
        let native = |p: &Path| p.to_string_lossy().replace('/', "\\");
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", "mklink", "/J"]).arg(native(link)).arg(native(target));
        let st = cmd.output().unwrap();
        assert!(st.status.success(), "mklink /J failed: {}", String::from_utf8_lossy(&st.stderr));
    }
}

#[test]
fn links_are_recorded_never_followed_and_never_resolved_through() {
    let outside = TempWork::new("outside");
    std::fs::write(outside.path().join("secret.txt"), "outside\n").unwrap();
    let work = TempWork::new("links");
    let task = TaskSpec::read(&suite_task("mini-inspect-01")).unwrap();
    let sb = Sandbox::build(work.path(), &task).unwrap();
    make_dir_link(&sb.write_root("out").unwrap().join("escape"), outside.path());

    let snap = sb.snapshot().unwrap();
    let link: Vec<&SnapEntry> = snap.entries.iter().filter(|e| e.path.starts_with("roots/write/out/")).collect();
    assert_eq!(link.len(), 1);
    assert_eq!(link[0].path, "roots/write/out/escape");
    assert_eq!(link[0].kind, EntryKind::Link);
    assert!(snap.entries.iter().all(|e| !e.path.contains("secret")));

    let e = sb.resolve("roots/write/out/escape/secret.txt").unwrap_err();
    assert_eq!(e.code, ErrorCode::PathEscape);
    assert_eq!(sb.resolve("roots/write/out/escape").unwrap_err().code, ErrorCode::PathEscape);

    // Removing the run directory removes the link, not what it points to.
    sb.remove().unwrap();
    assert_eq!(std::fs::read_to_string(outside.path().join("secret.txt")).unwrap(), "outside\n");
}

#[test]
fn resolve_stays_inside_the_run_directory() {
    let work = TempWork::new("resolve");
    let task = TaskSpec::read(&suite_task("mini-inspect-01")).unwrap();
    let sb = Sandbox::build(work.path(), &task).unwrap();
    for bad in [
        "..",
        "../x",
        "roots/../../x",
        "roots/write/out/../../..",
        "/etc/passwd",
        "C:/Windows",
        "C:x",
        "roots\\write",
        "roots//write",
        "roots/write/out/",
        "roots/write/out/report.csv:stream",
        "roots/write/out/x.",
        "roots/write/out/NUL",
        "roots/write/out/com1.txt",
        "PROGRA~1",
        "",
    ] {
        assert_eq!(sb.resolve(bad).unwrap_err().code, ErrorCode::PathEscape, "{bad:?}");
    }
    let ok = sb.resolve("roots/write/out/new/report.csv").unwrap();
    assert!(ok.starts_with(sb.dir()));
    let e = sb.resolve("project/README.txt/x").unwrap_err();
    assert_eq!(e.code, ErrorCode::PathEscape, "through a file");
}

#[test]
fn check_rel_rules() {
    for ok in ["a", "in/part.qif", "out/report", "A-b_c.d/e"] {
        assert!(check_rel(ok).is_ok(), "{ok}");
    }
    for bad in ["", "/a", "a/", "a//b", ".", "..", "a/..", "a\\b", "a:b", "a.", "aux", "Lpt9.log", "é"] {
        assert!(check_rel(bad).is_err(), "{bad:?}");
    }
    assert!(check_rel(&"a".repeat(256)).is_err());
    assert!(check_rel("com10").is_ok(), "only COM0 to COM9 are device names");
}

#[test]
fn task_spec_reads_only_what_it_needs_and_fails_closed() {
    let dir = Path::new("tasks/x");
    let ok = br#"{"id":"t","start_state":{"path":"s.tar","digest":"sha256:0000000000000000000000000000000000000000000000000000000000000000"},"roots":{"read":["in"],"write":[]},"extra":1}"#;
    let t = TaskSpec::parse(ok, dir).unwrap();
    assert_eq!(t.start_archive, dir.join("s.tar"));
    assert_eq!(t.read_roots, vec!["in".to_string()]);
    assert!(t.write_roots.is_empty());
    let bads: [&[u8]; 7] = [
        br#"{"id":"t"}"#,
        br#"[]"#,
        br#"{"id":"t","start_state":{"path":"../s.tar","digest":"sha256:0000000000000000000000000000000000000000000000000000000000000000"},"roots":{"read":[],"write":[]}}"#,
        br#"{"id":"t","start_state":{"path":"s.tar","digest":"md5:00"},"roots":{"read":[],"write":[]}}"#,
        br#"{"id":"t","start_state":{"path":"s.tar","digest":"sha256:0000000000000000000000000000000000000000000000000000000000000000"},"roots":{"read":[1],"write":[]}}"#,
        br#"{"id":"t","id":"u"}"#,
        b"not json",
    ];
    for bad in bads {
        assert_eq!(TaskSpec::parse(bad, dir).unwrap_err().code, ErrorCode::InvalidTask);
    }
}

/// REQ-006: the crate opens no network connection and starts no process itself (it only
/// hands a prepared `Command` to its caller), so it cannot run promotion, adoption or
/// release commands.
#[test]
fn req6_no_network_and_no_process_launch_in_the_library() {
    let src = root("crates/fid-eval-harness/src");
    let mut n = 0;
    for e in src.read_dir().unwrap() {
        let p = e.unwrap().path();
        let text = std::fs::read_to_string(&p).unwrap();
        for needle in ["std::net", "TcpStream", "UdpSocket", "TcpListener", ".spawn(", ".output(", ".status("] {
            assert!(!text.contains(needle), "{}: contains {needle}", p.display());
        }
        n += 1;
    }
    assert!(n >= 5);
    let manifest = std::fs::read_to_string(root("crates/fid-eval-harness/Cargo.toml")).unwrap();
    let deps: Vec<&str> = manifest
        .split("[dependencies]")
        .nth(1)
        .unwrap()
        .split("\n[")
        .next()
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .map(|l| l.split('=').next().unwrap().trim())
        .collect();
    assert_eq!(deps, ["fid-canon", "fid-types", "fid-uuid", "getrandom", "sha2"]);
}
