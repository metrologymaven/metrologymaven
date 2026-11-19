#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! AC-4 of RPT.MODEL.TMPL.FORMAT: a structure-aware fuzzer (seed 3, 1 000 000 iterations).
//!
//! Each input starts from a valid fixture (the v1 template or the v0 sample), gets one to
//! three tree mutations (replace a node, drop or add a key, grow or shrink an array, nest
//! deeper, graft a subtree from elsewhere) and, one time in three, a byte mutation of its
//! JSON text (flip, cut, splice a structural token or an invalid byte, duplicate a run).
//! The parser must never panic, every failure must be one of the two typed errors with a
//! bounded pointer, and every success must round-trip byte-identically.

use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_report::tmpl::format::*;
use std::collections::BTreeMap;
use std::path::PathBuf;

const ITERATIONS: u32 = 1_000_000;
const SEED: u64 = 3;

/// SplitMix64.
struct Rng(u64);

impl Rng {
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

    fn chance(&mut self, one_in: usize) -> bool {
        self.below(one_in) == 0
    }
}

const KEYS: [&str; 24] = [
    "clearance",
    "locked",
    "media_type",
    "pages",
    "schema_version",
    "template_id",
    "title",
    "blocks",
    "id",
    "continuation_block",
    "kind",
    "kind_version",
    "payload",
    "repeat_header_rows",
    "slot",
    "spec",
    "untrusted",
    "value",
    "text",
    "state",
    "review_ref",
    "schema",
    "a/b~c",
    "\u{202e}x",
];

const STRINGS: [&str; 16] = [
    "",
    "0",
    "1",
    "8",
    "9",
    "01",
    "99",
    "value_binding@1",
    "value_text@1",
    "characteristics",
    "pending",
    "cleared",
    "application/vnd.fid.report-template+json",
    "019a9ec0-6402-7000-8000-000000000003",
    "019a9ec0-6403-7000-8000-000000000004",
    "\u{0}\u{202e}\u{1F4CF}",
];

fn scalar(r: &mut Rng) -> CValue {
    match r.below(7) {
        0 => CValue::Null,
        1 => CValue::Bool(r.chance(2)),
        2 => CValue::Num((r.below(20) as f64) - 5.0),
        3 => CValue::Arr(Vec::new()),
        4 => CValue::Obj(BTreeMap::new()),
        _ => CValue::Str(STRINGS[r.below(STRINGS.len())].to_string()),
    }
}

/// A random node: a walk from the root that stops with probability 1/4 per level.
fn pick<'a>(mut v: &'a mut CValue, r: &mut Rng) -> &'a mut CValue {
    loop {
        let leaf = match &*v {
            CValue::Arr(a) => a.is_empty(),
            CValue::Obj(o) => o.is_empty(),
            _ => true,
        };
        if leaf || r.chance(4) {
            return v;
        }
        let i = r.next() as usize;
        v = match v {
            CValue::Arr(a) => {
                let n = a.len();
                &mut a[i % n]
            }
            CValue::Obj(o) => {
                let n = o.len();
                o.values_mut().nth(i % n).unwrap()
            }
            _ => unreachable!(),
        };
    }
}

fn some_subtree(v: &CValue, r: &mut Rng) -> CValue {
    let mut copy = v.clone();
    pick(&mut copy, r).clone()
}

fn mutate_tree(root: &mut CValue, r: &mut Rng) {
    let graft = some_subtree(root, r);
    let node = pick(root, r);
    match r.below(8) {
        0 | 1 => *node = scalar(r),
        2 => {
            if let CValue::Obj(o) = node {
                if !o.is_empty() {
                    let k = o.keys().nth(r.below(o.len())).cloned().unwrap();
                    o.remove(&k);
                }
            }
        }
        3 => {
            if let CValue::Obj(o) = node {
                o.insert(KEYS[r.below(KEYS.len())].to_string(), scalar(r));
            }
        }
        4 => {
            if let CValue::Arr(a) = node {
                if !a.is_empty() {
                    let i = r.below(a.len());
                    if r.chance(2) {
                        let dup = a[i].clone();
                        a.push(dup);
                    } else {
                        a.remove(i);
                    }
                }
            }
        }
        5 => {
            let mut inner = std::mem::replace(node, CValue::Null);
            for _ in 0..r.below(40) {
                let mut o = BTreeMap::new();
                o.insert("a".to_string(), inner);
                inner = CValue::Obj(o);
            }
            *node = inner;
        }
        _ => *node = graft,
    }
}

const SPLICES: [&[u8]; 12] =
    [b"{", b"}", b"[", b"]", b"\"", b"\\", b",", b":", b"\xff", b"\\u", b"\\ud800", b"\"untrusted\":true,"];

fn mutate_bytes(b: &mut Vec<u8>, r: &mut Rng) {
    if b.is_empty() {
        return;
    }
    let at = r.below(b.len());
    match r.below(4) {
        0 => b[at] ^= 1 << r.below(8),
        1 => {
            let end = (at + r.below(16)).min(b.len());
            b.drain(at..end);
        }
        2 => {
            let s = SPLICES[r.below(SPLICES.len())];
            b.splice(at..at, s.iter().copied());
        }
        _ => {
            let end = (at + r.below(64)).min(b.len());
            let run: Vec<u8> = b[at..end].to_vec();
            b.splice(end..end, run);
        }
    }
}

fn seeds() -> Vec<CValue> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/rpt/model/tmpl");
    ["valid_v1.canonical.json", "v0_sample.json"]
        .iter()
        .map(|f| parse(&std::fs::read(dir.join(f)).unwrap()).unwrap())
        .collect()
}

#[test]
fn ac4_structure_aware_fuzz_never_panics_and_fails_typed() {
    let seeds = seeds();
    // The iterations are split over SHARDS fixed SplitMix64 streams (stream k starts at
    // SEED + k), each run on its own thread; the inputs do not depend on the thread count.
    let per = ITERATIONS / SHARDS;
    let totals: Vec<[u32; 3]> = std::thread::scope(|sc| {
        let handles: Vec<_> = (0..SHARDS)
            .map(|k| {
                let seeds = &seeds;
                sc.spawn(move || run_shard(seeds, SEED + u64::from(k), per))
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let [ok, invalid, unsupported] = totals.iter().fold([0u32; 3], |a, t| [a[0] + t[0], a[1] + t[1], a[2] + t[2]]);
    println!("fuzz seed {SEED}: {ITERATIONS} iterations, {ok} ok, {invalid} invalid, {unsupported} unsupported");
    assert_eq!(ok + invalid + unsupported, ITERATIONS);
    // The generator reaches all three outcomes.
    assert!(ok > 1000 && invalid > 1000 && unsupported > 0);
}

const SHARDS: u32 = 8;

fn run_shard(seeds: &[CValue], seed: u64, n: u32) -> [u32; 3] {
    let mut r = Rng(seed);
    let (mut ok, mut invalid, mut unsupported) = (0u32, 0u32, 0u32);
    for _ in 0..n {
        let mut v = seeds[r.below(seeds.len())].clone();
        for _ in 0..1 + r.below(3) {
            mutate_tree(&mut v, &mut r);
        }
        let mut bytes = to_canonical_bytes(&v).unwrap();
        if r.chance(3) {
            mutate_bytes(&mut bytes, &mut r);
        }
        match parse_template(&bytes) {
            Ok(t) => {
                let s = serialise(&t).unwrap();
                let t2 = parse_template(s.as_bytes()).unwrap();
                assert_eq!(t2, t);
                assert_eq!(serialise(&t2).unwrap(), s);
                ok += 1;
            }
            Err(TemplateError::Invalid { pointer, reason }) => {
                assert!(pointer.len() <= MAX_ERROR_POINTER_BYTES);
                assert!(!reason.is_empty());
                invalid += 1;
            }
            Err(TemplateError::SchemaUnsupported { version }) => {
                assert!(version > SCHEMA_VERSION);
                unsupported += 1;
            }
        }
    }
    [ok, invalid, unsupported]
}
