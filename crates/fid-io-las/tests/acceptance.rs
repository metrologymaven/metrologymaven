//! Acceptance criteria of spec node IO.CLOUD.LAS.HDR (AC-1 .. AC-5) and the requirements
//! behind them. Fixture files and every expected value come from the independent stdlib
//! Python reference `fixtures/io/cloud/las/hdr/make_fixtures.py`; none is produced by this
//! crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_io_las::{
    fuzz_options, read_slice, write_file, CountUnit, ExtraValues, LasError, LasField, LasHeader, LasReader, LasWriter,
    PointFormat, RawPointBatch, ReaderOptions, VlrKind, DEFAULT_BATCH_POINTS,
};
use fid_io_reg::{LimitKind, LimitRequest, Limits, Principal};
use std::fmt::Write as _;
use std::io::Cursor;
use std::path::PathBuf;

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/io/cloud/las/hdr").join(name);
    std::fs::read(path).unwrap()
}

fn u16_le(b: &[u8], at: usize) -> usize {
    usize::from(u16::from_le_bytes([b[at], b[at + 1]]))
}

fn u32_le(b: &[u8], at: usize) -> usize {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]) as usize
}

/// The members of a ZIP archive of stored (uncompressed) entries, from the local headers.
fn unzip_stored(zip: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let mut at = 0;
    while zip.len() >= at + 30 && u32_le(zip, at) == 0x0403_4b50 {
        assert_eq!(u16_le(zip, at + 8), 0, "stored entries only");
        let size = u32_le(zip, at + 18);
        let name_len = u16_le(zip, at + 26);
        let extra_len = u16_le(zip, at + 28);
        let name = String::from_utf8(zip[at + 30..at + 30 + name_len].to_vec()).unwrap();
        let data_at = at + 30 + name_len + extra_len;
        out.push((name, zip[data_at..data_at + size].to_vec()));
        at = data_at + size;
    }
    out
}

fn pdrf_members() -> Vec<(String, Vec<u8>)> {
    unzip_stored(&fixture("pdrf_all.zip"))
}

/// Expected lines per fixture name, in file order.
fn expected() -> Vec<(String, String)> {
    let text = String::from_utf8(fixture("expected.txt")).unwrap();
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let (k, v) = l.split_once(' ').unwrap();
            (k.to_string(), v.to_string())
        })
        .collect()
}

fn describe(e: &LasError) -> String {
    match e {
        LasError::TruncatedFile { declared, actual, unit } => {
            format!("truncated_file {declared} {actual} {}", unit.as_str())
        }
        LasError::Malformed { field } => format!("malformed {field}"),
        LasError::LimitExceeded { limit, value, .. } => format!("limit_exceeded {limit} {value}"),
        LasError::LasVersionUnsupported { major, minor } => format!("las_version_unsupported {major} {minor}"),
        LasError::Io { kind, at } => format!("io {kind:?} {at}"),
    }
}

fn join<T: ToString>(v: &[T]) -> String {
    v.iter().map(ToString::to_string).collect::<Vec<_>>().join("/")
}

fn hex(b: &[u8]) -> String {
    b.iter().fold(String::new(), |mut s, x| {
        write!(s, "{x:02x}").unwrap();
        s
    })
}

/// One point in the expected.txt line format.
fn point_line(b: &RawPointBatch, j: usize, index: u64) -> String {
    let a = &b.attrs;
    let mut s = format!(
        "point {index} xi={} yi={} zi={} intensity={} return_number={} number_of_returns={} scan_direction={} \
         edge_of_flight_line={} classification={} class_flags={}",
        b.xi[j],
        b.yi[j],
        b.zi[j],
        a.intensity[j],
        a.return_number[j],
        a.number_of_returns[j],
        a.scan_direction[j],
        a.edge_of_flight_line[j],
        a.classification[j],
        a.class_flags[j]
    );
    if let Some(c) = &a.scanner_channel {
        write!(s, " scanner_channel={}", c[j]).unwrap();
    }
    if let Some(c) = &a.scan_angle {
        write!(s, " scan_angle={}", c[j]).unwrap();
    }
    if let Some(c) = &a.scan_angle_rank {
        write!(s, " scan_angle_rank={}", c[j]).unwrap();
    }
    write!(s, " user_data={} point_source_id={}", a.user_data[j], a.point_source_id[j]).unwrap();
    if let Some(c) = &a.gps_time {
        write!(s, " gps_time={:016x}", c[j].to_bits()).unwrap();
    }
    if let Some(c) = &a.rgb {
        write!(s, " rgb={}", join(&c[j])).unwrap();
    }
    if let Some(c) = &a.nir {
        write!(s, " nir={}", c[j]).unwrap();
    }
    if let Some(c) = &a.wave_packet {
        write!(s, " wave_packet={}", hex(&c[j])).unwrap();
    }
    for col in &a.extra {
        let n = col.spec.count;
        let r = j * n..(j + 1) * n;
        let text = match &col.values {
            ExtraValues::Raw(v) => hex(&v[r]),
            ExtraValues::U8(v) => join(&v[r]),
            ExtraValues::I8(v) => join(&v[r]),
            ExtraValues::U16(v) => join(&v[r]),
            ExtraValues::I16(v) => join(&v[r]),
            ExtraValues::U32(v) => join(&v[r]),
            ExtraValues::I32(v) => join(&v[r]),
            ExtraValues::U64(v) => join(&v[r]),
            ExtraValues::I64(v) => join(&v[r]),
            ExtraValues::F32(v) => v[r].iter().map(|x| format!("{:08x}", x.to_bits())).collect::<Vec<_>>().join("/"),
            ExtraValues::F64(v) => v[r].iter().map(|x| format!("{:016x}", x.to_bits())).collect::<Vec<_>>().join("/"),
        };
        write!(s, " extra:{}={text}", col.spec.name.as_display()).unwrap();
    }
    s
}

/// Reads `bytes` in batches of `batch` points and renders the expected.txt lines.
fn outcome(bytes: &[u8], batch: usize) -> Vec<String> {
    let opts = ReaderOptions { batch_points: batch, ..ReaderOptions::default() };
    let mut r = match LasReader::open_with(Cursor::new(bytes), opts) {
        Ok(r) => r,
        Err(e) => return vec![describe(&e)],
    };
    let h = r.header().clone();
    let mut points = Vec::new();
    let mut attrs = String::new();
    let mut index = 0u64;
    loop {
        match r.next_batch() {
            Ok(Some(b)) => {
                assert!(b.len() <= batch);
                attrs = b.attribute_names().join(",");
                for j in 0..b.len() {
                    points.push(point_line(&b, j, index));
                    index += 1;
                }
            }
            Ok(None) => break,
            Err(e) => return vec![describe(&e)],
        }
    }
    let wf = r.waveform();
    let mut lines = vec![
        format!(
            "ok version={}.{} format={} record_length={} points={} vlrs={} evlrs={} waveform_bytes={} \
             waveform_points={}",
            h.version_major,
            h.version_minor,
            r.format().id(),
            h.point_data_record_length,
            r.point_count(),
            r.vlrs().len(),
            r.evlrs().len(),
            wf.data_bytes,
            wf.points_with_packets
        ),
        format!("attrs={attrs}"),
    ];
    lines.extend(points);
    lines
}

/// AC-1 (REQ-IO.CLOUD.LAS.HDR-003): one file per point format 0 to 10; each yields the
/// expected point count, attribute set and every decoded value, with batching on.
#[test]
fn ac1_every_point_format_decodes_as_expected() {
    let members = pdrf_members();
    let names: Vec<_> = members.iter().map(|(n, _)| n.clone()).collect();
    let want: Vec<String> = (0..=10).map(|f| format!("pdrf_{f:02}.las")).collect();
    assert_eq!(names, want);
    let exp = expected();
    for (name, bytes) in &members {
        let want: Vec<&str> = exp.iter().filter(|(k, _)| k == name).map(|(_, v)| v.as_str()).collect();
        assert!(want.len() >= 3, "{name}");
        for batch in [1, 2, DEFAULT_BATCH_POINTS] {
            assert_eq!(outcome(bytes, batch), want, "{name} with batches of {batch}");
        }
    }
}

/// Every error fixture fails with exactly the hand-labelled error.
#[test]
fn error_fixtures_fail_as_labelled() {
    let mut seen = 0;
    for (name, want) in expected() {
        if name.starts_with("pdrf_") {
            continue;
        }
        assert_eq!(outcome(&fixture(&name), DEFAULT_BATCH_POINTS), vec![want.clone()], "{name}");
        seen += 1;
    }
    assert!(seen >= 25, "{seen}");
}

/// AC-2 (REQ-IO.CLOUD.LAS.HDR-001): 1,000 declared points, 999 present.
#[test]
fn ac2_short_points_is_truncated_file_with_both_numbers() {
    let err = LasReader::open(Cursor::new(fixture("short_points.las"))).unwrap_err();
    assert_eq!(err, LasError::TruncatedFile { declared: 1000, actual: 999, unit: CountUnit::Points });
    assert_eq!(err.code(), "truncated_file");
    let text = err.to_string();
    assert!(text.contains("1000") && text.contains("999"), "{text}");
}

/// AC-3 (REQ-IO.CLOUD.LAS.HDR-002): a VLR length running past the point data offset.
#[test]
fn ac3_vlr_length_past_point_data_is_malformed() {
    let err = LasReader::open(Cursor::new(fixture("vlr_past_offset.las"))).unwrap_err();
    assert_eq!(err, LasError::Malformed { field: LasField::VlrLength });
    assert_eq!(err.to_string(), "malformed: field vlr_length");
}

/// SplitMix64: seeded, repeatable test data.
struct SplitMix(u64);

impl SplitMix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// AC-4 (REQ-IO.CLOUD.LAS.HDR-004): 10^5 seeded format-7 records, every byte random
/// (every bit of a format-7 record is a field), read and written back byte-identical;
/// then typed columns written, read back, equal field by field.
#[test]
fn ac4_format7_records_round_trip_byte_identical() {
    const N: usize = 100_000;
    let mut rng = SplitMix(0x1A5_2026);
    let mut h = LasHeader::new(4, PointFormat::FORMAT_7, 0);
    h.point_count = N as u64;
    h.header_size = 375;
    h.offset_to_point_data = 375;
    h.scale = [0.001, 0.001, 0.0005];
    let mut file = h.to_bytes();
    assert_eq!(file.len(), 375);
    for _ in 0..N * 36 / 8 {
        file.extend_from_slice(&rng.next().to_le_bytes());
    }
    let read = read_slice(&file, ReaderOptions::default()).unwrap();
    assert_eq!(read.points.len(), N);
    let written = write_file(&read).unwrap();
    assert!(written == file, "raw format-7 records did not round-trip byte for byte");

    // Typed columns in, typed columns out.
    let mut b = RawPointBatch::with_capacity(PointFormat::FORMAT_7, &[], N);
    for _ in 0..N {
        b.xi.push(rng.next() as i32);
        b.yi.push(rng.next() as i32);
        b.zi.push(rng.next() as i32);
        let a = &mut b.attrs;
        a.intensity.push(rng.next() as u16);
        a.return_number.push(rng.below(16) as u8);
        a.number_of_returns.push(rng.below(16) as u8);
        a.scan_direction.push(rng.below(2) as u8);
        a.edge_of_flight_line.push(rng.below(2) as u8);
        a.classification.push(rng.next() as u8);
        a.class_flags.push(rng.below(16) as u8);
        a.scanner_channel.as_mut().unwrap().push(rng.below(4) as u8);
        a.scan_angle.as_mut().unwrap().push(rng.next() as i16);
        a.user_data.push(rng.next() as u8);
        a.point_source_id.push(rng.next() as u16);
        a.gps_time.as_mut().unwrap().push(f64::from_bits(rng.next()));
        a.rgb.as_mut().unwrap().push([rng.next() as u16, rng.next() as u16, rng.next() as u16]);
    }
    let mut w = LasWriter::new(Vec::new(), &h, &[], &[], &[]).unwrap();
    w.write_batch(&b).unwrap();
    let bytes = w.finish().unwrap();
    let back = read_slice(&bytes, ReaderOptions::default()).unwrap().points;
    assert_eq!(back.xi, b.xi);
    assert_eq!(back.yi, b.yi);
    assert_eq!(back.zi, b.zi);
    let (x, y) = (&back.attrs, &b.attrs);
    assert_eq!(x.intensity, y.intensity);
    assert_eq!(x.return_number, y.return_number);
    assert_eq!(x.number_of_returns, y.number_of_returns);
    assert_eq!(x.scan_direction, y.scan_direction);
    assert_eq!(x.edge_of_flight_line, y.edge_of_flight_line);
    assert_eq!(x.classification, y.classification);
    assert_eq!(x.class_flags, y.class_flags);
    assert_eq!(x.scanner_channel, y.scanner_channel);
    assert_eq!(x.scan_angle, y.scan_angle);
    assert_eq!(x.user_data, y.user_data);
    assert_eq!(x.point_source_id, y.point_source_id);
    assert_eq!(x.rgb, y.rgb);
    let bits = |v: &Option<Vec<f64>>| v.as_ref().unwrap().iter().map(|t| t.to_bits()).collect::<Vec<_>>();
    assert_eq!(bits(&x.gps_time), bits(&y.gps_time));
}

/// REQ-IO.CLOUD.LAS.HDR-004 on every format: each AC-1 file read and written back is
/// byte-identical, except the two whose waveform data is counted and not imported.
#[test]
fn every_format_file_round_trips_byte_identical() {
    for (name, bytes) in pdrf_members() {
        let file = read_slice(&bytes, ReaderOptions::default()).unwrap();
        match write_file(&file) {
            Ok(out) => assert!(out == bytes, "{name} did not round-trip"),
            Err(e) => {
                assert!(name == "pdrf_04.las" || name == "pdrf_09.las", "{name}: {e}");
                assert_eq!(e, LasError::Malformed { field: LasField::EvlrPayload });
                assert!(file.waveform.data_bytes > 0);
            }
        }
    }
}

/// REQ-IO.CLOUD.LAS.HDR-005: payloads are bounded, and CRS text is untrusted.
#[test]
fn payloads_are_bounded_and_untrusted() {
    let members = pdrf_members();
    let get = |n: &str| members.iter().find(|(m, _)| m == n).unwrap().1.clone();

    let file = read_slice(&get("pdrf_06.las"), ReaderOptions::default()).unwrap();
    let wkt = file.vlrs.iter().find(|v| v.kind() == VlrKind::WktCrs).unwrap();
    let shown = wkt.payload.bytes().unwrap().to_text().as_display().to_string();
    assert!(shown.starts_with("GEOGCS[\"fixture\""), "{shown}");
    assert!(shown.contains("\\u{1b}") && shown.contains("\\u{202e}"), "{shown}");
    assert!(!shown.chars().any(|c| c == '\u{1b}' || c == '\u{202e}'));
    assert_eq!(wkt.user_id_text().as_display().to_string(), "LASF_Projection");

    // pdrf_08 holds a 100-byte EVLR: a 50-byte bound refuses it before reading it.
    let opts = ReaderOptions { max_record_payload: 50, ..ReaderOptions::default() };
    let err = LasReader::open_with(Cursor::new(get("pdrf_08.las")), opts).unwrap_err();
    assert!(matches!(err, LasError::LimitExceeded { limit: LimitKind::Alloc, value: 50, .. }), "{err:?}");
    assert_eq!(fid_io_las::DEFAULT_MAX_PAYLOAD, 64 * 1024 * 1024);

    // Waveform data is counted, not loaded.
    let file = read_slice(&get("pdrf_09.las"), ReaderOptions::default()).unwrap();
    let wave = file.evlrs.iter().find(|v| v.kind() == VlrKind::WaveformData).unwrap();
    assert_eq!(wave.payload.bytes(), None);
    assert_eq!(wave.payload.len(), 64);
}

/// The IO.REG.LIMITS harness: file size, element count and tracked allocation.
#[test]
fn reader_limits_apply() {
    let bytes = fixture("short_points.las");
    let limits = |req: LimitRequest| Limits::resolve(Principal::Script, &req).unwrap();
    let opts = ReaderOptions { limits: limits(LimitRequest::default().bytes(1000)), ..ReaderOptions::default() };
    let err = LasReader::open_with(Cursor::new(&bytes), opts).unwrap_err();
    assert_eq!(err, LasError::LimitExceeded { limit: LimitKind::Bytes, value: 1000, at: 1000 });

    let (name, pdrf) = pdrf_members().remove(2);
    assert_eq!(name, "pdrf_02.las");
    let opts = ReaderOptions { limits: limits(LimitRequest::default().elements(2)), ..ReaderOptions::default() };
    let err = LasReader::open_with(Cursor::new(&pdrf), opts).unwrap_err();
    assert!(matches!(err, LasError::LimitExceeded { limit: LimitKind::Elements, value: 2, .. }), "{err:?}");

    let opts = ReaderOptions { limits: limits(LimitRequest::default().alloc(100)), ..ReaderOptions::default() };
    let mut r = LasReader::open_with(Cursor::new(&pdrf), opts).unwrap();
    let err = r.next_batch().unwrap_err();
    assert!(matches!(err, LasError::LimitExceeded { limit: LimitKind::Alloc, value: 100, .. }), "{err:?}");

    // An agent cannot raise a limit above its default.
    assert!(Limits::resolve(Principal::Agent, &LimitRequest::default().alloc(u64::MAX)).is_err());
}

/// Batches hold at most 65,536 points by default.
#[test]
fn default_batches_are_65536_points() {
    let n = DEFAULT_BATCH_POINTS + 1;
    let mut h = LasHeader::new(2, PointFormat::new(0).unwrap(), 0);
    h.legacy_point_count = n as u32;
    h.header_size = 227;
    h.offset_to_point_data = 227;
    let mut file = h.to_bytes();
    for i in 0..n {
        let mut rec = [0u8; 20];
        rec[..4].copy_from_slice(&(i as i32).to_le_bytes());
        file.extend_from_slice(&rec);
    }
    let mut r = LasReader::open(Cursor::new(file)).unwrap();
    let first = r.next_batch().unwrap().unwrap();
    assert_eq!(first.len(), DEFAULT_BATCH_POINTS);
    let second = r.next_batch().unwrap().unwrap();
    assert_eq!(second.xi, vec![DEFAULT_BATCH_POINTS as i32]);
    assert!(r.next_batch().unwrap().is_none());
}

/// The writer refuses inconsistent input, writing nothing of a bad batch.
#[test]
fn writer_rejects_inconsistent_input() {
    let f0 = PointFormat::new(0).unwrap();
    let mut h = LasHeader::new(2, f0, 0);
    h.legacy_point_count = 1;
    let mut b = RawPointBatch::with_capacity(f0, &[], 1);
    b.xi.push(1);
    b.yi.push(2);
    b.zi.push(3);
    let a = &mut b.attrs;
    a.intensity.push(0);
    a.return_number.push(8); // 3 bits only
    a.number_of_returns.push(1);
    a.scan_direction.push(0);
    a.edge_of_flight_line.push(0);
    a.classification.push(2);
    a.class_flags.push(0);
    a.scan_angle_rank.as_mut().unwrap().push(-5);
    a.user_data.push(0);
    a.point_source_id.push(7);

    let mut w = LasWriter::new(Vec::new(), &h, &[], &[], &[]).unwrap();
    let before = w.header().offset_to_point_data;
    assert_eq!(before, 227);
    assert_eq!(w.write_batch(&b).unwrap_err(), LasError::Malformed { field: LasField::ValueRange });
    b.attrs.return_number[0] = 7;
    let mut wrong = b.clone();
    wrong.attrs.gps_time = Some(vec![0.0]);
    assert_eq!(w.write_batch(&wrong).unwrap_err(), LasError::Malformed { field: LasField::Column });
    let mut other = b.clone();
    other.format = PointFormat::new(1).unwrap();
    assert_eq!(w.write_batch(&other).unwrap_err(), LasError::Malformed { field: LasField::PointDataFormat });
    w.write_batch(&b).unwrap();
    assert_eq!(w.write_batch(&b).unwrap_err(), LasError::Malformed { field: LasField::PointCount });
    let bytes = w.finish().unwrap();
    assert_eq!(bytes.len(), 227 + 20);
    let back = read_slice(&bytes, ReaderOptions::default()).unwrap();
    assert_eq!(back.points, b);

    let w = LasWriter::new(Vec::new(), &h, &[], &[], &[]).unwrap();
    assert_eq!(w.finish().unwrap_err(), LasError::Malformed { field: LasField::PointCount });

    let mut bad = h.clone();
    bad.version_minor = 5;
    assert_eq!(
        LasWriter::new(Vec::new(), &bad, &[], &[], &[]).unwrap_err(),
        LasError::LasVersionUnsupported { major: 1, minor: 5 }
    );
}

/// The default header is LAS 1.4, point format 6.
#[test]
fn default_output_is_1_4_format_6() {
    let h = LasHeader::default();
    assert_eq!((h.version_major, h.version_minor, h.point_data_format), (1, 4, 6));
    assert_eq!(h.point_data_record_length, 30);
    let w = LasWriter::new(Vec::new(), &h, &[], &[], &[]).unwrap();
    let written = w.header().clone();
    assert_eq!(written.offset_to_point_data, 375);
    let bytes = w.finish().unwrap();
    assert_eq!(bytes.len(), 375);
    let back = read_slice(&bytes, ReaderOptions::default()).unwrap();
    assert_eq!(back.header, written);
}

/// One to six seeded byte mutations (overwrite, bit flip, cut, insert).
fn mutate(rng: &mut SplitMix, data: &mut Vec<u8>) {
    const INTERESTING: [u8; 8] = [0, 1, 0x7f, 0x80, 0xff, 0xfe, 0x36, 0x3c];
    for _ in 0..1 + rng.below(6) {
        match rng.below(5) {
            0 | 1 if !data.is_empty() => {
                let i = rng.below(data.len().min(400));
                data[i] = INTERESTING[rng.below(INTERESTING.len())];
            }
            2 if !data.is_empty() => {
                let i = rng.below(data.len());
                data[i] ^= 1 << rng.below(8);
            }
            3 if !data.is_empty() => {
                let cut = rng.below(data.len());
                data.truncate(cut);
            }
            _ => {
                let i = rng.below(data.len() + 1);
                data.insert(i, rng.next() as u8);
            }
        }
    }
}

/// AC-5 on stable (the cargo-fuzz target `crates/fid-io-las/fuzz` runs the same property
/// on nightly): mutated fixtures never panic, and whatever reads also writes back and
/// reads again to the same bytes.
#[test]
fn ac5_fuzz_smoke_never_panics() {
    let mut seeds: Vec<Vec<u8>> = pdrf_members().into_iter().map(|(_, b)| b).collect();
    for (name, _) in expected() {
        if !name.starts_with("pdrf_") && !name.contains("points") {
            seeds.push(fixture(&name));
        }
    }
    let mut rng = SplitMix(0xF022);
    let mut accepted = 0;
    for round in 0..20_000 {
        let mut data = seeds[round % seeds.len()].clone();
        mutate(&mut rng, &mut data);
        let Ok(file) = read_slice(&data, fuzz_options()) else { continue };
        accepted += 1;
        match write_file(&file) {
            Ok(out) => {
                let again = read_slice(&out, fuzz_options()).unwrap();
                assert!(write_file(&again).unwrap() == out, "round {round}: write is not idempotent");
            }
            Err(e) => assert_eq!(e, LasError::Malformed { field: LasField::EvlrPayload }, "round {round}"),
        }
    }
    assert!(accepted > 100, "{accepted}");
}
