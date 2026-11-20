//! Section-level diff of two builds (spec node KRN.MAN.REPRO, REQ-KRN.MAN.REPRO-004, AC-3).
//!
//! The modules are hand-assembled from the binary format (header, then id, size, payload per
//! section) and the expected labels and sizes are counted by hand from those bytes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_wasm_check::module::WasmErrorKind;
use fid_wasm_check::sections::{diff, sections};

const HEADER: [u8; 8] = [0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];

/// One section with a payload shorter than 128 bytes (one-byte size).
fn sec(id: u8, payload: &[u8]) -> Vec<u8> {
    assert!(payload.len() < 128);
    let mut v = vec![id, payload.len() as u8];
    v.extend_from_slice(payload);
    v
}

fn custom(name: &str, data: &[u8]) -> Vec<u8> {
    let mut p = vec![name.len() as u8];
    p.extend_from_slice(name.as_bytes());
    p.extend_from_slice(data);
    sec(0, &p)
}

fn module(parts: &[Vec<u8>]) -> Vec<u8> {
    let mut m = HEADER.to_vec();
    for p in parts {
        m.extend_from_slice(p);
    }
    m
}

/// type section: one function type () -> () (4 bytes payload, 6 bytes in all).
fn types() -> Vec<u8> {
    sec(1, &[0x01, 0x60, 0x00, 0x00])
}

/// data section with arbitrary payload (the reader does not interpret it).
fn data(bytes: &[u8]) -> Vec<u8> {
    sec(11, bytes)
}

#[test]
fn labels_and_ranges() {
    let m = module(&[types(), custom("producers", b"x"), data(&[1, 2, 3]), custom("producers", b"y")]);
    let s = sections(&m).unwrap();
    let labels: Vec<&str> = s.iter().map(|x| x.label.as_str()).collect();
    assert_eq!(labels, ["type", "custom:producers", "data", "custom:producers#2"]);
    // type 6 bytes at 8; custom: payload 1+9+1 = 11, 13 in all; data 5 bytes; custom 13.
    let ranges: Vec<(usize, usize)> = s.iter().map(|x| x.range).collect();
    assert_eq!(ranges, [(8, 14), (14, 27), (27, 32), (32, 45)]);
}

#[test]
fn identical_modules() {
    let m = module(&[types(), data(&[7])]);
    let (lines, same) = diff(&m, &m.clone()).unwrap();
    assert!(same);
    assert!(lines[0].starts_with("identical sha256:"));
    assert_eq!(&lines[1..], ["same type 6", "same data 3"]);
}

#[test]
fn injected_build_id_is_named_and_does_not_shift_the_others() {
    // AC-3: a build-time stamp in its own custom section, inserted in the middle.
    let a = module(&[types(), data(&[1, 2])]);
    let b = module(&[types(), custom("build_id", &[0x17, 0x31, 0x99, 0x02]), data(&[1, 2])]);
    let (lines, same) = diff(&a, &b).unwrap();
    assert!(!same);
    assert!(lines[0].starts_with("differ sha256:"));
    // build_id: payload 1 + 8 + 4 = 13, section 15 bytes.
    assert_eq!(
        &lines[1..],
        ["same type 6", "same data 4", "ONLY_B custom:build_id 15", "differing sections: custom:build_id"]
    );
}

#[test]
fn changed_payload_is_reported_with_both_digests() {
    let a = module(&[types(), data(&[1, 2])]);
    let b = module(&[types(), data(&[1, 2, 3])]);
    let (lines, same) = diff(&a, &b).unwrap();
    assert!(!same);
    assert_eq!(lines[1], "same type 6");
    let d: Vec<&str> = lines[2].split(' ').collect();
    assert_eq!(&d[..4], ["DIFFERS", "data", "4", "5"]);
    assert!(d[4].starts_with("sha256:") && d[5].starts_with("sha256:") && d[4] != d[5]);
    assert_eq!(lines[3], "differing sections: data");
    assert_eq!(lines.len(), 4);
}

#[test]
fn section_missing_from_the_second_module() {
    let a = module(&[types(), custom("name", b"f")]);
    let b = module(&[types()]);
    let (lines, same) = diff(&a, &b).unwrap();
    assert!(!same);
    // custom "name": payload 1 + 4 + 1 = 6, section 8 bytes.
    assert_eq!(&lines[1..], ["same type 6", "ONLY_A custom:name 8", "differing sections: custom:name"]);
}

#[test]
fn reordered_custom_sections_differ_with_no_section_named() {
    let a = module(&[types(), custom("a", b""), custom("b", b"")]);
    let b = module(&[types(), custom("b", b""), custom("a", b"")]);
    let (lines, same) = diff(&a, &b).unwrap();
    assert!(!same);
    assert_eq!(lines.last().unwrap(), "differing sections: none");
}

#[test]
fn refused_module_is_an_error() {
    let mut bad = module(&[types()]);
    bad[0] = 0x01;
    assert_eq!(diff(&bad, &module(&[types()])).unwrap_err().kind, WasmErrorKind::BadMagic);
    let truncated = module(&[types()])[..12].to_vec();
    assert!(sections(&truncated).is_err());
}
