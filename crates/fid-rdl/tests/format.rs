//! UNC.RDL.FORMAT acceptance tests against `fixtures/rdl/format/` (written by
//! `fixtures/rdl/format/make_fixtures.py`, independently of this crate). Also built for
//! wasm32-wasip1 in CI, so AC-1's digests are checked on wasm32, x86-64 (Linux, Windows)
//! and ARM64 (macOS).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_rdl::{
    command_defs, failed_envelope, invoke_cli, invoke_mcp, public_view, replay, ActivityEntry, RdlDataset, RdlError,
    RdlStore, Unit, ViewForm, MAX_RECORD_BYTES, SHOW_DATASET,
};
use fid_replay::ReplayOutcome;
use fid_surface::{ErrorCode, Outcome};
use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/rdl/format")
}

fn read(rel: &str) -> Vec<u8> {
    std::fs::read(dir().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn valid_names() -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir().join("valid"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|n| n.ends_with(".json"))
        .collect();
    v.sort();
    v
}

fn record(name: &str) -> RdlDataset {
    RdlDataset::from_json(&read(&format!("valid/{name}"))).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn store() -> RdlStore {
    let mut s = RdlStore::new();
    for n in valid_names() {
        s.load_json(&read(&format!("valid/{n}"))).unwrap();
    }
    s
}

fn canon(v: &CValue) -> Vec<u8> {
    fid_canon::to_canonical_bytes(v).unwrap()
}

fn obj(v: &CValue) -> &std::collections::BTreeMap<String, CValue> {
    match v {
        CValue::Obj(o) => o,
        _ => panic!("not an object"),
    }
}

fn keys(v: &CValue) -> Vec<&str> {
    obj(v).keys().map(String::as_str).collect()
}

fn contains(hay: &[u8], needle: &str) -> bool {
    hay.windows(needle.len()).any(|w| w == needle.as_bytes())
}

// ------------------------------------------------------------------ AC-1

#[test]
fn ac1_digests_match_reference() {
    let text = String::from_utf8(read("digests.txt")).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), valid_names().len());
    let tiers: Vec<&str> = lines.iter().map(|l| l.split_whitespace().next().unwrap()).collect();
    for t in ["x_", "g_", "o_", "b_"] {
        assert!(tiers.iter().any(|n| n.starts_with(t)), "no {t} record");
    }
    for line in lines {
        let (name, want) = line.split_once(' ').unwrap();
        let rec = record(name);
        assert_eq!(rec.digest().as_str(), want, "{name}");
        // The digest is SHA-256 of the canonical bytes, and those are fid-canon's RFC 8785
        // form of the input file (the record round-trips exactly).
        let bytes = rec.canonical_bytes().unwrap();
        assert_eq!(fid_canon::digest_bytes(&bytes).to_string(), want, "{name}");
        assert_eq!(bytes, fid_canon::canonicalize(&read(&format!("valid/{name}"))).unwrap(), "{name}");
    }
}

#[test]
fn every_public_view_matches_reference() {
    for name in valid_names() {
        let want = read(&format!("expected/{}.public.json", name.trim_end_matches(".json")));
        assert_eq!(canon(&public_view(&record(&name)).unwrap()), want, "{name}");
    }
}

// ------------------------------------------------------------------ AC-2

#[test]
fn ac2_reference_only_view_strips_content_keeps_digests() {
    let rec = record("x_reference_only.json");
    assert_eq!(fid_rdl::view_form(&rec), ViewForm::ReferenceOnly);
    let view = public_view(&rec).unwrap();
    let k = keys(&view);
    assert!(!k.contains(&"reference") && !k.contains(&"generator") && !k.contains(&"cross_check"));
    for want in ["digest", "reference_digest", "licence", "provenance", "inputs", "id", "aim"] {
        assert!(k.contains(&want), "{want} missing");
    }
    let CValue::Arr(inputs) = &obj(&view)["inputs"] else { panic!() };
    assert_eq!(inputs.len(), rec.inputs().len());
    for (i, v) in inputs.iter().enumerate() {
        assert_eq!(keys(v), ["digest"]);
        assert_eq!(obj(v)["digest"], CValue::Str(rec.inputs()[i].digest.as_str().to_string()));
    }
    assert_eq!(keys(&obj(&view)["provenance"]), ["source"]);
    assert_eq!(obj(&view)["licence"], CValue::Str(rec.licence().to_string()));
    // No reference value, test uncertainty, unit, role or provenance method anywhere.
    let bytes = canon(&view);
    for o in &rec.reference().outputs {
        assert!(!contains(&bytes, &o.value_hex.to_wire()));
        assert!(!contains(&bytes, &o.test_uncertainty_hex.to_wire()));
        assert!(!contains(&bytes, &o.name));
    }
    assert!(!contains(&bytes, "\"mm\"") && !contains(&bytes, "points") && !contains(&bytes, "hand-written"));
}

// ------------------------------------------------------------------ AC-3

#[test]
fn ac3_show_tier_b_returns_only_id_aim_digest() {
    let s = store();
    let mut log = Vec::new();
    let out = invoke_cli(&s, &mut log, &["show-dataset", "--id", "rdl:B/fit.plane.mz/sealed/0001", "--json"]);
    assert_eq!(out.stdout, read("expected/show-tier-b.json"));
    assert_eq!(keys(out.envelope.result.as_ref().unwrap()), ["aim", "digest", "id"]);
    assert_eq!(log.len(), 1);
}

// ------------------------------------------------------------------ AC-4

#[test]
fn ac4_invalid_files_fail_with_named_field() {
    let mut n = 0;
    for e in std::fs::read_dir(dir().join("invalid")).unwrap() {
        let name = e.unwrap().file_name().into_string().unwrap();
        let stem = name.strip_suffix(".json").unwrap();
        let mut parts = stem.split("--");
        let (field, code) = (parts.next().unwrap(), parts.next().unwrap());
        let err = RdlDataset::from_json(&read(&format!("invalid/{name}"))).err().unwrap_or_else(|| panic!("{name}"));
        assert_eq!((err.field(), err.code()), (field, code), "{name}: {err}");
        assert!(err.to_string().starts_with(&format!("{code}: {field}: ")), "{name}");
        n += 1;
    }
    assert!(n >= 30);
}

#[test]
fn size_and_depth_bounds() {
    let base = read("valid/g_line_0001.json");
    let mut exact = base.clone();
    exact.resize(MAX_RECORD_BYTES, b' ');
    assert!(RdlDataset::from_json(&exact).is_ok());
    exact.push(b' ');
    let e = RdlDataset::from_json(&exact).unwrap_err();
    assert_eq!((e.field(), e.code()), ("record", "schema_invalid"));
    // Depth 32 (record plus 31 nested arrays) parses and fails only on the unknown key.
    let nested = |depth: usize| {
        let text = String::from_utf8(base.clone()).unwrap();
        let deep = format!("{}{}", "[".repeat(depth), "]".repeat(depth));
        text.replacen('{', &format!("{{\"comment\":{deep},"), 1).into_bytes()
    };
    assert!(RdlDataset::from_json(&nested(31)).unwrap_err().to_string().contains("unknown key"));
    assert!(RdlDataset::from_json(&nested(32)).unwrap_err().to_string().contains("deeper than 32"));
}

/// Fail-closed: no error about a reference-only record carries any of its content.
#[test]
fn errors_never_leak_restricted_content() {
    let base = String::from_utf8(read("valid/x_reference_only.json")).unwrap();
    let rec = record("x_reference_only.json");
    let mut secrets: Vec<String> = vec![rec.provenance().source.clone(), rec.licence().to_string(), "SECRET".into()];
    secrets.extend(rec.inputs().iter().map(|i| i.digest.hex().to_string()));
    for o in &rec.reference().outputs {
        secrets.push(o.value_hex.to_wire());
        secrets.push(o.test_uncertainty_hex.to_wire());
    }
    let variants = [
        base.replace("\"mm\"", "\"SECRET-unit\""),
        base.replacen('{', "{\"SECRET-key\":1,", 1),
        base.replacen('{', "{\"comment\":\"SECRET-payload\",", 1),
        base.replace("\"radius\"", "\"SECRET Name\""),
        base.replace("LicenseRef-Synthetic-Restricted", "LicenseRef-SECRET OR MIT"),
        base.replace("rdl:X/fit.circle.ls/circle2d/0001", "rdl:X/SECRET"),
        base.replace("\"precision_digits\": 17", "\"precision_digits\": \"SECRET\""),
    ];
    for v in variants {
        let e = RdlDataset::from_json(v.as_bytes()).unwrap_err();
        let shown = format!("{e} {e:?}").into_bytes();
        let env = failed_envelope(SHOW_DATASET, &e).encode().unwrap();
        for s in &secrets {
            assert!(!contains(&shown, s) && !contains(&env, s), "{e} leaks {s}");
        }
    }
}

// ------------------------------------------------------------------ AC-5

#[test]
fn ac5_list_tier_g_in_id_order() {
    let s = store();
    let mut log = Vec::new();
    let out = invoke_cli(&s, &mut log, &["list-dataset", "--tier", "G", "--json"]);
    assert_eq!(out.stdout, read("expected/list-tier-g.json"));
    let CValue::Arr(items) = out.envelope.result.as_ref().unwrap() else { panic!() };
    let ids: Vec<&CValue> = items.iter().map(|i| &obj(i)["id"]).collect();
    assert_eq!(
        ids,
        [&CValue::Str("rdl:G/fit.circle.ls/basic/0001".into()), &CValue::Str("rdl:G/fit.line.ls/basic/0001".into())]
    );
    // The MCP tool returns the same bytes.
    let mcp = invoke_mcp(&s, &mut log, "rdl_list_dataset", br#"{"tier":"G"}"#);
    assert_eq!(mcp.encode().unwrap(), out.stdout);
    // Aim filter, and both filters together.
    let a = invoke_cli(&s, &mut log, &["list-dataset", "--aim", "fit.circle.ls", "--json"]);
    let CValue::Arr(items) = a.envelope.result.as_ref().unwrap() else { panic!() };
    assert_eq!(items.len(), 2);
    let b = invoke_cli(&s, &mut log, &["list-dataset", "--aim", "fit.circle.ls", "--tier", "X"]);
    let CValue::Arr(items) = b.envelope.result.as_ref().unwrap() else { panic!() };
    assert_eq!(items.len(), 1);
    assert_eq!(b.stdout, canon(b.envelope.result.as_ref().unwrap()));
    assert_eq!(log.len(), 4);
}

// ------------------------------------------------------------------ AC-6

#[test]
fn ac6_replay_identical() {
    let s = store();
    let mut log = Vec::new();
    invoke_cli(&s, &mut log, &["show-dataset", "--id", "rdl:B/fit.plane.mz/sealed/0001", "--json"]);
    invoke_cli(&s, &mut log, &["list-dataset", "--tier", "G", "--json"]);
    invoke_cli(&s, &mut log, &["show-dataset", "--id", "rdl:G/fit.none.ls/x/0001", "--json"]);
    assert_eq!(log.len(), 3);
    for entry in &log {
        let back = ActivityEntry::from_json(&entry.to_json().unwrap()).unwrap();
        assert_eq!(&back, entry);
        assert_eq!(replay(&back, &s).unwrap(), ReplayOutcome::Identical);
    }
    // A store without the circle record diverges for the listing only.
    let mut other = RdlStore::new();
    for n in valid_names().into_iter().filter(|n| n != "g_circle_0001.json") {
        other.load_json(&read(&format!("valid/{n}"))).unwrap();
    }
    assert_eq!(replay(&log[0], &other).unwrap(), ReplayOutcome::Identical);
    assert_eq!(replay(&log[1], &other).unwrap(), ReplayOutcome::Divergent);
}

#[test]
fn activity_entry_parsing_is_strict() {
    let s = store();
    let mut log = Vec::new();
    invoke_cli(&s, &mut log, &["list-dataset", "--tier", "G"]);
    let good = String::from_utf8(log[0].to_json().unwrap()).unwrap();
    for bad in [
        good.replace("rdl.list_dataset@1", "rdl.generate_dataset@1"),
        good.replace("\"cli\"", "\"web\""),
        good.replace("\"G\"", "\"Q\""),
        good.replacen('{', "{\"extra\":1,", 1),
        good.replace("sha256:", "sha512:"),
    ] {
        assert!(ActivityEntry::from_json(bad.as_bytes()).is_err(), "{bad}");
    }
}

// ------------------------------------------------------------------ surfaces and misc

#[test]
fn cli_errors() {
    let s = store();
    let mut log = Vec::new();
    let nf = invoke_cli(&s, &mut log, &["show-dataset", "--id", "rdl:O/fit.line.ls/none/9999", "--json"]);
    let err = nf.envelope.error.as_ref().unwrap();
    assert_eq!((nf.envelope.outcome.clone(), err.code.clone()), (Outcome::Failed, ErrorCode::NotFound));
    assert_eq!(log.len(), 1);
    for args in [
        &["show-dataset", "--tier", "G"][..],
        &["list-dataset", "--tier", "Z"],
        &["list-dataset", "--tier", "G", "--tier", "X"],
        &["list-dataset", "--json", "--json"],
        &["show-dataset", "--id", "rdl:G/bad"],
        &["show-dataset"],
        &["remove-dataset"],
        &[],
    ] {
        let out = invoke_cli(&s, &mut log, args);
        assert_eq!(out.envelope.error.as_ref().map(|e| e.code.clone()), Some(ErrorCode::UsageError), "{args:?}");
    }
    assert_eq!(log.len(), 1, "usage errors are not logged");
    let plain = invoke_cli(&s, &mut log, &["show-dataset"]);
    assert!(plain.stdout.is_empty() && plain.stderr.starts_with("error: bad_params: id: "));
    let mcp = invoke_mcp(&s, &mut log, "rdl_show_dataset", br#"{"id":"x","id":"y"}"#);
    assert_eq!(mcp.error.map(|e| e.code), Some(ErrorCode::UsageError));
}

#[test]
fn store_refuses_duplicate_ids() {
    let mut s = store();
    let e = s.load_json(&read("valid/g_line_0001.json")).unwrap_err();
    assert_eq!(e, RdlError::SchemaInvalid { field: "id".into(), reason: "duplicate dataset id" });
}

#[test]
fn units_and_declared_si_conversion() {
    let table: Vec<(&str, &str, &str)> = Unit::ALL.iter().map(|u| (u.as_str(), u.si_unit(), u.si_factor())).collect();
    assert_eq!(
        table,
        [
            ("m", "m", "1"),
            ("mm", "m", "1/1000"),
            ("\u{00b5}m", "m", "1/1000000"),
            ("rad", "rad", "1"),
            ("deg", "rad", "pi/180")
        ]
    );
}

#[test]
fn registry_definitions() {
    let defs = command_defs().unwrap();
    assert_eq!(defs.len(), 2);
}

// ------------------------------------------------------------------ follow-ups of the #257 review

/// Free text refuses every Unicode format character (Cf, pinned table checked against
/// Python's unicodedata by make_fixtures.py) and every control character (Cc), and accepts
/// ordinary non-ASCII text.
#[test]
fn free_text_refuses_format_and_control_characters() {
    let base = String::from_utf8(read("valid/o_sphere_0001.json")).unwrap();
    let source = "open suite stand-in, hand-written";
    assert!(base.contains(source));
    let with = |c: char| base.replace(source, &format!("open suite{c}stand-in"));
    let table = String::from_utf8(read("format-chars.txt")).unwrap();
    let mut n = 0;
    for line in table.lines() {
        let (lo, hi) = line.split_once(' ').unwrap();
        let (lo, hi) = (u32::from_str_radix(lo, 16).unwrap(), u32::from_str_radix(hi, 16).unwrap());
        for u in [lo, (lo + hi) / 2, hi] {
            let c = char::from_u32(u).unwrap();
            assert!(fid_rdl::is_control_or_format(c), "U+{u:04X}");
            let e = RdlDataset::from_json(with(c).as_bytes()).unwrap_err();
            assert_eq!((e.field(), e.code()), ("provenance.source", "schema_invalid"), "U+{u:04X}");
            n += 1;
        }
    }
    assert_eq!(n, 63);
    // Control characters, as JSON escapes (raw U+0000..U+001F is not JSON at all).
    for c in ['\u{0}', '\u{1f}', '\u{7f}', '\u{85}', '\u{9f}'] {
        let escaped = base.replace(source, &format!("open suite\\u{:04x}stand-in", u32::from(c)));
        let e = RdlDataset::from_json(escaped.as_bytes()).unwrap_err();
        assert_eq!(e.field(), "provenance.source", "U+{:04X}", u32::from(c));
    }
    // Neighbours of the Cf ranges and ordinary non-ASCII text are fine.
    for c in ['\u{2029}', '\u{2065}', '\u{00b5}', '\u{00e9}', '\u{fffc}', '\u{1d17b}'] {
        assert!(RdlDataset::from_json(with(c).as_bytes()).is_ok(), "U+{:04X}", u32::from(c));
    }
}

#[test]
fn hex_float_edge_cases() {
    let rec = record("o_sphere_0001.json");
    let sub = rec.reference().outputs.iter().find(|o| o.name == "subnormal_probe").unwrap();
    assert_eq!(sub.value_hex.bits(), 1);
    assert_eq!(sub.value_hex.to_wire(), "0x0000000000000001");
    assert_eq!(sub.test_uncertainty_hex.bits(), 1);
}
