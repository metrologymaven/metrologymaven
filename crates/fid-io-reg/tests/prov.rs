//! Acceptance criteria of spec node IO.REG.PROV (AC-1 .. AC-5) and the pinned Design rules
//! behind them. Fixtures and expected values come from `fixtures/io/reg/prov/`, written by
//! the independent reference script `make_cases.py` next to them (Python `fractions` and
//! `hashlib`); the resolution table and error cases are hand-labelled from the spec.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_io_reg::prov::{
    factor_decimal, principal_commit, source_frame_id, source_frame_name, FrameId, LengthUnit, PrincipalCommit,
    PrincipalDraft, PrincipalKind, MAX_DECLARED, MAX_OPTIONS_BYTES, MAX_OPTIONS_DEPTH, MAX_TEXT_BYTES, SCHEMA,
};
use fid_io_reg::{
    resolve_unit, to_metres, BoundedSource, Budget, Detection, Direction, EffectiveOptions, Evidence, FormatDescriptor,
    FormatId, FormatSelection, FrameCapture, HashingSource, IoError, LimitKind, LimitRequest, Matcher, Origin, Phase,
    Principal, ProvError, ReaderId, ReaderInfo, Registry, Signature, SourceDigest, SourceProvenance, UnitSource,
    UntrustedText, VersionRange, VersionToken,
};
use fid_journal::{Field, RecordedSalts, Salt};
use fid_types::Digest;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/io/reg/prov").join(name)
}

fn expected_lines() -> Vec<String> {
    let text = String::from_utf8(std::fs::read(fixture("expected.txt")).unwrap()).unwrap();
    text.lines().map(str::to_string).collect()
}

fn records(tag: &str) -> Vec<Vec<String>> {
    expected_lines()
        .iter()
        .filter_map(|l| l.strip_prefix(tag).and_then(|r| r.strip_prefix(' ')))
        .map(|r| r.split(' ').map(str::to_string).collect())
        .collect()
}

fn one(tag: &str) -> String {
    let lines = expected_lines();
    let mut hits = lines.iter().filter_map(|l| l.strip_prefix(tag).and_then(|r| r.strip_prefix(' ')));
    let first = hits.next().unwrap().to_string();
    assert!(hits.next().is_none(), "{tag} twice");
    first
}

fn hex_bits(s: &str) -> u64 {
    u64::from_str_radix(s, 16).unwrap()
}

fn unit(sym: &str) -> LengthUnit {
    LengthUnit::ALL.into_iter().find(|u| u.symbol() == sym).unwrap()
}

fn source(bytes: Vec<u8>, limit: Option<u64>) -> HashingSource<std::io::Cursor<Vec<u8>>> {
    let mut req = LimitRequest::default();
    if let Some(l) = limit {
        req = req.bytes(l);
    }
    let budget = Budget::new(Principal::Agent, &req).unwrap();
    HashingSource::new(BoundedSource::new(std::io::Cursor::new(bytes), budget)).unwrap()
}

/// A real digest of `bytes` (SourceDigest has no public constructor).
fn digest_of(bytes: &[u8]) -> SourceDigest {
    source(bytes.to_vec(), None).finish().unwrap().0
}

fn salts(identity: u8, behalf: u8) -> RecordedSalts {
    let mut m = BTreeMap::new();
    m.insert(Field::Identity, Salt([identity; 16]));
    m.insert(Field::OnBehalfOf, Salt([behalf; 16]));
    RecordedSalts(m)
}

fn commit_of(kind: PrincipalKind, identity: &str, behalf: Option<&str>) -> PrincipalCommit {
    let draft = PrincipalDraft {
        kind,
        identity: CValue::Str(identity.to_string()),
        on_behalf_of: behalf.map(|b| {
            let mut m = BTreeMap::new();
            m.insert("id".to_string(), CValue::Str(b.to_string()));
            m.insert("kind".to_string(), CValue::Str("human".to_string()));
            CValue::Obj(m)
        }),
    };
    principal_commit(&draft, &mut salts(0x11, 0x22)).unwrap()
}

/// The AC-1 principal: `make_cases.py` (AC1_SALT = 0x00..0x0f, AC1_IDENTITY = "agent-7").
fn ac1_principal() -> PrincipalCommit {
    let mut m = BTreeMap::new();
    let mut salt = [0u8; 16];
    for (i, b) in salt.iter_mut().enumerate() {
        *b = i as u8;
    }
    m.insert(Field::Identity, Salt(salt));
    let draft =
        PrincipalDraft { kind: PrincipalKind::Agent, identity: CValue::Str("agent-7".into()), on_behalf_of: None };
    principal_commit(&draft, &mut RecordedSalts(m)).unwrap()
}

fn cube() -> Vec<u8> {
    std::fs::read(fixture("cube_mm.xyz")).unwrap()
}

fn fid(s: &str) -> FormatId {
    FormatId::new(s).unwrap()
}

fn xyz_reader() -> ReaderInfo {
    ReaderInfo { id: ReaderId::new("fid-test-xyz").unwrap(), version: VersionToken::new("0.1.0").unwrap() }
}

fn registry() -> Registry {
    let mut r = Registry::new();
    let sig = Signature::new(0, Matcher::Bytes(b"XYZ1".to_vec()), 10).unwrap();
    let d = FormatDescriptor::new(
        fid("fid-test-xyz"),
        VersionRange::new("1", "1").unwrap(),
        vec![sig],
        vec!["xyz".to_string()],
        CValue::Obj(BTreeMap::new()),
        Phase::new(0).unwrap(),
        Origin::FirstParty,
        Direction::Import,
    )
    .unwrap();
    r.register(d).unwrap();
    r
}

fn named_detection() -> Detection {
    let name = fid("fid-test-xyz");
    Detection::detect(&registry(), &[], 96, Some("cube_mm.xyz"), Some(&name)).unwrap()
}

fn options(json: &str) -> EffectiveOptions {
    EffectiveOptions::from_json(json.as_bytes()).unwrap()
}

/// The AC-1 record: the inputs listed in `make_cases.py` (`ac1_record`).
fn ac1_record() -> SourceProvenance {
    let mut src = source(cube(), None);
    let mut all = Vec::new();
    src.read_to_end(&mut all).unwrap();
    let (digest, _) = src.finish().unwrap();
    let unit = resolve_unit(None, Some(LengthUnit::Mm), false).unwrap();
    let mut b = SourceProvenance::builder(
        digest,
        named_detection(),
        xyz_reader(),
        ac1_principal(),
        options(r#"{"delimiter":"space"}"#),
        unit,
    );
    b.source_name(UntrustedText::new("cube_mm.xyz")).unwrap();
    b.build()
}

// ------------------------------------------------------------------ AC-1

#[test]
fn ac1_cube_digest_size_unit_and_conversion() {
    let want = one("cube");
    let (want_len, want_hex) = want.split_once(' ').unwrap();
    let mut src = source(cube(), None);
    let mut text = String::new();
    src.read_to_string(&mut text).unwrap();
    let (digest, _budget) = src.finish().unwrap();
    assert_eq!(digest.bytes(), 96);
    assert_eq!(want_len, "96");
    assert_eq!(digest.sha256().hex(), want_hex);

    let res = resolve_unit(None, Some(LengthUnit::Mm), false).unwrap();
    assert_eq!(res.unit(), LengthUnit::Mm);
    assert_eq!(res.factor_decimal(), "0.001");
    assert_eq!(res.source(), UnitSource::Caller);

    let want_si = records("cube_si");
    assert_eq!(want_si.len(), 8);
    for (i, line) in text.lines().enumerate() {
        let row = &want_si[i];
        assert_eq!(row[0], i.to_string());
        for (axis, tok) in line.split(' ').enumerate() {
            let v: f64 = tok.parse().unwrap();
            let si = res.to_metres(v).unwrap();
            assert_eq!(si.to_bits(), hex_bits(&row[axis + 1]), "corner {i} axis {axis}");
        }
    }
    // Hand check of the AC: point (25.4, 0, 0) becomes 0.0254 m.
    assert_eq!(res.to_metres(25.4).unwrap(), 0.0254);
    assert_eq!(text.lines().nth(1), Some("25.40 0 0"));
}

#[test]
fn digest_covers_the_whole_file_even_when_the_reader_stops_early() {
    let mut src = source(cube(), None);
    let mut head = [0u8; 10];
    assert_eq!(src.read_bounded(&mut head).unwrap(), 10);
    assert_eq!(src.bytes_hashed(), 10);
    let (digest, budget) = src.finish().unwrap();
    assert_eq!(digest.bytes(), 96);
    assert_eq!(digest.sha256().hex(), one("cube").split_once(' ').unwrap().1);
    assert_eq!(budget.bytes_used(), 96);
}

#[test]
fn hashing_stays_inside_the_byte_limit() {
    // The drain in finish() is charged like any read: a file above the limit fails.
    let src = source(cube(), Some(50));
    let err = src.finish().unwrap_err();
    assert_eq!(err, IoError::LimitExceeded { limit: LimitKind::Bytes, value: 50, at: fid_io_reg::Position::byte(50) });
    // Exactly at the limit is fine.
    let (d, _) = source(cube(), Some(96)).finish().unwrap();
    assert_eq!(d.bytes(), 96);
    // Empty input: SHA-256 of nothing (FIPS 180-2 known answer).
    let (d, _) = source(Vec::new(), None).finish().unwrap();
    assert_eq!(d.bytes(), 0);
    assert_eq!(d.sha256().hex(), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
}

// ------------------------------------------------------------------ AC-2, AC-3, resolution table

#[test]
fn ac2_no_declared_and_no_given_unit_is_unit_required() {
    for over in [false, true] {
        let e = resolve_unit(None, None, over).unwrap_err();
        assert_eq!(e, IoError::UnitRequired);
        assert_eq!(e.code(), "unit_required");
    }
}

#[test]
fn ac3_declared_millimetre_and_given_inch_is_unit_conflict() {
    // A 3MF model declaring unit="millimeter" hands the reader's declared unit `mm`.
    let e = resolve_unit(Some(LengthUnit::Mm), Some(LengthUnit::In), false).unwrap_err();
    assert_eq!(e, IoError::UnitConflict);
    assert_eq!(e.code(), "unit_conflict");
    assert!(fid_io_reg::CODES.contains(&"unit_conflict"));
    // With override_unit the caller's unit wins and the provenance records the override.
    let r = resolve_unit(Some(LengthUnit::Mm), Some(LengthUnit::In), true).unwrap();
    assert_eq!((r.unit(), r.source(), r.overridden()), (LengthUnit::In, UnitSource::Override, true));
}

#[test]
fn resolution_table_is_the_pinned_one() {
    use LengthUnit::{Ft, Mm};
    use UnitSource::{Caller, Declared, Override};
    // (declared, given, override) -> Ok((unit, source, overridden)) or Err(code); hand-labelled
    // from the table in NODE.md Design.
    type Row = (Option<LengthUnit>, Option<LengthUnit>, bool, Result<(LengthUnit, UnitSource, bool), &'static str>);
    let rows: [Row; 10] = [
        (None, None, false, Err("unit_required")),
        (None, None, true, Err("unit_required")),
        (None, Some(Ft), false, Ok((Ft, Caller, true))),
        (None, Some(Ft), true, Ok((Ft, Caller, true))),
        (Some(Mm), None, false, Ok((Mm, Declared, false))),
        (Some(Mm), None, true, Err("unit_required")),
        (Some(Mm), Some(Mm), false, Ok((Mm, Declared, false))),
        (Some(Mm), Some(Mm), true, Ok((Mm, Declared, false))),
        (Some(Mm), Some(Ft), false, Err("unit_conflict")),
        (Some(Mm), Some(Ft), true, Ok((Ft, Override, true))),
    ];
    for (d, g, o, want) in rows {
        let got = resolve_unit(d, g, o).map(|r| (r.unit(), r.source(), r.overridden())).map_err(|e| e.code());
        assert_eq!(got, want, "{d:?} {g:?} {o}");
    }
}

// ------------------------------------------------------------------ AC-4, conversion

#[test]
fn ac4_one_inch_is_the_nearest_f64_to_0_0254() {
    assert_eq!(to_metres(1.0, LengthUnit::In).unwrap().to_bits(), 0x3F9A_0275_2546_0AA6);
    assert_eq!(factor_decimal(LengthUnit::In), "0.0254");
}

#[test]
fn conversion_matches_the_exact_rational_reference() {
    let cases = records("conv");
    assert!(cases.len() > 1000, "{}", cases.len());
    let mut by_unit = BTreeMap::new();
    for c in &cases {
        let x = f64::from_bits(hex_bits(&c[1]));
        let got = to_metres(x, unit(&c[0])).unwrap();
        assert_eq!(got.to_bits(), hex_bits(&c[2]), "{} {}", c[0], c[1]);
        *by_unit.entry(c[0].clone()).or_insert(0) += 1;
    }
    let units: Vec<&str> = by_unit.keys().map(String::as_str).collect();
    assert_eq!(units, ["ft", "in", "m", "mm", "nm", "thou", "um"]);
}

#[test]
fn single_rounding_differs_from_multiplying_by_the_rounded_factor() {
    // The reason for the integer path: x * fl(0.001) rounds twice. Hand-picked from the
    // reference set: at least one case where the two differ must be in it.
    let cases = records("conv");
    let differs = cases.iter().filter(|c| c[0] == "mm").any(|c| {
        let x = f64::from_bits(hex_bits(&c[1]));
        (x * 1e-3).to_bits() != hex_bits(&c[2])
    });
    assert!(differs);
}

#[test]
fn non_finite_lengths_are_refused() {
    for u in LengthUnit::ALL {
        for x in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(to_metres(x, u).is_err());
        }
    }
    assert_eq!(to_metres(-0.0, LengthUnit::Mm).unwrap().to_bits(), (-0.0f64).to_bits());
}

// ------------------------------------------------------------------ AC-5, the record

#[test]
fn ac5_canonical_json_is_the_reference_bytes() {
    let rec = ac1_record();
    let bytes = rec.canonical_bytes().unwrap();
    assert_eq!(String::from_utf8(bytes.clone()).unwrap(), one("provenance"));
    // Stable across calls and through a parse and re-canonicalisation.
    assert_eq!(rec.canonical_bytes().unwrap(), bytes);
    assert_eq!(fid_canon::canonicalize(&bytes).unwrap(), bytes);
    let d = rec.digest().unwrap();
    assert_eq!(d, Digest::from_bytes(fid_canon::digest_bytes(&bytes).0));
}

#[test]
fn record_holds_every_req001_field() {
    let rec = ac1_record();
    let v = rec.to_cvalue();
    let CValue::Obj(map) = &v else { panic!() };
    let keys: Vec<&str> = map.keys().map(String::as_str).collect();
    let want = [
        "bytes",
        "declared",
        "evidence",
        "format",
        "format_selected_by",
        "format_version",
        "frame",
        "frame_name",
        "frame_origin",
        "options",
        "origin_claim",
        "principal",
        "reader",
        "reader_version",
        "schema",
        "sha256",
        "source_name",
        "unit",
        "unit_factor",
        "unit_overridden",
        "unit_source",
    ];
    assert_eq!(keys, want);
    assert_eq!(map["schema"], CValue::Str(SCHEMA.to_string()));
    assert_eq!(map["bytes"], CValue::U64(96));
    let CValue::Obj(p) = &map["principal"] else { panic!() };
    assert_eq!(p["kind"], CValue::Str("agent".to_string()));
    assert_eq!(p["on_behalf_of_commit"], CValue::Null);
}

#[test]
fn synthesized_frame_is_source_prefix_at_world_identity() {
    let rec = ac1_record();
    let want = one("frame");
    let (want_id, want_name) = want.split_once(' ').unwrap();
    assert_eq!(rec.frame_id().to_string(), want_id);
    assert_eq!(rec.frame_id(), source_frame_id(&rec.source().sha256()));
    assert_eq!(source_frame_name(&rec.source().sha256()), want_name);
    let f = rec.source_frame().unwrap();
    assert_eq!(f.id, rec.frame_id());
    assert_eq!(f.name, want_name);
    assert_eq!(f.parent, Some(FrameId::WORLD));
    assert_eq!(f.to_parent, fid_geo_frame::Transform::IDENTITY);
    assert_eq!(f.kind, fid_geo_frame::FrameKind::User);
    // It inserts into a fresh document frame tree.
    let mut tree = fid_geo_frame::FrameTree::new();
    tree.insert(f).unwrap();
}

#[test]
fn declared_frame_is_recorded_as_declared() {
    let id = FrameId::from_bytes([7; 16]);
    let mut b = builder_with(digest_of(&[1; 3]));
    b.frame(FrameCapture::Declared { id, label: Some(UntrustedText::new("PCS\u{202e}1")) }).unwrap();
    let rec = b.build();
    assert_eq!(rec.frame_id(), id);
    assert!(rec.source_frame().is_none());
    let CValue::Obj(map) = rec.to_cvalue() else { panic!() };
    assert_eq!(map["frame"], CValue::Str("07".repeat(16)));
    assert_eq!(map["frame_origin"], CValue::Str("declared".to_string()));
    assert_eq!(map["frame_name"], CValue::Str("PCS\u{202e}1".to_string()));
    assert!(rec.to_string().contains("frame: 07070707070707070707070707070707 declared \"PCS\\u{202e}1\""));
    let long = UntrustedText::new(&"a".repeat(256));
    let e = b.frame(FrameCapture::Declared { id, label: Some(long) }).unwrap_err();
    assert_eq!(e.code(), "too_large");
}

fn builder_with(digest: SourceDigest) -> fid_io_reg::ProvenanceBuilder {
    SourceProvenance::builder(
        digest,
        named_detection(),
        xyz_reader(),
        commit_of(PrincipalKind::Human, "alice", None),
        EffectiveOptions::empty(),
        resolve_unit(Some(LengthUnit::Mm), None, false).unwrap(),
    )
}

// ------------------------------------------------------------------ detection, reader, principal

#[test]
fn detection_result_records_how_the_format_was_chosen() {
    let reg = registry();
    let named = Detection::detect(&reg, b"XYZ1 data", 9, None, Some(&fid("fid-test-xyz"))).unwrap();
    assert_eq!(named.selected_by(), FormatSelection::Caller);
    let sig = Detection::detect(&reg, b"XYZ1 data", 9, Some("a.xyz"), None).unwrap();
    assert_eq!(sig.selected_by(), FormatSelection::ContentSignature);
    assert_eq!(sig.format(), &fid("fid-test-xyz"));
    // Detection errors pass through unchanged.
    let e = Detection::detect(&reg, b"nothing", 7, None, None).unwrap_err();
    assert_eq!(e.code(), "format_unknown");

    let mut b = builder_with(digest_of(&[0; 0]));
    let rec = b.format_version(VersionToken::new("1").unwrap()).build();
    let CValue::Obj(map) = rec.to_cvalue() else { panic!() };
    assert_eq!(map["format_selected_by"], CValue::Str("caller".to_string()));
    assert_eq!(map["format_version"], CValue::Str("1".to_string()));
    assert_eq!(map["reader"], CValue::Str("fid-test-xyz".to_string()));
    assert_eq!(map["reader_version"], CValue::Str("0.1.0".to_string()));
    assert_eq!(map["unit_source"], CValue::Str("declared".to_string()));
    assert_eq!(map["unit_overridden"], CValue::Bool(false));
}

#[test]
fn reader_ids_and_versions_are_checked() {
    assert!(ReaderId::new("Fid").is_err());
    assert!(ReaderId::new("").is_err());
    assert!(ReaderId::new(&"a".repeat(65)).is_err());
    assert!(VersionToken::new("*").is_err());
    assert!(VersionToken::new("1 0").is_err());
    assert!(VersionToken::new(&"1".repeat(33)).is_err());
    assert!(VersionToken::new("2.0.1+build-7").is_ok());
}

#[test]
fn every_principal_kind_is_recorded_by_its_journal_wire_name() {
    for &p in PrincipalKind::ALL {
        let rec = SourceProvenance::builder(
            digest_of(b""),
            named_detection(),
            xyz_reader(),
            commit_of(p, "id", None),
            EffectiveOptions::empty(),
            resolve_unit(Some(LengthUnit::M), None, false).unwrap(),
        )
        .build();
        let CValue::Obj(map) = rec.to_cvalue() else { panic!() };
        let CValue::Obj(pm) = &map["principal"] else { panic!() };
        assert_eq!(pm["kind"], CValue::Str(p.as_str().to_string()));
    }
}

// ------------------------------------------------------------------ evidence and untrusted text

#[test]
fn imported_content_is_never_measured() {
    let mut b = builder_with(digest_of(&[2; 1]));
    assert_eq!(b.build().evidence(), Evidence::Uncharacterized);
    b.origin_claim(UntrustedText::new("measured on CMM 42, calibrated")).unwrap();
    let rec = b.build();
    assert_eq!(rec.evidence(), Evidence::Claimed);
    // Exhaustive: adding an evidence value breaks this test on purpose.
    for e in [Evidence::Uncharacterized, Evidence::Claimed] {
        match e {
            Evidence::Uncharacterized => assert_eq!(e.as_str(), "uncharacterized"),
            Evidence::Claimed => assert_eq!(e.as_str(), "claimed"),
        }
    }
    let CValue::Obj(map) = rec.to_cvalue() else { panic!() };
    assert_eq!(map["evidence"], CValue::Str("claimed".to_string()));
    assert_eq!(map["origin_claim"], CValue::Str("measured on CMM 42, calibrated".to_string()));
    // A second claim is refused, not merged.
    let e = b.origin_claim(UntrustedText::new("other")).unwrap_err();
    assert_eq!(e, ProvError::DuplicateKey { field: "origin_claim" });
}

#[test]
fn untrusted_text_is_kept_exactly_and_shown_escaped() {
    let mut b = builder_with(digest_of(&[3; 1]));
    let hostile = "evil\u{202e}txt.xyz\n\"ignore previous\" \\";
    b.source_name(UntrustedText::new(hostile)).unwrap();
    b.declare("header.originating_system", UntrustedText::new("Tool\u{0}X")).unwrap();
    let rec = b.build();
    // Machine form: the exact text (RFC 8785 string escaping only).
    let CValue::Obj(map) = rec.to_cvalue() else { panic!() };
    assert_eq!(map["source_name"], CValue::Str(hostile.to_string()));
    let json = String::from_utf8(rec.canonical_bytes().unwrap()).unwrap();
    let want = format!(r#""source_name":"evil{}txt.xyz\n\"ignore previous\" \\""#, '\u{202e}');
    assert!(json.contains(&want), "{json}");
    assert!(json.contains(r#""declared":{"header.originating_system":"Tool\u0000X"}"#), "{json}");
    // People form: escaped, quoted, one line per field.
    let shown = rec.to_string();
    assert!(shown.contains("name: \"evil\\u{202e}txt.xyz\\u{a}\\\"ignore previous\\\" \\\\\"\n"), "{shown}");
    assert!(shown.contains("declared header.originating_system: \"Tool\\u{0}X\"\n"), "{shown}");
    assert!(!shown.contains('\u{202e}'));
    assert!(shown.lines().all(|l| !l.is_empty()));
}

#[test]
fn untrusted_text_above_the_bound_is_refused_not_cut() {
    let mut b = builder_with(digest_of(&[4; 1]));
    let long = UntrustedText::new(&"x".repeat(MAX_TEXT_BYTES + 1));
    assert_eq!(b.source_name(long.clone()).unwrap_err().code(), "too_large");
    assert_eq!(b.origin_claim(long.clone()).unwrap_err().code(), "too_large");
    assert_eq!(b.declare("k", long).unwrap_err().code(), "too_large");
    assert!(b.source_name(UntrustedText::new(&"x".repeat(MAX_TEXT_BYTES))).is_ok());
    let rec = b.build();
    assert!(rec.origin_claim().is_none());
    assert!(rec.declared().is_empty());
}

// ------------------------------------------------------------------ duplicate keys

#[test]
fn declared_keys_refuse_duplicates_case_variants_and_overflow() {
    let mut b = builder_with(digest_of(&[5; 1]));
    b.declare("schema", UntrustedText::new("a")).unwrap();
    assert_eq!(
        b.declare("schema", UntrustedText::new("b")).unwrap_err(),
        ProvError::DuplicateKey { field: "declared" }
    );
    // Keys are lowercase by rule, so a case variant is refused before it can collide.
    assert_eq!(b.declare("Schema", UntrustedText::new("c")).unwrap_err().code(), "invalid");
    for bad in ["", "a..b", ".a", "a.", "1a", "a-b", "a b", &"a".repeat(65)] {
        assert_eq!(b.declare(bad, UntrustedText::new("v")).unwrap_err().code(), "invalid", "{bad:?}");
    }
    assert_eq!(b.build().declared()["schema"], UntrustedText::new("a"));
    for i in 1..MAX_DECLARED {
        b.declare(&format!("k{i}"), UntrustedText::new("v")).unwrap();
    }
    let e = b.declare("one_more", UntrustedText::new("v")).unwrap_err();
    assert_eq!(e, ProvError::TooLarge { field: "declared", limit: MAX_DECLARED as u64 });
}

#[test]
fn options_refuse_duplicate_and_case_only_duplicate_keys() {
    let dup = EffectiveOptions::from_json(br#"{"unit":"mm","unit":"in"}"#).unwrap_err();
    assert_eq!(dup, ProvError::DuplicateKey { field: "options" });
    let case = EffectiveOptions::from_json(br#"{"Unit":"mm","unit":"in"}"#).unwrap_err();
    assert_eq!(case, ProvError::CaseDuplicateKey { field: "options" });
    let nested = EffectiveOptions::from_json(br#"{"a":[{"x":1},{"k":{"Key":1,"kEY":2}}]}"#).unwrap_err();
    assert_eq!(nested.code(), "case_duplicate_key");
    let nested_dup = EffectiveOptions::from_json(br#"{"a":{"k":1,"k":2}}"#).unwrap_err();
    assert_eq!(nested_dup.code(), "duplicate_key");
    // Built directly (no JSON text): the case rule still applies.
    let mut m = BTreeMap::new();
    m.insert("Delimiter".to_string(), CValue::Str("a".into()));
    m.insert("delimiter".to_string(), CValue::Str("b".into()));
    assert_eq!(EffectiveOptions::new(CValue::Obj(m)).unwrap_err().code(), "case_duplicate_key");
    // Distinct keys are fine, and the value is kept.
    let ok = options(r#"{"a":1,"b":{"a":2}}"#);
    assert!(matches!(ok.value(), CValue::Obj(m) if m.len() == 2));
}

#[test]
fn options_are_bounded() {
    assert_eq!(EffectiveOptions::from_json(b"[1]").unwrap_err().code(), "invalid");
    assert_eq!(EffectiveOptions::from_json(b"{").unwrap_err().code(), "invalid");
    assert_eq!(EffectiveOptions::new(CValue::F64Bits(f64::NAN)).unwrap_err().code(), "invalid");
    // Depth: MAX_OPTIONS_DEPTH nested objects pass, one more fails.
    let nest = |n: usize| format!("{}1{}", r#"{"a":"#.repeat(n), "}".repeat(n));
    assert!(EffectiveOptions::from_json(nest(MAX_OPTIONS_DEPTH).as_bytes()).is_ok());
    assert_eq!(EffectiveOptions::from_json(nest(MAX_OPTIONS_DEPTH + 1).as_bytes()).unwrap_err().code(), "too_large");
    // Far deeper than fid-canon's own cap: still a typed error, no stack overflow.
    assert_eq!(EffectiveOptions::from_json(nest(5000).as_bytes()).unwrap_err().code(), "too_large");
    // Size.
    let big = format!(r#"{{"a":"{}"}}"#, "x".repeat(MAX_OPTIONS_BYTES));
    assert_eq!(EffectiveOptions::from_json(big.as_bytes()).unwrap_err().code(), "too_large");
    let mut m = BTreeMap::new();
    m.insert("a".to_string(), CValue::Str("x".repeat(MAX_OPTIONS_BYTES)));
    assert_eq!(EffectiveOptions::new(CValue::Obj(m)).unwrap_err().code(), "too_large");
}

// ------------------------------------------------------------------ review fixes (#250)

#[test]
fn principal_keeps_on_behalf_of_so_alice_and_bob_differ() {
    // One server session imports the same file for alice and for bob.
    let file = cube();
    let rec = |behalf: &str| {
        SourceProvenance::builder(
            digest_of(&file),
            named_detection(),
            xyz_reader(),
            commit_of(PrincipalKind::Service, "server-session-1", Some(behalf)),
            EffectiveOptions::empty(),
            resolve_unit(Some(LengthUnit::Mm), None, false).unwrap(),
        )
        .build()
    };
    let (a, b) = (rec("alice"), rec("bob"));
    assert_eq!(a.principal().commit, b.principal().commit);
    assert!(a.principal().on_behalf_of_commit.is_some());
    assert_ne!(a.principal().on_behalf_of_commit, b.principal().on_behalf_of_commit);
    assert_ne!(a.canonical_bytes().unwrap(), b.canonical_bytes().unwrap());
    assert_ne!(a.digest().unwrap(), b.digest().unwrap());
    // No raw identity appears in the record.
    let json = String::from_utf8(a.canonical_bytes().unwrap()).unwrap();
    assert!(!json.contains("alice") && !json.contains("server-session-1"), "{json}");
    // The header check: equal commitments pass, bob's header fails alice's record.
    a.verify_principal(&a.principal()).unwrap();
    assert_eq!(a.verify_principal(&b.principal()).unwrap_err().code(), "principal_mismatch");
}

#[test]
fn principal_commit_matches_the_journal_commitment_rule() {
    let p = commit_of(PrincipalKind::Agent, "agent-7", Some("alice"));
    let want_id = fid_journal::commitment(&Salt([0x11; 16]), &CValue::Str("agent-7".into())).unwrap();
    assert_eq!(p.commit, want_id);
    let mut m = BTreeMap::new();
    m.insert("id".to_string(), CValue::Str("alice".into()));
    m.insert("kind".to_string(), CValue::Str("human".into()));
    let want_behalf = fid_journal::commitment(&Salt([0x22; 16]), &CValue::Obj(m)).unwrap();
    assert_eq!(p.on_behalf_of_commit, Some(want_behalf));
    // A missing salt is a typed error.
    let draft = PrincipalDraft { kind: PrincipalKind::Agent, identity: CValue::Str("x".into()), on_behalf_of: None };
    let e = principal_commit(&draft, &mut RecordedSalts::default()).unwrap_err();
    assert_eq!(e.code(), "principal_commit");
}

#[test]
fn hashing_source_refuses_a_source_already_read() {
    let budget = Budget::new(Principal::Agent, &LimitRequest::default()).unwrap();
    let mut inner = BoundedSource::new(std::io::Cursor::new(cube()), budget);
    let mut b = [0u8; 4];
    inner.read_bounded(&mut b).unwrap();
    let e = HashingSource::new(inner).unwrap_err();
    assert_eq!(e, ProvError::SourceAlreadyRead { offset: 4 });
    assert_eq!(e.code(), "source_already_read");
}

#[test]
fn options_deeper_than_the_parser_cap_report_the_parser_cap() {
    let nest = |n: usize| format!("{}1{}", r#"{"a":"#.repeat(n), "}".repeat(n));
    let e = EffectiveOptions::from_json(nest(5000).as_bytes()).unwrap_err();
    assert_eq!(e, ProvError::TooLarge { field: "options", limit: fid_canon::MAX_DEPTH as u64 });
}
