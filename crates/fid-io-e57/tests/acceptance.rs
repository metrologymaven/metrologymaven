//! Acceptance criteria of spec node IO.CLOUD.E57.PHYS (AC-1 .. AC-4) and the requirements
//! behind them. Fixture files and every expected outcome come from the independent stdlib
//! Python reference `fixtures/io/e57/phys/make_fixtures.py`; none is produced by this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_io_e57::{
    crc32c, crc32c_update, validate, E57Header, HeaderField, LogicalReader, LogicalWriter, PageLayout, PhysError,
    ReaderOptions, DEFAULT_CACHE_BUDGET, DEFAULT_PAGE_SIZE, HEADER_LEN, MAX_PAGE_SIZE,
};
use fid_io_reg::LimitKind;
use std::io::Cursor;
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/io/e57/phys").join(name)
}

fn read_fixture(name: &str) -> Vec<u8> {
    std::fs::read(fixture(name)).unwrap()
}

/// A deterministic byte generator (SplitMix64), so the AC-1 data is seeded and repeatable.
struct SplitMix(u64);

impl SplitMix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| (self.next() >> 56) as u8).collect()
    }
}

/// The outcome line format of expected.txt: open, then verify every page.
fn outcome(bytes: &[u8]) -> String {
    let mut r = match LogicalReader::open(Cursor::new(bytes)) {
        Ok(r) => r,
        Err(e) => return describe(&e),
    };
    if let Err(e) = r.verify_all() {
        return describe(&e);
    }
    let h = *r.header();
    format!(
        "ok page_size={} file_length={} xml_offset={} xml_length={} pages={}",
        h.page_size,
        h.file_length,
        h.xml_offset,
        h.xml_length,
        r.layout().page_count()
    )
}

fn describe(e: &PhysError) -> String {
    match e {
        PhysError::ChecksumMismatch { offset } => format!("checksum_mismatch {offset}"),
        PhysError::Malformed { field } => format!("malformed {field}"),
        PhysError::TruncatedFile { at } => format!("truncated_file {at}"),
        PhysError::LimitExceeded { limit, value, .. } => format!("limit_exceeded {limit} {value}"),
        PhysError::Io { kind, at } => format!("io {kind:?} {at}"),
    }
}

fn expected() -> Vec<(String, String)> {
    let text = std::fs::read_to_string(fixture("expected.txt")).unwrap();
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let (k, v) = l.split_once(' ').unwrap();
            (k.to_string(), v.to_string())
        })
        .collect()
}

/// Writes `logical[HEADER_LEN..]` through a LogicalWriter whose XML section is the last
/// `xml_len` bytes; returns the file bytes.
fn write_logical(payload: &[u8], page_size: u64, xml_len: u64) -> Vec<u8> {
    let mut w = LogicalWriter::new(Cursor::new(Vec::new()), page_size).unwrap();
    let split = payload.len() - xml_len as usize;
    w.append(&payload[..split]).unwrap();
    let xml_at = w.physical_position().unwrap();
    w.append(&payload[split..]).unwrap();
    let (cur, _) = w.finish(xml_at, xml_len).unwrap();
    cur.into_inner()
}

// ---- CRC-32C against the Python reference ---------------------------------------------------

#[test]
fn crc32c_matches_independent_python_vectors() {
    let text = std::fs::read_to_string(fixture("crc32c.txt")).unwrap();
    let mut n = 0;
    for line in text.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        let (input, crc) = line.split_once(' ').unwrap();
        let data: Vec<u8> = if input == "-" {
            Vec::new()
        } else {
            (0..input.len()).step_by(2).map(|i| u8::from_str_radix(&input[i..i + 2], 16).unwrap()).collect()
        };
        let want = u32::from_str_radix(crc.trim(), 16).unwrap();
        assert_eq!(crc32c(&data), want, "vector {n}");
        // Split feeding gives the same result as one call.
        for cut in [0, 1, 7, 8, 9, data.len() / 2, data.len()] {
            let cut = cut.min(data.len());
            let reg = crc32c_update(crc32c_update(0xFFFF_FFFF, &data[..cut]), &data[cut..]);
            assert_eq!(!reg, want, "vector {n} split at {cut}");
        }
        n += 1;
    }
    assert_eq!(n, 8);
}

// ---- Python fixtures: every outcome (REQ-001, REQ-002, AC-3, AC-4) --------------------------

#[test]
fn fixture_outcomes_match_reference() {
    let cases = expected();
    assert_eq!(cases.len(), 18);
    for (name, want) in cases {
        let got = outcome(&read_fixture(&name));
        assert_eq!(got, want, "{name}");
    }
}

/// AC-4, on synthetic files (see the PR: no corpus file is available to this leaf yet).
#[test]
fn ac4_every_page_checksum_verifies_on_reference_files() {
    for name in ["ok_1024.e57", "ok_64.e57"] {
        let bytes = read_fixture(name);
        let mut r = LogicalReader::open(Cursor::new(&bytes)).unwrap();
        r.verify_all().unwrap();
        // The logical stream is exactly the reference's logical bytes, then zero padding.
        let logical = read_fixture(&name.replace(".e57", ".logical"));
        let mut got = vec![0u8; r.logical_length() as usize];
        r.read_at(0, &mut got).unwrap();
        assert_eq!(&got[..logical.len()], &logical[..], "{name}");
        assert!(got[logical.len()..].iter().all(|&b| b == 0), "{name} padding");
        // The XML section reads back where the header says.
        let mut xml = vec![0u8; r.header().xml_length as usize];
        r.read_at(r.xml_logical_offset(), &mut xml).unwrap();
        assert!(xml.starts_with(b"<?xml"), "{name}");
    }
}

/// REQ-003: the writer reproduces the reference files byte for byte from their logical bytes.
#[test]
fn writer_reproduces_reference_files() {
    for (name, page_size) in [("ok_1024", 1024u64), ("ok_64", 64)] {
        let logical = read_fixture(&format!("{name}.logical"));
        let want = read_fixture(&format!("{name}.e57"));
        let header = E57Header::parse(logical[..HEADER_LEN].try_into().unwrap()).unwrap();
        let got = write_logical(&logical[HEADER_LEN..], page_size, header.xml_length);
        assert_eq!(got, want, "{name}");
    }
}

// ---- AC-1 / AC-2: 10 MiB seeded round trip ------------------------------------------------

const TEN_MIB: usize = 10 * 1024 * 1024;

fn ac1_file() -> (Vec<u8>, Vec<u8>) {
    let mut rng = SplitMix(0xE57);
    let data = rng.bytes(TEN_MIB);
    let file = write_logical(&data, DEFAULT_PAGE_SIZE, 64);
    (data, file)
}

#[test]
fn ac1_seeded_round_trip_in_random_ranges() {
    let (data, file) = ac1_file();
    let mut rng = SplitMix(2807);
    for budget in [DEFAULT_CACHE_BUDGET, 64 * 1024, 0] {
        let opts = ReaderOptions { cache_budget_bytes: budget, ..ReaderOptions::default() };
        let mut r = LogicalReader::open_with(Cursor::new(&file), opts).unwrap();
        let base = HEADER_LEN as u64;
        assert_eq!(r.layout().page_count(), (base + data.len() as u64).div_ceil(1020));
        let rounds = if budget == 0 { 200 } else { 2000 };
        for _ in 0..rounds {
            let start = rng.below(data.len() as u64);
            let len = rng.below((data.len() as u64 - start).min(70_000) + 1);
            let mut buf = vec![0u8; len as usize];
            r.read_at(base + start, &mut buf).unwrap();
            assert_eq!(&buf[..], &data[start as usize..(start + len) as usize]);
            assert!(r.cache().used_bytes() <= budget, "REQ-004");
        }
        if budget > 0 {
            r.verify_all().unwrap();
        }
        assert!(r.cache().used_bytes() <= budget);
    }
}

#[test]
fn ac2_flipped_byte_in_page_7_fails_with_its_offset() {
    let (_, mut file) = ac1_file();
    let page_size = DEFAULT_PAGE_SIZE as usize;
    file[7 * page_size + 123] ^= 0x40;
    let mut r = LogicalReader::open(Cursor::new(&file)).unwrap();
    // A range before page 7 still reads.
    let mut buf = vec![0u8; 1000];
    r.read_at(100, &mut buf).unwrap();
    // A range covering page 7 (logical 7 × 1020 .. 8 × 1020) fails naming 7 × page_size.
    let mut buf = vec![0u8; 3000];
    let err = r.read_at(6 * 1020 + 500, &mut buf).unwrap_err();
    assert_eq!(err, PhysError::ChecksumMismatch { offset: 7 * DEFAULT_PAGE_SIZE });
    assert_eq!(err.code(), "checksum_mismatch");
    // The failure is not cached away: reading again fails the same way.
    assert_eq!(r.read_at(7 * 1020, &mut buf[..1]).unwrap_err(), err);
}

// ---- AC-3 and REQ-002 on in-memory files -------------------------------------------------------

#[test]
fn ac3_file_length_larger_than_file_is_malformed() {
    let mut file = read_fixture("ok_1024.e57");
    let declared = file.len() as u64 + 1;
    file[16..24].copy_from_slice(&declared.to_le_bytes());
    // Header fields are checked before any page checksum, so page 0's stale checksum does
    // not mask the error.
    let err = LogicalReader::open(Cursor::new(&file)).unwrap_err();
    assert_eq!(err, PhysError::Malformed { field: HeaderField::FileLength });
    assert_eq!(err.code(), "malformed");
}

#[test]
fn header_rules_unit() {
    let ok = E57Header {
        major_version: 1,
        minor_version: 0,
        file_length: 4096,
        xml_offset: 1024,
        xml_length: 100,
        page_size: 1024,
    };
    assert!(validate(&ok, 4096).is_ok());
    let field = |h: E57Header, len: u64| match validate(&h, len) {
        Err(PhysError::Malformed { field }) => Some(field),
        _ => None,
    };
    assert_eq!(field(ok, 5120), Some(HeaderField::FileLength));
    assert_eq!(field(E57Header { xml_offset: u64::MAX, ..ok }, 4096), Some(HeaderField::XmlOffset));
    assert_eq!(field(E57Header { xml_length: u64::MAX, ..ok }, 4096), Some(HeaderField::XmlLength));
    // Four pages hold 4 × 1020 logical bytes; the XML section starts at logical 1020.
    assert_eq!(field(E57Header { xml_length: 3 * 1020 + 1, ..ok }, 4096), Some(HeaderField::XmlLength));
    assert!(validate(&E57Header { xml_length: 3 * 1020, ..ok }, 4096).is_ok());
    assert_eq!(field(E57Header { page_size: 0, ..ok }, 4096), Some(HeaderField::PageSize));
    assert_eq!(field(E57Header { major_version: 0, ..ok }, 4096), Some(HeaderField::MajorVersion));
    assert_eq!(
        validate(&E57Header { page_size: MAX_PAGE_SIZE * 2, file_length: MAX_PAGE_SIZE * 2, ..ok }, MAX_PAGE_SIZE * 2),
        Err(PhysError::LimitExceeded { limit: LimitKind::Alloc, value: MAX_PAGE_SIZE, at: 40 })
    );
    // Encode and parse are inverse.
    assert_eq!(E57Header::parse(&ok.encode()).unwrap(), ok);
}

#[test]
fn layout_mapping() {
    let l = PageLayout::new(1024, 4).unwrap();
    assert_eq!(l.physical_of(0), Some(0));
    assert_eq!(l.physical_of(1019), Some(1019));
    assert_eq!(l.physical_of(1020), Some(1024));
    assert_eq!(l.physical_of(7 * 1020 + 5), Some(7 * 1024 + 5));
    assert_eq!(l.logical_of(1024), Some(1020));
    assert_eq!(l.logical_of(1020), None);
    assert_eq!(l.logical_of(1023), None);
    assert_eq!(l.physical_of(u64::MAX), None);
    assert!(PageLayout::new(1024, u64::MAX).is_err());
}

#[test]
fn file_over_byte_limit_is_limit_exceeded() {
    let file = read_fixture("ok_1024.e57");
    let opts = ReaderOptions { max_file_bytes: 2048, ..ReaderOptions::default() };
    let err = LogicalReader::open_with(Cursor::new(&file), opts).unwrap_err();
    assert_eq!(err, PhysError::LimitExceeded { limit: LimitKind::Bytes, value: 2048, at: 2048 });
}

#[test]
fn reads_past_the_logical_end_fail_cleanly() {
    let file = read_fixture("ok_64.e57");
    let mut r = LogicalReader::open(Cursor::new(&file)).unwrap();
    let len = r.logical_length();
    let mut buf = [0u8; 2];
    assert_eq!(r.read_at(len - 1, &mut buf).unwrap_err().code(), "truncated_file");
    assert_eq!(r.read_at(u64::MAX, &mut buf).unwrap_err().code(), "truncated_file");
    r.read_at(len, &mut []).unwrap();
}

#[test]
fn writer_rejects_inconsistent_header() {
    let mut w = LogicalWriter::new(Cursor::new(Vec::new()), 1024).unwrap();
    w.append(&[7u8; 3000]).unwrap();
    // XML offset inside page 0's checksum.
    let err = w.finish(1022, 1).unwrap_err();
    assert_eq!(err, PhysError::Malformed { field: HeaderField::XmlOffset });
    assert_eq!(
        LogicalWriter::new(Cursor::new(Vec::new()), 16).unwrap_err(),
        PhysError::Malformed { field: HeaderField::PageSize }
    );
}

/// Hostile input: every single-bit flip of every byte, and every truncation, of a small
/// reference file returns a result (never panics), and reading after a successful open stays
/// within bounds.
#[test]
fn hostile_mutations_never_panic() {
    let base = read_fixture("ok_64.e57");
    let exercise = |bytes: &[u8]| {
        if let Ok(mut r) = LogicalReader::open(Cursor::new(bytes)) {
            let _ = r.verify_all();
            let mut buf = vec![0u8; 100];
            let _ = r.read_at(0, &mut buf);
            let _ = r.read_at(r.logical_length().saturating_sub(50), &mut buf);
        }
    };
    for i in 0..base.len() {
        for bit in 0..8 {
            let mut m = base.clone();
            m[i] ^= 1 << bit;
            exercise(&m[..]);
        }
    }
    for n in 0..base.len() {
        exercise(&base[..n]);
    }
    // Header values at the extremes.
    for v in [0u64, 1, 51, 52, u64::MAX, u64::MAX - 63, 1 << 63] {
        for at in [16usize, 24, 32, 40] {
            let mut m = base.clone();
            m[at..at + 8].copy_from_slice(&v.to_le_bytes());
            exercise(&m[..]);
        }
    }
}
