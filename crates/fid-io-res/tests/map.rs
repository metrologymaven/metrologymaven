//! Acceptance criteria of spec node IO.RES.CSV.MAP (AC-1 .. AC-4) plus the error paths,
//! the dialect and the no-panic property. Expected digests, lossy lists and RFC 4180
//! records come from `fixtures/io/res/csv/map/make_fixtures.py` (stdlib Python, its `json`,
//! `hashlib` and `csv` modules); error reasons are hand-labelled from the spec's rules.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, CValue};
use fid_io_reg::{IoError, LimitKind};
use fid_io_res::csv::map::{
    builtin, builtins, decode_utf8, load_user_map, load_user_map_bytes, ColMapError, DecimalMark, Delimiter, Dialect,
    Field, Layout, LineEnding, Origin, Scope, BUILTIN_REFS, LONG_REF, MAX_MAP_BYTES, WIDE_REF,
};
use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/io/res/csv/map")
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(fixtures().join(name)).unwrap()
}

fn json(name: &str) -> CValue {
    parse(&read(name)).unwrap()
}

fn s<'a>(v: &'a CValue, key: &str) -> &'a str {
    match v {
        CValue::Obj(m) => match m.get(key) {
            Some(CValue::Str(s)) => s,
            other => panic!("{key}: {other:?}"),
        },
        _ => panic!("not an object"),
    }
}

fn arr<'a>(v: &'a CValue, key: &str) -> &'a [CValue] {
    match v {
        CValue::Obj(m) => match m.get(key) {
            Some(CValue::Arr(a)) => a,
            other => panic!("{key}: {other:?}"),
        },
        _ => panic!("not an object"),
    }
}

fn strs(v: &[CValue]) -> Vec<String> {
    v.iter()
        .map(|x| match x {
            CValue::Str(s) => s.clone(),
            other => panic!("{other:?}"),
        })
        .collect()
}

fn names(fields: &[Field]) -> Vec<String> {
    fields.iter().map(|f| f.as_str().to_owned()).collect()
}

fn reason(r: Result<impl std::fmt::Debug, ColMapError>) -> &'static str {
    match r {
        Err(ColMapError::Invalid { reason, .. }) => reason,
        other => panic!("expected colmap_invalid, got {other:?}"),
    }
}

// ------------------------------------------------------------------ AC-1

#[test]
fn ac1_user_map_identity_matches_reference() {
    let expected = json("user_map.expected.json");
    let file = std::fs::File::open(fixtures().join("user_map.json")).unwrap();
    let map = load_user_map(file).unwrap();
    assert_eq!(map.id(), s(&expected, "id"));
    assert_eq!(map.version().to_string(), s(&expected, "version"));
    assert_eq!(map.digest().as_str(), s(&expected, "sha256"));
    assert_eq!(map.origin(), Origin::User);

    let id = map.identity();
    assert_eq!(id.to_cvalue(), expected);
    assert_eq!(map.reference(), "acme.cmm.semicolon@1");
}

#[test]
fn ac1_reordered_keys_give_the_same_digest() {
    let a = load_user_map_bytes(&read("user_map.json")).unwrap();
    let b = load_user_map_bytes(&read("user_map_reordered.json")).unwrap();
    assert_eq!(a.digest(), b.digest());
    assert_eq!(a, b);
}

#[test]
fn ac1_user_map_content() {
    let map = load_user_map_bytes(&read("user_map.json")).unwrap();
    assert_eq!(map.layout(), Layout::Long);
    let d = map.dialect();
    assert_eq!(d.delimiter(), Delimiter::Semicolon);
    assert_eq!(d.decimal_mark(), DecimalMark::Comma);
    assert!(d.header());
    assert_eq!(d.line_ending(), LineEnding::Crlf);
    assert_eq!(map.columns().len(), 8);
    let unit = map.column(Field::Unit).unwrap();
    assert_eq!(unit.index, Some(6));
    assert!(unit.header.is_none());
    assert!(unit.required);
    let actual = map.column(Field::Actual).unwrap();
    assert_eq!(actual.header.as_ref().unwrap().as_display().to_string(), "Istma\u{df}");
    // Lossy: the user map has none of the lossy-check fields.
    assert_eq!(map.lossy(), Field::LOSSY_CHECK.to_vec());
}

#[test]
fn ac1_bom_is_accepted_and_does_not_change_the_digest() {
    let mut with_bom = b"\xEF\xBB\xBF".to_vec();
    with_bom.extend_from_slice(&read("user_map.json"));
    let a = load_user_map_bytes(&with_bom).unwrap();
    let b = load_user_map_bytes(&read("user_map.json")).unwrap();
    assert_eq!(a.digest(), b.digest());
}

// ------------------------------------------------------------------ AC-2

#[test]
fn ac2_error_fixtures_fail_with_their_reasons() {
    for (file, want) in [
        ("dup_column.json", "duplicate_column"),
        ("unknown_field.json", "unknown_field"),
        ("comma_comma.json", "decimal_comma_with_comma_delimiter"),
    ] {
        let err = load_user_map_bytes(&read(file)).unwrap_err();
        assert_eq!(err.code(), "colmap_invalid", "{file}");
        assert_eq!(err.reason(), Some(want), "{file}");
    }
    let err = load_user_map_bytes(&read("dup_column.json")).unwrap_err();
    assert_eq!(err, ColMapError::Invalid { reason: "duplicate_column", column: Some(2) });
    assert_eq!(err.to_string(), "colmap_invalid: duplicate_column (column 2)");
}

// ------------------------------------------------------------------ AC-3

#[test]
fn ac3_lossy_list_is_exactly_the_omitted_fields() {
    let expected = json("lossy.expected.json");
    let want = strs(arr(&expected, "export_lossy.json"));
    assert_eq!(want, ["decision_rule", "record_status"]);
    let map = load_user_map_bytes(&read("export_lossy.json")).unwrap();
    assert_eq!(names(&map.lossy()), want);
    assert!(map.is_lossy());
}

// ------------------------------------------------------------------ AC-4

#[test]
fn ac4_builtin_maps_are_never_lossy_and_match_reference() {
    let expected = match json("builtin.expected.json") {
        CValue::Arr(a) => a,
        other => panic!("{other:?}"),
    };
    assert_eq!(expected.len(), BUILTIN_REFS.len());
    for e in &expected {
        let map = builtin(s(e, "reference")).unwrap();
        assert_eq!(map.reference(), s(e, "reference"));
        assert_eq!(map.id(), s(e, "id"));
        assert_eq!(map.version().to_string(), s(e, "version"));
        assert_eq!(map.digest().as_str(), s(e, "sha256"));
        assert!(strs(arr(e, "lossy")).is_empty());
        assert!(map.lossy().is_empty(), "{}", map.reference());
        assert_eq!(map.origin(), Origin::BuiltIn);
    }
    assert_eq!(builtins().unwrap().len(), 2);
}

#[test]
fn ac4_builtin_layouts() {
    let long = builtin(LONG_REF).unwrap();
    assert_eq!(long.layout(), Layout::Long);
    for f in Field::ALL {
        assert!(long.has_field(*f), "long lacks {f}");
    }
    let wide = builtin(WIDE_REF).unwrap();
    assert_eq!(wide.layout(), Layout::Wide);
    assert!(!wide.has_field(Field::CharacteristicId));
    for f in Field::ALL.iter().filter(|f| **f != Field::CharacteristicId) {
        assert!(wide.has_field(*f), "wide lacks {f}");
    }
    assert_eq!(wide.column(Field::Actual).map(|c| c.field.wide_scope()), Some(Scope::Characteristic));
    assert_eq!(reason(builtin("fid.results.long@2")), "unknown_builtin");
    assert_eq!(reason(builtin("acme.cmm.semicolon@1")), "unknown_builtin");
}

// ------------------------------------------------------------------ strictness

const DIALECT: &str =
    r#"{"delimiter":"comma","decimal_mark":".","header":true,"quote":"rfc4180","encoding":"utf-8","line_ending":"lf"}"#;
const COLS: &str = r#"[{"field":"characteristic_id","header":"ID","required":true}]"#;

fn map_json(id: &str, version: &str, layout: &str, dialect: &str, columns: &str) -> Vec<u8> {
    format!(
        r#"{{"schema":"colmap@1","id":"{id}","version":"{version}","layout":"{layout}","dialect":{dialect},"columns":{columns}}}"#
    )
    .into_bytes()
}

fn load(json: &str) -> Result<fid_io_res::csv::map::ColumnMap, ColMapError> {
    load_user_map_bytes(json.as_bytes())
}

fn with_cols(columns: &str) -> Vec<u8> {
    map_json("acme.t", "1.0.0", "long", DIALECT, columns)
}

fn with_dialect(dialect: &str) -> Vec<u8> {
    map_json("acme.t", "1.0.0", "long", dialect, COLS)
}

#[test]
fn minimal_map_loads() {
    let m = load_user_map_bytes(&with_cols(COLS)).unwrap();
    assert_eq!(m.columns().len(), 1);
}

#[test]
fn top_level_rules() {
    let ok = String::from_utf8(with_cols(COLS)).unwrap();
    assert_eq!(reason(load(&ok.replace(r#""schema":"colmap@1""#, r#""schema":"colmap@2""#))), "unsupported_schema");
    assert_eq!(reason(load(&ok.replace(r#""schema":"colmap@1","#, ""))), "missing_key");
    assert_eq!(reason(load(&ok.replace(r#""schema":"colmap@1""#, r#""schema":"colmap@1","note":"x""#))), "unknown_key");
    assert_eq!(reason(load(&ok.replace(r#""schema":"colmap@1""#, r#""Schema":"colmap@1""#))), "unknown_key");
    assert_eq!(reason(load(&ok.replace(r#""layout":"long""#, r#""layout":"Long""#))), "bad_layout");
    assert_eq!(reason(load(&ok.replace(r#""version":"1.0.0""#, r#""version":1"#))), "wrong_type");
    assert_eq!(reason(load("[]")), "wrong_type");
    assert_eq!(reason(load("{")), "json_syntax");
    assert_eq!(reason(load(r#"{"a":1,"a":2}"#)), "duplicate_key");
    assert_eq!(reason(load_user_map_bytes(b"{\"a\":\"\xff\"}")), "invalid_utf8");
    let too_long = "a".repeat(129);
    for bad in ["", "Acme", "-acme", "a..b", "a b", "acme/x", too_long.as_str()] {
        assert_eq!(reason(load_user_map_bytes(&map_json(bad, "1.0.0", "long", DIALECT, COLS))), "bad_id", "{bad}");
    }
    assert!(load_user_map_bytes(&map_json(&"a".repeat(128), "1.0.0", "long", DIALECT, COLS)).is_ok());
    assert_eq!(
        reason(load_user_map_bytes(&map_json("fid.results.long", "1.0.0", "long", DIALECT, COLS))),
        "reserved_id"
    );
    for bad in ["1", "1.0", "1.0.0.0", "01.0.0", "1.0.0-rc1", "1.a.0", "1..0", "1234567890.0.0", " 1.0.0"] {
        assert_eq!(
            reason(load_user_map_bytes(&map_json("acme.t", bad, "long", DIALECT, COLS))),
            "bad_version",
            "{bad}"
        );
    }
    let v = load_user_map_bytes(&map_json("acme.t", "10.0.123", "long", DIALECT, COLS)).unwrap();
    assert_eq!(v.version().to_string(), "10.0.123");
    assert_eq!(v.reference(), "acme.t@10");
}

#[test]
fn dialect_rules() {
    let d = DIALECT;
    let cases = [
        (d.replace(r#""comma""#, r#""Comma""#), "bad_delimiter"),
        (d.replace(r#""comma""#, r#""pipe""#), "bad_delimiter"),
        (d.replace(r#""decimal_mark":".""#, r#""decimal_mark":"·""#), "bad_decimal_mark"),
        (d.replace(r#""header":true"#, r#""header":"yes""#), "wrong_type"),
        (d.replace(r#""rfc4180""#, r#""none""#), "bad_quote"),
        (d.replace(r#""utf-8""#, r#""latin-1""#), "bad_encoding"),
        (d.replace(r#""utf-8""#, r#""UTF-8""#), "bad_encoding"),
        (d.replace(r#""lf""#, r#""cr""#), "bad_line_ending"),
        (d.replace(r#","line_ending":"lf""#, ""), "missing_key"),
        (d.replace(r#""line_ending":"lf""#, r#""line_ending":"lf","escape":"\\""#), "unknown_key"),
        (d.replace(r#""decimal_mark":".""#, r#""decimal_mark":",""#), "decimal_comma_with_comma_delimiter"),
    ];
    for (dialect, want) in cases {
        assert_eq!(reason(load_user_map_bytes(&with_dialect(&dialect))), want, "{dialect}");
    }
    for (delim, mark) in [("semicolon", ","), ("tab", ","), ("semicolon", "."), ("tab", ".")] {
        let dialect = d
            .replace(r#""comma""#, &format!("\"{delim}\""))
            .replace(r#""decimal_mark":".""#, &format!("\"decimal_mark\":\"{mark}\""));
        assert!(load_user_map_bytes(&with_dialect(&dialect)).is_ok(), "{dialect}");
    }
}

#[test]
fn column_rules() {
    let id = r#"{"field":"characteristic_id","header":"ID","required":true}"#;
    let cases = [
        (format!("[{id},{{\"field\":\"actual\",\"required\":true}}]"), "no_locator", Some(1)),
        (format!("[{id},{{\"field\":\"Actual\",\"index\":1,\"required\":true}}]"), "unknown_field", Some(1)),
        (format!("[{id},{{\"field\":\"actual\",\"index\":1,\"required\":true,\"type\":\"decimal\"}}]"), "unknown_key", Some(1)),
        (format!("[{id},{{\"field\":\"actual\",\"index\":1}}]"), "missing_key", Some(1)),
        (format!("[{id},{{\"field\":\"actual\",\"index\":1,\"required\":1}}]"), "wrong_type", Some(1)),
        (format!("[{id},{{\"field\":\"actual\",\"index\":\"1\",\"required\":true}}]"), "wrong_type", Some(1)),
        (format!("[{id},{{\"field\":\"actual\",\"index\":-1,\"required\":true}}]"), "bad_index", Some(1)),
        (format!("[{id},{{\"field\":\"actual\",\"index\":1.5,\"required\":true}}]"), "bad_index", Some(1)),
        (format!("[{id},{{\"field\":\"actual\",\"index\":65536,\"required\":true}}]"), "bad_index", Some(1)),
        (format!("[{id},{{\"field\":\"actual\",\"header\":\"\",\"required\":true}}]"), "bad_header", Some(1)),
        (format!("[{id},{{\"field\":\"actual\",\"header\":\"a\\nb\",\"required\":true}}]"), "bad_header", Some(1)),
        (format!("[{id},{{\"field\":\"actual\",\"header\":\"{}\",\"required\":true}}]", "h".repeat(257)), "bad_header", Some(1)),
        (format!("[{id},{{\"field\":\"characteristic_id\",\"header\":\"ID2\",\"required\":true}}]"), "duplicate_field", Some(1)),
        (format!("[{id},{{\"field\":\"actual\",\"header\":\"ID\",\"required\":true}}]"), "duplicate_column", Some(1)),
        (
            format!("[{id},{{\"field\":\"actual\",\"index\":3,\"required\":true}},{{\"field\":\"unit\",\"index\":3,\"required\":true}}]"),
            "duplicate_column",
            Some(2),
        ),
        ("[]".to_owned(), "no_columns", None),
        ("{}".to_owned(), "wrong_type", None),
        ("[{\"field\":\"actual\",\"index\":0,\"required\":true}]".to_owned(), "no_characteristic_key", None),
    ];
    for (cols, want, col) in cases {
        let err = load_user_map_bytes(&with_cols(&cols)).unwrap_err();
        assert_eq!(err, ColMapError::Invalid { reason: want, column: col }, "{cols}");
    }
    // Headers differing only in case are different columns; index 65535 is the cap.
    let ok = format!("[{id},{{\"field\":\"actual\",\"header\":\"id\",\"index\":65535,\"required\":true}}]");
    let m = load_user_map_bytes(&with_cols(&ok)).unwrap();
    assert_eq!(m.column(Field::Actual).unwrap().index, Some(65_535));
    // Too many columns.
    let many: Vec<String> =
        (0..1025).map(|i| format!("{{\"field\":\"comment\",\"index\":{i},\"required\":false}}")).collect();
    assert_eq!(reason(load_user_map_bytes(&with_cols(&format!("[{}]", many.join(","))))), "too_many_columns");
}

#[test]
fn header_needs_a_header_row() {
    let dialect = DIALECT.replace(r#""header":true"#, r#""header":false"#);
    let err = load_user_map_bytes(&with_dialect(&dialect)).unwrap_err();
    assert_eq!(err, ColMapError::Invalid { reason: "header_without_header_row", column: Some(0) });
    let by_index =
        map_json("acme.t", "1.0.0", "long", &dialect, r#"[{"field":"characteristic_id","index":0,"required":true}]"#);
    assert!(load_user_map_bytes(&by_index).is_ok());
}

#[test]
fn wide_layout_rules() {
    let part = r#"{"field":"part_id","index":0,"required":true}"#;
    let ok = format!(r#"[{part},{{"field":"actual","header":"Actual","required":true}}]"#);
    let m = load_user_map_bytes(&map_json("acme.w", "1.0.0", "wide", DIALECT, &ok)).unwrap();
    assert_eq!(m.layout(), Layout::Wide);
    let no_header = DIALECT.replace(r#""header":true"#, r#""header":false"#);
    let cases = [
        (
            DIALECT.to_owned(),
            format!(r#"[{part},{{"field":"characteristic_id","header":"C","required":true}}]"#),
            "wide_characteristic_id_column",
        ),
        (
            DIALECT.to_owned(),
            format!(r#"[{part},{{"field":"actual","index":1,"required":true}}]"#),
            "wide_group_needs_header",
        ),
        (
            DIALECT.to_owned(),
            format!(r#"[{part},{{"field":"actual","header":"A","index":1,"required":true}}]"#),
            "wide_group_needs_header",
        ),
        (
            DIALECT.to_owned(),
            format!(r#"[{part},{{"field":"actual","header":"x:A","required":true}}]"#),
            "wide_header_separator",
        ),
        (no_header, format!(r#"[{part}]"#), "wide_needs_header_row"),
    ];
    for (dialect, cols, want) in cases {
        assert_eq!(reason(load_user_map_bytes(&map_json("acme.w", "1.0.0", "wide", &dialect, &cols))), want, "{cols}");
    }
}

#[test]
fn size_and_depth_limits() {
    // Exactly 1 MiB (padded with trailing spaces) loads; one byte more is limit_exceeded.
    let mut exact = with_cols(COLS);
    exact.resize(MAX_MAP_BYTES as usize, b' ');
    assert!(load_user_map_bytes(&exact).is_ok());
    assert!(load_user_map(exact.as_slice()).is_ok());
    let mut over = exact.clone();
    over.push(b' ');
    for err in [load_user_map_bytes(&over).unwrap_err(), load_user_map(over.as_slice()).unwrap_err()] {
        assert_eq!(err.code(), "limit_exceeded");
        assert!(
            matches!(err, ColMapError::Io(IoError::LimitExceeded { limit: LimitKind::Bytes, value, .. }) if value == MAX_MAP_BYTES)
        );
    }
    // Nesting deeper than a map needs.
    let deep = with_cols(r#"[{"field":"characteristic_id","header":[[["x"]]],"required":true}]"#);
    let err = load_user_map_bytes(&deep).unwrap_err();
    assert!(matches!(err, ColMapError::Io(IoError::LimitExceeded { limit: LimitKind::Depth, .. })), "{err:?}");
    let hostile = "[".repeat(100_000);
    assert_eq!(load(&hostile).unwrap_err().code(), "limit_exceeded");
}

// ------------------------------------------------------------------ dialect

fn dialect(name: &str) -> Dialect {
    let delim = Delimiter::parse(name).unwrap();
    Dialect::new(delim, DecimalMark::Point, true, LineEnding::Lf).unwrap()
}

#[test]
fn dialect_matches_python_csv_oracle() {
    let cases = match json("dialect_cases.json") {
        CValue::Arr(a) => a,
        other => panic!("{other:?}"),
    };
    assert!(cases.len() >= 200);
    for c in &cases {
        let d = dialect(s(c, "delimiter"));
        let fields = strs(arr(c, "fields"));
        let refs: Vec<&str> = fields.iter().map(String::as_str).collect();
        let record = s(c, "record");
        let encoded = d.encode_record(&refs);
        assert_eq!(encoded.strip_suffix('\n').unwrap(), record, "encode {fields:?}");
        assert_eq!(d.decode_record(record).unwrap(), fields, "decode {record:?}");
    }
}

#[test]
fn dialect_line_endings_and_names() {
    let lf = Dialect::new(Delimiter::Tab, DecimalMark::Comma, false, LineEnding::Lf).unwrap();
    let crlf = Dialect::new(Delimiter::Semicolon, DecimalMark::Point, true, LineEnding::Crlf).unwrap();
    assert_eq!(lf.encode_record(&["a", "b"]), "a\tb\n");
    assert_eq!(crlf.encode_record(&["a", "b"]), "a;b\r\n");
    assert_eq!(crlf.encode_record(&[]), "\r\n");
    assert_eq!(
        Dialect::new(Delimiter::Comma, DecimalMark::Comma, true, LineEnding::Lf),
        Err("decimal_comma_with_comma_delimiter")
    );
    assert_eq!(
        crlf.to_string(),
        "delimiter=semicolon decimal_mark=. header=true quote=rfc4180 encoding=utf-8 line_ending=crlf"
    );
}

#[test]
fn dialect_decode_errors_are_hand_labelled() {
    let d = dialect("comma");
    let cases: &[(&str, &str, u64)] = &[
        ("", "empty_record", 0),
        ("a\"b", "bare_quote", 1),
        ("\"abc", "unterminated_quote", 0),
        ("x,\"ab\"\"", "unterminated_quote", 2),
        ("\"ab\"c", "text_after_quote", 4),
        ("\"ab\" ,c", "text_after_quote", 4),
        ("a\nb", "unquoted_line_break", 1),
        ("a,b\r", "unquoted_line_break", 3),
    ];
    for (rec, want, at) in cases {
        match d.decode_record(rec) {
            Err(IoError::Malformed { at: pos, reason }) => {
                assert_eq!((reason, pos.byte), (*want, *at), "{rec:?}");
            }
            other => panic!("{rec:?}: {other:?}"),
        }
    }
    // A semicolon is plain text in a comma dialect and vice versa.
    assert_eq!(d.decode_record("a;b").unwrap(), ["a;b"]);
    assert_eq!(dialect("semicolon").decode_record("1,5;2").unwrap(), ["1,5", "2"]);
    // Field cap.
    let many = ",".repeat(70_000);
    assert!(matches!(d.decode_record(&many), Err(IoError::Malformed { reason: "too_many_fields", .. })));
}

#[test]
fn decimal_conversion_is_exact_and_locale_free() {
    let comma = Dialect::new(Delimiter::Semicolon, DecimalMark::Comma, true, LineEnding::Lf).unwrap();
    let point = dialect("comma");
    // Hand-labelled from the grammar in the spec's dialect rules.
    for (text, want) in
        [("0,0100", "0.0100"), ("-0,012", "-0.012"), ("+12", "+12"), ("1,5e-3", "1.5e-3"), ("7E+2", "7E+2")]
    {
        assert_eq!(comma.to_point_decimal(text).unwrap(), want);
        assert_eq!(comma.from_point_decimal(want).unwrap(), text);
    }
    assert_eq!(point.to_point_decimal("0.0100").unwrap(), "0.0100");
    assert_eq!(point.from_point_decimal("-0.012").unwrap(), "-0.012");
    let bad: &[(&Dialect, &str, &str)] = &[
        (&comma, "0.0100", "wrong_decimal_mark"),
        (&comma, ".5", "wrong_decimal_mark"),
        (&point, "0,5", "wrong_decimal_mark"),
        (&point, "1,000.5", "wrong_decimal_mark"),
        (&comma, "1.000,5", "wrong_decimal_mark"),
        (&point, "", "bad_decimal"),
        (&point, "-", "bad_decimal"),
        (&point, "5.", "bad_decimal"),
        (&point, " 5", "bad_decimal"),
        (&point, "5 ", "bad_decimal"),
        (&point, "1e", "bad_decimal"),
        (&point, "1e+", "bad_decimal"),
        (&point, "NaN", "bad_decimal"),
        (&point, "inf", "bad_decimal"),
        (&point, "0x10", "bad_decimal"),
        (&point, "\u{663}", "bad_decimal"),
        (&point, "1\u{a0}000", "bad_decimal"),
        (&point, "--1", "bad_decimal"),
    ];
    for (d, text, want) in bad {
        match d.to_point_decimal(text) {
            Err(IoError::Malformed { reason, .. }) => assert_eq!(reason, *want, "{text:?}"),
            other => panic!("{text:?}: {other:?}"),
        }
    }
    let long = "1".repeat(1025);
    assert!(matches!(point.to_point_decimal(&long), Err(IoError::Malformed { reason: "decimal_too_long", .. })));
}

#[test]
fn utf8_and_bom() {
    assert_eq!(decode_utf8(b"\xEF\xBB\xBFa;b").unwrap(), "a;b");
    assert_eq!(decode_utf8(b"a;b").unwrap(), "a;b");
    // Only one BOM is dropped.
    assert_eq!(decode_utf8(b"\xEF\xBB\xBF\xEF\xBB\xBFa").unwrap(), "\u{feff}a");
    for (bytes, at) in [(&b"ab\xffc"[..], 2u64), (&b"\xEF\xBB\xBF\xC3"[..], 3), (&b"\xFF\xFEa\x00"[..], 0)] {
        match decode_utf8(bytes) {
            Err(IoError::Malformed { at: pos, reason: "invalid_utf8" }) => assert_eq!(pos.byte, at),
            other => panic!("{bytes:?}: {other:?}"),
        }
    }
}

// ------------------------------------------------------------------ never panic

/// Deterministic mutations of the fixtures: every outcome must be a returned value.
#[test]
fn hostile_inputs_never_panic() {
    let seeds = [read("user_map.json"), read("dup_column.json"), with_cols(COLS)];
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let d = dialect("comma");
    let bytes_pool = b"{}[]\",:;\t\r\n\\ae0-.\xff\xc3";
    for round in 0..3000u32 {
        let mut input = seeds[(round as usize) % seeds.len()].clone();
        for _ in 0..(1 + next() % 8) {
            if input.is_empty() {
                break;
            }
            let pos = (next() as usize) % input.len();
            match next() % 3 {
                0 => input[pos] = bytes_pool[(next() as usize) % bytes_pool.len()],
                1 => {
                    input.remove(pos);
                }
                _ => input.insert(pos, bytes_pool[(next() as usize) % bytes_pool.len()]),
            }
        }
        let _ = load_user_map_bytes(&input);
        if let Ok(text) = decode_utf8(&input) {
            let _ = d.decode_record(text);
            let _ = d.to_point_decimal(text);
        }
    }
}
