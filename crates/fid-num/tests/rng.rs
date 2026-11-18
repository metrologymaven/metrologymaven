//! Acceptance criteria of spec node KRN.NUM.RNG (AC-1 .. AC-5).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_num::rng::{philox4x32_10, shuffle, Address, Cursor};
use fid_num::NumError;
use fid_testkit::Sha256Hasher;

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/num/");

fn hex32(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap()
}

/// AC-1: known-answer vectors, including the three published Random123 vectors.
#[test]
fn ac1_philox_known_answers() {
    let text = std::fs::read_to_string(format!("{FIX}rng-philox-kat-v1.txt")).unwrap();
    let mut n = 0;
    for line in text.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        let (input, output) = line.split_once("->").unwrap();
        let i: Vec<u32> = input.split_whitespace().map(hex32).collect();
        let o: Vec<u32> = output.split_whitespace().map(hex32).collect();
        let got = philox4x32_10([i[0], i[1], i[2], i[3]], [i[4], i[5]]);
        assert_eq!(got.to_vec(), o, "{line}");
        n += 1;
    }
    assert!(n >= 13, "fixture too small: {n}");
}

/// AC-2: the golden digest of 10^6 uniform draws (same on every platform CI runs).
#[test]
fn ac2_stream_golden_digest() {
    let text = std::fs::read_to_string(format!("{FIX}rng-stream-v1.txt")).unwrap();
    let field = |k: &str| -> String {
        text.lines().find_map(|l| l.strip_prefix(&format!("{k} "))).map(str::to_string).unwrap()
    };
    let mut cur = Cursor::new(Address { seed: 0x0123_4567_89AB_CDEF, stream: 7 }, 0);
    let mut h = Sha256Hasher::new();
    for i in 0..1_000_000u64 {
        let x = cur.uniform_f64();
        assert!((0.0..1.0).contains(&x));
        if i < 3 {
            assert_eq!(format!("{:016x}", x.to_bits()), field(&format!("first{i}")), "draw {i}");
        }
        h.update(&x.to_le_bytes());
    }
    assert_eq!(h.finish(), field("digest"));
}

/// AC-3: same address, same permutation; another stream, another permutation.
#[test]
fn ac3_shuffle_reproducible() {
    let run = |stream: u64| {
        let mut v: Vec<u32> = (0..1000).collect();
        let mut cur = Cursor::new(Address { seed: 99, stream }, 0);
        shuffle(&mut v, &mut cur).unwrap();
        v
    };
    let a = run(7);
    assert_eq!(a, run(7));
    assert_ne!(a, run(8));
    let mut sorted = a.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, (0..1000).collect::<Vec<u32>>(), "must be a permutation");
}

/// AC-4: below(0) is a domain error.
#[test]
fn ac4_below_zero() {
    let mut cur = Cursor::new(Address { seed: 1, stream: 1 }, 0);
    assert!(matches!(cur.below(0), Err(NumError::Domain { op: "rng.below", .. })));
}

/// AC-5: 10^6 draws of below(10) pass a chi-square uniformity test at alpha = 0.001.
#[test]
fn ac5_below_uniform() {
    let mut cur = Cursor::new(Address { seed: 2026, stream: 3 }, 0);
    let mut counts = [0u64; 10];
    let n = 1_000_000u64;
    for _ in 0..n {
        counts[cur.below(10).unwrap() as usize] += 1;
    }
    let expected = n as f64 / 10.0;
    let chi2: f64 = counts.iter().map(|&c| (c as f64 - expected).powi(2) / expected).sum();
    // Critical value of chi-square with 9 degrees of freedom at alpha = 0.001.
    assert!(chi2 < 27.877, "chi2 = {chi2}, counts = {counts:?}");
}

/// No hidden state: a draw depends only on (seed, stream, index).
#[test]
fn draws_are_addressable() {
    let addr = Address { seed: 5, stream: 6 };
    let mut a = Cursor::new(addr, 0);
    for _ in 0..41 {
        a.next_u64();
    }
    let mut b = Cursor::new(addr, 41);
    assert_eq!(a.next_u64(), b.next_u64());
}
