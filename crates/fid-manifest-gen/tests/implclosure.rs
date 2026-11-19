//! Acceptance criteria of spec node KRN.MAN.MANI.IMPL (AC-1 .. AC-4) and the typed errors.
//!
//! The fixture `fixtures/man/implclosure-ws-v1` and its expected table come from
//! `fixtures/man/make_implclosure_fixtures.py` (Python json and hashlib, never this crate); the
//! closures and the AC-2 change sets below are hand-labelled from the fixture's edge list.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};

use fid_canon::CValue;
use fid_kci::kci::{sha256_digest, Digest};
use fid_kci::manifest::{AlgorithmEntry, KernelManifest, ManifestCore, ManifestError, NativeGate, ReplayRuntime};
use fid_kci::AlgorithmId;
use fid_manifest_gen::implclosure::{
    check_manifest, impl_digests, impl_digests_from_files, FsTree, ImplDigestTable, ImplError, IoKind,
    ManifestCheckError, Metadata, Registration,
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

fn run_on(meta: &Path, ws: &Path, inp: &Inputs) -> Result<ImplDigestTable, ImplError> {
    impl_digests_from_files(meta, ws, &inp.algorithms, &inp.registrations, &inp.allowlist)
}

fn run(ws: &Path) -> ImplDigestTable {
    run_on(&fixture().join("metadata.json"), ws, &inputs("inputs.json")).unwrap()
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
    let closures: Vec<Vec<String>> = t
        .entries()
        .iter()
        .map(|e| e.closure.iter().map(|c| format!("{}@{}", c.name, c.version)).collect())
        .collect();
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
    // Byte-order sort of paths: "-" < "." < "/".
    assert_eq!(
        sha256_digest(&fs::read(ws.join("crates/kfit/src/a-b.rs")).unwrap()),
        sha256_digest(b"// not a module: still under src/, so still hashed\n")
    );
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

/// Digests after `change` is applied to a fresh copy of the workspace, as one flag per
/// algorithm (FIT, GEO, STAT): did its digest change?
fn changed_after(name: &str, change: impl Fn(&Path)) -> [bool; 3] {
    let base = digests(&run(&fixture().join("ws")));
    let ws = ws_copy(name);
    change(&ws);
    let after = digests(&run(&ws));
    [base[0] != after[0], base[1] != after[1], base[2] != after[2]]
}

fn flip_one_byte(path: &Path) {
    let mut b = fs::read(path).unwrap();
    let i = b.len() / 2;
    b[i] ^= 0x01;
    fs::write(path, b).unwrap();
}

#[test]
fn ac2_one_byte_in_a_crate_only_a_uses_changes_only_a() {
    assert_eq!(changed_after("ac2-kfit", |ws| flip_one_byte(&ws.join("crates/kfit/src/a/b.rs"))), [true, false, false]);
}

#[test]
fn ac2_test_bench_and_example_files_change_nothing() {
    assert_eq!(changed_after("ac2-tests", |ws| flip_one_byte(&ws.join("crates/knum/tests/num.rs"))), [false; 3]);
    assert_eq!(changed_after("ac2-benches", |ws| flip_one_byte(&ws.join("crates/kgeo/benches/b.rs"))), [false; 3]);
    assert_eq!(changed_after("ac2-examples", |ws| flip_one_byte(&ws.join("crates/kfit/examples/e.rs"))), [false; 3]);
    assert_eq!(changed_after("ac2-new-test", |ws| fs::write(ws.join("crates/kstat/tests/new.rs"), b"x").unwrap()), [
        false;
        3
    ]);
}

#[test]
fn ac2_changes_follow_the_closures() {
    // knum is in every closure; the binary file under src/ counts too.
    assert_eq!(changed_after("ac2-knum", |ws| flip_one_byte(&ws.join("crates/knum/src/table.bin"))), [true; 3]);
    // kgeo's build script: kgeo is in FIT's and GEO's closures.
    assert_eq!(changed_after("ac2-build", |ws| flip_one_byte(&ws.join("crates/kgeo/build.rs"))), [true, true, false]);
    // vext is reached only over kstat's build-only, wasm32-gated edge.
    assert_eq!(changed_after("ac2-vext", |ws| flip_one_byte(&ws.join("vendor/vext/src/lib.rs"))), [false, false, true]);
    // kstat dev-depends on kfit, which does not put kfit in STAT's closure (see the kfit case).
    assert_eq!(changed_after("ac2-kstat", |ws| flip_one_byte(&ws.join("crates/kstat/src/lib.rs"))), [false, false, true]);
    // A new file anywhere under src/ is part of the crate.
    assert_eq!(changed_after("ac2-new-src", |ws| fs::write(ws.join("crates/kfit/src/new.rs"), b"").unwrap()), [
        true, false, false
    ]);
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
fn edited_metadata(edit: impl Fn(&mut std::collections::BTreeMap<String, CValue>)) -> Vec<u8> {
    let mut v = fid_canon::parse(&fs::read(fixture().join("metadata.json")).unwrap()).unwrap();
    let CValue::Obj(m) = &mut v else { panic!() };
    edit(m);
    fid_canon::to_canonical_bytes(&v).unwrap()
}

fn digest_metadata(bytes: &[u8]) -> Result<ImplDigestTable, ImplError> {
    let inp = inputs("inputs.json");
    let md = Metadata::parse(bytes)?;
    impl_digests(&md, &inp.algorithms, &inp.registrations, &inp.allowlist, &FsTree::new(fixture().join("ws")))
}

fn package_mut<'a>(m: &'a mut std::collections::BTreeMap<String, CValue>, name: &str) -> &'a mut std::collections::BTreeMap<String, CValue> {
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

fn set_target_path(p: &mut std::collections::BTreeMap<String, CValue>, kind: &str, path: &str) {
    let Some(CValue::Arr(ts)) = p.get_mut("targets") else { panic!() };
    for t in ts {
        let CValue::Obj(t) = t else { panic!() };
        if t.get("kind") == Some(&CValue::Arr(vec![CValue::Str(kind.into())])) {
            t.insert("src_path".into(), CValue::Str(path.into()));
        }
    }
}

#[test]
fn metadata_invalid_fails_closed() {
    let bad = |reason: &'static str| -> Result<ImplDigestTable, ImplError> { Err(ImplError::MetadataInvalid { reason }) };
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
    assert_eq!(
        digest_metadata(&edited_metadata(|m| set_target_path(package_mut(m, "kfit"), "lib", "/toy/ws/crates/kfit/lib.rs"))),
        bad("target_outside_src")
    );
    assert_eq!(
        digest_metadata(&edited_metadata(|m| set_target_path(package_mut(m, "kgeo"), "custom-build", "/toy/ws/build.rs"))),
        bad("build_script_outside_crate")
    );
    assert_eq!(
        digest_metadata(&edited_metadata(|m| {
            package_mut(m, "knum").insert("manifest_path".into(), CValue::Null);
        })),
        bad("package_manifest_path")
    );
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
}

#[test]
fn missing_src_directory_is_an_io_error() {
    let ws = ws_copy("io-missing-src");
    fs::remove_dir_all(ws.join("crates/kstat/src")).unwrap();
    let err = run_on(&fixture().join("metadata.json"), &ws, &inputs("inputs.json")).unwrap_err();
    assert_eq!(err, ImplError::Io { path: "crates/kstat/src".into(), kind: IoKind::NotFound });
    let err = run_on(&fixture().join("no-such-metadata.json"), &ws, &inputs("inputs.json")).unwrap_err();
    assert_eq!(err.code(), "io");
}

#[cfg(unix)]
#[test]
fn symbolic_links_are_refused() {
    let ws = ws_copy("io-symlink");
    std::os::unix::fs::symlink("lib.rs", ws.join("crates/kfit/src/link.rs")).unwrap();
    let err = run_on(&fixture().join("metadata.json"), &ws, &inputs("inputs.json")).unwrap_err();
    assert_eq!(err, ImplError::Io { path: "crates/kfit/src/link.rs".into(), kind: IoKind::Symlink });
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
