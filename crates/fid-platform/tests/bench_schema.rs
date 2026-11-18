//! Acceptance criteria of spec node PLAT.HW.SCHEMA (AC-1 .. AC-5) and requirement edge
//! cases. Expected values are hand-labelled from the spec's rules (the fixtures in
//! `fixtures/plat/bench/` state their expected outcome in their header comments) or come
//! from the independent Python reference `fixtures/plat/bench/make_expected.py`; none is
//! produced by running this crate. AC-4 against a real JSON Schema validator runs in CI as
//! `python tools/check_bench_schema.py`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_platform::bench_schema::{
    parse_suite, result_json_schema_bytes, validate_result, validate_suite, BenchResult, Better, Context, MetricResult,
    SchemaError, Status, Unit, WorkloadResult, MAX_BYTES,
};
use fid_platform::matrix::{Arch, OsFamily};
use fid_types::Digest;
use std::path::PathBuf;
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture(name: &str) -> PathBuf {
    root().join("fixtures/plat/bench").join(name)
}

fn read(path: PathBuf) -> Vec<u8> {
    std::fs::read(path).unwrap()
}

/// Runs `fid-platform plat-hw <args>` from the repository root (what `cargo xtask plat-hw`
/// runs; the wrapper is exercised by the rust.yml step).
fn cli(args: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_fid-platform"))
        .current_dir(root())
        .arg("plat-hw")
        .args(args)
        .output()
        .unwrap();
    (out.status.code().unwrap(), String::from_utf8(out.stdout).unwrap())
}

fn suite_file(name: &str) -> String {
    fixture(name).to_string_lossy().into_owned()
}

// ------------------------------------------------------------------ AC-1 .. AC-3

#[test]
fn ac1_mini_suite_validates() {
    let (code, out) = cli(&["validate-suite", "--file", &suite_file("mini-suite.toml")]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out, "mini.load\nmini.render\n");

    let s = parse_suite(&read(fixture("mini-suite.toml"))).unwrap();
    assert_eq!(s.schema_version, 1);
    assert_eq!(s.suite_version, "0.0.0-fixture");
    assert_eq!(s.workloads.len(), 2);
    let w = &s.workloads[0];
    assert_eq!(w.id, "mini.load");
    assert_eq!(w.dataset.id, "synthetic.cloud-a");
    assert_eq!(w.dataset.sha256.as_str(), format!("sha256:{}", "1".repeat(64)));
    assert_eq!(w.repetitions, 5);
    assert_eq!(w.timeout_s, 120);
    assert_eq!(w.metrics.len(), 2);
    assert_eq!(
        (w.metrics[0].name.as_str(), w.metrics[0].unit, w.metrics[0].better),
        ("load_time", Unit::S, Better::Lower)
    );
    assert_eq!((w.metrics[1].name.as_str(), w.metrics[1].unit), ("memory_peak", Unit::MiB));
    let r = &s.workloads[1];
    assert_eq!((r.id.as_str(), r.repetitions, r.timeout_s), ("mini.render", 3, 300));
    assert_eq!((r.metrics[0].unit, r.metrics[0].better), (Unit::Fps, Better::Higher));
    assert_eq!(r.metrics[1].unit, Unit::PointsPerS);
}

#[test]
fn ac2_missing_digest_names_the_workload() {
    let (code, out) = cli(&["validate-suite", "--file", &suite_file("suite-no-digest.toml")]);
    assert_eq!(code, 1);
    assert_eq!(out, "MissingDigest { workload: no-digest.load }\n");
    let errs = validate_suite(&read(fixture("suite-no-digest.toml"))).unwrap_err();
    assert_eq!(errs, vec![SchemaError::MissingDigest { workload: "no-digest.load".into() }]);
}

#[test]
fn ac3_bad_unit() {
    let (code, out) = cli(&["validate-suite", "--file", &suite_file("suite-bad-unit.toml")]);
    assert_eq!(code, 1);
    assert_eq!(out.lines().count(), 1, "{out}");
    assert!(out.starts_with("BadUnit { workload: bad-unit.load, metric: distance, unit: furlongs }"), "{out}");
    let errs = validate_suite(&read(fixture("suite-bad-unit.toml"))).unwrap_err();
    assert_eq!(
        errs,
        vec![SchemaError::BadUnit {
            workload: "bad-unit.load".into(),
            metric: "distance".into(),
            unit: "furlongs".into()
        }]
    );
}

#[test]
fn cli_usage_and_unreadable_file() {
    assert_eq!(cli(&["validate-suite", "--file", "no/such/file.toml"]).0, 2);
    assert_eq!(cli(&["validate-suite", "--bogus"]).0, 2);
    assert_eq!(cli(&["result-schema", "--extra"]).0, 2);
}

// ------------------------------------------------------------------ suite edge cases

const HEAD: &str = "schema_version = 1\nsuite_version = \"1\"\n";

fn workload(id: &str, extra: &str) -> String {
    format!(
        "[[workloads]]\nid = \"{id}\"\ndataset = {{ id = \"d\", sha256 = \"sha256:{}\" }}\nrepetitions = 1\ntimeout_s = 1\n{extra}\
         [[workloads.metrics]]\nname = \"m\"\nunit = \"ms\"\nbetter = \"lower\"\n",
        "a".repeat(64)
    )
}

fn suite_errs(text: &str) -> Vec<SchemaError> {
    validate_suite(text.as_bytes()).unwrap_err()
}

fn is_parse(e: &SchemaError, needle: &str) -> bool {
    matches!(e, SchemaError::Parse { msg, .. } if msg.contains(needle))
}

#[test]
fn suite_boundaries_accept() {
    let ok = format!("{HEAD}{}", workload("w", ""));
    assert!(validate_suite(ok.as_bytes()).is_ok());
    let hundred = ok.replace("repetitions = 1", "repetitions = 100");
    assert_eq!(parse_suite(hundred.as_bytes()).unwrap().workloads[0].repetitions, 100);
    for unit in ["s", "ms", "fps", "MiB", "points_per_s", "triangles_per_s"] {
        let t = ok.replace("unit = \"ms\"", &format!("unit = \"{unit}\""));
        assert_eq!(parse_suite(t.as_bytes()).unwrap().workloads[0].metrics[0].unit.as_str(), unit);
    }
}

#[test]
fn suite_bad_repetitions() {
    let base = format!("{HEAD}{}", workload("w", ""));
    for (n, found) in [("0", 0), ("101", 101), ("-3", -3), ("1000", 1000)] {
        let t = base.replace("repetitions = 1", &format!("repetitions = {n}"));
        assert_eq!(suite_errs(&t), vec![SchemaError::BadRepetitions { workload: "w".into(), found }]);
    }
}

#[test]
fn suite_duplicate_workload() {
    let t = format!("{HEAD}{}{}", workload("w", ""), workload("w", ""));
    assert_eq!(suite_errs(&t), vec![SchemaError::DuplicateWorkload { id: "w".into() }]);
}

#[test]
fn suite_unsupported_schema_is_fatal() {
    let t = format!("schema_version = 2\nbogus = 1\n{}", workload("w", ""));
    assert_eq!(suite_errs(&t), vec![SchemaError::UnsupportedSchema { found: 2 }]);
}

#[test]
fn suite_unknown_fields_are_rejected() {
    let t = format!("{HEAD}extra = 1\n{}", workload("w", "colour = \"red\"\n"));
    let errs = suite_errs(&t);
    assert_eq!(errs.len(), 2, "{errs:?}");
    assert!(is_parse(&errs[0], "unknown field `extra` in the manifest"));
    assert!(is_parse(&errs[1], "unknown field `colour` in workload w"));
    let t = format!("{HEAD}{}", workload("w", "")).replace("better = \"lower\"", "better = \"lower\"\nweight = 2");
    assert!(is_parse(&suite_errs(&t)[0], "unknown field `weight`"));
}

#[test]
fn suite_missing_dataset_and_empty_digest_are_missing_digest() {
    let base = format!("{HEAD}{}", workload("w", ""));
    let no_dataset = base.replace(&format!("dataset = {{ id = \"d\", sha256 = \"sha256:{}\" }}\n", "a".repeat(64)), "");
    assert_eq!(suite_errs(&no_dataset), vec![SchemaError::MissingDigest { workload: "w".into() }]);
    let empty = base.replace(&format!("sha256:{}", "a".repeat(64)), "");
    assert_eq!(suite_errs(&empty), vec![SchemaError::MissingDigest { workload: "w".into() }]);
    let upper = base.replace(&"a".repeat(64), &"A".repeat(64));
    assert!(is_parse(&suite_errs(&upper)[0], "sha256"));
}

#[test]
fn suite_other_field_errors() {
    let base = format!("{HEAD}{}", workload("w", ""));
    assert!(is_parse(&suite_errs(&base.replace("better = \"lower\"", "better = \"down\""))[0], "better"));
    assert!(is_parse(&suite_errs(&base.replace("timeout_s = 1", "timeout_s = 0"))[0], "timeout_s"));
    assert!(is_parse(&suite_errs(&base.replace("id = \"w\"", "id = \"W W\""))[0], "id in workload"));
    assert!(is_parse(&suite_errs(HEAD)[0], "workloads is missing"));
    assert!(is_parse(&suite_errs(&format!("{HEAD}workloads = []\n"))[0], "workloads is empty"));
    let dup_metric = base.clone() + "[[workloads.metrics]]\nname = \"m\"\nunit = \"s\"\nbetter = \"lower\"\n";
    assert!(is_parse(&suite_errs(&dup_metric)[0], "duplicate metric name `m`"));
    // A workload without an id is named by its index.
    let no_id = base.replace("id = \"w\"\n", "").replace("unit = \"ms\"", "unit = \"km\"");
    let errs = suite_errs(&no_id);
    assert!(is_parse(&errs[0], "id is missing in workload workloads[0]"));
    assert_eq!(
        errs[1],
        SchemaError::BadUnit { workload: "workloads[0]".into(), metric: "m".into(), unit: "km".into() }
    );
}

#[test]
fn suite_parse_failures_are_bounded() {
    let errs = suite_errs("schema_version = 1\n[[workloads]\n");
    assert!(matches!(errs.as_slice(), [SchemaError::Parse { line: Some(_), .. }]), "{errs:?}");
    assert!(matches!(suite_errs("a = \"\u{0}").as_slice(), [SchemaError::Parse { .. }]));
    assert!(matches!(validate_suite(&[0xff, 0xfe]).unwrap_err().as_slice(), [SchemaError::Parse { .. }]));
    let huge = vec![b'#'; MAX_BYTES + 1];
    assert!(matches!(validate_suite(&huge).unwrap_err().as_slice(), [SchemaError::Parse { .. }]));
}

// ------------------------------------------------------------------ AC-4 and result rules

fn sample() -> Vec<u8> {
    read(fixture("result-sample.json"))
}

fn mutate(f: impl FnOnce(&mut String)) -> Result<BenchResult, SchemaError> {
    let mut t = String::from_utf8(sample()).unwrap();
    f(&mut t);
    validate_result(t.as_bytes())
}

#[test]
fn ac4_sample_result_is_valid_and_needs_gpu_driver_version() {
    let r = validate_result(&sample()).unwrap();
    assert_eq!(r.schema_version, 1);
    assert_eq!(r.context.os, OsFamily::Ubuntu);
    assert_eq!(r.context.arch, Arch::X86_64);
    assert_eq!(r.context.ram_mib, 16384);
    assert_eq!(r.context.machine, "local");
    let m = &r.workloads[0].metrics[0];
    assert_eq!((m.median, m.min, m.max, m.status), (Some(1.25), Some(1.125), Some(1.5), Status::Ok));
    assert_eq!(r.workloads[1].metrics[0].status, Status::Skipped);
    assert_eq!(r.workloads[1].metrics[1].median, None);

    let err = mutate(|t| *t = t.replace("    \"gpu_driver_version\": \"0.0.0-fixture\",\n", "")).unwrap_err();
    assert!(
        matches!(&err, SchemaError::Parse { msg, .. } if msg == "gpu_driver_version is missing in context"),
        "{err}"
    );
}

#[test]
fn result_rule_violations() {
    let digest1 = format!("\"dataset_sha256\": \"sha256:{}\",\n", "1".repeat(64));
    let e = mutate(|t| *t = t.replacen(&format!("          {digest1}"), "", 1)).unwrap_err();
    assert_eq!(e, SchemaError::MissingDigest { workload: "mini.load".into() });
    let e = mutate(|t| *t = t.replacen("\"unit\": \"s\"", "\"unit\": \"furlongs\"", 1)).unwrap_err();
    assert_eq!(
        e,
        SchemaError::BadUnit { workload: "mini.load".into(), metric: "load_time".into(), unit: "furlongs".into() }
    );
    let e = mutate(|t| *t = t.replacen("\"repetitions\": 5", "\"repetitions\": 101", 1)).unwrap_err();
    assert_eq!(e, SchemaError::BadRepetitions { workload: "mini.load".into(), found: 101 });
    let e = mutate(|t| *t = t.replacen("\"schema_version\": 1", "\"schema_version\": 2", 1)).unwrap_err();
    assert_eq!(e, SchemaError::UnsupportedSchema { found: 2 });
    let e = mutate(|t| *t = t.replace("\"id\": \"mini.render\"", "\"id\": \"mini.load\"")).unwrap_err();
    assert_eq!(e, SchemaError::DuplicateWorkload { id: "mini.load".into() });

    let parse = |f: &dyn Fn(&mut String), needle: &str| {
        let e = mutate(f).unwrap_err();
        assert!(matches!(&e, SchemaError::Parse { msg, .. } if msg.contains(needle)), "{needle}: {e}");
    };
    parse(&|t: &mut String| *t = t.replacen("\"median\": 1.25", "\"median\": 2", 1), "min <= median <= max");
    parse(&|t: &mut String| *t = t.replacen("\"median\": 1.25", "\"median\": null", 1), "status ok needs");
    parse(
        &|t: &mut String| *t = t.replacen("\"median\": null", "\"median\": 1", 1),
        "needs median, min and max to be null",
    );
    parse(&|t: &mut String| *t = t.replacen("\"min\": 1.125", "\"min\": -1", 1), "non-negative");
    parse(&|t: &mut String| *t = t.replacen("\"status\": \"ok\"", "\"status\": \"crashed\"", 1), "status in");
    parse(&|t: &mut String| *t = t.replace("\"os\": \"ubuntu\"", "\"os\": \"plan9\""), "os in context");
    parse(&|t: &mut String| *t = t.replace("\"ram_mib\": 16384", "\"ram_mib\": 0.5"), "ram_mib");
    parse(
        &|t: &mut String| *t = t.replace("\"machine\": \"local\"", "\"machine\": \"local\", \"colour\": \"red\""),
        "unknown field `colour` in context",
    );
    parse(
        &|t: &mut String| *t = t.replacen("\"status\": \"ok\"", "\"status\": \"ok\", \"note\": 1", 1),
        "unknown field `note`",
    );
    parse(&|t: &mut String| *t = t.replace("\"machine\": \"local\"", "\"machine\": \"\""), "machine in context");
    parse(
        &|t: &mut String| *t = t.replacen("\"schema_version\": 1", "\"schema_version\": 1, \"schema_version\": 1", 1),
        "duplicate_key",
    );
    parse(
        &|t: &mut String| *t = t.replacen("\"name\": \"memory_peak\"", "\"name\": \"load_time\"", 1),
        "duplicate metric name",
    );
    assert!(validate_result(b"[]").is_err());
    assert!(validate_result(&vec![b' '; MAX_BYTES + 1]).is_err());
}

// ------------------------------------------------------------------ AC-5 and REQ-005

fn built() -> BenchResult {
    let digest = Digest::parse(&format!("sha256:{}", "ab".repeat(32))).unwrap();
    BenchResult {
        schema_version: 1,
        context: Context {
            suite_version: "1".into(),
            app_version: "0.0.1".into(),
            os: OsFamily::Windows,
            os_version: "10.0.19045".into(),
            arch: Arch::Aarch64,
            cpu_model: "test cpu".into(),
            ram_mib: 8192,
            gpu_backend: "dx12".into(),
            adapter_name: "test adapter".into(),
            gpu_driver_version: "1.2.3".into(),
            machine: "local".into(),
        },
        workloads: vec![WorkloadResult {
            id: "w".into(),
            metrics: vec![
                MetricResult {
                    name: "t".into(),
                    unit: Unit::Ms,
                    dataset_sha256: digest,
                    repetitions: 7,
                    median: Some(0.5),
                    min: Some(0.25),
                    max: Some(0.75),
                    status: Status::Ok,
                },
                MetricResult {
                    name: "f".into(),
                    unit: Unit::TrianglesPerS,
                    dataset_sha256: digest,
                    repetitions: 7,
                    median: None,
                    min: None,
                    max: None,
                    status: Status::Failed,
                },
            ],
        }],
    }
}

#[test]
fn ac5_serialisation_is_byte_identical_with_fixed_key_order() {
    let r = built();
    let a = r.to_json().unwrap();
    let b = r.clone().to_json().unwrap();
    assert_eq!(a, b);
    // Hand-written expected bytes: RFC 8785 order (keys sorted), no whitespace.
    let expected = format!(
        "{{\"context\":{{\"adapter_name\":\"test adapter\",\"app_version\":\"0.0.1\",\"arch\":\"aarch64\",\
         \"cpu_model\":\"test cpu\",\"gpu_backend\":\"dx12\",\"gpu_driver_version\":\"1.2.3\",\"machine\":\"local\",\
         \"os\":\"windows\",\"os_version\":\"10.0.19045\",\"ram_mib\":8192,\"suite_version\":\"1\"}},\
         \"schema_version\":1,\"workloads\":[{{\"id\":\"w\",\"metrics\":[\
         {{\"dataset_sha256\":\"sha256:{d}\",\"max\":0.75,\"median\":0.5,\"min\":0.25,\"name\":\"t\",\"repetitions\":7,\
         \"status\":\"ok\",\"unit\":\"ms\"}},\
         {{\"dataset_sha256\":\"sha256:{d}\",\"max\":null,\"median\":null,\"min\":null,\"name\":\"f\",\"repetitions\":7,\
         \"status\":\"failed\",\"unit\":\"triangles_per_s\"}}]}}]}}",
        d = "ab".repeat(32)
    );
    assert_eq!(String::from_utf8(a.clone()).unwrap(), expected);
    // What is written reads back to the same value.
    assert_eq!(validate_result(&a).unwrap(), r);
    let mut nan = r;
    nan.workloads[0].metrics[0].median = Some(f64::NAN);
    assert!(matches!(nan.to_json(), Err(SchemaError::Parse { msg, .. }) if msg.contains("non_finite_number")));
}

/// REQ-PLAT.HW.SCHEMA-005: the writer refuses every document its own reader would reject.
#[test]
fn to_json_refuses_documents_the_reader_rejects() {
    let parse_err = |f: &dyn Fn(&mut BenchResult), needle: &str| {
        let mut r = built();
        f(&mut r);
        let e = r.to_json().unwrap_err();
        assert!(matches!(&e, SchemaError::Parse { msg, .. } if msg.contains(needle)), "{needle}: {e}");
    };
    parse_err(&|r: &mut BenchResult| r.workloads[0].metrics[0].median = None, "status ok needs");
    parse_err(&|r: &mut BenchResult| r.workloads[0].metrics[0].max = None, "status ok needs");
    parse_err(&|r: &mut BenchResult| r.workloads[0].metrics[0].median = Some(0.9), "min <= median <= max");
    parse_err(&|r: &mut BenchResult| r.workloads[0].metrics[0].min = Some(0.6), "min <= median <= max");
    parse_err(&|r: &mut BenchResult| r.workloads[0].metrics[0].min = Some(-0.5), "non-negative");
    parse_err(&|r: &mut BenchResult| r.workloads[0].metrics[0].max = Some(f64::INFINITY), "non_finite_number");
    parse_err(&|r: &mut BenchResult| r.workloads[0].metrics[1].median = Some(1.0), "to be null");
    parse_err(&|r: &mut BenchResult| r.workloads[0].metrics[1].status = Status::Ok, "status ok needs");
    parse_err(&|r: &mut BenchResult| r.workloads[0].metrics[1].name = "t".into(), "duplicate metric name");
    parse_err(&|r: &mut BenchResult| r.workloads[0].metrics[0].name = "Bad Name".into(), "does not match");
    parse_err(&|r: &mut BenchResult| r.workloads[0].id = String::new(), "does not match");
    parse_err(&|r: &mut BenchResult| r.workloads[0].metrics.clear(), "is empty");
    parse_err(&|r: &mut BenchResult| r.workloads.clear(), "workloads is empty");
    parse_err(&|r: &mut BenchResult| r.context.gpu_driver_version = String::new(), "gpu_driver_version");

    let mut r = built();
    r.workloads[0].metrics[0].repetitions = 0;
    assert_eq!(r.to_json().unwrap_err(), SchemaError::BadRepetitions { workload: "w".into(), found: 0 });
    let mut r = built();
    r.workloads[0].metrics[1].repetitions = 101;
    assert_eq!(r.to_json().unwrap_err(), SchemaError::BadRepetitions { workload: "w".into(), found: 101 });
    let mut r = built();
    r.schema_version = 2;
    assert_eq!(r.to_json().unwrap_err(), SchemaError::UnsupportedSchema { found: 2 });
    let mut r = built();
    let w = r.workloads[0].clone();
    r.workloads.push(w);
    assert_eq!(r.to_json().unwrap_err(), SchemaError::DuplicateWorkload { id: "w".into() });

    // The boundaries the reader accepts are written: min == median == max, all zero.
    let mut r = built();
    let m = &mut r.workloads[0].metrics[0];
    (m.median, m.min, m.max, m.repetitions) = (Some(0.0), Some(0.0), Some(0.0), 100);
    assert_eq!(validate_result(&r.to_json().unwrap()).unwrap(), r);
}

#[test]
fn sample_serialises_to_the_python_reference_bytes() {
    let r = validate_result(&sample()).unwrap();
    assert_eq!(r.to_json().unwrap(), read(fixture("result-sample.canonical.json")));
}

#[test]
fn exported_schema_matches_the_committed_file() {
    // platform/bench-result.schema.json is checked against the independent Python statement
    // of the schema by `make_expected.py --check`; here the Rust export must equal it.
    let committed = read(root().join("platform/bench-result.schema.json"));
    let exported = result_json_schema_bytes().unwrap();
    assert_eq!(
        String::from_utf8(exported).unwrap(),
        String::from_utf8(committed).unwrap(),
        "run `cargo xtask plat-hw result-schema`, or fix the export to match the spec"
    );
}

#[test]
fn result_schema_command_writes_the_file() {
    let dir = std::env::temp_dir().join(format!("fid-platform-bench-schema-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("platform")).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_fid-platform"))
        .current_dir(&dir)
        .args(["plat-hw", "result-schema"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let written = std::fs::read(dir.join("platform/bench-result.schema.json")).unwrap();
    assert_eq!(written, read(root().join("platform/bench-result.schema.json")));
    std::fs::remove_dir_all(&dir).unwrap();
}
