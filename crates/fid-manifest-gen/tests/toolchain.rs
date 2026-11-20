//! Spec node KRN.MAN.REPRO: the kernel build settings check (AC-4) and the toolchain-and-flags
//! digest (`fid_manifest_gen::toolchain`).
//!
//! Every case under `fixtures/man/repro/cases` and its `expected.txt` come from
//! `fixtures/man/repro/make_fixtures.py` (Python tomllib, json and hashlib, never this crate).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};

use fid_manifest_gen::toolchain::{record, settings, Inputs, ToolchainError, PROFILE_KEYS};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

struct Case {
    files: Vec<(String, Option<Vec<u8>>)>,
}

impl Case {
    fn load(dir: &Path) -> Case {
        let names = [
            "rust-toolchain.toml",
            "cargo-config.toml",
            "kernel-build.toml",
            "Cargo.toml",
            "workspace-config.toml",
            "Cargo.lock",
            "rustc-vV.txt",
            "rustc-cfg.txt",
        ];
        Case { files: names.iter().map(|n| (n.to_string(), fs::read(dir.join(n)).ok())).collect() }
    }

    fn get(&self, name: &str) -> Option<&[u8]> {
        self.files.iter().find(|(n, _)| n == name).and_then(|(_, b)| b.as_deref())
    }

    fn req(&self, name: &str) -> &[u8] {
        self.get(name).unwrap_or_else(|| panic!("{name} missing"))
    }

    fn set(&mut self, name: &str, bytes: Vec<u8>) {
        for (n, b) in &mut self.files {
            if n == name {
                *b = Some(bytes.clone());
            }
        }
    }

    fn inputs(&self) -> Inputs<'_> {
        Inputs {
            rust_toolchain: self.req("rust-toolchain.toml"),
            cargo_config: self.req("cargo-config.toml"),
            kernel_build: self.req("kernel-build.toml"),
            workspace_manifest: self.req("Cargo.toml"),
            workspace_config: self.get("workspace-config.toml"),
            lockfile: self.req("Cargo.lock"),
        }
    }

    fn run(&self) -> Result<(Vec<u8>, String), ToolchainError> {
        let vv = String::from_utf8(self.req("rustc-vV.txt").to_vec()).unwrap();
        let cfg = String::from_utf8(self.req("rustc-cfg.txt").to_vec()).unwrap();
        record(&self.inputs(), &vv, &cfg).map(|r| (r.canonical, r.digest.to_string()))
    }
}

fn cases() -> Vec<(String, PathBuf)> {
    let dir = root().join("fixtures/man/repro/cases");
    let mut out: Vec<(String, PathBuf)> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| {
            let p = e.unwrap().path();
            (p.file_name().unwrap().to_str().unwrap().to_string(), p)
        })
        .collect();
    out.sort();
    out
}

fn base() -> Case {
    Case::load(&root().join("fixtures/man/repro/cases/ok"))
}

#[test]
fn every_fixture_case_matches_the_reference() {
    let all = cases();
    assert!(all.len() >= 20, "fixture cases missing");
    for (name, dir) in all {
        let case = Case::load(&dir);
        let want = String::from_utf8(fs::read(dir.join("expected.txt")).unwrap()).unwrap();
        let got = match case.run() {
            Ok((canonical, digest)) => format!("record {}\ndigest {digest}\n", String::from_utf8(canonical).unwrap()),
            Err(e) => format!("error {e}\n"),
        };
        assert_eq!(got, want, "case {name}");
    }
}

#[test]
fn digest_is_deterministic_and_host_independent() {
    let (a, da) = base().run().unwrap();
    let (b, db) = base().run().unwrap();
    assert_eq!((a.clone(), da.clone()), (b, db));
    // Hand label: the host line of `rustc -vV` is not part of the record.
    let (c, dc) = Case::load(&root().join("fixtures/man/repro/cases/ok-other-host")).run().unwrap();
    assert_eq!((a, da), (c, dc));
}

#[test]
fn every_bit_affecting_input_changes_the_digest() {
    let (_, d0) = base().run().unwrap();
    let edits: [(&str, &str, &str); 7] = [
        ("Cargo.lock", "version = 4", "version = 3"),
        ("cargo-config.toml", "opt-level = 3", "opt-level = 2"),
        ("cargo-config.toml", "strip = \"symbols\"", "strip = \"none\""),
        ("cargo-config.toml", "target-cpu=generic", "target-cpu=mvp"),
        ("cargo-config.toml", "-relaxed-simd,-simd128", "-relaxed-simd,-simd128,-bulk-memory"),
        ("kernel-build.toml", "/fid/src", "/fid/source"),
        ("rustc-cfg.txt", "target_feature=\"sign-ext\"\n", ""),
    ];
    for (file, old, new) in edits {
        let mut c = base();
        let text = String::from_utf8(c.req(file).to_vec()).unwrap();
        assert_eq!(text.matches(old).count(), 1, "{file}: {old}");
        c.set(file, text.replace(old, new).into_bytes());
        if file == "cargo-config.toml" && new.contains("bulk-memory") {
            // Disabling a feature the toolchain still reports is refused, not just re-digested.
            assert_eq!(c.run().unwrap_err(), ToolchainError::FeatureNotDisabled { feature: "bulk-memory".into() });
            continue;
        }
        let (_, d) = c.run().unwrap_or_else(|e| panic!("{file}: {old} -> {new}: {e}"));
        assert_ne!(d, d0, "{file}: {old} -> {new}");
    }
}

/// AC-4: the repository's own kernel build settings are complete and accepted.
#[test]
fn ac4_repository_settings_are_complete() {
    let r = root();
    let read = |p: &str| fs::read(r.join(p)).unwrap();
    let (tc, cc, kb, wm, lock) = (
        read("kernel-build/rust-toolchain.toml"),
        read("kernel-build/.cargo/config.toml"),
        read("kernel-build/kernel-build.toml"),
        read("Cargo.toml"),
        read("Cargo.lock"),
    );
    let wc = fs::read(r.join(".cargo/config.toml")).ok();
    let inputs = Inputs {
        rust_toolchain: &tc,
        cargo_config: &cc,
        kernel_build: &kb,
        workspace_manifest: &wm,
        workspace_config: wc.as_deref(),
        lockfile: &lock,
    };
    let st = settings(&inputs).unwrap();
    // REQ-KRN.MAN.REPRO-001: pinned toolchain, codegen-units = 1, fixed lto, panic = abort.
    assert_eq!(st.channel.split('.').count(), 3);
    assert_eq!(st.profile.get("codegen-units").map(String::as_str), Some("int:1"));
    assert_eq!(st.profile.get("panic").map(String::as_str), Some("str:abort"));
    assert!(st.profile.contains_key("lto"));
    assert_eq!(st.profile.len(), PROFILE_KEYS.len());
    // Path remapping to fixed prefixes for the workspace, CARGO_HOME, vendor and target dirs.
    assert_eq!(st.remap.len(), 4);
    assert!(st.remap.values().all(|v| v.starts_with('/')));
    // No FMA: relaxed SIMD is disabled.
    assert!(st.disabled_features.contains("relaxed-simd"));
    assert_eq!(st.target, "wasm32-wasip1");
    assert_eq!(st.package, "fid-kernel-wasm");
    let args = st.rustc_cfg_args();
    assert_eq!(&args[..4], ["--print", "cfg", "--target", "wasm32-wasip1"]);
}

#[test]
fn a_missing_setting_is_named() {
    // Removing each profile key in turn names exactly that key.
    for key in PROFILE_KEYS {
        let mut c = base();
        let text = String::from_utf8(c.req("cargo-config.toml").to_vec()).unwrap();
        let kept: String =
            text.lines().filter(|l| !l.starts_with(&format!("{key} ="))).map(|l| format!("{l}\n")).collect();
        assert_ne!(kept, text, "{key}");
        c.set("cargo-config.toml", kept.into_bytes());
        assert_eq!(
            settings(&c.inputs()).unwrap_err(),
            ToolchainError::Missing {
                file: "kernel-build/.cargo/config.toml",
                key: format!("profile.kernel-artifact.{key}")
            }
        );
    }
}
