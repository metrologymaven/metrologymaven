//! KRN.MAN.WASM and KRN.MAN.MANI.SCHEMA: the manifest's WASM artifact digest and replay
//! runtime must match the module and the pinned settings. Manifests are written by
//! `fixtures/man/wasm/make_fixtures.py` (json, hashlib) and labelled there.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_kci::manifest::KernelManifest;
use fid_wasm_check::allow::Allowlist;
use fid_wasm_check::manifest::{cross_check, CrossCheckError};
use fid_wasm_check::runtime::RuntimeConfig;
use std::path::{Path, PathBuf};

fn fx(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/man/wasm").join(rel)
}

fn read(rel: &str) -> Vec<u8> {
    std::fs::read(fx(rel)).unwrap()
}

fn setup() -> (Allowlist, RuntimeConfig) {
    let allow = Allowlist::parse(&read("test.allow")).unwrap();
    let rt = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../fid-kernel-wasm/wasm-runtime.toml")).unwrap();
    (allow, RuntimeConfig::parse_canonical(&rt).unwrap())
}

fn code(r: Result<fid_kci::Digest, CrossCheckError>) -> String {
    match r {
        Ok(_) => "ok".to_string(),
        Err(e) => e.code(),
    }
}

#[test]
fn every_labelled_manifest_cross_checks_as_labelled() {
    let (allow, rt) = setup();
    let module = read("modules/ok-wasi.wasm");
    let labels = String::from_utf8(read("manifest/labels.txt")).unwrap();
    let mut n = 0;
    for line in labels.lines().filter(|l| !l.starts_with('#')) {
        let (file, label) = line.split_once('\t').unwrap();
        let manifest = read(&format!("manifest/{file}"));
        assert_eq!(code(cross_check(&manifest, &module, &allow, &rt)), label, "{file}");
        n += 1;
    }
    assert_eq!(n, 6);
}

#[test]
fn ok_manifest_returns_its_digest() {
    let (allow, rt) = setup();
    let manifest = read("manifest/ok.json");
    let d = cross_check(&manifest, &read("modules/ok-wasi.wasm"), &allow, &rt).unwrap();
    assert_eq!(d, KernelManifest::parse_canonical(&manifest).unwrap().manifest_digest());
    assert_eq!(d, fid_kci::kci::sha256_digest(&manifest));
}

#[test]
fn an_unverifiable_or_denied_module_fails_closed() {
    let (allow, rt) = setup();
    let manifest = read("manifest/ok.json");
    assert_eq!(code(cross_check(&manifest, &read("modules/err-magic.wasm"), &allow, &rt)), "module:bad_magic");
    assert_eq!(code(cross_check(&manifest, &read("modules/bad-random-get.wasm"), &allow, &rt)), "imports_denied");
    // A module that passes the import check but is not the one the manifest names.
    assert_eq!(code(cross_check(&manifest, &read("modules/ok-empty.wasm"), &allow, &rt)), "artifact_digest_mismatch");
}
