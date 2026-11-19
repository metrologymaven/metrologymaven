//! TRUST.PKG.READER AC-3: the 42 KB zip bomb (fixture from
//! `fixtures/pkg/make_reader_fixtures.py`) inflates to 44 MB, but its manifest says 4096
//! bytes; the reader stops with `pkg.limit` and peak resident memory stays under 128 MiB.
//!
//! This file holds a single test so it runs in a process of its own: on Linux the peak
//! resident set (`VmHWM`) of that process is the reader's peak plus the test harness.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_pkg::reader::PackageReader;
use std::io::Cursor;

const BOMB: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/pkg/malformed/bomb_member_length/package.zip");
const LIMIT_KIB: u64 = 128 * 1024;

#[test]
fn ac3_zip_bomb_stops_with_limit_under_128_mib() {
    let bytes = std::fs::read(BOMB).unwrap();
    assert!((40_000..=46_000).contains(&bytes.len()));
    for _ in 0..3 {
        let mut r = PackageReader::open(Cursor::new(&bytes[..])).unwrap();
        let e = r.verify_all().unwrap_err();
        assert_eq!((e.code(), e.reason()), ("pkg.limit", "member_length"));
        let e = r.read_member("results/records.json", u64::MAX).unwrap_err();
        assert_eq!(e.reason(), "member_length");
    }
    if cfg!(target_os = "linux") {
        let status = std::fs::read_to_string("/proc/self/status").unwrap();
        let hwm = status
            .lines()
            .find_map(|l| l.strip_prefix("VmHWM:"))
            .map(|v| v.trim().trim_end_matches("kB").trim().parse::<u64>().unwrap())
            .expect("VmHWM");
        assert!(hwm < LIMIT_KIB, "peak resident memory {hwm} KiB");
        eprintln!("peak resident memory: {hwm} KiB");
    }
}
