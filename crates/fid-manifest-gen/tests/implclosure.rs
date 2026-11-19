//! Acceptance criteria of spec node KRN.MAN.MANI.IMPL (AC-1 .. AC-6) and the typed errors.
//!
//! The fixture `fixtures/man/implclosure-ws-v1` and its expected table come from
//! `fixtures/man/make_implclosure_fixtures.py` (Python json, tomllib and hashlib, never this
//! crate); the closures and the AC-2 change sets below are hand-labelled from the fixture's edge
//! list, and the script checks the same labels against its own results.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use fid_canon::CValue;
use fid_kci::kci::{sha256_digest, Digest};
use fid_kci::manifest::{AlgorithmEntry, KernelManifest, ManifestCore, ManifestError, NativeGate, ReplayRuntime};
use fid_kci::AlgorithmId;
use fid_manifest_gen::implclosure::{
    check_manifest, impl_digests, impl_digests_from_files, FsTree, ImplDigestTable, ImplError, IoKind,
    ManifestCheckError, Metadata, Registration, TrackedFiles,
};

const FIT: &str = "fit.plane.lsq@1.10.0";
const GEO: &str = "geo.point.dist@1.2.0";
const STAT: &str = "stat.mean.basic@2.0.0";

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/man/implclosure-ws-v1")
}

fn id(s: &str) -> AlgorithmId {
    AlgorithmId::parse(s).unwrap()
}

struct Inputs {
    algorithms: Vec<AlgorithmId>,
    registrations: Vec<Registration>,
    allowlist: Vec<String>,
}

fn strs(v: &CValue) -> Vec<String> {
    match v {
        CValue::Arr(a) => a
            .iter()
            .map(|x| match x {
                CValue::Str(s) => s.clone(),
                _ => panic!("not a string"),
            })
            .collect(),
        _ => panic!("not an array"),
    }
}

fn inputs(name: &str) -> Inputs {
    let v = fid_canon::parse(&fs::read(fixture().join(name)).unwrap()).unwrap();
    let CValue::Obj(m) = v else { panic!("not an object") };
    let CValue::Arr(regs) = &m["registrations"] else { panic!("registrations") };
    let registrations = regs
        .iter()
        .map(|r| {
            let CValue::Obj(r) = r else { panic!("registration") };
            let (CValue::Str(a), CValue::Str(c)) = (&r["algorithm"], &r["crate"]) else { panic!("registration") };
            Registration { algorithm: id(a), crate_name: c.clone() }
        })
        .collect();
    Inputs {
        algorithms: strs(&m["algorithms"]).iter().map(|s| id(s)).collect(),
        registrations,
        allowlist: strs(&m["allowlist"]),
    }
}

/// The fixture's tracked files, as `git ls-files` lists them.
fn tracked_list() -> Vec<String> {
    fs::read_to_string(fixture().join("tracked-files.txt")).unwrap().lines().map(str::to_string).collect()
}

fn tracked() -> TrackedFiles {
    TrackedFiles::new(tracked_list()).unwrap()
}

fn run_with(meta: &Path, ws: &Path, files: Vec<String>, inp: &Inputs) -> Result<ImplDigestTable, ImplError> {
    let tracked = TrackedFiles::new(files)?;
    impl_digests_from_files(meta, ws, tracked, &inp.algorithms, &inp.registrations, &inp.allowlist)
}

fn run_on(meta: &Path, ws: &Path, inp: &Inputs) -> Result<ImplDigestTable, ImplError> {
    run_with(meta, ws, tracked_list(), inp)
}

fn try_run(ws: &Path) -> Result<ImplDigestTable, ImplError> {
    run_on(&fixture().join("metadata.json"), ws, &inputs("inputs.json"))
}

fn run(ws: &Path) -> ImplDigestTable {
    try_run(ws).unwrap()
}

fn expected() -> Vec<u8> {
    fs::read(fixture().join("expected-table.json")).unwrap()
}

fn tmp(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join("fid-manifest-gen-implclosure").join(name);
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn copy_dir(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for e in fs::read_dir(src).unwrap() {
        let e = e.unwrap();
        let to = dst.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &to);
        } else {
            fs::copy(e.path(), &to).unwrap();
        }
    }
}

/// A copy of the toy workspace at a fresh temporary directory.
fn ws_copy(name: &str) -> PathBuf {
    let d = tmp(name);
    copy_dir(&fixture().join("ws"), &d);
    d
}

fn digests(t: &ImplDigestTable) -> Vec<Digest> {
    t.entries().iter().map(|e| e.impl_digest).collect()
}

fn flip_one_byte(path: &Path) {
    let mut b = fs::read(path).unwrap();
    let i = b.len() / 2;
    b[i] ^= 0x01;
    fs::write(path, b).unwrap();
}

// ---------------------------------------------------------------------------------------------
// AC-1

#[test]
fn ac1_table_equals_expected() {
    let t = run(&fixture().join("ws"));
    assert_eq!(t.canonical_json().unwrap(), expected());
    let ids: Vec<String> = t.entries().iter().map(|e| e.algorithm.to_string()).collect();
    assert_eq!(ids, [FIT, GEO, STAT]);
    assert_eq!(t.get(&id(GEO)), Some(&t.entries()[1].impl_digest));
    assert_eq!(t.get(&id("geo.point.dist@1.2.1")), None);
}

#[test]
fn ac1_closures_are_the_hand_labelled_ones() {
    let t = run(&fixture().join("ws"));
    let closures: Vec<Vec<String>> =
        t.entries().iter().map(|e| e.closure.iter().map(|c| format!("{}@{}", c.name, c.version)).collect()).collect();
    assert_eq!(
        closures,
        [
            vec!["kfit@0.3.0", "kgeo@0.2.0", "knum@0.1.0"],
            vec!["kgeo@0.2.0", "knum@0.1.0"],
            vec!["knum@0.1.0", "kstat@0.1.0", "vext@1.4.2"],
        ]
    );
}

/// The fixture is hashed as committed bytes on every runner: CRLF and binary files survive the
/// checkout unchanged (`-text` in .gitattributes), so the expected table holds on Windows too.
#[test]
fn ac1_fixture_sources_are_committed_bytes() {
    let ws = fixture().join("ws");
    let crlf = fs::read(ws.join("crates/kgeo/src/lib.rs")).unwrap();
    assert_eq!(crlf, b"pub fn dist(a: u32, b: u32) -> u32 {\r\n    a.abs_diff(b)\r\n}\r\n");
    assert_eq!(fs::read(ws.join("crates/knum/src/table.bin")).unwrap(), b"\x00\x01\r\n\xff\xfe");
    assert_eq!(
        sha256_digest(&fs::read(ws.join("crates/kfit/src/a-b.rs")).unwrap()),
        sha256_digest(b"// not a module: still under src/, so still hashed\n")
    );
}

/// The tracked-file list the fixture tests use is what git itself reports for the fixture
/// (through the hardened `git ls-files` runner). Needs a git checkout, so not under WASI.
#[cfg(not(target_os = "wasi"))]
#[test]
fn ac1_tracked_list_is_what_git_reports() {
    let from_git = fid_manifest_gen::implclosure::git_tracked_files(&fixture().join("ws")).unwrap();
    assert_eq!(from_git, tracked());
    assert_eq!(from_git.paths().count(), tracked_list().len());
}

#[test]
fn ac1_algorithm_order_and_repeats_do_not_matter() {
    let mut inp = inputs("inputs.json");
    inp.algorithms.reverse();
    inp.algorithms.push(id(GEO));
    inp.registrations.reverse();
    let t = run_on(&fixture().join("metadata.json"), &fixture().join("ws"), &inp).unwrap();
    assert_eq!(t.canonical_json().unwrap(), expected());
}

// ---------------------------------------------------------------------------------------------
// AC-2

/// Applies `change` to a fresh copy of the workspace (and its tracked list) and returns, per
/// algorithm (FIT, GEO, STAT), whether its digest changed.
fn changed_with(name: &str, meta: &Path, change: impl Fn(&Path, &mut Vec<String>)) -> [bool; 3] {
    let base = digests(&run(&fixture().join("ws")));
    let ws = ws_copy(name);
    let mut files = tracked_list();
    change(&ws, &mut files);
    let after = digests(&run_with(meta, &ws, files, &inputs("inputs.json")).unwrap());
    [base[0] != after[0], base[1] != after[1], base[2] != after[2]]
}

fn changed_after(name: &str, change: impl Fn(&Path)) -> [bool; 3] {
    changed_with(name, &fixture().join("metadata.json"), |ws, _| change(ws))
}

fn flipped(name: &str, file: &str) -> [bool; 3] {
    changed_after(name, |ws| flip_one_byte(&ws.join(file)))
}

#[test]
fn ac2_one_byte_in_a_crate_only_a_uses_changes_only_a() {
    assert_eq!(flipped("ac2-kfit", "crates/kfit/src/a/b.rs"), [true, false, false]);
}

#[test]
fn ac2_test_bench_and_example_files_change_nothing() {
    assert_eq!(flipped("ac2-tests", "crates/knum/tests/num.rs"), [false; 3]);
    assert_eq!(flipped("ac2-benches", "crates/kgeo/benches/b.rs"), [false; 3]);
    assert_eq!(flipped("ac2-examples", "crates/kfit/examples/e.rs"), [false; 3]);
    // An untracked file outside src/ is not read at all.
    assert_eq!(
        changed_after("ac2-new-test", |ws| fs::write(ws.join("crates/kstat/tests/new.rs"), b"x").unwrap()),
        [false; 3]
    );
}

#[test]
fn ac2_changes_follow_the_closures() {
    // knum is in every closure; the binary file under src/ counts too.
    assert_eq!(flipped("ac2-knum", "crates/knum/src/table.bin"), [true; 3]);
    // vext is reached only over kstat's wasm32-gated edge.
    assert_eq!(flipped("ac2-vext", "vendor/vext/src/lib.rs"), [false, false, true]);
    // kstat dev-depends on kfit, which does not put kfit in STAT's closure (see the kfit case).
    assert_eq!(flipped("ac2-kstat", "crates/kstat/src/lib.rs"), [false, false, true]);
    // A new tracked file anywhere under src/ is part of the crate.
    let add = |ws: &Path, files: &mut Vec<String>| {
        fs::write(ws.join("crates/kfit/src/new.rs"), b"").unwrap();
        files.push("crates/kfit/src/new.rs".to_string());
    };
    assert_eq!(changed_with("ac2-new-src", &fixture().join("metadata.json"), add), [true, false, false]);
    // Line endings are bytes: converting the CRLF file to LF changes kgeo's digest.
    assert_eq!(
        changed_after("ac2-eol", |ws| {
            let p = ws.join("crates/kgeo/src/lib.rs");
            let lf: Vec<u8> = fs::read(&p).unwrap().into_iter().filter(|b| *b != b'\r').collect();
            fs::write(&p, lf).unwrap();
        }),
        [true, true, false]
    );
}

#[test]
fn ac2_manifests_config_and_lockfile_are_hashed() {
    // A closure crate's own manifest.
    assert_eq!(flipped("ac2-kfit-toml", "crates/kfit/Cargo.toml"), [true, false, false]);
    assert_eq!(flipped("ac2-vext-toml", "vendor/vext/Cargo.toml"), [false, false, true]);
    // The root manifest and the Cargo configuration change how every crate builds.
    assert_eq!(flipped("ac2-root-toml", "Cargo.toml"), [true; 3]);
    assert_eq!(flipped("ac2-config", ".cargo/config.toml"), [true; 3]);
    let legacy = |ws: &Path, files: &mut Vec<String>| {
        fs::write(ws.join(".cargo/config"), b"[build]\n").unwrap();
        files.push(".cargo/config".to_string());
    };
    assert_eq!(changed_with("ac2-legacy-config", &fixture().join("metadata.json"), legacy), [true; 3]);
    // Only the lockfile entries of closure crates count: vext's checksum changes only STAT.
    let checksum = |ws: &Path| {
        let p = ws.join("Cargo.lock");
        let text = fs::read_to_string(&p).unwrap();
        let start = text.find("checksum = \"").unwrap() + "checksum = \"".len();
        let mut out = text.clone();
        out.replace_range(start..start + 64, &"0".repeat(64));
        fs::write(&p, out).unwrap();
    };
    assert_eq!(changed_after("ac2-lock", checksum), [false, false, true]);
}

/// The fixture metadata with `edit` applied to its top-level object, written to a fresh file.
fn metadata_edit(name: &str, edit: impl Fn(&mut BTreeMap<String, CValue>)) -> PathBuf {
    let p = tmp(name).join("metadata.json");
    fs::write(&p, edited_metadata(edit)).unwrap();
    p
}

fn node_mut<'a>(m: &'a mut BTreeMap<String, CValue>, name: &str) -> &'a mut BTreeMap<String, CValue> {
    let Some(CValue::Obj(resolve)) = m.get_mut("resolve") else { panic!() };
    let Some(CValue::Arr(nodes)) = resolve.get_mut("nodes") else { panic!() };
    for n in nodes {
        if let CValue::Obj(n) = n {
            if matches!(n.get("id"), Some(CValue::Str(s)) if s.ends_with(&format!("#{name}@0.3.0"))) {
                return n;
            }
        }
    }
    panic!("no node {name}")
}

#[test]
fn ac2_toggling_an_enabled_feature_changes_the_digest() {
    let meta = metadata_edit("ac2-feature-meta", |m| {
        node_mut(m, "kfit").insert("features".into(), CValue::Arr(vec![CValue::Str("default".into())]));
    });
    assert_eq!(changed_with("ac2-feature", &meta, |_, _| {}), [true, false, false]);
}

// ---------------------------------------------------------------------------------------------
// AC-3

#[test]
fn ac3_two_checkout_paths_give_identical_tables() {
    let a = ws_copy("ac3-a");
    let b = tmp("ac3-b/deeper/still");
    copy_dir(&fixture().join("ws"), &b);
    let ta = run(&a).canonical_json().unwrap();
    let tb = run(&b).canonical_json().unwrap();
    assert_eq!(ta, tb);
    assert_eq!(ta, expected());
}

fn json_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Metadata as `cargo metadata` would write it for the checkout at `root` (absolute paths).
fn metadata_for(name: &str, root: &str) -> PathBuf {
    let text = fs::read_to_string(fixture().join("metadata.json")).unwrap();
    let rewritten = text.replace("/toy/ws", &json_escape(root));
    let p = tmp(name).join("metadata.json");
    fs::write(&p, rewritten).unwrap();
    p
}

#[test]
fn ac3_real_absolute_paths_and_windows_separators() {
    let inp = inputs("inputs.json");
    let ws = ws_copy("ac3-real");
    let meta = metadata_for("ac3-real-meta", &ws.display().to_string());
    assert_eq!(run_on(&meta, &ws, &inp).unwrap().canonical_json().unwrap(), expected());
    // Metadata written on Windows (backslashes, a drive letter) reads the same on any runner.
    let meta = metadata_for("ac3-win-meta", "C:\\Users\\x\\ws");
    let text = fs::read_to_string(&meta).unwrap().replace("/crates/", "\\\\crates\\\\").replace("/src/", "\\\\src\\\\");
    fs::write(&meta, text).unwrap();
    assert_eq!(run_on(&meta, &fixture().join("ws"), &inp).unwrap().canonical_json().unwrap(), expected());
}

// ---------------------------------------------------------------------------------------------
// AC-4 and the other typed errors

#[test]
fn ac4_closure_crate_missing_from_allowlist() {
    let err = run_on(&fixture().join("metadata.json"), &fixture().join("ws"), &inputs("inputs-unlisted.json"));
    assert_eq!(err, Err(ImplError::NotAllowlisted { crate_name: "vext".into(), algorithm: id(STAT) }));
    assert_eq!(err.unwrap_err().code(), "not_allowlisted");
}

#[test]
fn registering_crate_must_be_allowlisted_too() {
    let mut inp = inputs("inputs.json");
    inp.allowlist.retain(|c| c != "kfit");
    let err = run_on(&fixture().join("metadata.json"), &fixture().join("ws"), &inp).unwrap_err();
    assert_eq!(err, ImplError::NotAllowlisted { crate_name: "kfit".into(), algorithm: id(FIT) });
}

fn run_inputs(inp: &Inputs) -> Result<ImplDigestTable, ImplError> {
    run_on(&fixture().join("metadata.json"), &fixture().join("ws"), inp)
}

#[test]
fn unknown_algorithm_and_no_registering_crate() {
    let mut inp = inputs("inputs.json");
    inp.registrations.push(Registration { algorithm: id("zzz.extra.alg@1.0.0"), crate_name: "knum".into() });
    assert_eq!(run_inputs(&inp), Err(ImplError::UnknownAlgorithm { algorithm: id("zzz.extra.alg@1.0.0") }));

    let mut inp = inputs("inputs.json");
    inp.registrations.retain(|r| r.algorithm != id(GEO));
    assert_eq!(run_inputs(&inp), Err(ImplError::NoRegisteringCrate { algorithm: id(GEO) }));

    let mut inp = inputs("inputs.json");
    inp.registrations.iter_mut().filter(|r| r.algorithm == id(STAT)).for_each(|r| r.crate_name = "nope".into());
    assert_eq!(run_inputs(&inp), Err(ImplError::NoRegisteringCrate { algorithm: id(STAT) }));
}

#[test]
fn malformed_inputs() {
    let mut inp = inputs("inputs.json");
    inp.registrations.push(Registration { algorithm: id(FIT), crate_name: "kgeo".into() });
    assert_eq!(run_inputs(&inp), Err(ImplError::InputInvalid { reason: "duplicate_registration" }));
    for dup in ["KNUM", "k-num", "Vext", ""] {
        let mut inp = inputs("inputs.json");
        inp.allowlist.push(dup.to_string());
        if dup == "k-num" {
            inp.allowlist.push("k_num".to_string());
        }
        assert_eq!(run_inputs(&inp), Err(ImplError::InputInvalid { reason: "allowlist_duplicate_or_empty" }), "{dup}");
    }
}

/// The fixture metadata with `edit` applied, re-serialised.
fn edited_metadata(edit: impl Fn(&mut BTreeMap<String, CValue>)) -> Vec<u8> {
    let mut v = fid_canon::parse(&fs::read(fixture().join("metadata.json")).unwrap()).unwrap();
    let CValue::Obj(m) = &mut v else { panic!() };
    edit(m);
    fid_canon::to_canonical_bytes(&v).unwrap()
}

fn digest_metadata(bytes: &[u8]) -> Result<ImplDigestTable, ImplError> {
    let inp = inputs("inputs.json");
    let md = Metadata::parse(bytes)?;
    impl_digests(
        &md,
        &inp.algorithms,
        &inp.registrations,
        &inp.allowlist,
        &FsTree::new(fixture().join("ws"), tracked()),
    )
}

fn package_mut<'a>(m: &'a mut BTreeMap<String, CValue>, name: &str) -> &'a mut BTreeMap<String, CValue> {
    let Some(CValue::Arr(ps)) = m.get_mut("packages") else { panic!() };
    for p in ps {
        if let CValue::Obj(p) = p {
            if p.get("name") == Some(&CValue::Str(name.into())) {
                return p;
            }
        }
    }
    panic!("no package {name}")
}

fn targets_mut(p: &mut BTreeMap<String, CValue>) -> &mut Vec<CValue> {
    let Some(CValue::Arr(ts)) = p.get_mut("targets") else { panic!() };
    ts
}

fn set_target(p: &mut BTreeMap<String, CValue>, kind: &str, key: &str, value: CValue) {
    for t in targets_mut(p) {
        let CValue::Obj(t) = t else { panic!() };
        if t.get("kind") == Some(&CValue::Arr(vec![CValue::Str(kind.into())])) {
            t.insert(key.into(), value.clone());
        }
    }
}

#[test]
fn metadata_invalid_fails_closed() {
    let bad =
        |reason: &'static str| -> Result<ImplDigestTable, ImplError> { Err(ImplError::MetadataInvalid { reason }) };
    // Re-serialising the unchanged metadata keeps the table (member order is free).
    assert_eq!(digest_metadata(&edited_metadata(|_| {})).unwrap().canonical_json().unwrap(), expected());
    assert_eq!(digest_metadata(b"{"), bad("json"));
    let set = |key: &'static str, v: CValue| {
        edited_metadata(move |m| {
            m.insert(key.into(), v.clone());
        })
    };
    assert_eq!(digest_metadata(&set("resolve", CValue::Null)), bad("no_resolve"));
    assert_eq!(digest_metadata(&set("version", CValue::Num(2.0))), bad("format_version"));
    assert_eq!(digest_metadata(&set("workspace_root", CValue::Str("/other".into()))), bad("package_outside_workspace"));
    let lib_outside = edited_metadata(|m| {
        set_target(package_mut(m, "kfit"), "lib", "src_path", CValue::Str("/toy/ws/crates/kfit/lib.rs".into()))
    });
    assert_eq!(digest_metadata(&lib_outside), bad("target_outside_src"));
    let no_manifest = edited_metadata(|m| {
        package_mut(m, "knum").insert("manifest_path".into(), CValue::Null);
    });
    assert_eq!(digest_metadata(&no_manifest), bad("package_manifest_path"));
    let no_source = edited_metadata(|m| {
        package_mut(m, "knum").remove("source");
    });
    assert_eq!(digest_metadata(&no_source), bad("package_source"));
    // An unknown dependency kind.
    let weird = edited_metadata(|_| {});
    let weird = String::from_utf8(weird).unwrap().replace("\"kind\":\"dev\"", "\"kind\":\"weird\"");
    assert_eq!(digest_metadata(weird.as_bytes()), bad("dep_kind"));
    // A second package with the registering crate's name.
    let twice = edited_metadata(|m| {
        let mut copy = package_mut(m, "kgeo").clone();
        copy.insert("id".into(), CValue::Str("path+file:///toy/ws/other#kgeo@9.0.0".into()));
        let Some(CValue::Arr(ps)) = m.get_mut("packages") else { panic!() };
        ps.push(CValue::Obj(copy));
    });
    assert_eq!(digest_metadata(&twice), bad("ambiguous_crate_name"));
    // The package's source must match its lockfile entry.
    let other_source = edited_metadata(|m| {
        package_mut(m, "vext").insert("source".into(), CValue::Str("git+https://example.invalid/vext".into()));
    });
    assert_eq!(digest_metadata(&other_source), bad("not_in_lockfile"));
}

#[test]
fn build_scripts_and_proc_macros_in_the_closure_are_refused() {
    let bad =
        |reason: &'static str| -> Result<ImplDigestTable, ImplError> { Err(ImplError::MetadataInvalid { reason }) };
    let build = edited_metadata(|m| {
        let mut t = BTreeMap::new();
        t.insert("kind".to_string(), CValue::Arr(vec![CValue::Str("custom-build".into())]));
        t.insert("src_path".to_string(), CValue::Str("/toy/ws/crates/kgeo/src/build.rs".into()));
        targets_mut(package_mut(m, "kgeo")).push(CValue::Obj(t));
    });
    assert_eq!(digest_metadata(&build), bad("build_script_in_closure"));
    let proc_macro = edited_metadata(|m| {
        set_target(package_mut(m, "kfit"), "lib", "kind", CValue::Arr(vec![CValue::Str("proc-macro".into())]))
    });
    assert_eq!(digest_metadata(&proc_macro), bad("proc_macro_in_closure"));
}

fn edit_lock(name: &str, f: impl Fn(String) -> String) -> Result<ImplDigestTable, ImplError> {
    let ws = ws_copy(name);
    let p = ws.join("Cargo.lock");
    fs::write(&p, f(fs::read_to_string(&p).unwrap())).unwrap();
    try_run(&ws)
}

#[test]
fn lockfile_errors() {
    let bad =
        |reason: &'static str| -> Result<ImplDigestTable, ImplError> { Err(ImplError::MetadataInvalid { reason }) };
    let renamed = edit_lock("lock-missing", |t: String| t.replace("name = \"vext\"", "name = \"vext2\""));
    assert_eq!(renamed, bad("not_in_lockfile"));
    assert_eq!(edit_lock("lock-version", |t: String| t.replace("version = 4", "version = 1")), bad("lockfile_version"));
    let dup_key =
        edit_lock("lock-dup-key", |t: String| t.replace("name = \"knum\"", "name = \"knum\"\nname = \"knum\""));
    assert_eq!(dup_key, bad("lockfile_toml"));
    let dup_entry = "[[package]]\nname = \"knum\"\nversion = \"0.1.0\"\n";
    assert_eq!(edit_lock("lock-dup-entry", |t: String| format!("{t}\n{dup_entry}")), bad("lockfile_duplicate"));
}

// ---------------------------------------------------------------------------------------------
// AC-5: only git-tracked files; nothing compiled may come from outside the hashed set

#[test]
fn ac5_untracked_and_missing_files_are_errors() {
    let io = |path: &str, kind| Err(ImplError::Io { path: path.into(), kind });
    // An untracked file below src/ would be compiled if a module named it: refused.
    let ws = ws_copy("ac5-untracked-src");
    fs::write(ws.join("crates/kfit/src/stray.rs"), b"").unwrap();
    assert_eq!(try_run(&ws), io("crates/kfit/src/stray.rs", IoKind::Untracked));
    // An untracked Cargo configuration file is read by Cargo: refused.
    let ws = ws_copy("ac5-untracked-config");
    fs::write(ws.join(".cargo/config"), b"[build]\n").unwrap();
    assert_eq!(try_run(&ws), io(".cargo/config", IoKind::Untracked));
    // A tracked file that is missing on disk.
    let ws = ws_copy("ac5-missing");
    fs::remove_dir_all(ws.join("crates/kstat/src")).unwrap();
    assert_eq!(try_run(&ws), io("crates/kstat/src/lib.rs", IoKind::NotFound));
    let mut files = tracked_list();
    files.retain(|f| !f.starts_with("crates/kstat/src/"));
    let err = run_with(&fixture().join("metadata.json"), &ws, files, &inputs("inputs.json"));
    assert_eq!(err, io("crates/kstat/src", IoKind::NotFound));
    let err = run_on(&fixture().join("no-such-metadata.json"), &ws, &inputs("inputs.json")).unwrap_err();
    assert_eq!(err.code(), "io");
}

#[test]
fn ac5_tracked_lists_are_validated() {
    let collision = TrackedFiles::new(["src/Lib.rs".to_string(), "src/lib.rs".to_string()]);
    assert_eq!(collision, Err(ImplError::InputInvalid { reason: "tracked_case_collision" }));
    for bad in ["a//b", "./a", "a/../b", "a\\b", "/a", ""] {
        let err = TrackedFiles::new([bad.to_string()]);
        assert_eq!(err, Err(ImplError::Io { path: bad.into(), kind: IoKind::BadName }), "{bad:?}");
    }
    let ok = TrackedFiles::from_ls_files_stage(b"100644 0123 0\tsrc/a.rs\x00100755 4567 0\tsrc/b.rs\0").unwrap();
    assert_eq!(ok.paths().collect::<Vec<_>>(), ["src/a.rs", "src/b.rs"]);
    assert_eq!(
        TrackedFiles::from_ls_files_stage(b"120000 0123 0\tsrc/link.rs\0"),
        Err(ImplError::Io { path: "src/link.rs".into(), kind: IoKind::Symlink })
    );
    assert_eq!(
        TrackedFiles::from_ls_files_stage(b"160000 0123 0\tvendor/sub\0"),
        Err(ImplError::Io { path: "vendor/sub".into(), kind: IoKind::NotAFile })
    );
    assert_eq!(
        TrackedFiles::from_ls_files_stage(b"100644 0123 0\tsrc/A.rs\x00100644 4567 0\tsrc/a.rs\0"),
        Err(ImplError::InputInvalid { reason: "tracked_case_collision" })
    );
}

#[cfg(unix)]
#[test]
fn ac5_symbolic_links_are_refused() {
    let ws = ws_copy("ac5-symlink");
    std::os::unix::fs::symlink("lib.rs", ws.join("crates/kfit/src/link.rs")).unwrap();
    let mut files = tracked_list();
    files.push("crates/kfit/src/link.rs".to_string());
    let err = run_with(&fixture().join("metadata.json"), &ws, files, &inputs("inputs.json")).unwrap_err();
    assert_eq!(err, ImplError::Io { path: "crates/kfit/src/link.rs".into(), kind: IoKind::Symlink });
}

/// Runs the fixture with `src/a/b.rs` of kfit replaced by `text`, plus extra tracked files
/// (path relative to kfit's `src/a/`, bytes).
fn with_source(name: &str, text: &str, extra: &[(&str, &[u8])]) -> Result<ImplDigestTable, ImplError> {
    let ws = ws_copy(name);
    let dir = ws.join("crates/kfit/src/a");
    fs::write(dir.join("b.rs"), text).unwrap();
    let mut files = tracked_list();
    for (rel, bytes) in extra {
        fs::write(dir.join(rel), bytes).unwrap();
        files.push(format!("crates/kfit/src/a/{rel}"));
    }
    run_with(&fixture().join("metadata.json"), &ws, files, &inputs("inputs.json"))
}

#[test]
fn ac6_includes_and_path_attributes_must_stay_in_the_hashed_set() {
    let bad =
        |reason: &'static str| -> Result<ImplDigestTable, ImplError> { Err(ImplError::MetadataInvalid { reason }) };
    // Allowed: a literal path to a hashed file of the same crate (the crate's own Cargo.toml
    // counts; the fixture's knum already uses include_bytes! and #[path]).
    assert!(with_source("ac6-ok", "pub const M: &str = include_str!(\"../../Cargo.toml\");\n", &[]).is_ok());
    // Another crate's file, a missing file, an absolute path, a tests/ file.
    let outside = [
        "include_str!(\"../../../knum/src/lib.rs\")",
        "include_bytes!(\"missing.bin\")",
        "include_bytes!(\"/etc/passwd\")",
        "include!(\"../../tests/t.rs\")",
    ];
    for (i, inc) in outside.iter().enumerate() {
        let src = format!("pub fn f() {{ let _ = {inc}; }}\n");
        assert_eq!(with_source(&format!("ac6-out-{i}"), &src, &[]), bad("include_outside_hashed_set"), "{inc}");
    }
    // Anything but one plain string literal.
    let src = "pub const P: &str = include_str!(concat!(env!(\"OUT_DIR\"), \"/gen.rs\"));\n";
    assert_eq!(with_source("ac6-concat", src, &[]), bad("include_not_literal"));
    let src = "macro_rules! m { ($p:expr) => { include_bytes!($p) }; }\n";
    assert_eq!(with_source("ac6-macro", src, &[]), bad("include_not_literal"));
    // #[path] out of the crate, and through cfg_attr.
    let src = "#[path = \"../../../kgeo/src/lib.rs\"]\nmod g;\n";
    assert_eq!(with_source("ac6-path", src, &[]), bad("path_attr_outside_hashed_set"));
    let src = "#[cfg_attr(unix, path = \"../../../../outside.rs\")]\nmod g;\n";
    assert_eq!(with_source("ac6-cfg-attr", src, &[]), bad("path_attr_outside_hashed_set"));
    // include! compiles its target as Rust whatever the extension, so the target is scanned too.
    let src = "include!(\"gen.inc\");\n";
    let inner: &[u8] = b"pub const S: &str = include_str!(\"../../../../secret.txt\");\n";
    assert_eq!(with_source("ac6-transitive", src, &[("gen.inc", inner)]), bad("include_outside_hashed_set"));
    let fine: &[u8] = b"pub fn two() -> u32 { 2 }\n";
    let t = with_source("ac6-transitive-ok", src, &[("gen.inc", fine)]).unwrap();
    assert_ne!(digests(&t)[0], digests(&run(&fixture().join("ws")))[0]);
}

// ---------------------------------------------------------------------------------------------
// Checking a manifest against the table (strict read through KernelManifest::parse_canonical)

fn h(label: &str) -> Digest {
    sha256_digest(label.as_bytes())
}

fn manifest_for(rows: &[(AlgorithmId, Digest)]) -> KernelManifest {
    let core = ManifestCore {
        name: "toy-kernel".into(),
        semver: "0.1.0".into(),
        numeric_profile: "strict".into(),
        source_tree_digest: h("tree"),
        lockfile_digest: h("lock"),
        toolchain_digest: h("toolchain"),
        replay_runtime: ReplayRuntime { version: "1.0.0".into(), config_digest: h("runtime") },
        algorithms: rows
            .iter()
            .map(|(a, d)| AlgorithmEntry { id: a.clone(), card_digest: h(&a.to_string()), impl_digest: *d })
            .collect(),
        facade_selftest_digest: h("selftest"),
        solver_limits_digest: h("limits"),
        reference_suite_digest: h("suite"),
        reference_results_digest: h("results"),
        native_gate: NativeGate::MeasuredOnly,
    };
    KernelManifest::new(core, vec![]).unwrap()
}

#[test]
fn manifest_check_uses_the_strict_reader() {
    let t = run(&fixture().join("ws"));
    let rows: Vec<(AlgorithmId, Digest)> = t.entries().iter().map(|e| (e.algorithm.clone(), e.impl_digest)).collect();
    let good = manifest_for(&rows);
    assert_eq!(check_manifest(good.canonical_json(), &t).unwrap(), good);

    let mut stale = rows.clone();
    stale[1].1 = h("old");
    let err = check_manifest(manifest_for(&stale).canonical_json(), &t).unwrap_err();
    assert_eq!(err, ManifestCheckError::ImplDigestMismatch { algorithm: id(GEO) });

    let err = check_manifest(manifest_for(&rows[..2]).canonical_json(), &t).unwrap_err();
    assert_eq!(err, ManifestCheckError::NotInManifest { algorithm: id(STAT) });

    let mut extra = rows.clone();
    extra.insert(0, (id("aaa.extra.alg@1.0.0"), h("x")));
    let err = check_manifest(manifest_for(&extra).canonical_json(), &t).unwrap_err();
    assert_eq!(err, ManifestCheckError::NotInTable { algorithm: id("aaa.extra.alg@1.0.0") });

    // Valid but not canonical (a trailing newline): the strict reader refuses it.
    let mut loose = good.canonical_json().to_vec();
    loose.push(b'\n');
    let err = check_manifest(&loose, &t).unwrap_err();
    assert!(matches!(err, ManifestCheckError::Manifest(ManifestError::NonCanonical { .. })), "{err:?}");
}
