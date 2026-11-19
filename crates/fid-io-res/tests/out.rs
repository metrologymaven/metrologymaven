//! Acceptance criteria of spec node IO.RES.CSV.OUT (AC-1 .. AC-6) plus the requirements'
//! edge cases. Expected tables, lossy lists and digests come from
//! `fixtures/io/res/csv/out/make_fixtures.py` (stdlib Python: its own rendering of the
//! spec rules, written with the `csv` module); error codes and the AC-3 cells are
//! hand-labelled from the spec.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, CValue};
use fid_io_res::csv::map::{builtin, load_user_map_bytes, ColumnMap, Field, LONG_REF, WIDE_REF};
use fid_io_res::csv::out::{
    check_marked, export_results_csv, load_records, neutralise, read_table, render, replay, split_records,
    unneutralise, ExportEvent, ExportRequest, OutError, ResultRow, TMP_PREFIX,
};
use fid_replay::ReplayOutcome;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/io/res/csv/out")
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(fixtures().join(name)).unwrap()
}

fn records() -> Vec<ResultRow> {
    load_records(&read("records.json")).unwrap()
}

fn user_map(name: &str) -> ColumnMap {
    load_user_map_bytes(&read(name)).unwrap()
}

fn expected(case: &str) -> CValue {
    match parse(&read("expected.json")).unwrap() {
        CValue::Obj(m) => m.get(case).cloned().unwrap(),
        _ => panic!("expected.json"),
    }
}

fn field<'a>(v: &'a CValue, key: &str) -> &'a CValue {
    match v {
        CValue::Obj(m) => m.get(key).unwrap(),
        _ => panic!("not an object"),
    }
}

fn str_field<'a>(v: &'a CValue, key: &str) -> &'a str {
    match field(v, key) {
        CValue::Str(s) => s,
        other => panic!("{key}: {other:?}"),
    }
}

fn num_field(v: &CValue, key: &str) -> u64 {
    match field(v, key) {
        CValue::Num(x) => *x as u64,
        other => panic!("{key}: {other:?}"),
    }
}

fn lossy_list(v: &CValue) -> Vec<String> {
    match field(v, "lossy") {
        CValue::Arr(a) => a
            .iter()
            .map(|x| match x {
                CValue::Str(s) => s.clone(),
                _ => panic!("lossy"),
            })
            .collect(),
        _ => panic!("lossy"),
    }
}

/// A fresh, empty export root folder.
fn root(tag: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("fid-io-res-out-{}-{tag}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn request<'a>(map: &'a ColumnMap, target: &'a str) -> ExportRequest<'a> {
    ExportRequest { map, target, characteristics: None, overwrite: false, dry_run: false }
}

fn listing(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> =
        std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    names
}

/// Exports the fixture records through `map` and compares with the oracle's golden table.
fn golden_case(case: &str, map: &ColumnMap) {
    let exp = expected(case);
    let dir = root(case);
    let out = export_results_csv(&records(), &request(map, "root://out/r.csv"), &dir).unwrap();
    let written = std::fs::read(dir.join("r.csv")).unwrap();
    assert_eq!(written, read(str_field(&exp, "golden")), "{case}: bytes differ from the golden table");
    assert_eq!(out.bytes, written);
    assert_eq!(out.report.sha256().as_str(), str_field(&exp, "sha256"));
    assert_eq!(out.report.bytes(), num_field(&exp, "bytes"));
    assert_eq!(out.event.rows, num_field(&exp, "rows"));
    assert_eq!(out.event.output.sha256.as_str(), str_field(&exp, "sha256"));
    let lossy = lossy_list(&exp);
    assert_eq!(out.event.lossy, lossy);
    let mut sorted = lossy.clone();
    sorted.sort();
    assert_eq!(out.report.lossy().iter().map(|n| n.as_str().to_owned()).collect::<Vec<_>>(), sorted);
    assert_eq!(listing(&dir), vec!["r.csv".to_owned()], "no temp file may remain");
}

#[test]
fn ac1_long_builtin_matches_golden_and_is_not_lossy() {
    let map = builtin(LONG_REF).unwrap();
    golden_case("long", &map);
    let out = export_results_csv(&records(), &request(&map, "root://out/r.csv"), &root("ac1")).unwrap();
    assert!(out.report.lossy().is_empty());
    assert_eq!(out.event.rows, 13);
    assert!(!out.report.dry_run());
    assert_eq!(out.event.map, map.identity());
}

#[test]
fn wide_builtin_matches_golden() {
    golden_case("wide", &builtin(WIDE_REF).unwrap());
}

#[test]
fn ac2_user_map_without_decision_rule_is_lossy() {
    let map = user_map("no_decision_rule.json");
    golden_case("no_decision_rule", &map);
    let out = export_results_csv(&records(), &request(&map, "root://out/r.csv"), &root("ac2")).unwrap();
    assert_eq!(out.event.lossy, vec!["decision_rule".to_owned()]);
    let report: Vec<&str> = out.report.lossy().iter().map(|n| n.as_str()).collect();
    assert_eq!(report, vec!["decision_rule"]);
    let journal = String::from_utf8(out.event.canonical_bytes().unwrap()).unwrap();
    assert!(journal.contains(r#""lossy":["decision_rule"]"#), "{journal}");
}

#[test]
fn ac3_formula_text_is_neutralised_and_decimals_are_not() {
    let map = builtin(LONG_REF).unwrap();
    let rows = vec![ResultRow::new("R1")
        .with(Field::CharacteristicId, "C1")
        .with(Field::CharacteristicName, "=HYPERLINK(...)")
        .with(Field::Actual, "-0.012")
        .with(Field::Unit, "mm")];
    let bytes = render(&rows, &map).unwrap().bytes;
    let table = read_table(&bytes, &map).unwrap();
    assert_eq!(table.len(), 2);
    assert_eq!(table[1][1], "'=HYPERLINK(...)");
    assert_eq!(table[1][3], "-0.012");
    // Every trigger the spec lists, and only those.
    for (text, want) in [
        ("=1+1", "'=1+1"),
        ("+1", "'+1"),
        ("-1", "'-1"),
        ("@x", "'@x"),
        ("\tx", "'\tx"),
        ("\rx", "'\rx"),
        ("x=1", "x=1"),
        (" =1", " =1"),
        ("'=1", "'=1"),
        ("", ""),
    ] {
        assert_eq!(neutralise(text), want, "{text:?}");
        assert_eq!(unneutralise(&neutralise(text)), text, "{text:?}");
    }
    // Signed decimals in every decimal field are written as authored.
    let row = ResultRow::new("R2")
        .with(Field::CharacteristicId, "C2")
        .with(Field::Nominal, "+0.5")
        .with(Field::Deviation, "-1e-3")
        .with(Field::LowerLimit, "-0.010")
        .with(Field::UpperLimit, "+0.010");
    let bytes = render(&[row], &map).unwrap().bytes;
    let table = read_table(&bytes, &map).unwrap();
    assert_eq!(&table[1][2..7], &["+0.5", "", "-1e-3", "-0.010", "+0.010"]);
    // A CR-led cell is neutralised, then quoted.
    let row = ResultRow::new("R3").with(Field::CharacteristicId, "C3").with(Field::Comment, "\rcr");
    let text = String::from_utf8(render(&[row], &map).unwrap().bytes).unwrap();
    assert!(text.ends_with(",\"'\rcr\"\n"), "{text:?}");
}

#[test]
fn ac4_semicolon_map_with_decimal_comma() {
    let map = user_map("semicolon_comma.json");
    golden_case("semicolon", &map);
    let rows = vec![ResultRow::new("R1")
        .with(Field::CharacteristicId, "C1")
        .with(Field::Actual, "0.0100")
        .with(Field::Unit, "mm")];
    let text = String::from_utf8(render(&rows, &map).unwrap().bytes).unwrap();
    assert!(text.contains(";0,0100;"), "{text:?}");
    assert!(text.ends_with("\r\n"));
}

#[test]
fn ac5_existing_target_without_overwrite_fails() {
    let map = builtin(LONG_REF).unwrap();
    let dir = root("ac5");
    std::fs::write(dir.join("r.csv"), b"old").unwrap();
    let err = export_results_csv(&records(), &request(&map, "root://out/r.csv"), &dir).unwrap_err();
    assert_eq!(err.code(), "target_exists");
    assert_eq!(std::fs::read(dir.join("r.csv")).unwrap(), b"old");
    // Also for a dry run.
    let mut dry = request(&map, "root://out/r.csv");
    dry.dry_run = true;
    assert_eq!(export_results_csv(&records(), &dry, &dir).unwrap_err().code(), "target_exists");
    // With overwrite, the file is replaced and no temp file remains.
    let mut req = request(&map, "root://out/r.csv");
    req.overwrite = true;
    export_results_csv(&records(), &req, &dir).unwrap();
    assert_eq!(std::fs::read(dir.join("r.csv")).unwrap(), read("r.golden.csv"));
    assert_eq!(listing(&dir), vec!["r.csv".to_owned()]);
}

#[test]
fn ac6_replay_into_empty_root_is_identical() {
    let map = builtin(LONG_REF).unwrap();
    let out = export_results_csv(&records(), &request(&map, "root://out/r.csv"), &root("ac6a")).unwrap();
    let journal = out.event.canonical_bytes().unwrap();
    let event = ExportEvent::from_json(&journal).unwrap();
    assert_eq!(event, out.event);
    let fresh = root("ac6b");
    assert_eq!(replay(&event, &records(), &map, &fresh).unwrap(), ReplayOutcome::Identical);
    assert_eq!(std::fs::read(fresh.join("r.csv")).unwrap(), read("r.golden.csv"));
    // Other input or another map: not replayable, nothing written.
    let other = root("ac6c");
    let fewer: Vec<ResultRow> = records().into_iter().skip(1).collect();
    assert_eq!(replay(&event, &fewer, &map, &other).unwrap(), ReplayOutcome::NotReplayable);
    let wide = builtin(WIDE_REF).unwrap();
    assert_eq!(replay(&event, &records(), &wide, &other).unwrap(), ReplayOutcome::NotReplayable);
    assert!(listing(&other).is_empty());
    // A tampered event fails to read back.
    let text = String::from_utf8(journal).unwrap();
    let bad = text.replace("export.results_csv@1", "export.results_csv@2");
    assert_eq!(ExportEvent::from_json(bad.as_bytes()).unwrap_err().code(), "event_invalid");
}

#[test]
fn req5_output_does_not_depend_on_input_order() {
    let map = builtin(LONG_REF).unwrap();
    let mut rows = records();
    let a = render(&rows, &map).unwrap();
    rows.reverse();
    let b = render(&rows, &map).unwrap();
    assert_eq!(a, b);
    rows.rotate_left(5);
    assert_eq!(render(&rows, &map).unwrap(), a);
    let wide = builtin(WIDE_REF).unwrap();
    assert_eq!(render(&records(), &wide).unwrap(), render(&rows, &wide).unwrap());
}

#[test]
fn req4_transcription_marking_kit() {
    let rows = records();
    for (map, golden) in
        [(builtin(LONG_REF).unwrap(), "r.golden.csv"), (builtin(WIDE_REF).unwrap(), "r.wide.golden.csv")]
    {
        check_marked(&read(golden), &map, &rows).unwrap();
    }
    // Hand-made defect: R-T02 (C2, P3) shown as evaluated.
    let long = builtin(LONG_REF).unwrap();
    let golden = String::from_utf8(read("r.golden.csv")).unwrap();
    let line = "UEC-0,,transcribed,VendorX,4.2,P3,2026-11-17T14:05:00Z,'=cmd";
    assert!(golden.contains(line));
    let tampered = golden.replace(line, "UEC-0,,evaluated,VendorX,4.2,P3,2026-11-17T14:05:00Z,'=cmd");
    match check_marked(tampered.as_bytes(), &long, &rows).unwrap_err() {
        OutError::TranscriptionUnmarked { result } => assert_eq!(result, "R-T02"),
        other => panic!("{other:?}"),
    }
    // A wrong source version is unmarked too.
    let tampered = golden.replace(
        "transcribed,VendorX,4.2,P3,2026-11-17T14:05:00Z,'-from",
        "transcribed,VendorX,4.1,P3,2026-11-17T14:05:00Z,'-from",
    );
    match check_marked(tampered.as_bytes(), &long, &rows).unwrap_err() {
        OutError::TranscriptionUnmarked { result } => assert_eq!(result, "R-T03"),
        other => panic!("{other:?}"),
    }
    // A map that cannot mark at all fails the kit for the first transcribed result.
    let semi = user_map("semicolon_comma.json");
    assert_eq!(
        check_marked(&read("r.semicolon.golden.csv"), &semi, &rows).unwrap_err().code(),
        "transcription_unmarked"
    );
}

#[test]
fn split_records_respects_quotes() {
    assert_eq!(split_records("a,b\n\"x\ny\",z\n", "\n"), vec!["a,b", "\"x\ny\",z"]);
    assert_eq!(split_records("a\r\n\"\r\n\"\r\nb", "\r\n"), vec!["a", "\"\r\n\"", "b"]);
    assert!(split_records("", "\n").is_empty());
}

#[test]
fn errors_no_results_and_bad_records() {
    let map = builtin(LONG_REF).unwrap();
    assert_eq!(render(&[], &map).unwrap_err().code(), "no_results");
    let mut req = request(&map, "root://out/r.csv");
    req.characteristics = Some(vec!["NOPE".to_owned()]);
    let dir = root("errors");
    assert_eq!(export_results_csv(&records(), &req, &dir).unwrap_err().code(), "no_results");
    req.characteristics = Some(vec!["C2".to_owned()]);
    let out = export_results_csv(&records(), &req, &dir).unwrap();
    assert_eq!(out.event.rows, 3);
    assert_eq!(out.event.characteristics, Some(vec!["C2".to_owned()]));

    let base = || ResultRow::new("R1").with(Field::CharacteristicId, "C1");
    let cases: Vec<(Vec<ResultRow>, &str)> = vec![
        (vec![base().with(Field::Actual, "1,5")], "bad_decimal"),
        (vec![base().with(Field::Actual, "1.")], "bad_decimal"),
        (vec![base().with(Field::Actual, "NaN")], "bad_decimal"),
        (vec![base().with(Field::Status, "OK")], "bad_enum"),
        (
            vec![base().with(Field::RecordStatus, "transcribed").with(Field::SourceSoftware, "X")],
            "transcription_source_missing",
        ),
        (vec![base(), base()], "duplicate_result_id"),
        (vec![ResultRow::new("R1")], "missing_characteristic_id"),
        (vec![ResultRow::new("").with(Field::CharacteristicId, "C1")], "missing_result_id"),
    ];
    for (rows, reason) in cases {
        let e = render(&rows, &map).unwrap_err();
        assert_eq!((e.code(), e.reason()), ("records_invalid", Some(reason)), "{reason}");
    }

    let wide = builtin(WIDE_REF).unwrap();
    let dup = vec![
        base().with(Field::PartId, "P1"),
        ResultRow::new("R2").with(Field::CharacteristicId, "C1").with(Field::PartId, "P1"),
    ];
    assert_eq!(render(&dup, &wide).unwrap_err().reason(), Some("wide_duplicate"));
    let conflict = vec![
        base().with(Field::PartId, "P1").with(Field::TimestampText, "t1"),
        ResultRow::new("R2")
            .with(Field::CharacteristicId, "C2")
            .with(Field::PartId, "P1")
            .with(Field::TimestampText, "t2"),
    ];
    assert_eq!(render(&conflict, &wide).unwrap_err().reason(), Some("wide_part_conflict"));
    let sep = vec![ResultRow::new("R1").with(Field::CharacteristicId, "C:1")];
    assert_eq!(render(&sep, &wide).unwrap_err().reason(), Some("wide_characteristic_separator"));

    for (json, reason) in [
        (
            r#"{"schema":"results_csv.records@1","records":[{"result_id":"R","characteristic_id":"C","bogus":"x"}]}"#,
            "unknown_key",
        ),
        (
            r#"{"schema":"results_csv.records@1","records":[{"result_id":"R","characteristic_id":"C","actual":0.5}]}"#,
            "wrong_type",
        ),
        (r#"{"schema":"results_csv.records@2","records":[]}"#, "unsupported_schema"),
        (r#"{"schema":"results_csv.records@1"}"#, "missing_key"),
        (r#"{"schema":"results_csv.records@1","records":[{"characteristic_id":"C"}]}"#, "missing_key"),
        (r#"{"schema":"results_csv.records@1","records":[{"result_id":"R","result_id":"S"}]}"#, "duplicate_key"),
        ("{", "json_syntax"),
    ] {
        assert_eq!(load_records(json.as_bytes()).unwrap_err().reason(), Some(reason), "{json}");
    }
}

#[test]
fn target_handles_and_dry_run() {
    let map = builtin(LONG_REF).unwrap();
    let dir = root("handles");
    assert_eq!(
        export_results_csv(&records(), &request(&map, "/tmp/r.csv"), &dir).unwrap_err().code(),
        "raw_path_forbidden"
    );
    assert_eq!(
        export_results_csv(&records(), &request(&map, "root://out/../r.csv"), &dir).unwrap_err().code(),
        "path_outside_root"
    );
    assert_eq!(
        export_results_csv(&records(), &request(&map, "root://out/missing/r.csv"), &dir).unwrap_err().code(),
        "io"
    );
    let mut dry = request(&map, "root://out/r.csv");
    dry.dry_run = true;
    let out = export_results_csv(&records(), &dry, &dir).unwrap();
    assert!(out.report.dry_run());
    assert_eq!(out.bytes, read("r.golden.csv"));
    assert!(listing(&dir).is_empty(), "a dry run writes nothing");
    std::fs::create_dir(dir.join("sub")).unwrap();
    export_results_csv(&records(), &request(&map, "root://out/sub/r.csv"), &dir).unwrap();
    assert_eq!(std::fs::read(dir.join("sub").join("r.csv")).unwrap(), read("r.golden.csv"));
    assert!(listing(&dir.join("sub")).iter().all(|n| !n.starts_with(TMP_PREFIX)));
}

#[cfg(unix)]
#[test]
fn symlink_targets_are_refused() {
    let map = builtin(LONG_REF).unwrap();
    let dir = root("links");
    let outside = root("links-outside");
    std::os::unix::fs::symlink(outside.join("victim.csv"), dir.join("r.csv")).unwrap();
    std::os::unix::fs::symlink(&outside, dir.join("sub")).unwrap();
    for target in ["root://out/r.csv", "root://out/sub/r.csv"] {
        for overwrite in [false, true] {
            let mut req = request(&map, target);
            req.overwrite = overwrite;
            assert_eq!(export_results_csv(&records(), &req, &dir).unwrap_err().code(), "target_is_link", "{target}");
        }
    }
    assert!(listing(&outside).is_empty());
}

#[test]
fn never_panics_on_odd_rows() {
    let maps = [builtin(LONG_REF).unwrap(), builtin(WIDE_REF).unwrap(), user_map("semicolon_comma.json")];
    let texts = ["", "\"", "\r\n", ";", ",", "'", "=", "\u{feff}x", "\u{202e}rtl", "a\"b\"c", "-", "C:1"];
    for map in &maps {
        for (i, t) in texts.iter().enumerate() {
            let row = ResultRow::new(&format!("R{i}"))
                .with(Field::CharacteristicId, t)
                .with(Field::CharacteristicName, t)
                .with(Field::PartId, t)
                .with(Field::Comment, t)
                .with(Field::SourceSoftware, t);
            if let Ok(r) = render(&[row.clone()], map) {
                let table = read_table(&r.bytes, map).unwrap();
                assert!(!table.is_empty());
                check_marked(&r.bytes, map, &[row]).unwrap();
            }
        }
    }
}
