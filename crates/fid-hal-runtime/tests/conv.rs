//! Acceptance criteria of spec node HAL.ACQ.CONV (AC-1 .. AC-6). Expected values come from
//! the independent rational-arithmetic reference `fixtures/hal/acq/conv/make_fixtures.py`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_hal_idl::{EncodingDescriptor, NumericType, RawValue, SiUnit};
use fid_hal_runtime::conv::{decode, ConvError, Decoded, RawArray, ValidityMask};
use fid_testkit::Sha256Hasher;

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/hal/acq/conv/");

fn read(name: &str) -> Vec<u8> {
    std::fs::read(format!("{FIX}{name}")).unwrap()
}

fn golden(name: &str) -> String {
    String::from_utf8(read(name)).unwrap().trim().to_string()
}

/// SHA-256 of the valid values, f64 little-endian, in index order (masked samples skipped).
fn digest(d: &Decoded) -> String {
    let mut h = Sha256Hasher::new();
    for (_, v) in d.valid() {
        h.update(&v.to_le_bytes());
    }
    h.finish()
}

fn desc(t: NumericType, scale: f64, offset: f64) -> EncodingDescriptor {
    EncodingDescriptor::new(t, scale, offset, SiUnit::Metre).unwrap()
}

fn i32_profile() -> Vec<i32> {
    read("i32_profile.bin").chunks_exact(4).map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

/// AC-1: the i32 profile (scale 1e-6 m, offset 0.05 m) decodes to the golden digest; this
/// test runs on the Linux, Windows and macOS CI runners.
#[test]
fn ac1_i32_profile_golden_digest() {
    let raw = i32_profile();
    assert_eq!(raw.len(), 2000);
    let d = decode(RawArray::I32(&raw), &desc(NumericType::I32, 1e-6, 0.05), &ValidityMask::all_valid(2000)).unwrap();
    assert_eq!(d.unit(), SiUnit::Metre);
    assert_eq!(d.mask(), &ValidityMask::all_valid(2000));
    assert_eq!(digest(&d), golden("i32_profile.f64.sha256"));
}

/// AC-2: masked NaN and infinity entries are skipped and the result matches the golden digest.
#[test]
fn ac2_f32_mixed_masked_nans_skipped() {
    let raw: Vec<f32> =
        read("f32_mixed.bin").chunks_exact(4).map(|c| f32::from_bits(u32::from_le_bytes([c[0], c[1], c[2], c[3]]))).collect();
    let bits: Vec<bool> = read("f32_mixed.mask").iter().map(|b| *b == 1).collect();
    assert_eq!(raw.len(), 2000);
    assert_eq!(bits.len(), 2000);
    let masked_non_finite = raw.iter().zip(&bits).filter(|(v, m)| !**m && !v.is_finite()).count();
    assert!(masked_non_finite > 50, "fixture has {masked_non_finite} masked non-finite values");
    let mask = ValidityMask::from_bools(&bits);
    let d = decode(RawArray::F32(&raw), &desc(NumericType::F32, 1e-3, -0.125), &mask).unwrap();
    assert_eq!(d.mask(), &mask);
    assert_eq!(d.values().len(), 2000);
    for (i, &m) in bits.iter().enumerate() {
        let v = d.values()[i];
        assert!(v.is_finite(), "index {i} is not finite");
        if !m {
            assert_eq!(v.to_bits(), 0, "masked index {i} holds {v}");
            assert_eq!(d.get(i), None);
        }
    }
    assert_eq!(digest(&d), golden("f32_mixed.f64.sha256"));
}

/// AC-3: an i64 raw value of 2^53 + 1 fails with `encoding_precision_loss` at its index.
#[test]
fn ac3_i64_above_2_pow_53_is_precision_loss() {
    let two53: i64 = 1 << 53;
    let raw = [0, two53, -two53, two53 + 1, 4];
    let e = decode(RawArray::I64(&raw), &desc(NumericType::I64, 1e-9, 0.0), &ValidityMask::all_valid(5)).unwrap_err();
    assert_eq!(e, ConvError::PrecisionLoss { index: 3 });
    assert_eq!(e.code(), "encoding_precision_loss");
    assert_eq!(e.index(), Some(3));
}

/// AC-4: an unmasked f32 infinity fails with `encoding_value_not_finite`.
#[test]
fn ac4_unmasked_f32_infinity_is_not_finite() {
    let raw = [1.0f32, f32::INFINITY, 2.0];
    let e = decode(RawArray::F32(&raw), &desc(NumericType::F32, 1e-3, 0.0), &ValidityMask::all_valid(3)).unwrap_err();
    assert_eq!(e, ConvError::ValueNotFinite { index: 1 });
    assert_eq!(e.code(), "encoding_value_not_finite");
    // The same value masked is skipped.
    let ok = decode(RawArray::F32(&raw), &desc(NumericType::F32, 1e-3, 0.0), &ValidityMask::from_bools(&[true, false, true]));
    assert!(ok.is_ok());
}

/// AC-5: 2000 raw values with a 1999-bit mask fail with `encoding_length_mismatch`.
#[test]
fn ac5_length_mismatch() {
    let raw = vec![7i32; 2000];
    let e = decode(RawArray::I32(&raw), &desc(NumericType::I32, 1e-6, 0.0), &ValidityMask::all_valid(1999)).unwrap_err();
    assert_eq!(e, ConvError::LengthMismatch { raw: 2000, mask: 1999 });
    assert_eq!(e.code(), "encoding_length_mismatch");
}

/// AC-6: 10 000 profiles of 2 000 i32 points. The 20 M points/s floor is [unverified until
/// measured] on reference hardware, so the timed run is opt-in:
/// `cargo test --release -p fid-hal-runtime -- --ignored ac6`.
#[test]
#[ignore = "throughput on reference hardware; run with --release -- --ignored"]
fn ac6_throughput() {
    let profile = i32_profile();
    let d = desc(NumericType::I32, 1e-6, 0.05);
    let mask = ValidityMask::all_valid(profile.len());
    let start = std::time::Instant::now();
    let mut acc = 0u64;
    for _ in 0..10_000 {
        let out = decode(RawArray::I32(&profile), &d, &mask).unwrap();
        acc = acc.wrapping_add(out.values()[1999].to_bits());
    }
    let secs = start.elapsed().as_secs_f64();
    let rate = 2.0e7 / secs;
    println!("AC-6: {rate:.3e} points/s ({secs:.3} s, check {acc:x})");
    assert!(rate >= 2.0e7, "{rate:.3e} points/s is below 20 M points/s");
}

// ------------------------------------------------------------------ element cases

fn hex64(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s, 16).unwrap())
}

fn list(s: &str) -> Vec<&str> {
    if s == "." {
        Vec::new()
    } else {
        s.split(',').collect()
    }
}

/// Runs one case from `cases-v1.txt` and returns the result.
fn run_case(kind: &str, d: &EncodingDescriptor, raws: &[&str], mask: &ValidityMask) -> Result<Decoded, ConvError> {
    match kind {
        "i16" => decode(RawArray::I16(&raws.iter().map(|r| r.parse().unwrap()).collect::<Vec<_>>()), d, mask),
        "i32" => decode(RawArray::I32(&raws.iter().map(|r| r.parse().unwrap()).collect::<Vec<_>>()), d, mask),
        "i64" => decode(RawArray::I64(&raws.iter().map(|r| r.parse().unwrap()).collect::<Vec<_>>()), d, mask),
        "u16" => decode(RawArray::U16(&raws.iter().map(|r| r.parse().unwrap()).collect::<Vec<_>>()), d, mask),
        "u32" => decode(RawArray::U32(&raws.iter().map(|r| r.parse().unwrap()).collect::<Vec<_>>()), d, mask),
        "f32" => decode(
            RawArray::F32(&raws.iter().map(|r| f32::from_bits(u32::from_str_radix(r, 16).unwrap())).collect::<Vec<_>>()),
            d,
            mask,
        ),
        "f64" => decode(RawArray::F64(&raws.iter().map(|r| hex64(r)).collect::<Vec<_>>()), d, mask),
        other => panic!("unknown type {other}"),
    }
}

/// Every element case of the reference (all numeric types, signed zeros, subnormals,
/// overflow, every error) matches bit for bit, including the cases that tell a fused
/// multiply-add from two rounded operations (REQ-001).
#[test]
fn reference_element_cases() {
    let text = String::from_utf8(read("cases-v1.txt")).unwrap();
    let mut n = 0;
    let mut no_fma = 0;
    for line in text.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        let (lhs, rhs) = line.split_once(" -> ").unwrap();
        let f: Vec<&str> = lhs.split_whitespace().collect();
        let [name, kind, scale, offset, raws, mask] = f[..] else { panic!("bad line {line}") };
        let t = NumericType::parse(kind).unwrap();
        let d = desc(t, hex64(scale), hex64(offset));
        let mask_bits: Vec<bool> = if mask == "." { Vec::new() } else { mask.chars().map(|c| c == '1').collect() };
        let m = ValidityMask::from_bools(&mask_bits);
        let raws = list(raws);
        let got = run_case(kind, &d, &raws, &m);
        let want: Vec<&str> = rhs.split_whitespace().collect();
        match (want[0], got) {
            ("ok", Ok(dec)) => {
                let outs = list(want[1]);
                assert_eq!(outs.len(), dec.values().len(), "{name}");
                assert_eq!(dec.mask(), &m, "{name}");
                for (i, o) in outs.iter().enumerate() {
                    if *o == "-" {
                        assert_eq!(dec.get(i), None, "{name} index {i}");
                    } else {
                        let v = dec.get(i).unwrap();
                        assert_eq!(format!("{:016x}", v.to_bits()), *o, "{name} index {i}");
                    }
                }
            }
            ("err", Err(e)) => {
                assert_eq!(e.code(), want[1], "{name}");
                assert_eq!(e.index(), Some(want[2].parse().unwrap()), "{name}");
            }
            (w, g) => panic!("{name}: expected {w}, got {g:?}"),
        }
        if name.starts_with("no_fma") {
            no_fma += 1;
        }
        n += 1;
    }
    assert!(n >= 30, "fixture too small: {n}");
    assert_eq!(no_fma, 2);
}

/// The whole-array converter agrees with the per-value reference definition of
/// HAL.IDL.UNITS (`EncodingDescriptor::decode`) on the i32 profile.
#[test]
fn agrees_with_hal_idl_reference_decode() {
    let raw = i32_profile();
    let d = desc(NumericType::I32, 1e-6, 0.05);
    let out = decode(RawArray::I32(&raw), &d, &ValidityMask::all_valid(raw.len())).unwrap();
    for (i, &r) in raw.iter().enumerate() {
        let q = d.decode(RawValue::I32(r)).unwrap();
        assert_eq!(q.value().to_bits(), out.values()[i].to_bits(), "index {i}");
    }
}

/// Masked out-of-range integers are skipped, not reported (REQ-004 over REQ-002).
#[test]
fn masked_precision_loss_is_skipped() {
    let raw = [i64::MIN, 5, i64::MAX];
    let d = decode(RawArray::I64(&raw), &desc(NumericType::I64, 2.0, 1.0), &ValidityMask::from_bools(&[false, true, false]))
        .unwrap();
    assert_eq!(d.valid().collect::<Vec<_>>(), vec![(1, 11.0)]);
}
