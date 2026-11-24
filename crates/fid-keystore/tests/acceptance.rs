//! Acceptance criteria for spec node `TRUST.SIGN.KEYSTORE.TRAIT` (AC-1 .. AC-4) and error paths.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_keystore::conformance::{run_conformance, run_conformance_with_hook, CONFORMANCE_LABEL};
use fid_keystore::mem::MemKeyStore;
use fid_keystore::{
    decode_sidecar, encode_sidecar, sidecar_path, Alg, KeyStore, KeyStoreError, UvRelease, MAX_LABEL_BYTES, UV_METHODS,
};
use fid_sign_core::{sign_envelope, verify, IN_TOTO_PAYLOAD_TYPE};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn scratch(label: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("fid-keystore-test-{}-{n}-{label}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn python_bin() -> &'static str {
    for candidate in ["python3", "python"] {
        if Command::new(candidate).arg("--version").output().is_ok_and(|o| o.status.success()) {
            return candidate;
        }
    }
    panic!("neither python3 nor python found on PATH");
}

fn run_scan(args: &[String]) -> (i32, String, String) {
    let script = repo_root().join("ci/keystore/key-bytes-scan");
    let out = Command::new(python_bin()).arg(&script).args(args).output().unwrap();
    (out.status.code().unwrap(), String::from_utf8(out.stdout).unwrap(), String::from_utf8(out.stderr).unwrap())
}

fn path_arg(p: &Path) -> String {
    p.to_str().expect("scratch paths are UTF-8").to_string()
}

fn run_key_bytes_scan(root: Option<&Path>) -> (i32, String, String) {
    match root {
        Some(r) => run_scan(&["--root".to_string(), path_arg(r)]),
        None => run_scan(&[]),
    }
}

#[test]
fn ac1_mem_keystore_passes_conformance_suite() {
    let cfg = scratch("ac1");
    let store = MemKeyStore::new(&cfg);
    let report = run_conformance(&store).unwrap();
    assert_eq!(report.created.alg, Alg::EcdsaP256Sha256);
    assert_eq!(report.created.label, CONFORMANCE_LABEL);
    assert!(!report.created.hardware);
    assert!(store.list().unwrap().is_empty());
    let _ = fs::remove_dir_all(&cfg);
}

#[test]
fn ac2_key_bytes_scan_passes_crate_and_fails_on_derive_debug_and_sinks() {
    // 1. Clean scan of the real crate source passes.
    let (code, stdout, stderr) = run_key_bytes_scan(None);
    assert_eq!(code, 0, "stdout={stdout}\nstderr={stderr}");

    // 2. AC-2 exact scenario: test copy adding #[derive(Debug)] with fields to PrivateKeyMaterial.
    let tmp = scratch("ac2-derive-debug");
    let mem_src = fs::read_to_string(repo_root().join("crates/fid-keystore/src/mem.rs")).unwrap();
    let mutated = mem_src.replace("struct PrivateKeyMaterial {", "#[derive(Debug)]\nstruct PrivateKeyMaterial {");
    assert_ne!(mem_src, mutated, "expected struct PrivateKeyMaterial in mem.rs");
    let mutated_path = tmp.join("mem.rs");
    fs::write(&mutated_path, mutated).unwrap();

    let (code, _, stderr) = run_key_bytes_scan(Some(&tmp));
    assert_eq!(code, 1, "expected scan failure on #[derive(Debug)], stderr={stderr}");
    assert!(stderr.contains("PrivateKeyMaterial"), "must name the type: {stderr}");
    assert!(stderr.contains("derive(Debug)"), "must report derive(Debug): {stderr}");
    let _ = fs::remove_dir_all(&tmp);

    // 2b. Regression probes for the #444 review: every real leak path in a mutated copy of
    //     mem.rs must make the scan fail. Each anchor must occur exactly once in mem.rs.
    for (name, anchor, replacement, needles) in mem_rs_leak_mutations() {
        assert_eq!(mem_src.matches(anchor.as_str()).count(), 1, "probe {name}: anchor {anchor:?} not unique in mem.rs");
        let mutated = mem_src.replace(anchor.as_str(), &replacement);
        assert_ne!(mem_src, mutated, "probe {name} did not change mem.rs");
        let dir = scratch(&format!("ac2-mut-{name}"));
        fs::write(dir.join("mem.rs"), mutated).unwrap();
        let (code, _, stderr) = run_key_bytes_scan(Some(&dir));
        assert_eq!(code, 1, "probe {name}: mutated mem.rs must fail the scan, stderr={stderr}");
        for needle in needles {
            assert!(stderr.contains(needle), "probe {name}: expected {needle:?} in {stderr}");
        }
        let _ = fs::remove_dir_all(&dir);
    }

    // 3. Additional REQ-004 violation cases: Serialize, Display, content-bearing Debug,
    //    file/log/journal/package writers, export APIs, pub fn returning private key type.
    let cases = [
        (
            "derive_serialize",
            "#[derive(serde::Serialize)]\nstruct SecretKey { raw: [u8; 32] }\n",
            "SecretKey",
        ),
        (
            "impl_display",
            "struct PrivateKey([u8; 32]);\nimpl std::fmt::Display for PrivateKey { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, \"x\") } }\n",
            "PrivateKey",
        ),
        (
            "content_debug",
            "struct KeyMaterial { bytes: [u8; 32] }\nimpl std::fmt::Debug for KeyMaterial { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, \"{:?}\", self.bytes) } }\n",
            "KeyMaterial",
        ),
        (
            "file_writer",
            "struct PrivateKeyMaterial { bytes: [u8; 32] }\nfn bad(k: &PrivateKeyMaterial) { let _ = std::fs::write(\"out\", k.bytes); }\n",
            "file writer",
        ),
        (
            "log_writer",
            "struct PrivateKeyMaterial { bytes: [u8; 32] }\nfn bad(k: &PrivateKeyMaterial) { eprintln!(\"{:?}\", k); }\n",
            "log writer",
        ),
        (
            "journal_writer",
            "struct PrivateKeyMaterial { bytes: [u8; 32] }\nfn bad(k: &PrivateKeyMaterial) { append_event(k); }\n",
            "journal writer",
        ),
        (
            "package_writer",
            "struct PrivateKeyMaterial { bytes: [u8; 32] }\nfn bad(k: &PrivateKeyMaterial) { PackageWriter::write_entry(k); }\n",
            "package writer",
        ),
        (
            "export_api",
            "fn bad() { let _ = NCryptExportKey(); }\n",
            "NCryptExportKey",
        ),
        (
            "pub_fn_leak",
            "pub struct PrivateKeyMaterial { bytes: [u8; 32] }\npub fn leak() -> PrivateKeyMaterial { loop {} }\n",
            "PrivateKeyMaterial",
        ),
    ];
    for (name, snippet, needle) in cases {
        let dir = scratch(&format!("ac2-{name}"));
        fs::write(dir.join("probe.rs"), snippet).unwrap();
        let (code, _, stderr) = run_key_bytes_scan(Some(&dir));
        assert_eq!(code, 1, "case {name} must fail scan");
        assert!(stderr.contains(needle), "case {name}: expected {needle:?} in {stderr}");
        let _ = fs::remove_dir_all(&dir);
    }
}

type Mutation = (String, String, String, Vec<&'static str>);

/// `(name, anchor in mem.rs, replacement, substrings the scan's stderr must contain)`.
fn mem_rs_leak_mutations() -> Vec<Mutation> {
    const PKM: &str = "struct PrivateKeyMaterial {";
    const SLOT: &str = "struct MemKeySlot {";
    const STORE: &str = "pub struct MemKeyStore {";
    const SLOT_DEBUG: &str = ".field(\"material\", &\"<redacted>\")";
    const STORE_DEBUG: &str = ".field(\"keys\", &\"<redacted>\")";
    const ED_LET: &str = "let material = PrivateKeyMaterial { alg, bytes: seed };";
    const P256_LET: &str = "let material = PrivateKeyMaterial { alg, bytes: scalar };";
    const DELETE: &str = "slot.deleted.store(true, Ordering::Release);";
    const PKM_FIELD: &str = "    bytes: [u8; 32],\n}";
    const SLOT_FIELD: &str = "    material: PrivateKeyMaterial,";
    const STORE_FN: &str = "    pub fn config_dir(&self) -> &Path {";
    const ED_RNG: &str =
        "getrandom::getrandom(&mut seed).map_err(|e| KeyStoreError::Backend(format!(\"getrandom:{e}\")))?;";

    let m = |name: &str, anchor: &str, replacement: String, needles: Vec<&'static str>| {
        (name.to_string(), anchor.to_string(), replacement, needles)
    };
    let mut cases = vec![
        // cfg_attr(..., derive(Debug|Serialize)) on the private key type.
        m("cfg_attr_debug", PKM, format!("#[cfg_attr(test, derive(Debug))]\n{PKM}"), vec!["derive(Debug)", "PrivateKeyMaterial"]),
        m(
            "cfg_attr_serialize",
            PKM,
            format!("#[cfg_attr(any(test, feature = \"serde\"), derive(Clone, serde::Serialize))]\n{PKM}"),
            vec!["derive(Serialize)", "PrivateKeyMaterial"],
        ),
        // derive(Serialize) / derive(Debug) on structs holding key bytes (directly or transitively).
        m("slot_derive_serialize", SLOT, format!("#[derive(serde::Serialize)]\n{SLOT}"), vec!["derive(Serialize)", "MemKeySlot"]),
        m("store_derive_serialize", STORE, format!("#[derive(Serialize)]\n{STORE}"), vec!["derive(Serialize)", "MemKeyStore"]),
        m(
            "slot_cfg_attr_debug",
            SLOT,
            format!("#[cfg_attr(debug_assertions, derive(Debug))]\n{SLOT}"),
            vec!["derive(Debug)", "MemKeySlot"],
        ),
        // Content-Debug check on every type with a private-key-typed field.
        m("slot_debug_material", SLOT_DEBUG, ".field(\"material\", &self.material)".to_string(), vec!["Debug", "MemKeySlot"]),
        m("slot_debug_bytes", SLOT_DEBUG, ".field(\"material\", &self.material.bytes)".to_string(), vec!["Debug", "MemKeySlot"]),
        m("store_debug_keys", STORE_DEBUG, ".field(\"keys\", &self.keys)".to_string(), vec!["Debug", "MemKeyStore"]),
        // `seed` / `scalar` reaching format! into errors (inline format args included).
        m("seed_inline_format_error", ED_RNG, ED_RNG.replace("{e}", "{e}:{seed:?}"), vec!["`seed`", "error or format string"]),
        m(
            "scalar_format_error",
            P256_LET,
            format!("{P256_LET}\n                    return Err(KeyStoreError::Backend(format!(\"{{:?}}\", scalar)));"),
            vec!["`scalar`", "error or format string"],
        ),
        // write( / write_all( / write! / fs::write sinks, including `.material`.
        m("fs_write_seed", ED_LET, format!("{ED_LET}\n            let _ = std::fs::write(\"leak.bin\", seed);"), vec!["`seed`", "file writer"]),
        m("write_material", DELETE, format!("{DELETE}\n        let _ = out.write(&slot.material.bytes);"), vec![".material", "file writer"]),
        m(
            "write_all_material",
            DELETE,
            format!("{DELETE}\n        let _ = out.write_all(&slot.material.bytes);"),
            vec![".material", "file writer"],
        ),
        m(
            "write_macro_material",
            DELETE,
            format!("{DELETE}\n        let _ = write!(out, \"{{:?}}\", slot.material);"),
            vec![".material", "file writer"],
        ),
        // One-level copy of key bytes into a local before the sink.
        m(
            "indirect_copy",
            ED_LET,
            format!("{ED_LET}\n            let copy = material.bytes;\n            let _ = std::fs::write(\"leak.bin\", copy);"),
            vec!["`copy`", "file writer"],
        ),
        // pub fn returning raw key arrays / key types / key bytes, and pub byte fields.
        m(
            "pub_fn_raw_array",
            STORE_FN,
            format!("    pub fn leak_seed(&self) -> [u8; 32] {{\n        [0u8; 32]\n    }}\n\n{STORE_FN}"),
            vec!["leak_seed", "raw key array"],
        ),
        m(
            "pub_fn_private_type",
            STORE_FN,
            format!("    pub fn material(&self) -> Option<&PrivateKeyMaterial> {{\n        None\n    }}\n\n{STORE_FN}"),
            vec!["`material`", "returns private-key type PrivateKeyMaterial"],
        ),
        m(
            "pub_fn_key_bytes",
            STORE_FN,
            format!("    pub fn export(&self) -> Vec<u8> {{\n        Vec::new()\n    }}\n\n{STORE_FN}"),
            vec!["`export`", "returns key bytes"],
        ),
        m("pub_byte_field", PKM_FIELD, "    pub bytes: [u8; 32],\n}".to_string(), vec!["pub byte field `bytes`", "PrivateKeyMaterial"]),
        m("pub_private_field", SLOT_FIELD, "    pub material: PrivateKeyMaterial,".to_string(), vec!["pub field `material`", "MemKeySlot"]),
    ];
    // Bare (unqualified) log and panic macros.
    for (mac, kind) in [
        ("error", "log writer"),
        ("warn", "log writer"),
        ("info", "log writer"),
        ("debug", "log writer"),
        ("trace", "log writer"),
        ("panic", "panic message"),
    ] {
        cases.push(m(
            &format!("bare_{mac}"),
            ED_LET,
            format!("{ED_LET}\n            {mac}!(\"{{:?}}\", seed);"),
            vec!["`seed`", kind],
        ));
    }
    cases
}

#[test]
fn uv_release_issue_scan_allows_only_fid_uv_and_keystore_tests() {
    // 1. The real workspace passes.
    let (code, stdout, stderr) = run_scan(&["--uv-release-issue".to_string()]);
    assert_eq!(code, 0, "stdout={stdout}\nstderr={stderr}");

    // 2. Allowed locations: crate fid-uv, fid-keystore's tests/, #[cfg(test)] items in its src/.
    let call = "fn mint(fp: Fingerprint) { let _ = UvRelease::issue(fp, \"windows_hello\"); }\n";
    let allowed = [
        ("crates/fid-uv/src/lib.rs", call.to_string()),
        ("crates/fid-uv/src/gate/os.rs", call.to_string()),
        ("crates/fid-keystore/tests/extra.rs", call.to_string()),
        ("crates/fid-keystore/src/probe.rs", format!("#[cfg(test)]\nmod tests {{\n    {call}}}\n")),
        ("crates/fid-other/src/lib.rs", "// UvRelease::issue is mentioned only in a comment.\nfn f() {}\n".to_string()),
        ("crates/fid-other/src/doc.rs", "const S: &str = \"UvRelease::issue\";\n".to_string()),
    ];
    let ws = scratch("uv-allowed");
    for (rel, body) in &allowed {
        let p = ws.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, body).unwrap();
    }
    let (code, _, stderr) = run_scan(&["--uv-release-issue".to_string(), "--workspace".to_string(), path_arg(&ws)]);
    assert_eq!(code, 0, "allowed locations must pass: {stderr}");

    // 3. Each forbidden use, added to the allowed workspace, must fail and name the file.
    let forbidden = [
        ("crates/fid-other/src/lib.rs", call.to_string()),
        (
            "crates/fid-other/src/q.rs",
            "fn m(fp: Fingerprint) { fid_keystore::UvRelease::issue(fp, \"passphrase\").ok(); }\n".to_string(),
        ),
        (
            "crates/fid-other/src/angle.rs",
            "fn m(fp: Fingerprint) { <UvRelease>::issue(fp, \"token_pin\").ok(); }\n".to_string(),
        ),
        ("crates/fid-other/src/alias.rs", "use fid_keystore::UvRelease as Release;\n".to_string()),
        ("crates/fid-other/src/ty.rs", "type Release = fid_keystore::UvRelease;\n".to_string()),
        ("crates/fid-other/src/t.rs", format!("#[cfg(test)]\nmod tests {{\n    {call}}}\n")),
        ("crates/fid-keystore/src/leak.rs", call.to_string()),
        ("crates/fid-uv-shim/src/lib.rs", call.to_string()),
        ("crates/fid-other/tests/it.rs", call.to_string()),
        ("xtask/src/main.rs", call.to_string()),
    ];
    for (rel, body) in &forbidden {
        let dir = scratch("uv-forbidden");
        for (arel, abody) in &allowed {
            let p = dir.join(arel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, abody).unwrap();
        }
        let p = dir.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, body).unwrap();
        let (code, _, stderr) =
            run_scan(&["--uv-release-issue".to_string(), "--workspace".to_string(), path_arg(&dir)]);
        assert_eq!(code, 1, "{rel} must fail the UvRelease::issue rule: {stderr}");
        assert!(stderr.contains(rel), "{rel}: expected the path in {stderr}");
        let _ = fs::remove_dir_all(&dir);
    }
    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn uv_release_issue_validates_the_closed_uv_method_set() {
    let fp = fid_keystore::Fingerprint::of_spki_der(b"acceptance-uv-method");
    for method in UV_METHODS {
        let release = UvRelease::issue(fp.clone(), method).unwrap();
        assert_eq!(release.uv_method(), method);
        assert_eq!(release.fingerprint(), &fp);
    }
    assert_eq!(UV_METHODS, ["windows_hello", "macos_local_auth", "token_pin", "token_pin_touch", "passphrase"]);
    // Integration tests always build with `test-backend` (self dev-dependency), so `test_uv` is accepted here.
    assert_eq!(UvRelease::issue(fp.clone(), "test_uv").unwrap().uv_method(), "test_uv");
    for bad in ["", "none", "Windows_Hello", "token-pin", "biometric", " passphrase"] {
        assert_eq!(UvRelease::issue(fp.clone(), bad), Err(KeyStoreError::Unsupported), "{bad:?}");
    }
}

#[test]
fn ac3_release_build_with_test_backend_fails_and_no_non_dev_manifest_enables_it() {
    // 1. Direct release-mode compiler check: `not(debug_assertions)` + `feature = "test-backend"`
    //    must hit `compile_error!`.
    let tmp = scratch("ac3-rustc");
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
    let out = Command::new(&rustc)
        .args([
            "--edition",
            "2021",
            "--crate-type",
            "lib",
            "--emit=metadata",
            "-C",
            "debug-assertions=off",
            "--cfg",
            "feature=\"test-backend\"",
            "-o",
        ])
        .arg(tmp.join("probe.rmeta"))
        .arg(repo_root().join("crates/fid-keystore/src/lib.rs"))
        .output()
        .unwrap();
    assert!(!out.status.success(), "release build with test-backend must fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("test-backend") && stderr.contains("release builds"), "unexpected rustc error: {stderr}");
    let _ = fs::remove_dir_all(&tmp);

    // 2. Workspace manifest scan: only [dev-dependencies] may enable `test-backend` on fid-keystore.
    let root = repo_root();
    let mut files = vec![root.join("Cargo.toml")];
    collect_manifests(&root.join("crates"), &mut files);
    collect_manifests(&root.join("vendor"), &mut files);
    assert!(files.len() > 50, "expected workspace manifests, found {}", files.len());
    let mut bad = Vec::new();
    for f in &files {
        let text = fs::read_to_string(f).unwrap();
        let own = f.parent().and_then(|d| d.file_name()).is_some_and(|n| n == "fid-keystore");
        for v in manifest_violations(&text, own) {
            bad.push(format!("{}: {v}", f.display()));
        }
    }
    assert!(bad.is_empty(), "test-backend enabled outside [dev-dependencies]:\n{}", bad.join("\n"));

    // Non-vacuous check of the manifest scanner itself.
    for sample in [
        "[dependencies]\nfid-keystore = { path = \"../fid-keystore\", features = [\"test-backend\"] }\n",
        "[build-dependencies]\nfid-keystore = { path = \"x\", features = [\"test-backend\"] }\n",
        "[workspace.dependencies]\nfid-keystore = { path = \"x\", features = [\"test-backend\"] }\n",
        "[features]\ndefault = [\"fid-keystore/test-backend\"]\n",
    ] {
        assert_eq!(manifest_violations(sample, false).len(), 1, "{sample}");
    }
    assert_eq!(manifest_violations("[features]\ndefault = [\"test-backend\"]\n", true).len(), 1);
    assert!(manifest_violations("[features]\ntest-backend = []\n", true).is_empty());
    assert!(manifest_violations(
        "[dev-dependencies]\nfid-keystore = { path = \".\", features = [\"test-backend\"] }\n",
        true
    )
    .is_empty());
}

#[test]
fn ac4_sidecar_contains_metadata_only_and_zero_private_key_bytes() {
    let cfg = scratch("ac4");
    let store = MemKeyStore::with_created_utc(&cfg, "2027-03-15T09:30:00Z").unwrap();
    let mut checked_path: Option<PathBuf> = None;

    let report = run_conformance_with_hook(
        &store,
        |fp| UvRelease::test_release(fp.clone()),
        |info| {
            let path = sidecar_path(&cfg, &info.fingerprint);
            if !path.is_file() {
                return Err(format!("sidecar file missing at {}", path.display()));
            }
            let raw_bytes = fs::read(&path).map_err(|e| e.to_string())?;
            // Canonical JSON with the exact five fields.
            let decoded = decode_sidecar(&raw_bytes).map_err(|e| e.to_string())?;
            if &decoded != info {
                return Err(format!("decoded sidecar {decoded:?} != created KeyInfo {info:?}"));
            }
            let expected_canon = encode_sidecar(info).map_err(|e| e.to_string())?;
            if raw_bytes != expected_canon {
                return Err("sidecar file is not RFC 8785 canonical JSON".to_string());
            }
            // AC-4: check against MemKeyStore's debug-only secret verification helper.
            let absent =
                store.debug_assert_absent_from_bytes(&info.fingerprint, &raw_bytes).map_err(|e| e.to_string())?;
            if !absent {
                return Err("sidecar file leaked private key bytes".to_string());
            }
            checked_path = Some(path);
            Ok(())
        },
    )
    .unwrap();

    // After conformance finishes, delete() must also have removed the sidecar file.
    let path = checked_path.unwrap();
    assert!(!path.exists(), "sidecar should be removed after delete: {}", path.display());
    assert_eq!(report.created.created, "2027-03-15T09:30:00Z");

    let _ = fs::remove_dir_all(&cfg);
}

#[test]
fn ed25519_lifecycle_wrong_uv_release_and_deleted_handle_refusal() {
    let cfg = scratch("ed25519");
    let store = MemKeyStore::with_created_utc(&cfg, "2027-02-01T10:00:00Z").unwrap();

    // Create Ed25519 and P-256 keys; list() returns deterministic sorted order.
    let k1 = store.create(Alg::Ed25519, "ed-key", false).unwrap();
    let k2 = store.create(Alg::EcdsaP256Sha256, "p256-key", true).unwrap();
    let listed = store.list().unwrap();
    assert_eq!(listed.len(), 2);
    assert!(listed[0].sort_key() <= listed[1].sort_key());

    // Sign with Ed25519 handle and verify via fid-sign-core.
    let h1 = store.open(&k1.fingerprint).unwrap();
    let dbg_str = format!("{h1:?}");
    assert!(dbg_str.contains("<redacted>"), "{dbg_str}");

    let signer = h1.release_signer(UvRelease::test_release(k1.fingerprint.clone()));
    let env = sign_envelope(b"{\"alg\":\"ed25519\"}", IN_TOTO_PAYLOAD_TYPE, &[&signer]).unwrap();
    let pk1 = store.public_key(&k1.fingerprint).unwrap();
    assert_eq!(verify(&env, &pk1).unwrap(), k1.fingerprint);

    // Mismatched UV release fingerprint must be refused.
    let wrong_rel = UvRelease::test_release(k2.fingerprint.clone());
    assert!(h1.sign_with_release(wrong_rel, b"pae").is_err());

    // Deleting k1 invalidates already-opened handles.
    store.delete(&k1.fingerprint).unwrap();
    let rel_after_del = UvRelease::test_release(k1.fingerprint.clone());
    assert!(h1.sign_with_release(rel_after_del, b"pae").is_err());

    // Label length limit: 128 UTF-8 bytes ok, 129 rejected.
    let exact_128 = "é".repeat(64);
    assert_eq!(exact_128.len(), MAX_LABEL_BYTES);
    assert!(store.create(Alg::Ed25519, &exact_128, false).is_ok());
    let over_128 = format!("{exact_128}a");
    assert_eq!(store.create(Alg::Ed25519, &over_128, false), Err(KeyStoreError::Unsupported));

    let _ = fs::remove_dir_all(&cfg);
}

// ---- workspace Cargo.toml manifest scanner helpers (mirrors fid-bus/tests/test_hooks.rs) ----

fn header(line: &str) -> Option<String> {
    let t = line.trim();
    if !t.starts_with('[') {
        return None;
    }
    let inner = t.trim_start_matches('[');
    let end = inner.find(']')?;
    Some(inner[..end].trim().replace([' ', '"', '\''], ""))
}

fn is_dev_section(section: &str) -> bool {
    section == "dev-dependencies"
        || section.starts_with("dev-dependencies.")
        || (section.starts_with("target.") && section.contains(".dev-dependencies"))
}

fn manifest_violations(text: &str, own: bool) -> Vec<String> {
    let mut section = String::new();
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if let Some(h) = header(line) {
            section = h;
            continue;
        }
        let code = line.split('#').next().unwrap_or("");
        if !code.contains("test-backend") {
            continue;
        }
        let declaration = own && section == "features" && code.trim_start().starts_with("test-backend");
        if !(is_dev_section(&section) || declaration) {
            out.push(format!("line {}: [{section}] {}", i + 1, line.trim()));
        }
    }
    out
}

fn collect_manifests(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if p.is_dir() {
            if name != "target" && !name.starts_with('.') {
                collect_manifests(&p, out);
            }
        } else if name == "Cargo.toml" {
            out.push(p);
        }
    }
}
