//! Acceptance criteria of spec node AUTO.SURF.PROJ (AC-1 .. AC-6), the seeded fuzz corpus
//! from the independent Python reference (`fixtures/manifest/make_fixtures.py`) and a
//! mutation fuzz that only checks the projection never panics and stays well-formed.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{digest_bytes, parse, CValue};
use fid_surface::projection::{MAX_MANIFEST_BYTES, MAX_MANIFEST_DEPTH};
use fid_surface::{project, McpProjection, ProjError};
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> Vec<u8> {
    std::fs::read(root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn obj(v: &CValue) -> &std::collections::BTreeMap<String, CValue> {
    match v {
        CValue::Obj(o) => o,
        other => panic!("expected an object, got {other:?}"),
    }
}

fn string(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        other => panic!("expected a string, got {other:?}"),
    }
}

fn strings(v: &CValue) -> Vec<String> {
    match v {
        CValue::Arr(items) => items.iter().map(|i| string(i).to_string()).collect(),
        other => panic!("expected an array, got {other:?}"),
    }
}

fn outcome(bytes: &[u8]) -> Result<String, (String, Vec<String>)> {
    match project(bytes) {
        Ok(p) => Ok(digest_bytes(&p.to_canonical_bytes().unwrap()).to_string()),
        Err(e) => Err((e.code().to_string(), e.commands().iter().map(|c| c.to_string()).collect())),
    }
}

/// Expected `{error, commands}` for one of the hand-specified error manifests.
fn expected_error(file: &str) -> (String, Vec<String>) {
    let all = parse(&read("fixtures/projection/errors.expected.json")).unwrap();
    let e = obj(obj(&all).get(file).unwrap());
    (string(e.get("error").unwrap()).to_string(), strings(e.get("commands").unwrap()))
}

/// AC-1: the golden projection, byte for byte.
#[test]
fn ac1_min_classes_golden() {
    let manifest = read("fixtures/manifest/min-classes.json");
    let golden = read("fixtures/projection/min-classes.golden.json");
    let p = project(&manifest).unwrap();
    assert_eq!(String::from_utf8(p.to_canonical_bytes().unwrap()).unwrap(), String::from_utf8(golden).unwrap());
    let none: Vec<&str> =
        p.commands.iter().filter(|c| c.mcp_projection == McpProjection::None).map(|c| c.side_effect.as_str()).collect();
    let mut none_sorted = none.clone();
    none_sorted.sort_unstable();
    assert_eq!(none_sorted, ["A0", "M1", "S1"]);
    assert!(p.commands.iter().any(|c| c.mcp_name == "align_run_bestfit_v2" && c.major == 2));
    // REQ-001: the digest of the exact input bytes.
    assert_eq!(p.manifest_digest.as_str(), digest_bytes(&manifest).to_string());
}

/// AC-2: the three names of `align.run_bestfit@1`.
#[test]
fn ac2_run_bestfit_names() {
    let p = project(&read("fixtures/manifest/min-classes.json")).unwrap();
    let c = p.commands.iter().find(|c| c.command_id.as_str() == "align.run_bestfit@1").unwrap();
    assert_eq!(c.cli_path.join(" "), "fid align run-bestfit");
    assert_eq!(c.mcp_name, "align_run_bestfit");
    assert_eq!(format!("{}.{}", c.py_module, c.py_name), "fiducial.align.run_bestfit");
}

fn assert_error(file: &str, code: &str) {
    let (want_code, want_cmds) = expected_error(file);
    assert_eq!(want_code, code, "reference disagrees with the spec for {file}");
    let got = project(&read(&format!("fixtures/manifest/{file}")));
    let err: ProjError = got.expect_err("must fail and return no projection");
    assert_eq!(err.code(), code, "{err}");
    assert_eq!(err.commands(), want_cmds.iter().map(String::as_str).collect::<Vec<_>>(), "{err}");
}

/// AC-3: an M1 command declaring `exposed`.
#[test]
fn ac3_projection_mismatch() {
    assert_error("bad-projection.json", "projection_mismatch");
}

/// AC-4: `demo.run_fit@2` and `demo.run_fit_v2@1` both derive `demo_run_fit_v2`.
#[test]
fn ac4_name_collision() {
    assert_error("collision.json", "name_collision");
    let e = project(&read("fixtures/manifest/collision.json")).unwrap_err();
    assert!(e.to_string().contains("demo_run_fit_v2"), "{e}");
}

/// AC-5: a 65-character MCP name.
#[test]
fn ac5_name_too_long() {
    assert_error("long-name.json", "name_too_long");
}

/// Declarations stricter than the class ceiling project to themselves, as the bus decides
/// (CORE.BUS.POL.PROJ `mcp_projection`: the stricter of the declaration and the class
/// ceiling): `request.reject_request@1` (W1, `human_only`, `never`) and
/// `request.file_request@1` (W1, `never`) project to `none`, an R0 or W2 command declaring
/// `request_only` to `request_only`, a W3 command declaring `never` to `none`. Golden from
/// the Python reference.
#[test]
fn stricter_declarations_project_to_themselves() {
    let m = read("fixtures/manifest/stricter.json");
    let p = project(&m).unwrap();
    let golden = read("fixtures/projection/stricter.golden.json");
    assert_eq!(String::from_utf8(p.to_canonical_bytes().unwrap()).unwrap(), String::from_utf8(golden).unwrap());
    let of = |id: &str| p.commands.iter().find(|c| c.command_id.as_str() == id).unwrap().mcp_projection;
    assert_eq!(of("request.reject_request@1"), McpProjection::None);
    assert_eq!(of("request.file_request@1"), McpProjection::None);
    assert_eq!(of("align.preview_fit@1"), McpProjection::RequestOnly);
    assert_eq!(of("align.set_datum@1"), McpProjection::RequestOnly);
    assert_eq!(of("rpt.release_report@1"), McpProjection::None);
}

/// The rule, hand-labelled for every ceiling and declaration: at the ceiling projects to the
/// ceiling, stricter to the declaration, looser is refused.
#[test]
fn for_declared_table() {
    use fid_surface::DeclaredProjection as D;
    use McpProjection as M;
    let table = [
        (M::Exposed, D::Exposed, Some(M::Exposed)),
        (M::Exposed, D::RequestOnly, Some(M::RequestOnly)),
        (M::Exposed, D::Never, Some(M::None)),
        (M::Named, D::Exposed, Some(M::Named)),
        (M::Named, D::RequestOnly, Some(M::RequestOnly)),
        (M::Named, D::Never, Some(M::None)),
        (M::RequestOnly, D::Exposed, None),
        (M::RequestOnly, D::RequestOnly, Some(M::RequestOnly)),
        (M::RequestOnly, D::Never, Some(M::None)),
        (M::None, D::Exposed, None),
        (M::None, D::RequestOnly, None),
        (M::None, D::Never, Some(M::None)),
    ];
    for (ceiling, declared, want) in table {
        assert_eq!(McpProjection::for_declared(declared, ceiling), want, "{ceiling:?} {declared:?}");
    }
}

/// AC-6 (in-process half; the cross-OS half is AC-1's golden file on every CI runner).
#[test]
fn ac6_deterministic() {
    let m = read("fixtures/manifest/min-classes.json");
    let a = project(&m).unwrap().to_canonical_bytes().unwrap();
    let b = project(&m).unwrap().to_canonical_bytes().unwrap();
    assert_eq!(a, b);
}

/// The seeded corpus: every case's outcome equals the Python reference's.
#[test]
fn fuzz_corpus_matches_reference() {
    let corpus = parse(&read("fixtures/projection/fuzz.json")).unwrap();
    let CValue::Arr(cases) = obj(&corpus).get("cases").unwrap() else { panic!("cases") };
    assert!(cases.len() >= 200);
    let (mut ok, mut err) = (0, 0);
    for case in cases {
        let case = obj(case);
        let manifest = string(case.get("manifest").unwrap()).as_bytes();
        let expect = obj(case.get("expect").unwrap());
        let id = case.get("case").unwrap();
        let got = outcome(manifest);
        if let Some(d) = expect.get("ok") {
            ok += 1;
            assert_eq!(got, Ok(string(d).to_string()), "case {id:?}");
        } else {
            err += 1;
            let want = (string(expect.get("error").unwrap()).to_string(), strings(expect.get("commands").unwrap()));
            assert_eq!(got, Err(want), "case {id:?}");
        }
    }
    assert!(ok > 50 && err > 50, "corpus is unbalanced: {ok} ok, {err} errors");
}

/// A tiny deterministic PRNG for the mutation fuzz (xorshift64*, pinned seed).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Mutated manifests: never a panic; an `Ok` is always canonical and digests its input.
#[test]
fn mutation_fuzz_never_panics() {
    let base = read("fixtures/manifest/min-classes.json");
    let mut rng = Rng(0x5EED_F00D_0000_0001);
    let alphabet = b"{}[]\":,.@_-0123456789aezAEZ \\\x00\xff";
    let mut accepted = 0;
    for _ in 0..3000 {
        let mut m = base.clone();
        for _ in 0..1 + rng.below(4) {
            let at = rng.below(m.len());
            match rng.below(4) {
                0 => m[at] = alphabet[rng.below(alphabet.len())],
                1 => {
                    m.remove(at);
                }
                2 => m.insert(at, alphabet[rng.below(alphabet.len())]),
                _ => m.truncate(at),
            }
            if m.is_empty() {
                break;
            }
        }
        if let Ok(p) = project(&m) {
            accepted += 1;
            assert_eq!(p.manifest_digest.as_str(), digest_bytes(&m).to_string());
            let bytes = p.to_canonical_bytes().unwrap();
            assert_eq!(fid_canon::canonicalize(&bytes).unwrap(), bytes);
        }
    }
    // Almost every mutation breaks canonical form or a definition digest.
    assert!(accepted < 300, "{accepted} mutated manifests accepted");
}

fn empty_with(commands: &str) -> Vec<u8> {
    format!(r#"{{"commands":{commands},"format":1,"quantity_catalogue_digest":"sha256:{}"}}"#, "0".repeat(64))
        .into_bytes()
}

#[test]
fn refusals_are_typed() {
    // Over the byte limit.
    let mut big = empty_with("[]");
    big.resize(MAX_MANIFEST_BYTES + 1, b' ');
    assert_eq!(project(&big).unwrap_err().code(), "manifest_parse");
    // Over the depth limit (inside a command, where depth is otherwise unchecked).
    let deep = format!("{}{}", "[".repeat(MAX_MANIFEST_DEPTH), "]".repeat(MAX_MANIFEST_DEPTH));
    assert_eq!(project(&empty_with(&format!("[{deep}]"))).unwrap_err().code(), "manifest_parse");
    // Unknown top-level field.
    let extra = br#"{"commands":[],"extra":1,"format":1,"quantity_catalogue_digest":"sha256:0000000000000000000000000000000000000000000000000000000000000000"}"#;
    assert_eq!(project(extra).unwrap_err().code(), "manifest_parse");
    // A definition digest that does not match its definition is refused, not trusted.
    let m = String::from_utf8(read("fixtures/manifest/bad-projection.json")).unwrap();
    let tampered =
        m.replacen("\"title\":\"Fixture align.run_bestfit@1\"", "\"title\":\"Fixture align.run_bestfit@9\"", 1);
    assert_ne!(tampered, m);
    let e = project(tampered.as_bytes()).unwrap_err();
    assert_eq!(e.code(), "manifest_parse");
    assert!(e.to_string().contains("definition_digest"), "{e}");
}
