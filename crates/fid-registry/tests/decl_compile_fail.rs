//! CORE.REG.DEF.MACRO AC-2: compile-fail cases. A small harness instead of a new dependency:
//! it builds a scratch crate that depends on this `fid-registry` by path, compiles
//! `fixtures/core/reg/def/macro/demo_ok/lib.rs` as its library (the control: it must build),
//! then checks each `fixtures/core/reg/def/macro/fail/<case>.rs` as an example and requires
//! that it fails with the message on the case's first line (`// expect: <text>`).
//!
//! Messages are matched as substrings of the compiler's output, so the test does not depend
//! on the exact layout of rustc diagnostics across toolchain versions. Cargo runs offline
//! with a copy of the workspace `Cargo.lock`, so it uses exactly the pinned dependencies
//! (CI fetches them first). Each nested `cargo` is killed after [`CARGO_TIMEOUT`] and the
//! test then fails saying so.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const CASES: [&str; 13] = [
    "f64_without_quantity",
    "f64_alias_without_quantity",
    "result_type_mismatch",
    "params_not_fidschema",
    "missing_side_effect",
    "missing_web",
    "default_type_mismatch",
    "default_out_of_range",
    "path_like_on_number",
    "path_like_on_list",
    "extensions_without_path_like",
    "extensions_unsorted",
    "extension_with_dot",
];

/// Longest one nested `cargo check` may run before it is killed and the test fails.
const CARGO_TIMEOUT: Duration = Duration::from_secs(300);

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn cargo(dir: &Path, args: &[&str]) -> (bool, String) {
    let mut child = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string()))
        .args(args)
        .args(["--offline", "--color", "never", "--quiet", "--target-dir"])
        .arg(dir.join("target"))
        .current_dir(dir)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("cargo runs");
    // Drain both pipes on threads so a full pipe cannot stall cargo.
    let drain = |mut r: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut s = Vec::new();
            let _ = r.read_to_end(&mut s);
            String::from_utf8_lossy(&s).into_owned()
        })
    };
    let out = drain(Box::new(child.stdout.take().expect("stdout")));
    let err = drain(Box::new(child.stderr.take().expect("stderr")));
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().expect("wait for cargo") {
            break status;
        }
        if start.elapsed() > CARGO_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            panic!("nested `cargo {}` did not finish within {} s; killed", args.join(" "), CARGO_TIMEOUT.as_secs());
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    let text = format!("{}{}", out.join().unwrap_or_default(), err.join().unwrap_or_default());
    (status.success(), text)
}

#[test]
fn ac2_compile_fail_cases() {
    let fixtures = root().join("fixtures/core/reg/def/macro");
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("fid-registry-macro-fail");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::create_dir_all(dir.join("examples")).unwrap();
    let registry = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let manifest = format!(
        "[package]\nname = \"fid-registry-macro-fail\"\nversion = \"0.0.0\"\nedition = \"2021\"\npublish = false\n\n\
         [dependencies]\nfid-registry = {{ path = '{}' }}\n\n[workspace]\n",
        registry.display()
    );
    std::fs::write(dir.join("Cargo.toml"), manifest).unwrap();
    std::fs::copy(root().join("Cargo.lock"), dir.join("Cargo.lock")).unwrap();
    std::fs::copy(fixtures.join("demo_ok/lib.rs"), dir.join("src/lib.rs")).unwrap();

    // Control: the passing fixture builds as a crate of its own.
    let (ok, out) = cargo(&dir, &["check", "--lib"]);
    assert!(ok, "demo_ok must build:\n{out}");

    for case in CASES {
        let src = std::fs::read_to_string(fixtures.join(format!("fail/{case}.rs"))).unwrap();
        let expect = src
            .lines()
            .next()
            .and_then(|l| l.strip_prefix("// expect: "))
            .unwrap_or_else(|| panic!("{case}: first line must be `// expect: <message>`"))
            .trim();
        std::fs::write(dir.join(format!("examples/{case}.rs")), &src).unwrap();
        let (ok, out) = cargo(&dir, &["check", "--example", case]);
        assert!(!ok, "{case} must not compile:\n{out}");
        assert!(out.contains("error"), "{case}: no error reported:\n{out}");
        assert!(out.contains(expect), "{case}: expected {expect:?} in:\n{out}");
        // A macro must report errors, never panic.
        assert!(!out.contains("proc-macro derive panicked") && !out.contains("custom attribute panicked"), "{out}");
        std::fs::remove_file(dir.join(format!("examples/{case}.rs"))).unwrap();
    }
}

#[test]
fn every_fail_fixture_is_a_case() {
    let mut found: Vec<String> = std::fs::read_dir(root().join("fixtures/core/reg/def/macro/fail"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    found.sort();
    let mut want: Vec<String> = CASES.iter().map(|c| format!("{c}.rs")).collect();
    want.sort();
    assert_eq!(found, want);
}
