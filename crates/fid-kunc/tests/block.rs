//! Acceptance criteria of spec node UNC.PROP.MCM.BLOCK (AC-1 .. AC-4, AC-H1) plus
//! requirement checks.
//!
//! Models, the kernel manifest, the digests, the expected samples, output values and failed
//! trial indices come from the independent Python reference
//! `fixtures/unc/mcm/block/make_fixtures.py` (its own Philox4x32-10, AS241 with a 60-digit
//! decimal logarithm, the SCHEMA oracle's canonical form), never from this crate. The
//! manifest's core digest from that file plays the digest compiled into the test kernel: the
//! test kernel table is built with it as a constant and never derives it from the manifest.
//!
//! AC-H1 is measured by `ac_h1_ks_rejections`. Plain `cargo test` runs 100 seeds at 2000
//! trials each (4 shuffled blocks of 500) on every target, wasm32 included;
//! `UNC_MCM_BLOCK_FULL=1 cargo test --release -p fid-kunc --test block ac_h1 -- --nocapture`
//! runs the hypothesis scale, 10^6 trials per seed in 100 shuffled blocks of 10 000. Either
//! way the rejection count must lie in the exact two-sided 99 % band of Binomial(100, 0.01)
//! from the reference file.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::cell::RefCell;
use std::rc::Rc;

use fid_kci::kci::json::Json;
use fid_kci::kci::sha256_digest;
use fid_kci::{
    parse_card, AlgorithmFailure, AlgorithmId, AlgorithmTable, CardRegistry, InputArity, KernelAlgorithm, KernelError,
    KernelManifest,
};
use fid_kunc::dist::{Correlation, Distribution, MvNormal, Quantity, UnitCode};
use fid_kunc::model::{
    validate, CorrelationGroup, Edge, Endpoint, InPort, InputNode, InputSourceKind, ModelParts, OperatorNode, OutPort,
    OutputNode, SourceRef, UncertaintyModel, ValidModel,
};
use fid_kunc::prop::mcm::block::{
    run_block, BlockModel, BlockOut, BlockSpec, SamplingPlan, TrialFailure, ALGORITHM_ID, FAIL_ALGORITHM,
    FAIL_BAD_OUTPUT, FAIL_NON_FINITE,
};
use fid_kunc::prop::mcm::McmError;
use fid_num::{f, rng};
use fid_types::Digest;

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/unc/mcm/block/");

fn read(name: &str) -> Vec<u8> {
    std::fs::read(format!("{FIX}{name}")).unwrap_or_else(|e| panic!("{name}: {e}"))
}

// ------------------------------------------------------------------ a small JSON reader

/// The reference file holds only objects, arrays and strings.
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
    fn f(&self) -> f64 {
        f64::from_bits(u64::from_str_radix(self.s(), 16).unwrap())
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
    parse_json(&read("expected.json"))
}

fn digest(s: &str) -> Digest {
    Digest::parse(s).unwrap()
}

// ------------------------------------------------------------------ the test kernel

const SUM: &str = "test.mcm.sum@1.0.0";
const NEG: &str = "test.mcm.neg@1.0.0";
const CIRCLE: &str = "test.mcm.circle_fit@1.0.0";
const BAD: &str = "test.mcm.bad@1.0.0";

#[derive(Clone, Copy, PartialEq)]
enum Op {
    Sum,
    Neg,
    Circle,
    Bad,
}

type Calls = Rc<RefCell<Vec<String>>>;
type Inputs = Rc<RefCell<Vec<Vec<f64>>>>;

/// A test algorithm. `calls` logs the parameter text of every call; `seen` logs the decoded
/// input values of every call.
struct Alg {
    id: &'static str,
    op: Op,
    arity: InputArity,
    calls: Option<Calls>,
    seen: Option<Inputs>,
}

fn doubles(blob: &[u8]) -> Vec<f64> {
    assert_eq!(blob.len() % 8, 0);
    blob.chunks_exact(8).map(|c| f64::from_le_bytes(c.try_into().unwrap())).collect()
}

fn fail(code: &'static str) -> KernelError {
    KernelError::Algorithm(AlgorithmFailure { code })
}

/// cos and sin of k pi / 4, k = 0..7 (exact table; S = sqrt(1/2) rounded once).
const S: f64 = std::f64::consts::FRAC_1_SQRT_2;
const COS8: [f64; 8] = [1.0, S, 0.0, -S, -1.0, -S, 0.0, S];
const SIN8: [f64; 8] = [0.0, S, 1.0, S, 0.0, -S, -1.0, -S];

/// Fits a circle to the 8 points `(offset + r cos, r sin)` of a ring: for an evenly spaced
/// point set the least-squares centre is the centroid and the radius the root mean square
/// distance to it. A negative nominal radius is a domain error of the probe compensation.
fn circle_fit(r: f64, offset: f64) -> Result<f64, KernelError> {
    if r < 0.0 {
        return Err(fail("negative_radius"));
    }
    let xs: Vec<f64> = COS8.iter().map(|c| offset + r * c).collect();
    let ys: Vec<f64> = SIN8.iter().map(|s| r * s).collect();
    let cx = xs.iter().fold(0.0, |a, x| a + x) / 8.0;
    let cy = ys.iter().fold(0.0, |a, y| a + y) / 8.0;
    let ss = xs.iter().zip(&ys).fold(0.0, |a, (x, y)| a + ((x - cx) * (x - cx) + (y - cy) * (y - cy)));
    Ok(2.0 * f::sqrt(ss / 8.0).unwrap())
}

impl KernelAlgorithm for Alg {
    fn id(&self) -> AlgorithmId {
        AlgorithmId::parse(self.id).unwrap()
    }
    fn input_arity(&self) -> InputArity {
        self.arity
    }
    fn validate(&self, params: &[u8]) -> Result<(), KernelError> {
        if params == br#"{"mode":"invalid"}"# {
            return Err(KernelError::InvalidParams { reason: "mode" });
        }
        Ok(())
    }
    fn run(&self, params: &[u8], seed: Option<u64>, inputs: &[&[u8]]) -> Result<Vec<u8>, KernelError> {
        assert!(seed.is_none(), "operators get no seed");
        let text = String::from_utf8(params.to_vec()).unwrap();
        if let Some(c) = &self.calls {
            c.borrow_mut().push(text.clone());
        }
        let vals: Vec<Vec<f64>> = inputs.iter().map(|b| doubles(b)).collect();
        if let Some(s) = &self.seen {
            s.borrow_mut().push(vals.iter().flatten().copied().collect());
        }
        let y = match self.op {
            Op::Sum => vals[0].iter().skip(1).fold(vals[0][0], |a, x| a + x),
            Op::Neg => -vals[0][0],
            Op::Circle => circle_fit(vals[0][0], vals[1][0])?,
            Op::Bad => match text.as_str() {
                r#"{"mode":"nan"}"# => f64::NAN,
                r#"{"mode":"inf"}"# => f64::INFINITY,
                r#"{"mode":"short"}"# => return Ok(vec![0; 4]),
                r#"{"mode":"impersonate"}"# => return Err(KernelError::UnknownAlgorithm(self.id())),
                _ => return Err(fail("bad_mode")),
            },
        };
        Ok(y.to_le_bytes().to_vec())
    }
}

fn card(id: &str, arity: &str) -> String {
    [
        format!("id = \"{id}\""),
        "title = \"Test operator of the UNC.PROP.MCM.BLOCK tests\"".to_string(),
        "method = \"Plain binary64 arithmetic on little-endian doubles.\"".to_string(),
        "applicability = \"Tests of the Monte Carlo trial block only.\"".to_string(),
        "references = [\"Spec node UNC.PROP.MCM.BLOCK.\"]".to_string(),
        "determinism = \"D0\"".to_string(),
        "failure_modes = [\"None beyond the test cases.\"]".to_string(),
        "validation = \"crates/fid-kunc/tests/block.rs\"".to_string(),
        format!("input_arity = \"{arity}\""),
    ]
    .join("\n")
}

#[derive(Default, Clone)]
struct Kernel {
    /// The compiled-in core digest; `None` builds a kernel without one.
    core: Option<Digest>,
    calls: Option<Calls>,
    seen: Option<Inputs>,
    /// Leave the circle fit out of the table.
    no_circle: bool,
    /// Declare the sum with this input arity (card and implementation alike).
    sum_arity: Option<&'static str>,
}

impl Kernel {
    fn compiled() -> Kernel {
        Kernel { core: Some(digest(expected().get("core_digest").s())), ..Kernel::default() }
    }

    fn build(&self) -> AlgorithmTable {
        let sum_arity = self.sum_arity.unwrap_or("1");
        let mut specs = vec![(SUM, Op::Sum, sum_arity), (NEG, Op::Neg, "1"), (BAD, Op::Bad, "1")];
        if !self.no_circle {
            specs.push((CIRCLE, Op::Circle, "2"));
        }
        let mut algs: Vec<Box<dyn KernelAlgorithm>> = Vec::new();
        let mut cards = Vec::new();
        for (id, op, arity) in specs {
            let a = InputArity::parse(arity).unwrap();
            algs.push(Box::new(Alg { id, op, arity: a, calls: self.calls.clone(), seen: self.seen.clone() }));
            cards.push(parse_card(&card(id, arity)).unwrap());
        }
        let t = AlgorithmTable::new(algs, CardRegistry::new(cards).unwrap()).unwrap();
        match self.core {
            Some(d) => t.with_manifest_core_digest(d),
            None => t,
        }
    }
}

fn manifest() -> KernelManifest {
    KernelManifest::parse_canonical(&read("manifest.json")).unwrap()
}

fn valid(m: &UncertaintyModel) -> ValidModel {
    validate(m, &manifest()).unwrap_or_else(|e| panic!("{:?}", e.iter().map(ToString::to_string).collect::<Vec<_>>()))
}

fn fixture_model(name: &str) -> ValidModel {
    valid(&UncertaintyModel::parse(&read(name)).unwrap())
}

fn spec_for(v: &ValidModel, plan: &SamplingPlan, seed: u64, block_index: u64, block_size: u64) -> BlockSpec {
    BlockSpec { model_digest: v.model().digest(), inputs_digest: plan.digest(), seed, block_index, block_size }
}

/// Runs blocks of `v` in the given order on a fresh kernel; results in that order.
fn blocks(v: &ValidModel, seed: u64, size: u64, order: &[u64]) -> Vec<BlockOut> {
    let table = Kernel::compiled().build();
    let manifest = manifest();
    let bm = BlockModel::prepare(v, &manifest, &table).unwrap();
    let plan = SamplingPlan::new(v).unwrap();
    order.iter().map(|&b| run_block(&bm, &plan, &spec_for(v, &plan, seed, b, size)).unwrap()).collect()
}

fn canon(out: &BlockOut) -> Vec<u8> {
    out.canonical_json().unwrap()
}

/// Distance in units in the last place between two finite doubles of the same sign.
fn ulps(a: f64, b: f64) -> u64 {
    (a.to_bits() as i64 - b.to_bits() as i64).unsigned_abs()
}

// ------------------------------------------------------------------ model building

fn src(label: &str) -> Option<SourceRef> {
    Some(SourceRef {
        kind: InputSourceKind::Declared,
        digest: sha256_digest(label.as_bytes()),
        author: "metrology-maven tests".to_owned(),
    })
}

fn m(x: f64) -> Quantity {
    Quantity::new(x, UnitCode::Metre)
}

fn input(id: &str, d: Distribution) -> InputNode {
    InputNode { id: id.into(), quantity_kind: "length".into(), unit: UnitCode::Metre, distribution: d, source: src(id) }
}

fn normal(id: &str, mu: f64, sigma: f64) -> InputNode {
    input(id, Distribution::normal(m(mu), m(sigma)).unwrap())
}

fn port_in(name: &str, arity: u32) -> InPort {
    InPort { name: name.into(), quantity_kind: "length".into(), unit: UnitCode::Metre, arity }
}

fn port_out(name: &str) -> OutPort {
    OutPort { name: name.into(), quantity_kind: "length".into(), unit: UnitCode::Metre }
}

fn operator(id: &str, alg: &str, params: &[(&str, &str)], ins: Vec<InPort>, outs: Vec<OutPort>) -> OperatorNode {
    let (name, version) = alg.split_once('@').unwrap();
    OperatorNode {
        id: id.into(),
        algorithm_id: name.into(),
        algorithm_version: version.into(),
        params: Json::Obj(params.iter().map(|(k, v)| ((*k).to_owned(), Json::Str((*v).to_owned()))).collect()),
        ports_in: ins,
        ports_out: outs,
    }
}

fn output(id: &str) -> OutputNode {
    OutputNode { id: id.into(), quantity_kind: "length".into(), unit: UnitCode::Metre, characteristic: None }
}

fn edge(from: (&str, &str), to: (&str, &str)) -> Edge {
    Edge {
        from: Endpoint { node: from.0.into(), port: from.1.into() },
        to: Endpoint { node: to.0.into(), port: to.1.into() },
    }
}

fn parts(name: &str, inputs: Vec<InputNode>, operators: Vec<OperatorNode>, outputs: Vec<OutputNode>) -> ModelParts {
    ModelParts {
        name: name.into(),
        semver: "1.0.0".into(),
        inputs,
        operators,
        outputs,
        edges: Vec::new(),
        correlations: Vec::new(),
    }
}

/// x1, x2, x3 -> a = sum(x1, x2), b = neg(a), c = sum(b, x3); outputs y = c and z = a.
fn chain3() -> ValidModel {
    let mut p = parts(
        "chain3",
        vec![normal("x1", 1.0, 0.1), normal("x2", 2.0, 0.1), normal("x3", 3.0, 0.1)],
        vec![
            operator("a", SUM, &[("tag", "a")], vec![port_in("terms", 2)], vec![port_out("value")]),
            operator("b", NEG, &[("tag", "b")], vec![port_in("x", 1)], vec![port_out("value")]),
            operator("c", SUM, &[("tag", "c")], vec![port_in("terms", 2)], vec![port_out("value")]),
        ],
        vec![output("y"), output("z")],
    );
    p.edges = vec![
        edge(("x1", "value"), ("a", "terms")),
        edge(("x2", "value"), ("a", "terms")),
        edge(("a", "value"), ("b", "x")),
        edge(("b", "value"), ("c", "terms")),
        edge(("x3", "value"), ("c", "terms")),
        edge(("c", "value"), ("y", "value")),
        edge(("a", "value"), ("z", "value")),
    ];
    valid(&UncertaintyModel::new(p).unwrap())
}

/// x -> bad(mode) -> y.
fn bad_model(mode: &str) -> ValidModel {
    let mut p = parts(
        "bad",
        vec![normal("x", 1.0, 0.1)],
        vec![operator("op", BAD, &[("mode", mode)], vec![port_in("x", 1)], vec![port_out("value")])],
        vec![output("y")],
    );
    p.edges = vec![edge(("x", "value"), ("op", "x")), edge(("op", "value"), ("y", "value"))];
    valid(&UncertaintyModel::new(p).unwrap())
}

// ------------------------------------------------------------------ identity and digests

#[test]
fn algorithm_id_and_spec_card() {
    assert_eq!(ALGORITHM_ID, "unc.prop.mcm_block@1.0.0");
    let card = parse_card(include_str!("../cards/unc.prop.mcm_block@1.0.0.toml")).unwrap();
    let id = AlgorithmId::parse(ALGORITHM_ID).unwrap();
    let report = fid_kci::check(&[id], std::slice::from_ref(&card.id));
    assert!(report.is_clean(), "{report:?}");
    assert_eq!(card.determinism, "D2");
    assert!(card.references.iter().all(|r| r.starts_with("JCGM 101:2008")));
    // The card's two data inputs are run_block's bound model and sampling plan; the block
    // spec is the parameter set. The binding stops compiling if the signature changes.
    let _: fn(&BlockModel<'_>, &SamplingPlan, &BlockSpec) -> Result<BlockOut, McmError> = run_block;
    assert_eq!(card.input_arity, InputArity::exactly(2));
}

/// The manifest, model and inputs digests equal the oracle's.
#[test]
fn digests_match_the_oracle() {
    let e = expected();
    let man = manifest();
    assert_eq!(man.core_digest().as_str(), e.get("core_digest").s());
    assert_eq!(man.manifest_digest().as_str(), e.get("manifest_digest").s());
    for name in ["linear2.json", "fail_fit.json", "rect4.json"] {
        let v = fixture_model(name);
        let want = e.get("models").get(name);
        assert_eq!(v.model().digest().as_str(), want.get("model_digest").s(), "{name}");
        let plan = SamplingPlan::new(&v).unwrap();
        assert_eq!(plan.digest().as_str(), want.get("inputs_digest").s(), "{name}");
        assert_eq!(plan.model_digest(), v.model().digest());
    }
}

/// The plan's canonical JSON, hand-written from the module documentation.
#[test]
fn plan_canonical_json() {
    let v = fixture_model("linear2.json");
    let plan = SamplingPlan::new(&v).unwrap();
    let want = format!(
        concat!(
            r#"{{"algorithm":"unc.prop.mcm_block@1.0.0","model":"{}","samplers":["#,
            r#"{{"inputs":["x1"],"kind":"normal","stream":"0"}},{{"inputs":["x2"],"kind":"normal","stream":"1"}}]}}"#
        ),
        v.model().digest().as_str()
    );
    assert_eq!(String::from_utf8(plan.canonical_json().to_vec()).unwrap(), want);
    assert_eq!(plan.digest(), sha256_digest(want.as_bytes()));
}

/// BlockOut canonical bytes, hand-written from the module documentation.
#[test]
fn block_out_canonical_json() {
    let out = BlockOut {
        block_index: 12,
        values: vec![vec![1.0, -2.5], vec![0.0, f64::MIN_POSITIVE]],
        failed: vec![
            TrialFailure { index: 7, code: FAIL_ALGORITHM, detail: Some("negative_radius") },
            TrialFailure { index: 9, code: FAIL_BAD_OUTPUT, detail: None },
        ],
    };
    let want = concat!(
        r#"{"block_index":"12","failed":[{"code":"algorithm_failed","detail":"negative_radius","index":"7"},"#,
        r#"{"code":"bad_output","index":"9"}],"values":[["3ff0000000000000","c004000000000000"],"#,
        r#"["0000000000000000","0010000000000000"]]}"#
    );
    assert_eq!(String::from_utf8(canon(&out)).unwrap(), want);
}

// ------------------------------------------------------------------ AC-1, AC-2

/// AC-1: linear2, seed 7, block size 10 000: blocks 0..3 computed in order 3, 1, 0, 2 and
/// in order 0..3 give the same bytes; every value agrees with the oracle (bit for bit where
/// both samples come from the central region of AS241, within 4 ulp otherwise).
#[test]
fn ac1_block_order_does_not_matter() {
    let e = expected();
    let l2 = e.get("linear2");
    let (seed, size) = (l2.get("seed").u(), l2.get("block_size").u());
    assert_eq!((seed, size), (7, 10_000));
    let v = fixture_model("linear2.json");
    let shuffled = blocks(&v, seed, size, &[3, 1, 0, 2]);
    let ordered = blocks(&v, seed, size, &[0, 1, 2, 3]);
    for (k, &b) in [3usize, 1, 0, 2].iter().enumerate() {
        assert_eq!(shuffled[k].block_index, b as u64);
        assert_eq!(canon(&shuffled[k]), canon(&ordered[b]), "block {b}");
    }
    let mut y = Vec::new();
    for out in &ordered {
        assert!(out.failed.is_empty());
        assert_eq!(out.values.len(), 1);
        assert_eq!(out.values[0].len(), 10_000);
        y.extend_from_slice(&out.values[0]);
    }
    let rows = l2.get("y_every_101").a();
    assert_eq!(rows.len(), 397);
    let mut exact = 0;
    for r in rows {
        let r = r.a();
        let (t, want) = (r[0].u() as usize, r[1].f());
        if r[2].s() == "exact" {
            assert_eq!(y[t].to_bits(), want.to_bits(), "trial {t}");
            exact += 1;
        } else {
            assert!(ulps(y[t], want) <= 4, "trial {t}: {} vs {want}", y[t]);
        }
    }
    assert!(exact > 250, "most rows are in the central region: {exact}");
}

/// AC-2: the X1 sample of trial 12 345 is the UNC.MODEL.DIST normal draw at RNG address
/// (7, 0, 12 345), bit for bit, and X2 the draw at (7, 1, 12 345); both agree with the
/// oracle.
#[test]
fn ac2_sample_address() {
    let e = expected();
    let l2 = e.get("linear2");
    let v = fixture_model("linear2.json");
    let seen: Inputs = Rc::new(RefCell::new(Vec::new()));
    let table = Kernel { seen: Some(seen.clone()), ..Kernel::compiled() }.build();
    let manifest = manifest();
    let bm = BlockModel::prepare(&v, &manifest, &table).unwrap();
    let plan = SamplingPlan::new(&v).unwrap();
    run_block(&bm, &plan, &spec_for(&v, &plan, 7, 1, 10_000)).unwrap();
    let calls = seen.borrow();
    assert_eq!(calls.len(), 10_000);
    let at = &calls[12_345 - 10_000];
    let inputs = v.model().inputs();
    assert_eq!((inputs[0].id.as_str(), inputs[1].id.as_str()), ("x1", "x2"));
    let x1 = inputs[0].distribution.sample_at(7, 0, 12_345).unwrap();
    let x2 = inputs[1].distribution.sample_at(7, 1, 12_345).unwrap();
    assert_eq!(at[0].to_bits(), x1.to_bits());
    assert_eq!(at[1].to_bits(), x2.to_bits());
    for (got, key) in [(x1, "x1_12345"), (x2, "x2_12345")] {
        let want = l2.get(key).f();
        if l2.get(&format!("{key}_region")).s() == "exact" {
            assert_eq!(got.to_bits(), want.to_bits(), "{key}");
        } else {
            assert!(ulps(got, want) <= 2, "{key}");
        }
    }
}

// ------------------------------------------------------------------ AC-3

/// AC-3: fail_fit, seed 7, blocks 0 and 1 of 1000: the failed trials are exactly the
/// oracle's negative-radius trials, with code `algorithm_failed` and detail
/// `negative_radius`; the values exclude them and the blocks complete. Spot values are the
/// diameter of the oracle's radius.
#[test]
fn ac3_failed_trials_are_recorded_and_excluded() {
    let e = expected();
    let ff = e.get("fail_fit");
    let (seed, size) = (ff.get("seed").u(), ff.get("block_size").u());
    let v = fixture_model("fail_fit.json");
    let outs = blocks(&v, seed, size, &[0, 1]);
    let want: Vec<u64> = ff.get("failed").a().iter().map(J::u).collect();
    let got: Vec<u64> = outs.iter().flat_map(|o| o.failed.iter().map(|f| f.index)).collect();
    assert_eq!(got, want);
    assert!(want.len() > 20);
    let mut d = Vec::new();
    for (b, out) in outs.iter().enumerate() {
        for fl in &out.failed {
            assert_eq!((fl.code, fl.detail), (FAIL_ALGORITHM, Some("negative_radius")));
            assert_eq!(fl.index / size, b as u64);
        }
        assert_eq!(out.values[0].len() as u64, size - out.failed.len() as u64);
        d.extend_from_slice(&out.values[0]);
    }
    // Map trial index to its position among the successful trials.
    let ok: Vec<u64> = (0..2 * size).filter(|t| !want.contains(t)).collect();
    let spots = ff.get("spot_radius_offset").a();
    assert!(spots.len() > 10);
    for s in spots {
        let s = s.a();
        let (t, r) = (s[0].u(), s[1].f());
        let pos = ok.binary_search(&t).unwrap();
        assert!((d[pos] - 2.0 * r).abs() <= 1e-12 * r, "trial {t}: {} vs 2 x {r}", d[pos]);
    }
}

// ------------------------------------------------------------------ AC-4

/// AC-4: with a counting kernel table, every trial calls every operator exactly once, in
/// Kahn order (a, b, c), and nothing else is called.
#[test]
fn ac4_each_operator_once_per_trial() {
    let v = chain3();
    assert_eq!(v.order(), ["x1", "x2", "a", "b", "x3", "c", "y", "z"]);
    let calls: Calls = Rc::new(RefCell::new(Vec::new()));
    let table = Kernel { calls: Some(calls.clone()), ..Kernel::compiled() }.build();
    let manifest = manifest();
    let bm = BlockModel::prepare(&v, &manifest, &table).unwrap();
    let plan = SamplingPlan::new(&v).unwrap();
    let out = run_block(&bm, &plan, &spec_for(&v, &plan, 3, 2, 250)).unwrap();
    assert!(out.failed.is_empty());
    let log = calls.borrow();
    assert_eq!(log.len(), 3 * 250);
    for (t, c) in log.chunks_exact(3).enumerate() {
        assert_eq!(c, [r#"{"tag":"a"}"#, r#"{"tag":"b"}"#, r#"{"tag":"c"}"#], "trial {t}");
    }
    // Outputs in ID order: y = -(x1 + x2) + x3, z = x1 + x2.
    let inputs = v.model().inputs();
    for k in [0usize, 1, 249] {
        let t = 500 + k as u64;
        let x: Vec<f64> = (0..3).map(|i| inputs[i].distribution.sample_at(3, i as u64, t).unwrap()).collect();
        assert_eq!(out.values[1][k].to_bits(), (x[0] + x[1]).to_bits());
        assert_eq!(out.values[0][k].to_bits(), (-(x[0] + x[1]) + x[2]).to_bits());
    }
}

// ------------------------------------------------------------------ REQ-003, REQ-004

#[test]
fn req004_non_finite_outputs_fail_the_trial() {
    for (mode, code, detail) in [
        ("nan", FAIL_NON_FINITE, Some("output")),
        ("inf", FAIL_NON_FINITE, Some("output")),
        ("short", FAIL_BAD_OUTPUT, None),
        ("other", FAIL_ALGORITHM, Some("bad_mode")),
    ] {
        let v = bad_model(mode);
        let out = blocks(&v, 1, 20, &[5]).remove(0);
        assert!(out.values[0].is_empty(), "{mode}");
        let want: Vec<TrialFailure> = (100..120).map(|index| TrialFailure { index, code, detail }).collect();
        assert_eq!(out.failed, want, "{mode}");
    }
}

/// Kernel errors that do not depend on the trial end the block: invalid parameters, and an
/// algorithm that tries to look like a kernel check (the kernel confines it to an algorithm
/// failure, which is per trial).
#[test]
fn kernel_errors_that_are_not_per_trial_end_the_block() {
    let v = bad_model("invalid");
    let table = Kernel::compiled().build();
    let manifest = manifest();
    let bm = BlockModel::prepare(&v, &manifest, &table).unwrap();
    let plan = SamplingPlan::new(&v).unwrap();
    let e = run_block(&bm, &plan, &spec_for(&v, &plan, 1, 3, 10)).unwrap_err();
    assert_eq!(e, McmError::Kernel { code: "invalid_params", trial: 30 });
    assert_eq!(e.code(), "kernel");

    let out = blocks(&bad_model("impersonate"), 1, 4, &[0]).remove(0);
    assert_eq!(out.failed.len(), 4);
    assert_eq!(out.failed[0].code, FAIL_ALGORITHM);
    assert_eq!(out.failed[0].detail, Some(fid_kci::kci::IMPERSONATION_CODE));
}

// ------------------------------------------------------------------ provenance

#[test]
fn provenance_is_checked_before_any_trial() {
    let v = fixture_model("linear2.json");
    let manifest = manifest();

    let unset = Kernel::default().build();
    assert_eq!(
        BlockModel::prepare(&v, &manifest, &unset).unwrap_err(),
        McmError::Provenance { detail: "kernel_manifest_unset" }
    );

    let other = Kernel { core: Some(sha256_digest(b"another kernel")), ..Kernel::default() }.build();
    let e = BlockModel::prepare(&v, &manifest, &other).unwrap_err();
    assert_eq!(e, McmError::Provenance { detail: "kernel_manifest_mismatch" });
    assert_eq!(e.code(), "provenance");

    // A model validated against another manifest (same algorithms, another kernel name).
    let mut core = manifest.core().clone();
    core.name = "fid-kernel-other".into();
    let second = KernelManifest::new(core, manifest.artifacts().to_vec()).unwrap();
    let w = validate(v.model(), &second).unwrap();
    let table = Kernel::compiled().build();
    assert_eq!(
        BlockModel::prepare(&w, &manifest, &table).unwrap_err(),
        McmError::Provenance { detail: "model_manifest" }
    );
    // Bound to the second manifest, the kernel's compiled-in digest no longer matches.
    assert_eq!(
        BlockModel::prepare(&w, &second, &table).unwrap_err(),
        McmError::Provenance { detail: "kernel_manifest_mismatch" }
    );
    assert!(BlockModel::prepare(&v, &manifest, &table).is_ok());
}

#[test]
fn operators_must_be_in_the_kernel_with_a_matching_arity() {
    let manifest = manifest();
    let no_circle = Kernel { no_circle: true, ..Kernel::compiled() }.build();
    assert_eq!(
        BlockModel::prepare(&fixture_model("fail_fit.json"), &manifest, &no_circle).unwrap_err(),
        McmError::UnsupportedModel { detail: "operator_not_in_kernel" }
    );
    let two = Kernel { sum_arity: Some("2"), ..Kernel::compiled() }.build();
    assert_eq!(
        BlockModel::prepare(&fixture_model("linear2.json"), &manifest, &two).unwrap_err(),
        McmError::UnsupportedModel { detail: "operator_arity" }
    );
}

// ------------------------------------------------------------------ block spec checks

#[test]
fn block_spec_is_checked() {
    let v = fixture_model("linear2.json");
    let w = fixture_model("rect4.json");
    let table = Kernel::compiled().build();
    let manifest = manifest();
    let bm = BlockModel::prepare(&v, &manifest, &table).unwrap();
    let plan = SamplingPlan::new(&v).unwrap();
    let other_plan = SamplingPlan::new(&w).unwrap();
    let good = spec_for(&v, &plan, 7, 0, 10);
    assert!(run_block(&bm, &plan, &good).is_ok());

    let cases = [
        (BlockSpec { block_size: 0, ..good }, &plan, McmError::InvalidBlock { detail: "block_size" }),
        (BlockSpec { block_index: u64::MAX, ..good }, &plan, McmError::InvalidBlock { detail: "index" }),
        (BlockSpec { block_index: u64::MAX / 10, ..good }, &plan, McmError::InvalidBlock { detail: "index" }),
        (
            BlockSpec { model_digest: w.model().digest(), ..good },
            &plan,
            McmError::BlockSpecMismatch { field: "model_digest" },
        ),
        (
            BlockSpec { inputs_digest: other_plan.digest(), ..good },
            &plan,
            McmError::BlockSpecMismatch { field: "inputs_digest" },
        ),
        (good, &other_plan, McmError::BlockSpecMismatch { field: "model_digest" }),
    ];
    for (spec, p, want) in cases {
        assert_eq!(run_block(&bm, p, &spec).unwrap_err(), want, "{spec:?}");
    }
    // The last addressable index of a normal input is u64::MAX - 1 here (end = u64::MAX).
    let last = BlockSpec { block_index: u64::MAX / 5 - 1, block_size: 5, ..good };
    assert!(run_block(&bm, &plan, &last).is_ok());
}

/// A scaled-t input addresses at most 2^56 trials (UNC.MODEL.DIST gamma sub-stream).
#[test]
fn scaled_t_index_limit() {
    let t = Distribution::scaled_t(m(0.0), m(1.0), Quantity::new(5.0, UnitCode::One)).unwrap();
    let mut p = parts(
        "st",
        vec![input("x", t)],
        vec![operator("op", NEG, &[], vec![port_in("x", 1)], vec![port_out("value")])],
        vec![output("y")],
    );
    p.edges = vec![edge(("x", "value"), ("op", "x")), edge(("op", "value"), ("y", "value"))];
    let v = valid(&UncertaintyModel::new(p).unwrap());
    let table = Kernel::compiled().build();
    let manifest = manifest();
    let bm = BlockModel::prepare(&v, &manifest, &table).unwrap();
    let plan = SamplingPlan::new(&v).unwrap();
    let max = fid_kunc::dist::MAX_GAMMA_INDEX;
    let ok = spec_for(&v, &plan, 1, max / 2, 2);
    assert!(run_block(&bm, &plan, &ok).is_ok());
    let over = spec_for(&v, &plan, 1, max / 2 + 1, 2);
    assert_eq!(run_block(&bm, &plan, &over).unwrap_err(), McmError::InvalidBlock { detail: "index" });
}

// ------------------------------------------------------------------ correlation groups

fn correlated(second: InputNode) -> ModelParts {
    let mut p = parts(
        "corr",
        vec![normal("u", 1.0, 0.5), second, normal("w", 3.0, 0.25)],
        vec![operator("s", SUM, &[], vec![port_in("terms", 3)], vec![port_out("value")])],
        vec![output("y")],
    );
    p.edges = vec![
        edge(("u", "value"), ("s", "terms")),
        edge(("v", "value"), ("s", "terms")),
        edge(("w", "value"), ("s", "terms")),
        edge(("s", "value"), ("y", "value")),
    ];
    // Group of u and w (ordinals 0 and 2); v (ordinal 1) is independent.
    p.correlations =
        vec![CorrelationGroup { inputs: vec!["w".into(), "u".into()], matrix: vec![vec![1.0, 0.6], vec![0.6, 1.0]] }];
    p
}

/// A correlation group of normal inputs is one multivariate normal at the stream of its
/// first input; the other inputs keep their own ordinals.
#[test]
fn correlated_normals_are_drawn_jointly() {
    let v = valid(&UncertaintyModel::new(correlated(normal("v", 2.0, 1.0))).unwrap());
    let plan = SamplingPlan::new(&v).unwrap();
    let text = String::from_utf8(plan.canonical_json().to_vec()).unwrap();
    assert!(text.ends_with(concat!(
        r#""samplers":[{"inputs":["u","w"],"kind":"mv_normal","stream":"0"},"#,
        r#"{"inputs":["v"],"kind":"normal","stream":"1"}]}"#
    )));
    let seen: Inputs = Rc::new(RefCell::new(Vec::new()));
    let table = Kernel { seen: Some(seen.clone()), ..Kernel::compiled() }.build();
    let manifest = manifest();
    let bm = BlockModel::prepare(&v, &manifest, &table).unwrap();
    run_block(&bm, &plan, &spec_for(&v, &plan, 9, 4, 8)).unwrap();
    let corr = Correlation::new(2, vec![1.0, 0.6, 0.6, 1.0]).unwrap();
    let mv = MvNormal::new(&[m(1.0), m(3.0)], &[m(0.5), m(0.25)], corr).unwrap();
    let vdist = Distribution::normal(m(2.0), m(1.0)).unwrap();
    for (k, got) in seen.borrow().iter().enumerate() {
        let t = 32 + k as u64;
        let uw = mv.sample(9, 0, t..t + 1).unwrap();
        let want = [uw[0], vdist.sample_at(9, 1, t).unwrap(), uw[1]];
        assert_eq!(got.iter().map(|x| x.to_bits()).collect::<Vec<_>>(), want.map(f64::to_bits).to_vec(), "trial {t}");
    }
}

#[test]
fn correlated_non_normal_is_refused() {
    let rect = input("v", Distribution::rect(m(-1.0), m(1.0)).unwrap());
    let mut p = correlated(rect);
    p.correlations =
        vec![CorrelationGroup { inputs: vec!["u".into(), "v".into()], matrix: vec![vec![1.0, 0.5], vec![0.5, 1.0]] }];
    let v = valid(&UncertaintyModel::new(p).unwrap());
    assert_eq!(SamplingPlan::new(&v).unwrap_err(), McmError::UnsupportedModel { detail: "correlated_non_normal" });
}

// ------------------------------------------------------------------ thread count

/// The same blocks computed on several threads (each with its own kernel, as the command
/// would schedule them) equal the single-threaded result byte for byte.
#[cfg(not(target_family = "wasm"))]
#[test]
fn thread_count_does_not_matter() {
    let v = fixture_model("fail_fit.json");
    let single: Vec<Vec<u8>> = blocks(&v, 11, 300, &[0, 1, 2, 3, 4, 5]).iter().map(canon).collect();
    for threads in [2u64, 3] {
        let handles: Vec<_> = (0..threads)
            .map(|k| {
                let v = v.clone();
                std::thread::spawn(move || {
                    let mine: Vec<u64> = (0..6).filter(|b| b % threads == k).rev().collect();
                    let outs = blocks(&v, 11, 300, &mine);
                    mine.into_iter().zip(outs.iter().map(canon)).collect::<Vec<_>>()
                })
            })
            .collect();
        let mut got: Vec<(u64, Vec<u8>)> = handles.into_iter().flat_map(|h| h.join().unwrap()).collect();
        got.sort_by_key(|(b, _)| *b);
        assert_eq!(got.into_iter().map(|(_, c)| c).collect::<Vec<_>>(), single, "{threads} threads");
    }
}

// ------------------------------------------------------------------ AC-H1

/// Analytic CDF of rect4, Y = 2 h S - 4 h with S Irwin-Hall(4) and h = sqrt(3):
/// F_S(s) = (1/24) sum_{k <= floor(s)} (-1)^k C(4, k) (s - k)^4 (the textbook closed form).
fn rect4_cdf(y: f64, h: f64) -> f64 {
    let s = (y + 4.0 * h) / (2.0 * h);
    if s <= 0.0 {
        return 0.0;
    }
    if s >= 4.0 {
        return 1.0;
    }
    let c = [1.0, -4.0, 6.0, -4.0, 1.0];
    let mut acc = 0.0;
    for (k, ck) in c.iter().enumerate() {
        let d = s - k as f64;
        if d <= 0.0 {
            break;
        }
        acc += ck * (d * d) * (d * d);
    }
    acc / 24.0
}

/// Kolmogorov-Smirnov statistic of `v` against `cdf`.
fn ks(mut v: Vec<f64>, cdf: impl Fn(f64) -> f64) -> f64 {
    v.sort_by(f64::total_cmp);
    let n = v.len() as f64;
    let mut d = 0.0f64;
    for (i, &x) in v.iter().enumerate() {
        let fx = cdf(x);
        let hi = (i + 1) as f64 / n - fx;
        let lo = fx - i as f64 / n;
        for c in [hi, lo] {
            if c > d {
                d = c;
            }
        }
    }
    d
}

/// The rect4 output values of `seed`, computed block by block in a shuffled order and
/// concatenated in block-index order.
fn rect4_values(
    bm: &BlockModel<'_>,
    plan: &SamplingPlan,
    v: &ValidModel,
    seed: u64,
    n_blocks: u64,
    size: u64,
) -> Vec<f64> {
    let mut order: Vec<u64> = (0..n_blocks).collect();
    let mut cur = rng::Cursor::new(rng::Address { seed: 0xB10C, stream: seed }, 0);
    rng::shuffle(&mut order, &mut cur).unwrap();
    let mut outs: Vec<BlockOut> =
        order.iter().map(|&b| run_block(bm, plan, &spec_for(v, plan, seed, b, size)).unwrap()).collect();
    outs.sort_by_key(|o| o.block_index);
    let mut y = Vec::with_capacity((n_blocks * size) as usize);
    for o in outs {
        assert!(o.failed.is_empty());
        y.extend_from_slice(&o.values[0]);
    }
    y
}

/// AC-H1: on the rectangular-input additive model (output distribution known in closed
/// form), the Kolmogorov-Smirnov test of each seed's trials against the analytic CDF at
/// alpha = 0.01 rejects for a number of the 100 seeds inside the exact two-sided 99 % band
/// of Binomial(100, 0.01) ([0, 4], from the reference file); blocks are computed in a
/// shuffled order. The critical value is the asymptotic Kolmogorov quantile from the
/// reference file with Stephens' finite-sample factor sqrt(n) + 0.12 + 0.11 / sqrt(n)
/// [unverified: Stephens 1970 modification]. Seeds 0x1B10_0000 + s, s = 0..99.
#[test]
fn ac_h1_ks_rejections() {
    let full = std::env::var("UNC_MCM_BLOCK_FULL").map(|v| v == "1").unwrap_or(false);
    let (n_blocks, size) = if full { (100u64, 10_000u64) } else { (4, 500) };
    let e = expected();
    let r = e.get("ac_h1");
    let c = r.get("ks_c_001").f();
    let band = r.get("binom_band").a();
    let (lo, hi) = (band[0].u(), band[1].u());
    assert_eq!((lo, hi), (0, 4));
    let h = r.get("half_width").f();
    let v = fixture_model("rect4.json");
    let table = Kernel::compiled().build();
    let manifest = manifest();
    let bm = BlockModel::prepare(&v, &manifest, &table).unwrap();
    let plan = SamplingPlan::new(&v).unwrap();
    let n = (n_blocks * size) as f64;
    let factor = f::sqrt(n).unwrap() + 0.12 + 0.11 / f::sqrt(n).unwrap();
    let mut rejections = 0u64;
    let mut worst = 0.0f64;
    for s in 0..100u64 {
        let y = rect4_values(&bm, &plan, &v, 0x1B10_0000 + s, n_blocks, size);
        let stat = ks(y, |x| rect4_cdf(x, h)) * factor;
        if stat > c {
            rejections += 1;
        }
        if stat > worst {
            worst = stat;
        }
    }
    println!(
        "AC-H1 n={n}: KS at alpha 0.01 rejected {rejections}/100 seeds; band [{lo}, {hi}] (exact two-sided 99 %); largest statistic {worst:.4} vs {c:.4}"
    );
    assert!(rejections >= lo && rejections <= hi, "AC-H1 falsified: {rejections} rejections");
}

// ------------------------------------------------------------------ per-target pins

fn hex_bytes(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Bit identity across targets. `crate_bits.txt` pins SHA-256 of the canonical bytes of
/// linear2 blocks 0..3 (seed 7, size 10 000), fail_fit blocks 0 and 1 (seed 7, size 1000),
/// chain3 block 2 (seed 3, size 250) and rect4 block 0 of the first four AC-H1 seeds
/// (size 500). It is produced by this test with `UNC_MCM_BLOCK_PRINT_BITS=1` (crate output,
/// not an oracle: the values are checked against the oracle by AC-1 .. AC-3) and CI runs this
/// file on wasm32-wasip1, x86-64 (Linux, Windows) and ARM64 (macOS), so a pass everywhere
/// shows identical bytes on all of them.
#[test]
fn block_bytes_are_pinned_per_target() {
    let mut lines = vec![
        "# UNC.PROP.MCM.BLOCK bit pins: crate output, regenerate with UNC_MCM_BLOCK_PRINT_BITS=1".to_string(),
        "# case sha256 of BlockOut::canonical_json".to_string(),
    ];
    let mut pin = |name: String, out: &BlockOut| {
        lines.push(format!("{name} sha256:{}", hex_bytes(&fid_kci::kci::sha256(&canon(out)))));
    };
    let l2 = fixture_model("linear2.json");
    for out in blocks(&l2, 7, 10_000, &[0, 1, 2, 3]) {
        pin(format!("linear2_seed7_block{}", out.block_index), &out);
    }
    let ff = fixture_model("fail_fit.json");
    for out in blocks(&ff, 7, 1000, &[0, 1]) {
        pin(format!("fail_fit_seed7_block{}", out.block_index), &out);
    }
    let c3 = chain3();
    pin("chain3_seed3_block2".into(), &blocks(&c3, 3, 250, &[2]).remove(0));
    let r4 = fixture_model("rect4.json");
    for s in 0..4u64 {
        pin(format!("rect4_seed{:x}_block0", 0x1B10_0000 + s), &blocks(&r4, 0x1B10_0000 + s, 500, &[0]).remove(0));
    }
    let text = lines.join("\n") + "\n";
    if std::env::var("UNC_MCM_BLOCK_PRINT_BITS").map(|v| v == "1").unwrap_or(false) {
        std::fs::write(format!("{FIX}crate_bits.txt"), &text).unwrap();
    }
    assert_eq!(text, String::from_utf8(read("crate_bits.txt")).unwrap());
}
