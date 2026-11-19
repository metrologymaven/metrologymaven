//! Acceptance criteria of spec node TRUST.PKG.READER (AC-1, AC-2, AC-3 behaviour, AC-5),
//! the limits of REQ-001 to REQ-005, result-schema validation of `verify.inspect_package@1`
//! and a bounded property test. AC-3's memory bound is `reader_bomb_memory.rs`; AC-4's
//! in-tree mutation fuzzing is `reader_fuzz.rs` (the cargo-fuzz target is `fuzz/`).
//!
//! Every expected code, reason and inventory comes from the independent Python generator
//! `fixtures/pkg/make_reader_fixtures.py`, never from this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_pkg::reader::{
    declarations, inspect, invoke_cli, invoke_mcp, FileOpener, HandleTable, Limits, PackageReader, ReadError,
    LIMIT_REASONS, MALFORMED_REASONS, MCP_INSPECT,
};
use fid_pkg::{build_package, hash_bytes, EvidenceTier, MemberInput, PackageInput, PkgPath};
use fid_registry::decl::CommandDecl;
use fid_registry::validate;
use fid_types::Digest;
use std::collections::BTreeMap;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/pkg");

fn fix(rel: &str) -> PathBuf {
    Path::new(FIX).join(rel)
}

fn read(rel: &str) -> Vec<u8> {
    let p = fix(rel);
    std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn open(bytes: &[u8]) -> Result<PackageReader<Cursor<&[u8]>>, ReadError> {
    PackageReader::open(Cursor::new(bytes))
}

/// Open, then verify every member: the strict path every verifier takes.
fn strict(bytes: &[u8]) -> Result<(), ReadError> {
    open(bytes)?.verify_all()
}

fn obj(v: &CValue) -> &BTreeMap<String, CValue> {
    match v {
        CValue::Obj(o) => o,
        other => panic!("expected object, got {other:?}"),
    }
}

fn st<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> &'a str {
    match o.get(k) {
        Some(CValue::Str(s)) => s,
        other => panic!("{k}: expected string, got {other:?}"),
    }
}

fn dirs(rel: &str) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> =
        std::fs::read_dir(fix(rel)).unwrap().map(|e| e.unwrap().path()).filter(|p| p.is_dir()).collect();
    v.sort();
    v
}

fn valid_names() -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(fix("reader/valid"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter_map(|n| n.strip_suffix(".zip").map(str::to_string))
        .collect();
    v.sort();
    v
}

// ------------------------------------------------------------------ AC-1

#[test]
fn ac1_cli_inspect_lists_every_member_ok() {
    let path = fix("golden/minimal.zip");
    let out = invoke_cli(&FileOpener, &["inspect-package", path.to_str().unwrap(), "--json"]);
    assert_eq!(out.code, 0, "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        String::from_utf8(out.stdout.clone()).unwrap(),
        String::from_utf8(read("reader/valid/minimal.inspect.json")).unwrap()
    );
    let v = fid_canon::parse(out.stdout.trim_ascii_end()).unwrap();
    let o = obj(&v);
    assert_eq!(st(o, "format_version"), "1.0.0");
    let CValue::Arr(members) = &o["members"] else { panic!("members") };
    assert_eq!(members.len(), 3);
    assert!(members.iter().all(|m| st(obj(m), "status") == "ok"));
}

#[test]
fn ac1_text_output_and_usage() {
    let path = fix("golden/minimal.zip");
    let p = path.to_str().unwrap();
    let out = invoke_cli(&FileOpener, &["inspect-package", p]);
    assert_eq!(out.code, 0);
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.starts_with("format_version 1.0.0\nevidence_tier results\n"), "{text}");
    assert_eq!(text.lines().filter(|l| l.starts_with("ok ")).count(), 3);
    for args in
        [&[][..], &["show"], &["inspect-package"], &["inspect-package", p, "--yaml"], &["inspect-package", p, p]]
    {
        assert_eq!(invoke_cli(&FileOpener, args).code, 2, "{args:?}");
    }
    let missing = invoke_cli(&FileOpener, &["inspect-package", "no/such/file.zip", "--json"]);
    assert_eq!(missing.code, 1);
    let v = fid_canon::parse(missing.stdout.trim_ascii_end()).unwrap();
    assert_eq!(st(obj(&obj(&v)["error"]), "code"), "pkg.io");
}

// ------------------------------------------------------------------ valid packages

#[test]
fn valid_packages_open_verify_and_inspect_as_expected() {
    let names = valid_names();
    assert!(names.len() >= 6, "{names:?}");
    for name in names {
        let bytes = read(&format!("reader/valid/{name}.zip"));
        strict(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        let inv = inspect(Cursor::new(&bytes[..])).unwrap();
        let mut got = inv.to_canonical_bytes();
        got.push(b'\n');
        assert_eq!(
            String::from_utf8(got).unwrap(),
            String::from_utf8(read(&format!("reader/valid/{name}.inspect.json"))).unwrap(),
            "{name}"
        );
    }
}

#[test]
fn verified_member_bytes_match_the_manifest() {
    let bytes = read("reader/valid/signed.zip");
    let mut r = open(&bytes).unwrap();
    let members = r.manifest().members.clone();
    for m in &members {
        let body = r.read_member(m.path.as_str(), 1 << 20).unwrap().unwrap();
        assert_eq!(body.len() as u64, m.length);
        assert_eq!(hash_bytes(&body), m.sha256);
        let mut s = r.open_member(m.path.as_str(), 1 << 20).unwrap().unwrap();
        let mut again = Vec::new();
        s.read_to_end(&mut again).unwrap();
        assert_eq!(again, body);
        assert_eq!(r.verify_member(m.path.as_str()), Ok(Some(())));
    }
    let statement = r.read_member("statement.json", 1 << 20).unwrap().unwrap();
    assert_eq!(statement, read("golden/minimal.statement.json"));
    assert_eq!(r.read_member("signatures/0.dsse.json", 1 << 20).unwrap().unwrap(), br#"{"payloadType":"test-only"}"#);
    assert_eq!(r.signature_count(), 2);
    assert!(r.has_statement());
    assert_eq!(r.read_member("results/absent.json", 1 << 20), Ok(None));
    assert_eq!(r.verify_member("results/absent.json"), Ok(None));
    // The caller's buffer cap is checked before anything is read.
    let e = r.read_member("results/records.json", 10).unwrap_err();
    assert_eq!((e.code(), e.reason()), ("pkg.limit", "buffer_cap"));
    assert_eq!(
        r.entry_names(),
        [
            "kernel/manifest.json",
            "manifest.json",
            "profile/profile.json",
            "results/records.json",
            "signatures/0.dsse.json",
            "signatures/1.dsse.json",
            "statement.json"
        ]
    );
}

#[test]
fn real_deflate_members_decode() {
    let bytes = read("reader/valid/deflate.zip");
    let mut r = open(&bytes).unwrap();
    for name in ["lines-l1", "random-l6", "text-l9"] {
        let body = r.read_member(&format!("points/{name}.bin"), 1 << 20).unwrap().unwrap();
        let listed = r.manifest().members.iter().find(|m| m.path.as_str() == format!("points/{name}.bin")).unwrap();
        assert_eq!(hash_bytes(&body), listed.sha256, "{name}");
        assert!(!read(&format!("reader/inflate/{name}.deflate")).is_empty());
    }
}

// ------------------------------------------------------------------ AC-2

#[test]
fn ac2_every_malformed_fixture_yields_its_expected_code() {
    let cases = dirs("malformed");
    assert!(cases.len() >= 80, "{} cases", cases.len());
    let mut codes = std::collections::BTreeSet::new();
    for dir in cases {
        let name = dir.file_name().unwrap().to_string_lossy().to_string();
        let expect = fid_canon::parse(read(&format!("malformed/{name}/expect.json")).trim_ascii_end()).unwrap();
        let o = obj(&expect);
        let bytes = read(&format!("malformed/{name}/package.zip"));
        let got = strict(&bytes).expect_err(&name);
        assert_eq!((got.code(), got.reason()), (st(o, "error_code"), st(o, "reason")), "{name}: {got}");
        assert!(
            LIMIT_REASONS.contains(&got.reason())
                || MALFORMED_REASONS.contains(&got.reason())
                || ["length", "sha256", "unlisted", "version"].contains(&got.reason()),
            "{name}"
        );
        codes.insert(got.code());
        let inspect_rel = format!("malformed/{name}/inspect.json");
        if fix(&inspect_rel).exists() {
            let inv = inspect(Cursor::new(&bytes[..])).unwrap_or_else(|e| panic!("{name}: {e}"));
            let mut b = inv.to_canonical_bytes();
            b.push(b'\n');
            assert_eq!(String::from_utf8(b).unwrap(), String::from_utf8(read(&inspect_rel)).unwrap(), "{name}");
        } else {
            // The container itself fails: inspect fails with the same error.
            assert_eq!(inspect(Cursor::new(&bytes[..])).unwrap_err(), got, "{name}");
        }
    }
    // Every code of the spec except pkg.io (which needs a failing source) is in the corpus.
    let want = ["pkg.digest_mismatch", "pkg.limit", "pkg.malformed", "pkg.unlisted_member", "pkg.unsupported_version"];
    assert_eq!(codes.into_iter().collect::<Vec<_>>(), want);
}

// ------------------------------------------------------------------ AC-3 (behaviour)

#[test]
fn ac3_zip_bomb_stops_with_limit() {
    let bytes = read("malformed/bomb_member_length/package.zip");
    assert!((40_000..=46_000).contains(&bytes.len()), "{} bytes", bytes.len());
    let mut r = open(&bytes).unwrap();
    let e = r.read_member("results/records.json", 1 << 30).unwrap_err();
    assert_eq!((e.code(), e.reason()), ("pkg.limit", "member_length"));
    let e = r.verify_member("results/records.json").unwrap_err();
    assert_eq!(e.reason(), "member_length");
}

// ------------------------------------------------------------------ one view of the signed files

/// Serves package `a` until `switch` is set, then package `b` (same length): a source that
/// changes under the reader.
struct TwoFaced {
    a: Vec<u8>,
    b: Vec<u8>,
    switch: std::rc::Rc<std::cell::Cell<bool>>,
    pos: u64,
}

impl Read for TwoFaced {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let data = if self.switch.get() { &self.b } else { &self.a };
        let rest = data.get(self.pos as usize..).unwrap_or_default();
        let n = rest.len().min(buf.len());
        buf[..n].copy_from_slice(&rest[..n]);
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for TwoFaced {
    fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
        self.pos = match to {
            SeekFrom::Start(p) => p,
            SeekFrom::End(d) => (self.a.len() as u64).saturating_add_signed(d),
            SeekFrom::Current(d) => self.pos.saturating_add_signed(d),
        };
        Ok(self.pos)
    }
}

#[test]
fn signed_files_are_served_as_read_at_open() {
    let pkg = |result_count: u64, created_by: &str| {
        let input = PackageInput {
            created_by: created_by.into(),
            evidence_tier: EvidenceTier::Results,
            profile_digest: Digest::parse(&format!("sha256:{}", "a".repeat(64))).unwrap(),
            kernel_manifest_digest: Digest::parse(&format!("sha256:{}", "b".repeat(64))).unwrap(),
            result_count,
            members: vec![MemberInput {
                path: PkgPath::parse("results/records.json").unwrap(),
                media_type: "application/json".into(),
                body: b"[]".to_vec(),
            }],
        };
        build_package(&input).unwrap()
    };
    // Same layout; the statements differ in one byte (result_count 2 vs 3) ...
    let (a, b) = (pkg(2, "writer A"), pkg(3, "writer A"));
    assert_eq!(a.container.len(), b.container.len());
    let switch = std::rc::Rc::new(std::cell::Cell::new(false));
    let src = TwoFaced { a: a.container.clone(), b: b.container.clone(), switch: switch.clone(), pos: 0 };
    let mut r = PackageReader::open(src).unwrap();
    assert_eq!(r.manifest_bytes(), &a.manifest_bytes[..]);
    assert_eq!(r.statement_sha256(), Some(hash_bytes(&a.statement_bytes)));
    switch.set(true);
    let e = r.read_member("statement.json", 1 << 20).unwrap_err();
    assert_eq!((e.code(), e.reason()), ("pkg.malformed", "source_changed"));
    assert_eq!(r.verify_member("statement.json").unwrap_err().reason(), "source_changed");
    // ... and manifests that differ in one byte (created_by "writer A" vs "writer B").
    let (a, b) = (pkg(2, "writer A"), pkg(2, "writer B"));
    assert_eq!(a.container.len(), b.container.len());
    let switch = std::rc::Rc::new(std::cell::Cell::new(false));
    let src = TwoFaced { a: a.container.clone(), b: b.container.clone(), switch: switch.clone(), pos: 0 };
    let mut r = PackageReader::open(src).unwrap();
    switch.set(true);
    // The manifest is served from the bytes parsed at open, never re-read ...
    assert_eq!(r.read_member("manifest.json", 1 << 20).unwrap().unwrap(), a.manifest_bytes);
    // ... and re-verifying the container copy notices the change.
    assert_eq!(r.verify_member("manifest.json").unwrap_err().reason(), "source_changed");
}

#[test]
fn zip64_end_record_with_extensible_data_is_refused() {
    let bytes = read("malformed/zip64_extensible_data/package.zip");
    assert_eq!(strict(&bytes).unwrap_err().reason(), "zip64_record");
    strict(&read("reader/valid/minimal.zip64.zip")).unwrap();
}

// ------------------------------------------------------------------ limits and failing sources

/// A source of `len` zero bytes that never holds them.
struct Huge {
    len: u64,
    pos: u64,
}

impl Read for Huge {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = (self.len.saturating_sub(self.pos)).min(buf.len() as u64) as usize;
        buf[..n].fill(0);
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for Huge {
    fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
        self.pos = match to {
            SeekFrom::Start(p) => p,
            SeekFrom::End(d) => self.len.saturating_add_signed(d),
            SeekFrom::Current(d) => self.pos.saturating_add_signed(d),
        };
        Ok(self.pos)
    }
}

/// A source whose reads fail.
struct Broken;

impl Read for Broken {
    fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("disk on fire"))
    }
}

impl Seek for Broken {
    fn seek(&mut self, _: SeekFrom) -> std::io::Result<u64> {
        Ok(100)
    }
}

#[test]
fn req001_package_size_limit_is_checked_first() {
    let over = Huge { len: 16 * 1024 * 1024 * 1024 + 1, pos: 0 };
    let e = PackageReader::open(over).unwrap_err();
    assert_eq!((e.code(), e.reason()), ("pkg.limit", "package_size"));
    let at = Huge { len: 16 * 1024 * 1024 * 1024, pos: 0 };
    assert_eq!(PackageReader::open(at).unwrap_err().reason(), "no_eocd");
}

#[test]
fn io_errors_are_pkg_io() {
    let e = PackageReader::open(Broken).unwrap_err();
    assert_eq!(e.code(), "pkg.io");
}

#[test]
fn limits_are_configurable_and_checked() {
    let bytes = read("golden/minimal.zip");
    let opened = |limits: Limits| PackageReader::open_with(Cursor::new(&bytes[..]), limits).map(|_| ());
    let d = Limits::default();
    assert_eq!(opened(d), Ok(()));
    let reason = |l: Limits| opened(l).unwrap_err().reason();
    assert_eq!(reason(Limits { max_package_bytes: bytes.len() as u64 - 1, ..d }), "package_size");
    assert_eq!(reason(Limits { max_members: 4, ..d }), "member_count");
    assert_eq!(reason(Limits { max_name_bytes: 19, ..d }), "path_length");
    assert_eq!(reason(Limits { max_manifest_bytes: 100, ..d }), "manifest_size");
    assert_eq!(reason(Limits { max_index_bytes: 1000, ..d }), "index_budget");
    assert_eq!(opened(Limits { max_members: 5, max_name_bytes: 20, ..d }), Ok(()));
}

// ------------------------------------------------------------------ AC-5

#[test]
fn ac5_mcp_result_equals_cli_json() {
    let path = fix("golden/minimal.zip");
    let mut table = HandleTable::new();
    table.insert("pkg-1", &path);
    let mcp = invoke_mcp(&table, MCP_INSPECT, br#"{"package":"pkg-1"}"#).unwrap();
    let cli = invoke_cli(&FileOpener, &["inspect-package", path.to_str().unwrap(), "--json"]);
    let mut with_newline = mcp.clone();
    with_newline.push(b'\n');
    assert_eq!(with_newline, cli.stdout);

    let err_code = |r: Result<Vec<u8>, Vec<u8>>| {
        let v = fid_canon::parse(&r.unwrap_err()).unwrap();
        st(obj(&obj(&v)["error"]), "code").to_string()
    };
    assert_eq!(err_code(invoke_mcp(&table, "verify_other", b"{}")), "mcp.unknown_tool");
    assert_eq!(err_code(invoke_mcp(&table, MCP_INSPECT, b"[]")), "params.invalid");
    assert_eq!(err_code(invoke_mcp(&table, MCP_INSPECT, br#"{"package":"pkg-1","x":1}"#)), "params.invalid");
    assert_eq!(err_code(invoke_mcp(&table, MCP_INSPECT, br#"{"package":""}"#)), "params.invalid");
    assert_eq!(err_code(invoke_mcp(&table, MCP_INSPECT, br#"{"package":"pkg-1","package":"x"}"#)), "params.invalid");
    // A raw path is not a handle on MCP.
    let raw = format!(r#"{{"package":{:?}}}"#, path.to_str().unwrap());
    assert_eq!(err_code(invoke_mcp(&table, MCP_INSPECT, raw.as_bytes())), "pkg.io");
    let mut bad = HandleTable::new();
    bad.insert("bomb", fix("malformed/digest_mismatch/package.zip"));
    let v = fid_canon::parse(&invoke_mcp(&bad, MCP_INSPECT, br#"{"package":"bomb"}"#).unwrap()).unwrap();
    let CValue::Arr(m) = &obj(&v)["members"] else { panic!() };
    assert!(m.iter().any(|m| st(obj(m), "status") == "digest_mismatch"));
}

// ------------------------------------------------------------------ declaration and schema

/// The JSON Schema keywords the declared schemas use, checked against a value.
fn conforms(schema: &CValue, v: &CValue, at: &str) -> Result<(), String> {
    let s = obj(schema);
    for (k, kv) in s {
        let ok = match (k.as_str(), kv) {
            ("description" | "title", _) => true,
            ("type", CValue::Str(t)) => {
                matches!(
                    (t.as_str(), v),
                    ("object", CValue::Obj(_))
                        | ("array", CValue::Arr(_))
                        | ("string", CValue::Str(_))
                        | ("boolean", CValue::Bool(_))
                ) || (t == "integer" && matches!(v, CValue::Num(n) if n.fract() == 0.0))
            }
            ("enum", CValue::Arr(e)) => e.contains(v),
            ("minimum", CValue::Num(n)) => matches!(v, CValue::Num(x) if x >= n),
            ("maximum", CValue::Num(n)) => matches!(v, CValue::Num(x) if x <= n),
            ("minLength", CValue::Num(n)) => matches!(v, CValue::Str(x) if x.chars().count() as f64 >= *n),
            ("maxLength", CValue::Num(n)) => matches!(v, CValue::Str(x) if x.chars().count() as f64 <= *n),
            ("pattern", CValue::Str(p)) => {
                let CValue::Str(x) = v else { return Err(format!("{at}: pattern on a non-string")) };
                matches_pattern(p, x)
            }
            ("required", CValue::Arr(r)) => {
                let CValue::Obj(o) = v else { return Err(format!("{at}: required on a non-object")) };
                r.iter().all(|k| matches!(k, CValue::Str(k) if o.contains_key(k)))
            }
            ("additionalProperties", CValue::Bool(false)) => {
                let (CValue::Obj(o), Some(CValue::Obj(props))) = (v, s.get("properties")) else {
                    return Err(format!("{at}: additionalProperties without properties"));
                };
                o.keys().all(|k| props.contains_key(k))
            }
            ("properties", CValue::Obj(props)) => {
                if let CValue::Obj(o) = v {
                    for (pk, ps) in props {
                        if let Some(pv) = o.get(pk) {
                            conforms(ps, pv, &format!("{at}/{pk}"))?;
                        }
                    }
                }
                true
            }
            ("items", item) => {
                if let CValue::Arr(a) = v {
                    for (i, x) in a.iter().enumerate() {
                        conforms(item, x, &format!("{at}/{i}"))?;
                    }
                }
                true
            }
            (other, _) => return Err(format!("{at}: keyword {other} not supported by the test checker")),
        };
        if !ok {
            return Err(format!("{at}: fails {k}"));
        }
    }
    Ok(())
}

/// The three patterns the inventory declares, checked by hand.
fn matches_pattern(p: &str, x: &str) -> bool {
    let dec = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && (s == "0" || !s.starts_with('0'));
    let hex = |s: &str| s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    match p {
        "^(0|[1-9][0-9]*)$" => dec(x),
        "^[0-9a-f]{64}$" => hex(x),
        "^sha256:[0-9a-f]{64}$" => x.strip_prefix("sha256:").is_some_and(hex),
        r"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$" => {
            let parts: Vec<&str> = x.split('.').collect();
            parts.len() == 3 && parts.iter().all(|s| dec(s))
        }
        other => panic!("pattern {other} not handled by the test checker"),
    }
}

#[test]
fn declaration_validates_and_results_conform_to_result_schema() {
    let decls = declarations();
    assert_eq!(decls.len(), 1);
    let d = &decls[0].def;
    assert_eq!((d.id.as_str(), d.major), ("verify.inspect_package", 1));
    assert_eq!(fid_registry::tool_name(&d.id, d.major), MCP_INSPECT);
    let raw: Vec<_> = decls.iter().map(CommandDecl::to_validation).collect();
    assert_eq!(validate(&raw), Vec::new());
    let catalogue = fid_registry::Catalogue::geo_frame().unwrap();
    for schema in [&d.params_schema, &d.result_schema] {
        assert_eq!(fid_registry::check_schema(schema, &catalogue), Vec::new());
    }
    conforms(&d.params_schema, &fid_canon::parse(br#"{"package":"pkg-1"}"#).unwrap(), "params").unwrap();
    assert!(conforms(&d.params_schema, &fid_canon::parse(br#"{"package":""}"#).unwrap(), "params").is_err());
    let mut n = 0;
    for name in valid_names() {
        let v = fid_canon::parse(read(&format!("reader/valid/{name}.inspect.json")).trim_ascii_end()).unwrap();
        conforms(&d.result_schema, &v, &name).unwrap();
        n += 1;
    }
    for dir in dirs("malformed") {
        let p = dir.join("inspect.json");
        if p.exists() {
            let v = fid_canon::parse(std::fs::read(&p).unwrap().trim_ascii_end()).unwrap();
            conforms(&d.result_schema, &v, &p.display().to_string()).unwrap();
            n += 1;
        }
    }
    assert!(n >= 12, "{n}");
    // A value outside the schema is refused by the checker.
    let bad = fid_canon::parse(
        br#"{"format_version":"1.0","evidence_tier":"results","members":[],"profile_digest":"x","signature_count":0}"#,
    )
    .unwrap();
    assert!(conforms(&d.result_schema, &bad, "bad").is_err());
}

// ------------------------------------------------------------------ property test

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

#[test]
fn property_written_packages_read_back_exactly() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let dirs = ["journal", "kernel", "points", "profile", "results"];
    let media = ["application/json", "application/zstd", "image/png", "application/octet-stream"];
    for round in 0..60 {
        let count = 1 + rng.below(8) as usize;
        let mut members = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        while members.len() < count {
            let path = format!("{}/m{}.bin", dirs[rng.below(5) as usize], rng.below(1000));
            if !seen.insert(path.clone()) {
                continue;
            }
            let big = rng.below(10) == 0;
            let len = rng.below(if big { 200_000 } else { 300 }) as usize;
            let body: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
            members.push(MemberInput {
                path: PkgPath::parse(&path).unwrap(),
                media_type: media[rng.below(4) as usize].into(),
                body,
            });
        }
        let input = PackageInput {
            created_by: "property test".into(),
            evidence_tier: EvidenceTier::Points,
            profile_digest: Digest::parse(&format!("sha256:{}", "a".repeat(64))).unwrap(),
            kernel_manifest_digest: Digest::parse(&format!("sha256:{}", "b".repeat(64))).unwrap(),
            result_count: round,
            members: members.clone(),
        };
        let built = build_package(&input).unwrap();
        let mut r = open(&built.container).unwrap_or_else(|e| panic!("round {round}: {e}"));
        assert_eq!(r.manifest(), &built.manifest);
        for m in &members {
            assert_eq!(r.read_member(m.path.as_str(), u64::MAX).unwrap().unwrap(), m.body, "round {round}");
        }
        assert_eq!(r.read_member("statement.json", u64::MAX).unwrap().unwrap(), built.statement_bytes);
        r.verify_all().unwrap();
        // A one-bit change anywhere is refused, or changes nothing the reader vouches for
        // (only informational header bits, such as "version made by" or permission bits,
        // can change unnoticed).
        for _ in 0..8 {
            let mut bent = built.container.clone();
            let at = rng.below(bent.len() as u64) as usize;
            bent[at] ^= 1 << rng.below(8);
            if let Ok(mut r2) = open(&bent) {
                if r2.verify_all().is_ok() {
                    assert_eq!(r2.manifest(), &built.manifest, "round {round}, byte {at}");
                    for m in &members {
                        let body = r2.read_member(m.path.as_str(), u64::MAX).unwrap().unwrap();
                        assert_eq!(body, m.body, "round {round}, byte {at}");
                    }
                }
            }
        }
    }
}
