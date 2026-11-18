//! Acceptance criteria of spec node TRUST.PKG.MANIFEST (AC-1 .. AC-7).
//!
//! Expected bytes and digests come from the independent Python reference
//! `fixtures/pkg/make_fixtures.py`, never from this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_pkg::{
    build_package, build_package_with, hash_bytes, hash_reader, EvidenceTier, Manifest, MemberInput, PackageInput,
    PkgError, PkgPath, SemVer, Zip64, FORMAT_NAME, PREDICATE_TYPE, STATEMENT_TYPE,
};
use fid_types::Digest;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/pkg");
const SCHEMA: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../schema/manifest-1.json");

fn read(rel: &str) -> Vec<u8> {
    let p = format!("{FIX}/{rel}");
    std::fs::read(&p).unwrap_or_else(|e| panic!("{p}: {e}"))
}

fn obj(v: &CValue) -> &BTreeMap<String, CValue> {
    match v {
        CValue::Obj(o) => o,
        other => panic!("expected object, got {other:?}"),
    }
}

fn arr(v: &CValue) -> &Vec<CValue> {
    match v {
        CValue::Arr(a) => a,
        other => panic!("expected array, got {other:?}"),
    }
}

fn st<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> &'a str {
    match o.get(k) {
        Some(CValue::Str(s)) => s,
        other => panic!("{k}: expected string, got {other:?}"),
    }
}

fn num(v: &CValue) -> u64 {
    match v {
        CValue::Num(n) if *n >= 0.0 && n.fract() == 0.0 => *n as u64,
        other => panic!("expected a whole number, got {other:?}"),
    }
}

fn load_contents(name: &str) -> PackageInput {
    let root = fid_canon::parse(&read(&format!("contents/{name}.json"))).unwrap();
    let o = obj(&root);
    let members = arr(&o["members"])
        .iter()
        .map(|m| {
            let m = obj(m);
            let body = match (m.get("body"), m.get("body_repeat")) {
                (Some(CValue::Str(b)), None) => b.as_bytes().to_vec(),
                (None, Some(CValue::Arr(r))) => match r.as_slice() {
                    [CValue::Str(text), count] => text.repeat(num(count) as usize).into_bytes(),
                    other => panic!("bad body_repeat {other:?}"),
                },
                other => panic!("bad body {other:?}"),
            };
            MemberInput {
                path: PkgPath::parse(st(m, "path")).unwrap(),
                media_type: st(m, "media_type").to_string(),
                body,
            }
        })
        .collect();
    PackageInput {
        created_by: st(o, "created_by").to_string(),
        evidence_tier: EvidenceTier::parse(st(o, "evidence_tier")).unwrap(),
        profile_digest: Digest::parse(st(o, "profile_digest")).unwrap(),
        kernel_manifest_digest: Digest::parse(st(o, "kernel_manifest_digest")).unwrap(),
        result_count: num(&o["result_count"]),
        members,
    }
}

fn pinned(rel: &str) -> String {
    let text = String::from_utf8(read(rel)).unwrap();
    let hex = text.split_whitespace().next().unwrap().to_string();
    assert_eq!(hex.len(), 64, "{rel}");
    hex
}

/// Equality with a readable report of the first differing byte.
fn assert_same_bytes(what: &str, got: &[u8], want: &[u8]) {
    if got == want {
        return;
    }
    let at = got.iter().zip(want).position(|(a, b)| a != b).unwrap_or(got.len().min(want.len()));
    let window = |b: &[u8]| -> String {
        let lo = at.saturating_sub(8);
        let hi = (at + 24).min(b.len());
        b.get(lo..hi).unwrap_or_default().iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" ")
    };
    let lo = at.saturating_sub(8);
    let (g, w, gl, wl) = (window(got), window(want), got.len(), want.len());
    panic!("{what}: differs at byte {at} (got {gl} bytes, want {wl} bytes)\n got[{lo}..]: {g}\nwant[{lo}..]: {w}");
}

fn temp_file(tag: &str, bytes: &[u8]) -> PathBuf {
    let p = std::env::temp_dir().join(format!("fid-pkg-{}-{tag}", std::process::id()));
    std::fs::write(&p, bytes).unwrap();
    p
}

/// Runs `cmd args`, returning stdout when the tool exists and succeeds.
fn run_tool(cmd: &str, args: &[&str]) -> Option<Vec<u8>> {
    let out = Command::new(cmd).args(args).env("PYTHONIOENCODING", "utf-8").output().ok()?;
    out.status.success().then_some(out.stdout)
}

/// Entry names from the central directory, read here without the crate (classic records).
fn central_names(zip: &[u8]) -> Vec<String> {
    let le16 = |at: usize| u16::from_le_bytes([zip[at], zip[at + 1]]) as usize;
    let le32 = |at: usize| u32::from_le_bytes([zip[at], zip[at + 1], zip[at + 2], zip[at + 3]]) as usize;
    let eocd = zip.len() - 22;
    assert_eq!(le32(eocd), 0x0605_4b50, "no end record");
    let count = le16(eocd + 10);
    let mut at = le32(eocd + 16);
    let mut names = Vec::new();
    for _ in 0..count {
        assert_eq!(le32(at), 0x0201_4b50);
        let (n, x, c) = (le16(at + 28), le16(at + 30), le16(at + 32));
        names.push(String::from_utf8(zip[at + 46..at + 46 + n].to_vec()).unwrap());
        at += 46 + n + x + c;
    }
    names
}

fn expected_order(pkg: &fid_pkg::BuiltPackage) -> Vec<String> {
    let mut v = vec!["manifest.json".to_string()];
    v.extend(pkg.manifest.members.iter().map(|m| m.path.as_str().to_string()));
    v.push("statement.json".to_string());
    v
}

// ------------------------------------------------------------------ AC-1, AC-7

#[test]
fn ac1_minimal_is_byte_identical_twice_and_matches_the_pinned_digest() {
    let input = load_contents("minimal");
    assert_eq!(input.members.len(), 3);
    let a = build_package(&input).unwrap();
    let b = build_package(&input).unwrap();
    assert_eq!(a, b);
    assert_same_bytes("manifest.json", &a.manifest_bytes, &read("golden/minimal.manifest.json"));
    assert_same_bytes("statement.json", &a.statement_bytes, &read("golden/minimal.statement.json"));
    assert_same_bytes("minimal.zip", &a.container, &read("golden/minimal.zip"));
    // AC-7: this same pinned value is checked on the Linux, Windows and macOS CI runners.
    assert_eq!(hash_bytes(&a.container).hex(), pinned("golden/minimal.zip.sha256"));
    assert!(!a.manifest_bytes.ends_with(b"\n") && !a.statement_bytes.ends_with(b"\n"));
}

#[test]
fn mixed_contents_and_forced_zip64_match_the_reference() {
    for name in ["minimal", "mixed"] {
        let input = load_contents(name);
        let pkg = build_package(&input).unwrap();
        assert_same_bytes(
            &format!("{name}.manifest.json"),
            &pkg.manifest_bytes,
            &read(&format!("golden/{name}.manifest.json")),
        );
        assert_same_bytes(
            &format!("{name}.statement.json"),
            &pkg.statement_bytes,
            &read(&format!("golden/{name}.statement.json")),
        );
        assert_eq!(hash_bytes(&pkg.container).hex(), pinned(&format!("golden/{name}.zip.sha256")), "{name}");
        let z64 = build_package_with(&input, Zip64::Always).unwrap();
        assert_eq!(z64.manifest_bytes, pkg.manifest_bytes);
        assert_eq!(hash_bytes(&z64.container).hex(), pinned(&format!("golden/{name}.zip64.zip.sha256")), "{name}");
    }
}

#[test]
fn member_input_order_does_not_matter() {
    let input = load_contents("mixed");
    let mut reversed = input.clone();
    reversed.members.reverse();
    assert_eq!(build_package(&input).unwrap().container, build_package(&reversed).unwrap().container);
}

// ------------------------------------------------------------------ AC-2

#[test]
fn ac2_listing_is_manifest_order_plus_statement() {
    let pkg = build_package(&load_contents("minimal")).unwrap();
    let want = expected_order(&pkg);
    assert_eq!(central_names(&pkg.container), want);

    let file = temp_file("ac2.zip", &pkg.container);
    let path = file.to_str().unwrap();
    match run_tool("unzip", &["-Z1", path]) {
        Some(out) => {
            let listed: Vec<String> = String::from_utf8(out).unwrap().lines().map(str::to_string).collect();
            assert_eq!(listed, want, "Info-ZIP unzip listing");
            assert!(run_tool("unzip", &["-tq", path]).is_some(), "unzip -t failed");
        }
        None => eprintln!("unzip not available: Info-ZIP listing skipped"),
    }
    let script = "import sys, zipfile\nz = zipfile.ZipFile(sys.argv[1])\nassert z.testzip() is None\n\
                  print('\\n'.join(z.namelist()))";
    match run_tool("python3", &["-c", script, path]).or_else(|| run_tool("python", &["-c", script, path])) {
        Some(out) => {
            let listed: Vec<String> = String::from_utf8(out).unwrap().lines().map(str::to_string).collect();
            assert_eq!(listed, want, "Python zipfile listing");
        }
        None => eprintln!("python not available: zipfile listing skipped"),
    }
    let _ = std::fs::remove_file(&file);
}

// ------------------------------------------------------------------ AC-3

#[test]
fn ac3_manifest_matches_schema_constants_and_member_digests() {
    let input = load_contents("minimal");
    let pkg = build_package(&input).unwrap();
    let parsed = Manifest::parse(&pkg.manifest_bytes).unwrap();
    assert_eq!(parsed, pkg.manifest);
    assert_eq!(parsed.to_canonical_bytes().unwrap(), pkg.manifest_bytes);

    // The published schema names the same constants as the crate.
    let schema = fid_canon::parse(&std::fs::read(SCHEMA).unwrap()).unwrap();
    let s = obj(&schema);
    assert_eq!(st(s, "$schema"), "https://json-schema.org/draft/2020-12/schema");
    let props = obj(&s["properties"]);
    assert_eq!(st(obj(&props["format"]), "const"), FORMAT_NAME);
    let tiers: Vec<&str> = arr(&obj(&props["evidence_tier"])["enum"])
        .iter()
        .map(|t| match t {
            CValue::Str(t) => t.as_str(),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(tiers, ["results", "features", "points"]);
    for t in tiers {
        assert_eq!(EvidenceTier::parse(t).unwrap().as_str(), t);
    }
    assert_eq!(st(obj(&obj(&s["$defs"])["digest"]), "pattern"), Digest::PATTERN);
    assert_eq!(arr(&s["required"]).len(), 7);

    // Every member digest equals an independent hash of its body, and sha256sum agrees.
    for m in &input.members {
        let entry = parsed.members.iter().find(|e| e.path == m.path).unwrap();
        assert_eq!(entry.length, m.body.len() as u64);
        assert_eq!(entry.sha256, hash_bytes(&m.body));
        let file = temp_file("ac3.member", &m.body);
        match run_tool("sha256sum", &[file.to_str().unwrap()]) {
            Some(out) => {
                let text = String::from_utf8(out).unwrap();
                let hex = text.split_whitespace().next().unwrap().trim_start_matches('\\');
                assert_eq!(hex, entry.sha256.hex(), "sha256sum {}", m.path);
            }
            None => eprintln!("sha256sum not available: skipped"),
        }
        let _ = std::fs::remove_file(&file);
    }
}

// ------------------------------------------------------------------ AC-4

#[test]
fn ac4_statement_is_an_in_toto_v1_statement() {
    let pkg = build_package(&load_contents("minimal")).unwrap();
    let root = fid_canon::parse(&pkg.statement_bytes).unwrap();
    let o = obj(&root);
    let keys: Vec<&str> = o.keys().map(String::as_str).collect();
    assert_eq!(keys, ["_type", "predicate", "predicateType", "subject"]);
    assert_eq!(st(o, "_type"), STATEMENT_TYPE);
    assert_eq!(st(o, "predicateType"), PREDICATE_TYPE);
    let subjects = arr(&o["subject"]);
    assert_eq!(subjects.len(), 4, "manifest.json plus three members");
    let manifest_hex = hash_bytes(&pkg.manifest_bytes).hex().to_string();
    for (i, sub) in subjects.iter().enumerate() {
        let sub = obj(sub);
        let digest = obj(&sub["digest"]);
        assert_eq!(digest.len(), 1);
        let hex = st(digest, "sha256");
        assert!(hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
        if i == 0 {
            assert_eq!(st(sub, "name"), "manifest.json");
            assert_eq!(hex, manifest_hex);
        } else {
            let m = &pkg.manifest.members[i - 1];
            assert_eq!(st(sub, "name"), m.path.as_str());
            assert_eq!(hex, m.sha256.hex());
        }
    }
    let p = obj(&o["predicate"]);
    assert_eq!(st(p, "manifest_sha256"), manifest_hex);
    assert_eq!(st(p, "format_version"), "1.0.0");
    assert_eq!(st(p, "result_count"), "2");
    assert_eq!(st(p, "evidence_tier"), "results");
}

// ------------------------------------------------------------------ AC-5

fn path_cases(rel: &str) -> Vec<String> {
    String::from_utf8(read(rel))
        .unwrap()
        .lines()
        .map(|l| l.trim_end_matches('\r'))
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| match fid_canon::parse(l.as_bytes()).unwrap() {
            CValue::Str(s) => s,
            other => panic!("{rel}: not a JSON string: {other:?}"),
        })
        .collect()
}

#[test]
fn ac5_every_bad_path_is_rejected_with_bad_path() {
    let bad = path_cases("paths/bad.txt");
    assert!(bad.len() >= 20);
    for p in &bad {
        let r = std::panic::catch_unwind(|| PkgPath::parse(p));
        match r {
            Ok(Err(e)) => assert_eq!(e.code(), "pkg.bad_path", "{p:?}"),
            Ok(Ok(_)) => panic!("accepted bad path {p:?}"),
            Err(_) => panic!("panicked on {p:?}"),
        }
    }
    for p in path_cases("paths/good.txt") {
        assert_eq!(PkgPath::parse(&p).map(|x| x.as_str().to_string()), Ok(p.clone()));
    }
}

// ------------------------------------------------------------------ AC-6

#[test]
fn ac6_minor_versions_are_read_and_other_majors_refused() {
    let m = Manifest::parse(&read("versions/v1_4_0.json")).unwrap();
    assert_eq!(m.format_version, SemVer::new(1, 4, 0));
    assert_eq!(m.evidence_tier, EvidenceTier::Features);
    let e = Manifest::parse(&read("versions/v2_0_0.json")).unwrap_err();
    assert_eq!(e.code(), "pkg.unsupported_version");
    assert_eq!(e, PkgError::UnsupportedVersion("2.0.0".into()));
}

// ------------------------------------------------------------------ errors

fn member(path: &str, body: &[u8]) -> MemberInput {
    MemberInput { path: PkgPath::parse(path).unwrap(), media_type: "application/json".into(), body: body.to_vec() }
}

fn with_members(members: Vec<MemberInput>) -> PackageInput {
    let mut input = load_contents("minimal");
    input.members = members;
    input
}

fn build_code(members: Vec<MemberInput>) -> Result<(), &'static str> {
    build_package(&with_members(members)).map(|_| ()).map_err(|e| e.code())
}

#[test]
fn builder_errors_are_typed() {
    assert_eq!(build_code(vec![member("results/a", b"1"), member("results/a", b"2")]), Err("pkg.duplicate_path"));
    assert_eq!(build_code(vec![member("results/a", b"1"), member("results/a/b", b"2")]), Err("pkg.duplicate_path"));
    assert_eq!(
        build_code(vec![member("results/a", b"1"), member("results/a-b", b""), member("results/a/b", b"2")]),
        Err("pkg.duplicate_path")
    );
    for p in ["manifest.json", "statement.json", "signatures/0.dsse.json", "other/x", "results"] {
        assert_eq!(build_code(vec![member(p, b"")]), Err("pkg.bad_path"), "{p}");
    }
    let mut bad_type = member("results/a", b"");
    bad_type.media_type = "Application/JSON".into();
    assert_eq!(build_code(vec![bad_type]), Err("pkg.schema_invalid"));
    let mut no_author = with_members(vec![]);
    no_author.created_by.clear();
    assert_eq!(build_package(&no_author).map_err(|e| e.code()).err(), Some("pkg.schema_invalid"));
    // An empty member list is a valid (if useless) package.
    assert!(build_package(&with_members(vec![])).is_ok());
}

#[test]
fn reader_errors_are_typed() {
    let good = String::from_utf8(read("golden/minimal.manifest.json")).unwrap();
    let code = |text: &str| Manifest::parse(text.as_bytes()).map(|_| ()).map_err(|e| e.code());
    assert_eq!(code(&good), Ok(()));
    assert_eq!(code("not json"), Err("pkg.schema_invalid"));
    assert_eq!(code("[]"), Err("pkg.schema_invalid"));
    assert_eq!(code(&good.replace(FORMAT_NAME, "other-format")), Err("pkg.schema_invalid"));
    assert_eq!(code(&good.replace("\"1.0.0\"", "\"0.9.0\"")), Err("pkg.unsupported_version"));
    assert_eq!(code(&good.replace("\"1.0.0\"", "\"1.0\"")), Err("pkg.schema_invalid"));
    assert_eq!(code(&good.replace("\"results\"", "\"everything\"")), Err("pkg.schema_invalid"));
    assert_eq!(code(&good.replace("\"length\":\"52\"", "\"length\":52")), Err("pkg.schema_invalid"));
    assert_eq!(code(&good.replace("\"length\":\"52\"", "\"length\":\"052\"")), Err("pkg.schema_invalid"));
    assert_eq!(code(&good.replace("kernel/manifest.json", "kernel/../x")), Err("pkg.bad_path"));
    assert_eq!(code(&good.replace("kernel/manifest.json", "zzz/manifest.json")), Err("pkg.bad_path"));
    // Out of order and repeated members.
    assert_eq!(code(&good.replace("kernel/manifest.json", "results/zz.json")), Err("pkg.schema_invalid"));
    assert_eq!(code(&good.replace("kernel/manifest.json", "profile/profile.json")), Err("pkg.duplicate_path"));
    let upper = good.replacen("a8f1e12ad5", "A8F1E12AD5", 2);
    assert_eq!(code(&upper), Err("pkg.schema_invalid"));
}

struct Failing;

impl std::io::Read for Failing {
    fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("disk gone"))
    }
}

#[test]
fn streaming_hash_matches_and_reports_io_errors() {
    let data: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
    let (len, digest) = hash_reader(&mut data.as_slice()).unwrap();
    assert_eq!(len, 200_000);
    assert_eq!(digest, hash_bytes(&data));
    assert_eq!(
        hash_bytes(b"abc").hex(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        "FIPS 180-2 test vector"
    );
    assert_eq!(hash_reader(&mut Failing).map_err(|e| e.code()), Err("pkg.io"));
}
