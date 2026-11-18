//! AC-6 of spec node IO.QIF.NORM: the round-trip harness over a P0-subset corpus
//! manifest, its JSON report and the gate exit code. Expected reports are hand-labelled.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_qif::roundtrip::{run_manifest, Projection, ProjectionError, RowStatus, StubProjection};
use std::path::PathBuf;
use std::process::Command;

fn corpus(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/io/qif/norm/corpus").join(name)
}

/// A projection that loses a trailing decimal zero, as a broken JSON mapping would.
struct DropsZero;

impl Projection for DropsZero {
    fn xml_to_json(&self, xml: &[u8]) -> Result<Vec<u8>, ProjectionError> {
        let text = String::from_utf8(xml.to_vec()).map_err(|_| ProjectionError { code: "not_utf8" })?;
        Ok(text.replace("12.50", "12.5").into_bytes())
    }

    fn json_to_xml(&self, json: &[u8]) -> Result<Vec<u8>, ProjectionError> {
        Ok(json.to_vec())
    }
}

/// A projection that always fails.
struct Fails;

impl Projection for Fails {
    fn xml_to_json(&self, _: &[u8]) -> Result<Vec<u8>, ProjectionError> {
        Err(ProjectionError { code: "projection_failed" })
    }

    fn json_to_xml(&self, json: &[u8]) -> Result<Vec<u8>, ProjectionError> {
        Ok(json.to_vec())
    }
}

#[test]
fn ac6_stub_projection_passes_with_one_row_per_p0_file() {
    let report = run_manifest(&corpus("p0.toml"), &StubProjection).unwrap();
    assert_eq!(
        report.to_json(),
        "{\"norm\":\"qif-norm@1\",\"total\":2,\"equal\":2,\"passed\":true,\"rows\":[\
         {\"file\":\"files/ws_a.qif\",\"status\":\"equal\"},\
         {\"file\":\"files/list_ws.qif\",\"status\":\"equal\"}]}\n"
    );
    assert_eq!(report.exit_code(), 0);
}

#[test]
fn ac6_an_inequality_fails_the_gate() {
    let report = run_manifest(&corpus("p0.toml"), &DropsZero).unwrap();
    assert_eq!(
        report.to_json(),
        "{\"norm\":\"qif-norm@1\",\"total\":2,\"equal\":1,\"passed\":false,\"rows\":[\
         {\"file\":\"files/ws_a.qif\",\"status\":\"different\",\"diff\":{\
         \"path\":\"/q:QIFDocument[1]/q:Characteristics[1]/q:Nominal[1]/q:Value[1]\",\
         \"line_a\":9,\"line_b\":9,\"a\":\"12.50\",\"b\":\"12.5\"}},\
         {\"file\":\"files/list_ws.qif\",\"status\":\"equal\"}]}\n"
    );
    assert_eq!(report.exit_code(), 1);

    let report = run_manifest(&corpus("p0.toml"), &Fails).unwrap();
    assert!(report.rows.iter().all(|r| r.status == RowStatus::Error("projection_failed")));
    assert_eq!(report.exit_code(), 1);
}

#[test]
fn ac6_fail_closed_entries() {
    let report = run_manifest(&corpus("bad.toml"), &StubProjection).unwrap();
    let statuses: Vec<_> = report.rows.iter().map(|r| (r.file.as_str(), r.status.clone())).collect();
    assert_eq!(
        statuses,
        vec![
            ("https://corpus.example.invalid/qif/remote.qif", RowStatus::Error("not_local")),
            ("../ws_a.qif", RowStatus::Error("path_outside_root")),
            ("files/does_not_exist.qif", RowStatus::Error("io")),
        ]
    );
    assert_eq!(report.exit_code(), 1);
}

#[test]
fn ac6_missing_manifest_is_an_error() {
    let e = run_manifest(&corpus("no_such_manifest.toml"), &StubProjection).unwrap_err();
    assert_eq!(e.code(), "manifest_missing");
}

#[test]
fn ac6_gate_binary_exit_codes() {
    let bin = env!("CARGO_BIN_EXE_qif-roundtrip");
    let ok = Command::new(bin).arg("--manifest").arg(corpus("p0.toml")).output().unwrap();
    assert_eq!(ok.status.code(), Some(0));
    assert!(String::from_utf8(ok.stdout).unwrap().contains("\"passed\":true"));
    let bad = Command::new(bin).arg("--manifest").arg(corpus("bad.toml")).output().unwrap();
    assert_eq!(bad.status.code(), Some(1));
    let usage = Command::new(bin).output().unwrap();
    assert_eq!(usage.status.code(), Some(2));
    let missing = Command::new(bin).arg("--manifest").arg(corpus("no_such_manifest.toml")).output().unwrap();
    assert_eq!(missing.status.code(), Some(2));
}
