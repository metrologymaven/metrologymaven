//! Acceptance criteria of spec node IO.CLOUD.LAS.QUANT (AC-1 .. AC-5) and the requirements
//! behind them. Every expected value in `fixtures/io/cloud/las/quant/cases.txt` comes from
//! the independent stdlib Python reference `make_cases.py` next to it (exact rationals,
//! rounded once); none is produced by this crate. The 10^6-triple test of AC-1 compares with
//! an exact 128-bit integer reference written in this file.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_io_las::quant::{
    import_batch, quantise, write_las, Axis, ExportError, ExportTarget, QuantError, QuantPlan, ValueReason, WriteScale,
    DEFAULT_WRITE_SCALE, MAX_WRITE_SCALE,
};
use fid_io_las::{
    read_slice, AttributeColumns, LasError, LasField, LasHeader, PointFormat, RawPointBatch, ReaderOptions,
};
use fid_num::rng::{Address, Cursor};
use std::path::PathBuf;

fn cases() -> Vec<Vec<String>> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/io/cloud/las/quant/cases.txt");
    let text = std::fs::read_to_string(path).unwrap();
    assert!(!text.contains('\r'), "cases.txt must use LF newlines");
    text.lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.split(' ').map(str::to_string).collect())
        .collect()
}

fn hx(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s, 16).unwrap())
}

fn axis(s: &str) -> Axis {
    match s {
        "x" => Axis::X,
        "y" => Axis::Y,
        "z" => Axis::Z,
        other => panic!("bad axis {other}"),
    }
}

fn bits3(v: [f64; 3]) -> [u64; 3] {
    v.map(f64::to_bits)
}

/// AC-1 (fixture part): decoded coordinates equal the exact value rounded once, including
/// ties, near-ties, cancellations and the 271 cases where two roundings go wrong.
#[test]
fn ac1_decode_matches_reference_cases() {
    let mut n = 0;
    for c in cases().iter().filter(|c| c[0] == "D") {
        let (s, o, v, want) = (hx(&c[1]), hx(&c[2]), c[3].parse::<i32>().unwrap(), hx(&c[4]));
        let plan = QuantPlan::new([s; 3], [o; 3]).unwrap();
        for a in Axis::ALL {
            let got = plan.decode(a, v);
            assert_eq!(got.to_bits(), want.to_bits(), "scale {s:e} offset {o:e} X {v}: got {got:e}, want {want:e}");
        }
        n += 1;
    }
    assert!(n > 1300, "{n} decode cases");
}

/// `x = m * 2^e` exactly.
fn parts(x: f64) -> (i128, i32) {
    let b = x.to_bits();
    let exp = ((b >> 52) & 0x7ff) as i32;
    let frac = (b & ((1 << 52) - 1)) as i128;
    let (m, e) = if exp == 0 { (frac, -1074) } else { (frac | (1 << 52), exp - 1075) };
    (if b >> 63 == 1 { -m } else { m }, e)
}

/// `2^e` for a normal exponent.
fn pow2(e: i32) -> f64 {
    assert!((-1022..=1023).contains(&e));
    f64::from_bits(((e + 1023) as u64) << 52)
}

/// `v * 2^e` rounded once to nearest-even (normal results only).
fn round_once(v: i128, e: i32) -> f64 {
    if v == 0 {
        return 0.0;
    }
    let mut n = v.unsigned_abs();
    let mut e = e;
    let len = 128 - n.leading_zeros() as i32;
    if len > 53 {
        let sh = (len - 53) as u32;
        let rem = n & ((1u128 << sh) - 1);
        let half = 1u128 << (sh - 1);
        n >>= sh;
        if rem > half || (rem == half && n & 1 == 1) {
            n += 1;
        }
        e += sh as i32;
        if n == 1 << 53 {
            n >>= 1;
            e += 1;
        }
    }
    let r = (n as f64) * pow2(e);
    if v < 0 {
        -r
    } else {
        r
    }
}

/// The exact `offset + scale * x`, rounded once.
fn reference(scale: f64, offset: f64, x: i32) -> f64 {
    let (sm, se) = parts(scale);
    let (om, oe) = parts(offset);
    let pm = sm * i128::from(x);
    if om == 0 {
        return round_once(pm, se);
    }
    let e = oe.min(se);
    let (so, sp) = (oe - e, se - e);
    assert!(so <= 70 && sp <= 40, "generator left the exact i128 range");
    round_once((om << so) + (pm << sp), e)
}

fn draw_scale(r: &mut Cursor) -> f64 {
    let frac = r.next_u64() >> 12;
    let exp = 1023 - 3 - r.below(38).unwrap();
    f64::from_bits((exp << 52) | frac)
}

/// AC-1: 10^6 seeded (X, scale, offset) triples equal the exact value rounded once. A third
/// have an independent offset, a third cancel the product, a third are of the product's size
/// (where two roundings fail most often); the naive formula must be wrong for many of them.
#[test]
fn ac1_million_seeded_triples_match_exact_reference() {
    let mut r = Cursor::new(Address { seed: 61, stream: 0 }, 0);
    let mut naive_wrong = 0u64;
    for _ in 0..1_000_000 {
        let s = draw_scale(&mut r);
        let x = r.next_u64() as u32 as i32;
        let p = s * f64::from(x);
        let o = match r.below(3).unwrap() {
            0 if r.below(8).unwrap() == 0 => 0.0,
            0 => {
                let frac = r.next_u64() >> 12;
                let exp = 1023 - 20 + r.below(51).unwrap();
                let sign = (r.next_u64() & 1) << 63;
                f64::from_bits(sign | (exp << 52) | frac)
            }
            1 => -p,
            _ => {
                let k = r.below(7).unwrap() as i32 - 3;
                let m = 1.0 + (r.next_u64() >> 11) as f64 * 2f64.powi(-53);
                -(p * m) * pow2(k)
            }
        };
        let want = reference(s, o, x);
        let plan = QuantPlan::new([s; 3], [o; 3]).unwrap();
        let got = plan.decode(Axis::Y, x);
        assert_eq!(got.to_bits(), want.to_bits(), "scale {s:e} offset {o:e} X {x}");
        let naive = o + p;
        if naive.to_bits() != want.to_bits() && !(naive == 0.0 && want == 0.0) {
            naive_wrong += 1;
        }
    }
    assert!(naive_wrong > 10_000, "only {naive_wrong} hard cases");
}

/// AC-2: a point set imported with scale 0.001 carries u_quant = 0.001 / sqrt(12), the root
/// from fid-num; REQ-002 for every axis, scale and offset.
#[test]
fn ac2_import_carries_quantisation_metadata() {
    let fixture = cases();
    let u = fixture.iter().filter(|c| c[0] == "U").collect::<Vec<_>>();
    assert_eq!(u.len(), 8);
    for c in &u {
        let (s, want) = (hx(&c[1]), hx(&c[2]));
        let plan = QuantPlan::new([s; 3], [0.0; 3]).unwrap();
        for q in plan.axes() {
            assert_eq!(q.u_quant().to_bits(), want.to_bits(), "scale {s:e}");
            assert_eq!(q.scale().to_bits(), s.to_bits());
        }
    }

    let f6 = PointFormat::new(6).unwrap();
    let mut h = LasHeader::new(4, f6, 0);
    h.scale = [0.001; 3];
    h.offset = [500000.0, 4300000.0, -12.5];
    let mut b = RawPointBatch::with_capacity(f6, &[], 2);
    b.xi.extend([1, -2]);
    b.yi.extend([3, 4]);
    b.zi.extend([-5, 6]);
    let imp = import_batch(&h, &b).unwrap();
    let u001 = hx(&u.iter().find(|c| hx(&c[1]) == 0.001).unwrap()[2]);
    assert_eq!(u001, 0.0002886751345948129);
    assert_eq!(u001, 0.001 / fid_num::f::sqrt(12.0).unwrap());
    for (q, o) in imp.quant.axes().iter().zip(h.offset) {
        assert_eq!((q.scale(), q.offset(), q.u_quant()), (0.001, o, u001));
    }
    assert_eq!(imp.xyz.len(), 2);
    for (p, (x, (y, z))) in imp.xyz.iter().zip(b.xi.iter().zip(b.yi.iter().zip(&b.zi))) {
        let want =
            [reference(0.001, h.offset[0], *x), reference(0.001, h.offset[1], *y), reference(0.001, h.offset[2], *z)];
        assert_eq!(bits3(*p), bits3(want));
    }
    assert_eq!(imp.xyz[0][2], -12.505);

    b.zi.pop();
    assert_eq!(import_batch(&h, &b).unwrap_err(), QuantError::ColumnMismatch { x: 2, y: 2, z: 1 });
}

/// The hostile header's scale and offset are checked per axis against the envelope.
#[test]
fn header_scale_and_offset_are_checked() {
    let mut n = 0;
    for c in cases().iter().filter(|c| c[0] == "H") {
        let s = [hx(&c[2]), hx(&c[3]), hx(&c[4])];
        let o = [hx(&c[5]), hx(&c[6]), hx(&c[7])];
        let h = LasHeader { scale: s, offset: o, ..LasHeader::default() };
        let got = QuantPlan::from_header(&h);
        match c[8].as_str() {
            "ok" => {
                assert!(got.is_ok(), "{}: {got:?}", c[1]);
            }
            code => {
                let e = got.unwrap_err();
                assert_eq!(e.code(), code, "{}", c[1]);
                let a = match e {
                    QuantError::ScaleInvalid { axis, .. } | QuantError::OffsetInvalid { axis, .. } => axis,
                    other => panic!("{}: {other:?}", c[1]),
                };
                assert_eq!(a, axis(&c[9]), "{}", c[1]);
            }
        }
        n += 1;
    }
    assert_eq!(n, 13);
    let mut h = LasHeader { scale: [0.001, -0.0, 0.001], ..LasHeader::default() };
    assert_eq!(
        QuantPlan::from_header(&h).unwrap_err(),
        QuantError::ScaleInvalid { axis: Axis::Y, reason: ValueReason::NotPositive }
    );
    h.scale = [0.001, 1e-200, 0.001];
    assert_eq!(
        QuantPlan::from_header(&h).unwrap_err(),
        QuantError::ScaleInvalid { axis: Axis::Y, reason: ValueReason::OutsideEnvelope }
    );
}

struct ExportCase {
    name: String,
    scale: WriteScale,
    points: Vec<[f64; 3]>,
    lines: Vec<Vec<String>>,
}

fn export_cases() -> Vec<ExportCase> {
    let mut out = Vec::new();
    let mut cur: Option<ExportCase> = None;
    for c in cases() {
        match c[0].as_str() {
            "EXPORT" => {
                let scale = if c[3] == "default" {
                    WriteScale::Default
                } else {
                    WriteScale::PerAxis([hx(&c[3]), hx(&c[4]), hx(&c[5])])
                };
                cur = Some(ExportCase { name: c[1].clone(), scale, points: Vec::new(), lines: Vec::new() });
            }
            "P" => cur.as_mut().unwrap().points.push([hx(&c[1]), hx(&c[2]), hx(&c[3])]),
            "END" => out.push(cur.take().unwrap()),
            _ if cur.is_some() => cur.as_mut().unwrap().lines.push(c),
            _ => {}
        }
    }
    out
}

fn line<'a>(case: &'a ExportCase, tag: &str) -> &'a [String] {
    &case.lines.iter().find(|l| l[0] == tag).unwrap_or_else(|| panic!("{}: no {tag}", case.name))[1..]
}

fn h3(v: &[String]) -> [f64; 3] {
    [hx(&v[0]), hx(&v[1]), hx(&v[2])]
}

/// Export cases: scale, offset, every integer, ties, incurred error and bounds equal the
/// reference; refusals name the reference's error, point and axis (REQ-003, -004, -005).
#[test]
fn export_matches_reference_cases() {
    let all = export_cases();
    assert_eq!(all.len(), 20);
    for case in &all {
        let got = quantise(&case.points, case.scale);
        if let Some(err) = case.lines.iter().find(|l| l[0] == "ERROR") {
            let e = got.unwrap_err();
            assert_eq!(e.code(), err[1], "{}", case.name);
            match e {
                QuantError::ScaleInvalid { axis: a, .. } | QuantError::ScaleTooCoarse { axis: a } => {
                    assert_eq!(a, axis(&err[2]), "{}", case.name)
                }
                QuantError::EmptyPointSet => assert_eq!(err.len(), 2),
                QuantError::NonFinite { index, axis: a }
                | QuantError::CoordinateOutsideEnvelope { index, axis: a }
                | QuantError::OutOfRange { index, axis: a } => {
                    assert_eq!((index, a), (err[2].parse().unwrap(), axis(&err[3])), "{}", case.name)
                }
                other => panic!("{}: {other:?}", case.name),
            }
            continue;
        }
        let q = got.unwrap_or_else(|e| panic!("{}: {e}", case.name));
        let r = &q.report;
        assert_eq!(r.points, case.points.len() as u64);
        assert_eq!(bits3(r.axes().map(|a| a.scale())), bits3(h3(line(case, "SCALE"))), "{} scale", case.name);
        assert_eq!(bits3(r.axes().map(|a| a.offset())), bits3(h3(line(case, "OFFSET"))), "{} offset", case.name);
        let want_x: Vec<[i32; 3]> = case
            .lines
            .iter()
            .filter(|l| l[0] == "X")
            .map(|l| [l[1].parse().unwrap(), l[2].parse().unwrap(), l[3].parse().unwrap()])
            .collect();
        let got_x: Vec<[i32; 3]> = (0..q.xi.len()).map(|i| [q.xi[i], q.yi[i], q.zi[i]]).collect();
        assert_eq!(got_x, want_x, "{} integers", case.name);
        let t = line(case, "TIES");
        assert_eq!(r.ties.to_vec(), t.iter().map(|v| v.parse::<u64>().unwrap()).collect::<Vec<_>>(), "{}", case.name);
        assert_eq!(bits3(r.max_err), bits3(h3(line(case, "MAXERR"))), "{} max_err", case.name);
        assert_eq!(bits3(r.max_read_err), bits3(h3(line(case, "MAXREAD"))), "{} max_read_err", case.name);
        let b = line(case, "BOUNDS");
        assert_eq!(bits3(r.min), bits3(h3(&b[0..3])), "{} min", case.name);
        assert_eq!(bits3(r.max), bits3(h3(&b[3..6])), "{} max", case.name);
        for (a, q) in r.axes().iter().enumerate() {
            assert_eq!(q.u_quant(), q.scale() / fid_num::f::sqrt(12.0).unwrap(), "{} axis {a}", case.name);
            assert!(r.max_err[a] <= q.scale() * 0.5, "{} axis {a}", case.name);
        }
    }
}

fn attrs_for(format: PointFormat, n: usize) -> AttributeColumns {
    let mut b = RawPointBatch::with_capacity(format, &[], n);
    let a = &mut b.attrs;
    for i in 0..n {
        a.intensity.push(i as u16);
        a.return_number.push(1);
        a.number_of_returns.push(1);
        a.scan_direction.push(0);
        a.edge_of_flight_line.push(0);
        a.classification.push(2);
        a.class_flags.push(0);
        a.scanner_channel.as_mut().unwrap().push(0);
        a.scan_angle.as_mut().unwrap().push(0);
        a.user_data.push(0);
        a.point_source_id.push(1);
        a.gps_time.as_mut().unwrap().push(i as f64);
    }
    b.attrs
}

fn header_for(n: usize) -> LasHeader {
    LasHeader { point_count: n as u64, ..LasHeader::default() }
}

fn target(header: &LasHeader, attrs: AttributeColumns) -> ExportTarget<'_> {
    ExportTarget { header, vlrs: &[], vlr_padding: &[], evlrs: &[], attrs }
}

/// AC-3: a 500 m cloud at scale 1e-7 m is `out_of_range{index, axis}` and no byte is
/// written.
#[test]
fn ac3_long_cloud_is_out_of_range_and_writes_nothing() {
    let pts: Vec<[f64; 3]> = (0..11).map(|i| [50.0 * f64::from(i), 0.0, 0.0]).collect();
    let f6 = PointFormat::new(6).unwrap();
    let mut sink: Vec<u8> = Vec::new();
    let h = header_for(pts.len());
    let err =
        write_las(&mut sink, target(&h, attrs_for(f6, pts.len())), &pts, WriteScale::PerAxis([1e-7; 3])).unwrap_err();
    assert_eq!(err, ExportError::Quant(QuantError::OutOfRange { index: 0, axis: Axis::X }));
    assert_eq!(err.code(), "out_of_range");
    assert!(sink.is_empty(), "{} bytes written", sink.len());

    // A later point and axis is named exactly; the same cloud within range writes.
    let mut pts2 = pts.clone();
    for p in &mut pts2 {
        p[0] /= 10.0;
        p[2] = 250.0;
    }
    pts2[7][2] = 500.0;
    pts2[8][2] = 0.0;
    assert_eq!(quantise(&pts2, WriteScale::Default).unwrap_err(), QuantError::OutOfRange { index: 7, axis: Axis::Z });
    pts2[7][2] = 250.0;
    let h2 = header_for(pts2.len());
    let (bytes, report) =
        write_las(Vec::new(), target(&h2, attrs_for(f6, pts2.len())), &pts2, WriteScale::Default).unwrap();
    assert!(!bytes.is_empty());
    assert_eq!(report.points, 11);
}

/// AC-4: a caller scale of 1e-6 m is `scale_too_coarse{axis: x}`; the first coarse axis is
/// named, and nothing is written.
#[test]
fn ac4_coarse_caller_scale_is_refused() {
    let pts = [[0.0, 0.0, 0.0]];
    assert_eq!(
        quantise(&pts, WriteScale::PerAxis([1e-6, 1e-7, 1e-7])).unwrap_err(),
        QuantError::ScaleTooCoarse { axis: Axis::X }
    );
    assert_eq!(
        quantise(&pts, WriteScale::PerAxis([1e-7, 1e-7, 1.0000000000000002e-7])).unwrap_err(),
        QuantError::ScaleTooCoarse { axis: Axis::Z }
    );
    assert_eq!(MAX_WRITE_SCALE, 1e-7);
    let f6 = PointFormat::new(6).unwrap();
    let mut sink: Vec<u8> = Vec::new();
    let h = header_for(1);
    let err = write_las(&mut sink, target(&h, attrs_for(f6, 1)), &pts, WriteScale::PerAxis([1e-6; 3])).unwrap_err();
    assert_eq!(err.code(), "scale_too_coarse");
    assert!(sink.is_empty());
}

/// AC-5: a 10 m cloud exported with defaults reports scale 1e-7 m and a maximum quantisation
/// error (`max_err`, the exact residual rounded once) of at most 5e-8 m per axis; the file
/// reads back so that the recomputed `|c - decoded|` equals `max_read_err`.
#[test]
fn ac5_ten_metre_cloud_default_export() {
    let mut r = Cursor::new(Address { seed: 62, stream: 0 }, 0);
    let n = 100_000;
    let pts: Vec<[f64; 3]> = (0..n)
        .map(|_| [r.uniform_f64() * 10.0, 100.0 + r.uniform_f64() * 10.0, -5.0 + r.uniform_f64() * 10.0])
        .collect();
    let f6 = PointFormat::new(6).unwrap();
    let h = header_for(n);
    let (bytes, report) = write_las(Vec::new(), target(&h, attrs_for(f6, n)), &pts, WriteScale::Default).unwrap();
    for (a, q) in report.axes().iter().enumerate() {
        assert_eq!(q.scale(), DEFAULT_WRITE_SCALE);
        assert_eq!(q.scale(), 1e-7);
        assert!(report.max_err[a] <= q.scale() * 0.5, "axis {a}: {:e}", report.max_err[a]);
        assert!(report.max_err[a] <= 5e-8, "axis {a}: {:e}", report.max_err[a]);
        assert!(report.max_err[a] > 4e-8, "axis {a}: {:e}", report.max_err[a]);
    }

    let file = read_slice(&bytes, ReaderOptions::default()).unwrap();
    assert_eq!(file.header.scale, [1e-7; 3]);
    assert_eq!(file.header.offset, report.axes().map(|q| q.offset()));
    assert_eq!(file.header.min, report.min);
    assert_eq!(file.header.max, report.max);
    let imp = import_batch(&file.header, &file.points).unwrap();
    assert_eq!(imp.quant, report.plan());
    let mut read_err = [0.0f64; 3];
    for (p, d) in pts.iter().zip(&imp.xyz) {
        for ((m, c), v) in read_err.iter_mut().zip(p).zip(d) {
            let e = (c - v).abs();
            *m = if e > *m { e } else { *m };
        }
    }
    assert_eq!(read_err, report.max_read_err);

    // The fixture's 10 m case agrees with the reference exactly (checked in full above).
    let fx = export_cases().into_iter().find(|c| c.name == "ac5_10m_default").unwrap();
    let q = quantise(&fx.points, WriteScale::Default).unwrap();
    assert!(q.report.max_err.iter().all(|e| *e <= 5e-8));
}

/// Review of #675: here the reader's rounding takes `|c - decode(X)|` above 5e-8 m, while the
/// quantisation error `max_err` stays at most scale / 2 (below 5e-8 m); both match the oracle.
#[test]
fn max_err_is_the_quantisation_residual_not_the_read_error() {
    let c = hx("4020cbeacc92146a");
    let pts = [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [c, 0.0, 0.0]];
    let r = quantise(&pts, WriteScale::Default).unwrap().report;
    assert!(r.max_err[0] <= 1e-7 * 0.5 && r.max_err[0] <= 5e-8, "{:e}", r.max_err[0]);
    assert!(r.max_read_err[0] > 5e-8, "{:e}", r.max_read_err[0]);
    let fx = export_cases().into_iter().find(|c| c.name == "read_err_over_half").unwrap();
    assert_eq!(fx.points, pts.to_vec());
    assert_eq!(bits3(r.max_err), bits3(h3(line(&fx, "MAXERR"))));
    assert_eq!(bits3(r.max_read_err), bits3(h3(line(&fx, "MAXREAD"))));
}

/// Review of #675: a header whose declared point count differs from the points is refused
/// before the writer is created, so nothing is written.
#[test]
fn point_count_mismatch_writes_nothing() {
    let pts = [[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]];
    let f6 = PointFormat::new(6).unwrap();
    for declared in [0usize, 1, 3] {
        let h = header_for(declared);
        let mut sink: Vec<u8> = Vec::new();
        let err = write_las(&mut sink, target(&h, attrs_for(f6, 2)), &pts, WriteScale::Default).unwrap_err();
        assert_eq!(err, ExportError::Las(LasError::Malformed { field: LasField::PointCount }), "declared {declared}");
        assert!(sink.is_empty(), "declared {declared}: {} bytes written", sink.len());
    }
    let h = header_for(2);
    let mut sink: Vec<u8> = Vec::new();
    write_las(&mut sink, target(&h, attrs_for(f6, 2)), &pts, WriteScale::Default).unwrap();
    assert!(!sink.is_empty());
}
