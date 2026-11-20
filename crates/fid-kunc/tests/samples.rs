//! Acceptance criteria of spec node UNC.PROP.MCM.SAMP (AC-1 .. AC-4) plus requirement checks.
//!
//! The pinned digests (`linear2.sha256`, `fail_fit.sha256`), header lengths, sizes and the
//! AC-2 would-be size come from the independent Python reference
//! `fixtures/unc/mcm/samp/make_fixtures.py` (its own Philox4x32-10, AS241 with a Python port
//! of the published fdlibm logarithm cross-checked against a 60-digit decimal logarithm, its
//! own byte layout), never from this crate. The blocks are computed by `run_block` on the
//! models, manifest and test kernel of the UNC.PROP.MCM.BLOCK fixtures. AC-3 and the
//! decoder and encoder checks are hand-labelled from the spec's rules.
//!
//! The pinned digests are checked on every CI target (Linux, Windows, macOS, wasm32-wasip1),
//! so one pin per output holds the bit identity across targets.
//!
//! AC-H1 (retention rate and recomputed conformance probabilities) needs UNC.DEC,
//! UNC.ORS.UNC.BASE and UNC.PROP.MCM.CMD, which are not built; it is not measured here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_kci::kci::sha256_digest;
use fid_kci::{
    parse_card, AlgorithmFailure, AlgorithmId, AlgorithmTable, CardRegistry, InputArity, KernelAlgorithm, KernelError,
    KernelManifest,
};
use fid_kunc::model::{validate, UncertaintyModel, ValidModel};
use fid_kunc::prop::mcm::block::{run_block, BlockModel, BlockOut, BlockSpec, SamplingPlan, TrialFailure};
use fid_kunc::prop::mcm::samples::{
    decode, digest, encode, Encoded, SampleSet, SampleSetHeader, DEFAULT_CAP, FORMAT, MAX_HEADER_BYTES,
};
use fid_kunc::prop::mcm::McmError;
use fid_types::Digest;

const BLOCK: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/unc/mcm/block/");
const SAMP: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/unc/mcm/samp/");

fn read(dir: &str, name: &str) -> Vec<u8> {
    std::fs::read(format!("{dir}{name}")).unwrap_or_else(|e| panic!("{name}: {e}"))
}

// ------------------------------------------------------------------ a small JSON reader

/// The reference files hold only objects, arrays and strings without escapes.
#[derive(Debug, Clone)]
enum J {
    S(String),
    A(Vec<J>),
    O(Vec<(String, J)>),
}

impl J {
    fn get(&self, key: &str) -> &J {
        match self {
            J::O(m) => &m.iter().find(|(k, _)| k == key).unwrap_or_else(|| panic!("missing {key}")).1,
            _ => panic!("not an object at {key}"),
        }
    }
    fn s(&self) -> &str {
        match self {
            J::S(s) => s,
            _ => panic!("not a string"),
        }
    }
    fn a(&self) -> &[J] {
        match self {
            J::A(v) => v,
            _ => panic!("not an array"),
        }
    }
    fn u(&self) -> u64 {
        self.s().parse().unwrap()
    }
    fn d(&self) -> Digest {
        Digest::parse(self.s()).unwrap()
    }
}

fn parse_json(b: &[u8]) -> J {
    fn ws(b: &[u8], i: &mut usize) {
        while *i < b.len() && b[*i].is_ascii_whitespace() {
            *i += 1;
        }
    }
    fn string(b: &[u8], i: &mut usize) -> String {
        assert_eq!(b[*i], b'"');
        let start = *i + 1;
        let end = start + b[start..].iter().position(|&c| c == b'"').unwrap();
        assert!(!b[start..end].contains(&b'\\'));
        *i = end + 1;
        String::from_utf8(b[start..end].to_vec()).unwrap()
    }
    fn value(b: &[u8], i: &mut usize) -> J {
        ws(b, i);
        match b[*i] {
            b'"' => J::S(string(b, i)),
            b'[' => {
                *i += 1;
                let mut v = Vec::new();
                loop {
                    ws(b, i);
                    if b[*i] == b']' {
                        *i += 1;
                        return J::A(v);
                    }
                    v.push(value(b, i));
                    ws(b, i);
                    if b[*i] == b',' {
                        *i += 1;
                    }
                }
            }
            b'{' => {
                *i += 1;
                let mut m = Vec::new();
                loop {
                    ws(b, i);
                    if b[*i] == b'}' {
                        *i += 1;
                        return J::O(m);
                    }
                    let k = string(b, i);
                    ws(b, i);
                    assert_eq!(b[*i], b':');
                    *i += 1;
                    m.push((k, value(b, i)));
                    ws(b, i);
                    if b[*i] == b',' {
                        *i += 1;
                    }
                }
            }
            c => panic!("unexpected byte {c}"),
        }
    }
    let mut i = 0;
    value(b, &mut i)
}

fn expected() -> J {
    parse_json(&read(SAMP, "expected.json"))
}

fn pinned(name: &str) -> String {
    String::from_utf8(read(SAMP, name)).unwrap().trim_end().to_owned()
}

// ------------------------------------------------------------------ the test kernel

const SUM: &str = "test.mcm.sum@1.0.0";
const CIRCLE: &str = "test.mcm.circle_fit@1.0.0";

#[derive(Clone, Copy)]
enum Op {
    Sum,
    Circle,
}

struct Alg {
    id: &'static str,
    op: Op,
    arity: InputArity,
}

fn doubles(blob: &[u8]) -> Vec<f64> {
    blob.chunks_exact(8).map(|c| f64::from_le_bytes(c.try_into().unwrap())).collect()
}

/// cos and sin of k pi / 4, k = 0..7 (exact table; S = sqrt(1/2) rounded once).
const S: f64 = std::f64::consts::FRAC_1_SQRT_2;
const COS8: [f64; 8] = [1.0, S, 0.0, -S, -1.0, -S, 0.0, S];
const SIN8: [f64; 8] = [0.0, S, 1.0, S, 0.0, -S, -1.0, -S];

/// The circle fit of the UNC.PROP.MCM.BLOCK test kernel (crates/fid-kunc/tests/block.rs):
/// centroid and RMS radius of 8 ring points, doubled; a negative radius fails.
fn circle_fit(r: f64, offset: f64) -> Result<f64, KernelError> {
    if r < 0.0 {
        return Err(KernelError::Algorithm(AlgorithmFailure { code: "negative_radius" }));
    }
    let xs: Vec<f64> = COS8.iter().map(|c| offset + r * c).collect();
    let ys: Vec<f64> = SIN8.iter().map(|s| r * s).collect();
    let cx = xs.iter().fold(0.0, |a, x| a + x) / 8.0;
    let cy = ys.iter().fold(0.0, |a, y| a + y) / 8.0;
    let ss = xs.iter().zip(&ys).fold(0.0, |a, (x, y)| a + ((x - cx) * (x - cx) + (y - cy) * (y - cy)));
    Ok(2.0 * fid_num::f::sqrt(ss / 8.0).unwrap())
}

impl KernelAlgorithm for Alg {
    fn id(&self) -> AlgorithmId {
        AlgorithmId::parse(self.id).unwrap()
    }
    fn input_arity(&self) -> InputArity {
        self.arity
    }
    fn validate(&self, _params: &[u8]) -> Result<(), KernelError> {
        Ok(())
    }
    fn run(&self, _params: &[u8], _seed: Option<u64>, inputs: &[&[u8]]) -> Result<Vec<u8>, KernelError> {
        let vals: Vec<Vec<f64>> = inputs.iter().map(|b| doubles(b)).collect();
        let y = match self.op {
            Op::Sum => vals[0].iter().skip(1).fold(vals[0][0], |a, x| a + x),
            Op::Circle => circle_fit(vals[0][0], vals[1][0])?,
        };
        Ok(y.to_le_bytes().to_vec())
    }
}

fn card(id: &str, arity: &str) -> String {
    [
        format!("id = \"{id}\""),
        "title = \"Test operator of the UNC.PROP.MCM.SAMP tests\"".to_string(),
        "method = \"Plain binary64 arithmetic on little-endian doubles.\"".to_string(),
        "applicability = \"Tests of the Monte Carlo sample sets only.\"".to_string(),
        "references = [\"Spec node UNC.PROP.MCM.SAMP.\"]".to_string(),
        "determinism = \"D0\"".to_string(),
        "failure_modes = [\"None beyond the test cases.\"]".to_string(),
        "validation = \"crates/fid-kunc/tests/samples.rs\"".to_string(),
        format!("input_arity = \"{arity}\""),
    ]
    .join("\n")
}

/// The test kernel with the core digest of the BLOCK fixtures compiled in.
fn kernel() -> AlgorithmTable {
    let e = parse_json(&read(BLOCK, "expected.json"));
    let mut algs: Vec<Box<dyn KernelAlgorithm>> = Vec::new();
    let mut cards = Vec::new();
    for (id, op, arity) in [(SUM, Op::Sum, "1"), (CIRCLE, Op::Circle, "2")] {
        algs.push(Box::new(Alg { id, op, arity: InputArity::parse(arity).unwrap() }));
        cards.push(parse_card(&card(id, arity)).unwrap());
    }
    AlgorithmTable::new(algs, CardRegistry::new(cards).unwrap())
        .unwrap()
        .with_manifest_core_digest(e.get("core_digest").d())
}

fn manifest() -> KernelManifest {
    KernelManifest::parse_canonical(&read(BLOCK, "manifest.json")).unwrap()
}

fn model(name: &str) -> ValidModel {
    let m = UncertaintyModel::parse(&read(BLOCK, name)).unwrap();
    validate(&m, &manifest()).unwrap_or_else(|e| panic!("{:?}", e.iter().map(ToString::to_string).collect::<Vec<_>>()))
}

/// Blocks of `name` (seed 7) computed in `order`, returned in that order.
fn blocks(name: &str, size: u64, order: &[u64]) -> Vec<BlockOut> {
    let v = model(name);
    let table = kernel();
    let man = manifest();
    let bm = BlockModel::prepare(&v, &man, &table).unwrap();
    let plan = SamplingPlan::new(&v).unwrap();
    order
        .iter()
        .map(|&b| {
            let spec = BlockSpec {
                model_digest: v.model().digest(),
                inputs_digest: plan.digest(),
                seed: 7,
                block_index: b,
                block_size: size,
            };
            run_block(&bm, &plan, &spec).unwrap()
        })
        .collect()
}

/// The header of a fixture case, built from the oracle's fields.
fn header(case: &J) -> SampleSetHeader {
    let e = expected();
    SampleSetHeader {
        model_digest: case.get("model_digest").d(),
        inputs_digest: case.get("inputs_digest").d(),
        seed: e.get("seed").u(),
        profile_digest: e.get("profile_digest").d(),
        outputs: case.get("outputs").a().iter().map(|o| o.s().to_owned()).collect(),
        trials: case.get("trials").u(),
        failed: case.get("failed").a().len() as u64,
    }
}

fn linear2() -> (Vec<BlockOut>, SampleSetHeader) {
    (blocks("linear2.json", 10_000, &[3, 1, 0, 2]), header(expected().get("linear2")))
}

fn fail_fit() -> (Vec<BlockOut>, SampleSetHeader) {
    (blocks("fail_fit.json", 1000, &[1, 0]), header(expected().get("fail_fit")))
}

fn header_len(bytes: &[u8]) -> usize {
    u64::from_le_bytes(bytes[..8].try_into().unwrap()) as usize
}

fn invalid(detail: &'static str) -> McmError {
    McmError::SamplesInvalid { detail }
}

// ------------------------------------------------------------------ AC-1

/// AC-1: blocks computed in two different orders (and handed over in either order) encode to
/// the same bytes, and the digest equals the oracle's `linear2.sha256`.
#[test]
fn ac1_order_independent_and_pinned() {
    let (shuffled, h) = linear2();
    let mut in_order = blocks("linear2.json", 10_000, &[0, 1, 2, 3]);
    let a = encode(&shuffled, &h, DEFAULT_CAP).unwrap();
    let b = encode(&in_order, &h, DEFAULT_CAP).unwrap();
    in_order.reverse();
    let c = encode(&in_order, &h, DEFAULT_CAP).unwrap();
    assert_eq!(a, b);
    assert_eq!(a, c);
    assert_eq!(a.digest.as_str(), pinned("linear2.sha256"));
    assert_eq!(a.digest, digest(&a.bytes));
    assert_eq!(a.digest, sha256_digest(&a.bytes));

    let case = expected().get("linear2").clone();
    assert_eq!(a.bytes.len() as u64, case.get("size").u());
    let hl = header_len(&a.bytes);
    assert_eq!(hl as u64, case.get("header_len").u());
    assert_eq!(sha256_digest(&a.bytes[8..8 + hl]).as_str(), case.get("header_digest").s());
    assert_eq!(h.canonical_json().unwrap(), a.bytes[8..8 + hl].to_vec());
}

/// The same pin for a set with failed trials: the failed-index list is the oracle's and the
/// digest equals `fail_fit.sha256`.
#[test]
fn fail_fit_pinned() {
    let (bs, h) = fail_fit();
    let enc = encode(&bs, &h, DEFAULT_CAP).unwrap();
    assert_eq!(enc.digest.as_str(), pinned("fail_fit.sha256"));
    let case = expected().get("fail_fit").clone();
    assert_eq!(enc.bytes.len() as u64, case.get("size").u());
    let set = decode(&enc.bytes, DEFAULT_CAP).unwrap();
    let want: Vec<u64> = case.get("failed").a().iter().map(J::u).collect();
    assert_eq!(set.failed, want);
    assert!(!want.is_empty());
}

// ------------------------------------------------------------------ AC-2

/// AC-2: 10 000 trials under a 1 KiB cap give `samples_not_retained` with the would-be size
/// from the oracle; the cap is inclusive.
#[test]
fn ac2_over_the_cap() {
    let e = expected();
    let ac2 = e.get("ac2");
    let bs = blocks("linear2.json", 10_000, &[0]);
    let mut h = header(e.get("linear2"));
    h.trials = ac2.get("trials").u();
    let size = ac2.get("size").u();
    let err = encode(&bs, &h, ac2.get("cap").u()).unwrap_err();
    assert_eq!(err, McmError::SamplesNotRetained { size });
    assert_eq!(err.code(), "samples_not_retained");
    assert_eq!(encode(&bs, &h, size - 1).unwrap_err(), McmError::SamplesNotRetained { size });
    assert_eq!(encode(&bs, &h, size).unwrap().bytes.len() as u64, size);
    // The decoder refuses input longer than its cap.
    let enc = encode(&bs, &h, size).unwrap();
    assert_eq!(decode(&enc.bytes, size - 1).unwrap_err(), invalid("too_large"));
}

// ------------------------------------------------------------------ AC-3 and decoder checks

/// AC-3 (error path): an encoding truncated by 8 bytes fails `samples_invalid`.
#[test]
fn ac3_truncated() {
    let (bs, h) = fail_fit();
    let enc = encode(&bs, &h, DEFAULT_CAP).unwrap();
    let err = decode(&enc.bytes[..enc.bytes.len() - 8], DEFAULT_CAP).unwrap_err();
    assert_eq!(err, invalid("length"));
    assert_eq!(err.code(), "samples_invalid");
}

/// Re-encodes `bytes` with the header text replaced (the prefix follows its length).
fn with_header(bytes: &[u8], header: &str) -> Vec<u8> {
    let hl = header_len(bytes);
    let mut out = (header.len() as u64).to_le_bytes().to_vec();
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(&bytes[8 + hl..]);
    out
}

/// REQ-UNC.PROP.MCM.SAMP-003 and the bounded decoder, hand-labelled.
#[test]
fn decoder_rejects() {
    let (bs, h) = fail_fit();
    let enc = encode(&bs, &h, DEFAULT_CAP).unwrap();
    let b = &enc.bytes;
    let hl = header_len(b);
    let text = String::from_utf8(b[8..8 + hl].to_vec()).unwrap();

    // Counts that disagree with the length.
    let mut longer = b.clone();
    longer.extend_from_slice(&[0; 8]);
    assert_eq!(decode(&longer, DEFAULT_CAP).unwrap_err(), invalid("length"));
    assert_eq!(decode(&b[..b.len() - 1], DEFAULT_CAP).unwrap_err(), invalid("length"));
    let fewer = text.replace(r#""trials":"2000""#, r#""trials":"1999""#);
    assert_ne!(fewer, text);
    assert_eq!(decode(&with_header(b, &fewer), DEFAULT_CAP).unwrap_err(), invalid("length"));
    for short in [&b[..0], &b[..7], &b[..8 + hl - 1]] {
        assert_eq!(decode(short, DEFAULT_CAP).unwrap_err(), invalid("length"));
    }

    // Unknown version (same length), or a missing one.
    let v2 = text.replace(FORMAT, "mcm-samples@2");
    assert_eq!(decode(&with_header(b, &v2), DEFAULT_CAP).unwrap_err(), invalid("version"));
    let unversioned = text.replace(r#","version":"mcm-samples@1""#, "");
    assert_eq!(decode(&with_header(b, &unversioned), DEFAULT_CAP).unwrap_err(), invalid("header"));

    // Header: non-canonical, extra member, malformed member, not JSON, too long a prefix.
    let spaced = text.replacen(',', ", ", 1);
    assert_eq!(decode(&with_header(b, &spaced), DEFAULT_CAP).unwrap_err(), invalid("header"));
    let extra = text.replace(r#"{"failed""#, r#"{"extra":"1","failed""#);
    assert_eq!(decode(&with_header(b, &extra), DEFAULT_CAP).unwrap_err(), invalid("header"));
    let lead0 = text.replace(r#""seed":"7""#, r#""seed":"07""#);
    assert_eq!(decode(&with_header(b, &lead0), DEFAULT_CAP).unwrap_err(), invalid("header"));
    let bad_digest = text.replacen("sha256:", "sha257:", 1);
    assert_eq!(decode(&with_header(b, &bad_digest), DEFAULT_CAP).unwrap_err(), invalid("header"));
    assert_eq!(decode(&with_header(b, "[]"), DEFAULT_CAP).unwrap_err(), invalid("header"));
    assert_eq!(decode(&with_header(b, "{"), DEFAULT_CAP).unwrap_err(), invalid("header"));
    let mut huge = b.clone();
    huge[..8].copy_from_slice(&(MAX_HEADER_BYTES + 1).to_le_bytes());
    assert_eq!(decode(&huge, DEFAULT_CAP).unwrap_err(), invalid("header"));
    huge[..8].copy_from_slice(&u64::MAX.to_le_bytes());
    assert_eq!(decode(&huge, DEFAULT_CAP).unwrap_err(), invalid("header"));

    // Outputs and counts.
    let no_outputs = text.replace(r#"["d"]"#, "[]");
    assert_eq!(decode(&with_header(b, &no_outputs), DEFAULT_CAP).unwrap_err(), invalid("outputs"));
    let unsorted = text.replace(r#"["d"]"#, r#"["d","c"]"#);
    assert_eq!(decode(&with_header(b, &unsorted), DEFAULT_CAP).unwrap_err(), invalid("outputs"));
    let over = text.replace(r#""failed":"53""#, r#""failed":"2001""#);
    assert_ne!(over, text);
    assert_eq!(decode(&with_header(b, &over), DEFAULT_CAP).unwrap_err(), invalid("counts"));

    // A NaN value; failed indices out of order or out of range.
    let mut nan = b.clone();
    nan[8 + hl..8 + hl + 8].copy_from_slice(&f64::NAN.to_le_bytes());
    assert_eq!(decode(&nan, DEFAULT_CAP).unwrap_err(), invalid("non_finite"));
    let mut inf = b.clone();
    inf[8 + hl..8 + hl + 8].copy_from_slice(&f64::NEG_INFINITY.to_le_bytes());
    assert_eq!(decode(&inf, DEFAULT_CAP).unwrap_err(), invalid("non_finite"));
    let n = b.len();
    let mut swapped = b.clone();
    let (x, y) = (b[n - 16..n - 8].to_vec(), b[n - 8..].to_vec());
    swapped[n - 16..n - 8].copy_from_slice(&y);
    swapped[n - 8..].copy_from_slice(&x);
    assert_eq!(decode(&swapped, DEFAULT_CAP).unwrap_err(), invalid("failed"));
    let mut beyond = b.clone();
    beyond[n - 8..].copy_from_slice(&2000u64.to_le_bytes());
    assert_eq!(decode(&beyond, DEFAULT_CAP).unwrap_err(), invalid("failed"));
}

// ------------------------------------------------------------------ AC-4

fn assert_lossless(bs: &[BlockOut], h: &SampleSetHeader, enc: &Encoded) {
    let set = decode(&enc.bytes, DEFAULT_CAP).unwrap();
    assert_eq!(&set.header, h);
    let mut sorted: Vec<&BlockOut> = bs.iter().collect();
    sorted.sort_by_key(|b| b.block_index);
    for (k, col) in set.outputs.iter().enumerate() {
        let want: Vec<u64> = sorted.iter().flat_map(|b| b.values[k].iter().map(|x| x.to_bits())).collect();
        let got: Vec<u64> = col.iter().map(|x| x.to_bits()).collect();
        assert_eq!(got, want);
    }
    let failed: Vec<u64> = sorted.iter().flat_map(|b| b.failed.iter().map(|f| f.index)).collect();
    assert_eq!(set.failed, failed);
    assert_eq!(&set.encode(DEFAULT_CAP).unwrap(), enc);
}

/// AC-4: AC-1 encoded and decoded is lossless (and so is the set with failures).
#[test]
fn ac4_round_trip() {
    let (bs, h) = linear2();
    assert_lossless(&bs, &h, &encode(&bs, &h, DEFAULT_CAP).unwrap());
    let (bs, h) = fail_fit();
    assert_lossless(&bs, &h, &encode(&bs, &h, DEFAULT_CAP).unwrap());
}

// ------------------------------------------------------------------ layout, hand-labelled

fn tiny_header() -> SampleSetHeader {
    SampleSetHeader {
        model_digest: sha256_digest(b"model"),
        inputs_digest: sha256_digest(b"inputs"),
        seed: 5,
        profile_digest: sha256_digest(b"profile"),
        outputs: vec!["a".into(), "b".into()],
        trials: 4,
        failed: 2,
    }
}

/// Two blocks of two trials with two outputs; trial 1 and trial 2 failed.
fn tiny_blocks() -> Vec<BlockOut> {
    let fail = |index| TrialFailure { index, code: "algorithm_failed", detail: Some("x") };
    vec![
        BlockOut { block_index: 1, values: vec![vec![-0.0], vec![2.5]], failed: vec![fail(2)] },
        BlockOut { block_index: 0, values: vec![vec![1.0], vec![-3.0]], failed: vec![fail(1)] },
    ]
}

/// The byte layout written out by hand from the module documentation.
#[test]
fn layout_by_hand() {
    let h = tiny_header();
    let enc = encode(&tiny_blocks(), &h, DEFAULT_CAP).unwrap();
    let json = format!(
        r#"{{"failed":"2","inputs":"{}","model":"{}","outputs":["a","b"],"profile":"{}","seed":"5","trials":"4","version":"mcm-samples@1"}}"#,
        sha256_digest(b"inputs").as_str(),
        sha256_digest(b"model").as_str(),
        sha256_digest(b"profile").as_str()
    );
    let mut want = (json.len() as u64).to_le_bytes().to_vec();
    want.extend_from_slice(json.as_bytes());
    for x in [1.0f64, -0.0, -3.0, 2.5] {
        want.extend_from_slice(&x.to_le_bytes());
    }
    for t in [1u64, 2] {
        want.extend_from_slice(&t.to_le_bytes());
    }
    assert_eq!(enc.bytes, want);
    assert_eq!(enc.digest, sha256_digest(&want));
    let set = decode(&enc.bytes, DEFAULT_CAP).unwrap();
    assert_eq!(set.outputs[0][1].to_bits(), (-0.0f64).to_bits());
    assert_eq!(set, SampleSet { header: h, outputs: vec![vec![1.0, -0.0], vec![-3.0, 2.5]], failed: vec![1, 2] });
}

/// A set whose trials all failed keeps empty columns and the full failed list.
#[test]
fn all_failed() {
    let h = SampleSetHeader { trials: 2, failed: 2, ..tiny_header() };
    let fail = |index| TrialFailure { index, code: "non_finite", detail: Some("output") };
    let bs = vec![BlockOut { block_index: 0, values: vec![vec![], vec![]], failed: vec![fail(0), fail(1)] }];
    let enc = encode(&bs, &h, DEFAULT_CAP).unwrap();
    let set = decode(&enc.bytes, DEFAULT_CAP).unwrap();
    assert_eq!(set.outputs, vec![Vec::<f64>::new(), Vec::new()]);
    assert_eq!(set.failed, vec![0, 1]);
}

// ------------------------------------------------------------------ encoder checks

#[test]
fn encoder_rejects() {
    let h = tiny_header();
    let check = |bs: Vec<BlockOut>, h: &SampleSetHeader, detail: &'static str| {
        assert_eq!(encode(&bs, h, DEFAULT_CAP).unwrap_err(), invalid(detail), "{detail}");
    };
    check(Vec::new(), &h, "blocks");
    let mut dup = tiny_blocks();
    dup[0].block_index = 0;
    check(dup, &h, "blocks");
    let mut gap = tiny_blocks();
    gap[0].block_index = 2;
    check(gap, &h, "blocks");
    let mut cols = tiny_blocks();
    cols[0].values.pop();
    check(cols, &h, "columns");
    let mut ragged = tiny_blocks();
    ragged[0].values[1].push(1.0);
    check(ragged, &h, "columns");
    let mut uneven = tiny_blocks();
    uneven[0].values[0].push(1.0);
    uneven[0].values[1].push(1.0);
    check(uneven, &h, "block_size");
    let mut nan = tiny_blocks();
    nan[1].values[0][0] = f64::NAN;
    check(nan, &h, "non_finite");
    let mut outside = tiny_blocks();
    outside[0].failed[0].index = 0; // block 1 covers trials 2 and 3
    check(outside, &h, "failed");
    check(tiny_blocks(), &SampleSetHeader { trials: 6, ..h.clone() }, "trials");
    check(tiny_blocks(), &SampleSetHeader { failed: 1, ..h.clone() }, "failed_count");
    for outputs in [vec![], vec!["b".to_owned(), "a".to_owned()], vec!["a".to_owned(), "a".to_owned()]] {
        check(tiny_blocks(), &SampleSetHeader { outputs, ..h.clone() }, "outputs");
    }
    for name in ["", "q\"", "q\\", "\u{e9}", &"n".repeat(65)] {
        check(tiny_blocks(), &SampleSetHeader { outputs: vec!["a".into(), name.into()], ..h.clone() }, "outputs");
    }
    check(tiny_blocks(), &SampleSetHeader { failed: 5, ..h.clone() }, "counts");
    check(tiny_blocks(), &SampleSetHeader { trials: 0, failed: 0, ..h.clone() }, "counts");

    // SampleSet::encode applies the same checks.
    let good = SampleSet { header: h.clone(), outputs: vec![vec![1.0, 2.0], vec![3.0, 4.0]], failed: vec![1, 2] };
    assert!(good.encode(DEFAULT_CAP).is_ok());
    let mut short = good.clone();
    short.outputs[1].pop();
    assert_eq!(short.encode(DEFAULT_CAP).unwrap_err(), invalid("length"));
    let mut inf = good.clone();
    inf.outputs[0][0] = f64::INFINITY;
    assert_eq!(inf.encode(DEFAULT_CAP).unwrap_err(), invalid("non_finite"));
    let mut unsorted = good;
    unsorted.failed = vec![2, 1];
    assert_eq!(unsorted.encode(DEFAULT_CAP).unwrap_err(), invalid("failed"));
}

/// Error codes and messages are stable.
#[test]
fn error_codes() {
    let e = McmError::SamplesNotRetained { size: 80_341 };
    assert_eq!(
        (e.code(), e.to_string().as_str()),
        ("samples_not_retained", "samples_not_retained: 80341 bytes over the cap")
    );
    let e = invalid("length");
    assert_eq!((e.code(), e.to_string().as_str()), ("samples_invalid", "samples_invalid: length"));
    assert_eq!(DEFAULT_CAP, 64 * 1024 * 1024);
    assert_eq!(FORMAT, "mcm-samples@1");
}

/// Performance budget (10^6 samples in under 50 ms [unverified]): measured only with
/// `UNC_MCM_SAMP_PERF=1 cargo test --release -p fid-kunc --test samples perf -- --nocapture`.
#[cfg(not(target_family = "wasm"))]
#[test]
fn perf_encode_one_million() {
    if std::env::var("UNC_MCM_SAMP_PERF").is_err() {
        return;
    }
    let n = 1_000_000u64;
    let col: Vec<f64> = (0..n).map(|i| i as f64 * 0.5).collect();
    let bs = vec![BlockOut { block_index: 0, values: vec![col], failed: Vec::new() }];
    let h = SampleSetHeader { outputs: vec!["y".into()], trials: n, failed: 0, ..tiny_header() };
    let start = std::time::Instant::now();
    let enc = encode(&bs, &h, DEFAULT_CAP).unwrap();
    let ms = start.elapsed().as_secs_f64() * 1000.0;
    let start = std::time::Instant::now();
    let d = sha256_digest(&enc.bytes);
    let sha_ms = start.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(d, enc.digest);
    println!(
        "encode 10^6 samples: {ms:.1} ms ({} bytes, digest included; SHA-256 alone {sha_ms:.1} ms)",
        enc.bytes.len()
    );
}
