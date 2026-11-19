//! TRUST.PKG.READER AC-4, in-tree part: a deterministic, bounded mutation fuzzer over every
//! fixture package (valid and malformed). Each mutated input must end in `Ok` or a typed
//! error with a known code, without panicking and within 5 s. The cargo-fuzz target that CI
//! runs for 10 minutes is `crates/fid-pkg/fuzz/fuzz_targets/reader.rs`.
//!
//! `PKG_READER_FUZZ_ITERS` sets the mutations per seed (default 150).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_pkg::reader::{inspect, PackageReader, LIMIT_REASONS, MALFORMED_REASONS};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/pkg");
const CODES: [&str; 6] =
    ["pkg.digest_mismatch", "pkg.io", "pkg.limit", "pkg.malformed", "pkg.unlisted_member", "pkg.unsupported_version"];
/// Seeds above this size (the zip bombs) are run once each but not mutated: a mutated bomb
/// may legitimately inflate up to 1000 times its size, which is slow in a debug build.
const MAX_MUTATED_SEED: usize = 16 * 1024;

fn seeds() -> Vec<(String, Vec<u8>)> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "zip") {
                out.push(p);
            }
        }
    }
    let mut paths = Vec::new();
    for sub in ["malformed", "reader/valid", "golden"] {
        walk(&Path::new(FIX).join(sub), &mut paths);
    }
    paths.sort();
    paths.into_iter().map(|p| (p.display().to_string(), std::fs::read(&p).unwrap())).collect()
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
}

const INTERESTING: [u8; 8] = [0x00, 0x01, 0x7f, 0x80, 0xff, 0x08, 0x2f, 0x5c];

fn mutate(rng: &mut Rng, seed: &[u8]) -> Vec<u8> {
    let mut v = seed.to_vec();
    for _ in 0..1 + rng.below(4) {
        let len = v.len();
        match rng.below(8) {
            0 if len > 0 => {
                let i = rng.below(len);
                v[i] ^= 1 << rng.below(8);
            }
            1 if len > 0 => {
                let i = rng.below(len);
                v[i] = INTERESTING[rng.below(INTERESTING.len())];
            }
            2 if len > 4 => {
                // A 16- or 32-bit field set to an edge value.
                let i = rng.below(len - 4);
                let val: u32 = [0, 1, 0xFFFF, 0xFFFF_FFFF, 0x7FFF_FFFF][rng.below(5)];
                let w = if rng.below(2) == 0 { 2 } else { 4 };
                v[i..i + w].copy_from_slice(&val.to_le_bytes()[..w]);
            }
            3 => v.truncate(rng.below(len + 1)),
            4 if len > 0 => {
                let i = rng.below(len);
                let n = 1 + rng.below(64.min(len - i));
                v.drain(i..i + n);
            }
            5 if len > 0 => {
                let i = rng.below(len);
                let n = 1 + rng.below(64.min(len - i));
                let chunk = v[i..i + n].to_vec();
                let at = rng.below(len);
                v.splice(at..at, chunk);
            }
            6 => {
                let at = rng.below(len + 1);
                let junk: Vec<u8> = (0..1 + rng.below(32)).map(|_| rng.next() as u8).collect();
                v.splice(at..at, junk);
            }
            _ if len > 8 => {
                // Swap two regions' first bytes (shuffles header fields).
                let (a, b) = (rng.below(len), rng.below(len));
                v.swap(a, b);
            }
            _ => {}
        }
    }
    v
}

fn run_one(name: &str, input: &[u8]) {
    let started = Instant::now();
    let outcome = PackageReader::open(Cursor::new(input)).and_then(|mut r| {
        let _ = r.member_results();
        r.verify_all()
    });
    if let Err(e) = &outcome {
        assert!(CODES.contains(&e.code()), "{name}: unknown code {}", e.code());
        if e.code() == "pkg.limit" {
            assert!(LIMIT_REASONS.contains(&e.reason()), "{name}: {e}");
        }
        if e.code() == "pkg.malformed" {
            assert!(MALFORMED_REASONS.contains(&e.reason()), "{name}: {e}");
        }
    }
    let _ = inspect(Cursor::new(input));
    assert!(started.elapsed() < Duration::from_secs(5), "{name}: took {:?}", started.elapsed());
}

#[test]
fn ac4_mutated_fixtures_never_panic_or_hang() {
    let iters: usize = std::env::var("PKG_READER_FUZZ_ITERS").ok().and_then(|s| s.parse().ok()).unwrap_or(150);
    let seeds = seeds();
    assert!(seeds.len() >= 85, "{} seeds", seeds.len());
    let mut rng = Rng(0x2545_F491_4F6C_DD1D);
    let mut runs = 0;
    for (name, seed) in &seeds {
        run_one(name, seed);
        if seed.len() > MAX_MUTATED_SEED {
            continue;
        }
        for i in 0..iters {
            let input = mutate(&mut rng, seed);
            run_one(&format!("{name} #{i}"), &input);
            runs += 1;
        }
    }
    assert!(runs >= 80 * iters, "{runs}");
}

#[test]
fn arbitrary_short_inputs_are_typed_errors() {
    let mut rng = Rng(7);
    for n in 0..300 {
        let input: Vec<u8> = (0..rng.below(200)).map(|_| rng.next() as u8).collect();
        run_one(&format!("random #{n}"), &input);
    }
    for input in [
        &b""[..],
        b"PK",
        b"PK\x05\x06",
        &[0x50, 0x4b, 0x05, 0x06, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    ] {
        run_one("short", input);
    }
}
