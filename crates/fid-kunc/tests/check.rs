//! Acceptance criteria of spec node UNC.MODEL.GRAPH.CHECK (AC-1 .. AC-3) plus requirement
//! checks.
//!
//! Expected findings, their order and the Kahn order of the valid model come from the
//! independent Python reference `fixtures/unc/model/graph/make_check_fixtures.py` (a second
//! implementation of the passes, held to hand labels); the manifest stub is written there
//! with `hashlib`. None come from this crate. The same file runs on wasm32-wasip1, x86-64
//! and ARM64 in CI, so equal findings there are the cross-target identity check.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use fid_kci::kci::json::Json;
use fid_kci::manifest::KernelManifest;
use fid_kunc::dist::{Distribution, Quantity, UnitCode};
use fid_kunc::model::{
    validate, Edge, Endpoint, InPort, InputNode, InputSourceKind, ModelError, ModelParts, OperatorNode, OutPort,
    OutputNode, SourceRef, UncertaintyModel, ValidModel,
};

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/unc/model/graph/");

fn read(rel: &str) -> Vec<u8> {
    std::fs::read(format!("{FIX}{rel}")).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn lines(rel: &str) -> Vec<String> {
    String::from_utf8(read(rel)).unwrap().lines().map(str::to_owned).collect()
}

fn model(rel: &str) -> UncertaintyModel {
    UncertaintyModel::parse(&read(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn stub() -> KernelManifest {
    KernelManifest::parse_canonical(&read("manifest_stub.json")).unwrap()
}

fn ok(m: &UncertaintyModel, manifest: &KernelManifest) -> ValidModel {
    validate(m, manifest).unwrap_or_else(|e| panic!("{:?}", e.iter().map(ToString::to_string).collect::<Vec<_>>()))
}

fn findings(m: &UncertaintyModel, manifest: &KernelManifest) -> Vec<String> {
    match validate(m, manifest) {
        Ok(_) => Vec::new(),
        Err(e) => e.iter().map(ToString::to_string).collect(),
    }
}

// ------------------------------------------------------------------ AC-1

#[test]
fn ac1_bore_ls_is_valid_against_the_stub() {
    let manifest = stub();
    let m = model("bore_ls.json");
    let v = ok(&m, &manifest);
    assert_eq!(v.model(), &m);
    assert_eq!(v.manifest_digest(), manifest.manifest_digest());
    assert_eq!(v.order(), lines("bore_ls.order.txt").as_slice());
    assert_eq!(v.clone().into_model(), m);
    // The shuffled document is the same model and gives the same result.
    let s = ok(&model("bore_ls_shuffled.json"), &manifest);
    assert_eq!(s, v);
}

// ------------------------------------------------------------------ AC-2

/// `bad/expected.txt`, grouped by file, in file order of first appearance.
fn expected() -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for line in lines("bad/expected.txt") {
        let (file, finding) = line.split_once('\t').unwrap();
        out.entry(file.to_owned()).or_default().push(finding.to_owned());
    }
    out
}

#[test]
fn ac2_every_bad_model_yields_the_labelled_findings_in_order() {
    let manifest = stub();
    let exp = expected();
    assert!(exp.len() >= 26, "{}", exp.len());
    for (file, want) in &exp {
        let m = model(&format!("bad/{file}"));
        let errs = validate(&m, &manifest).expect_err(file);
        let got: Vec<String> = errs.iter().map(ToString::to_string).collect();
        assert_eq!(&got, want, "{file}");
        for e in &errs {
            // Code, pass and node are what the line starts with.
            let mut words = e.to_string().split(' ').map(str::to_owned).collect::<Vec<_>>().into_iter();
            assert_eq!(words.next().as_deref(), Some(e.code()), "{file}");
            assert_eq!(words.next().as_deref(), Some(e.node()), "{file}");
            assert!((1..=8).contains(&e.pass()));
        }
        assert!(errs.windows(2).all(|w| w[0].pass() <= w[1].pass()), "{file}: passes out of order");
    }
}

#[test]
fn ac2_every_bad_file_is_labelled() {
    let exp = expected();
    let mut on_disk: Vec<String> = std::fs::read_dir(format!("{FIX}bad"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|n| n.ends_with(".json"))
        .collect();
    on_disk.sort();
    assert_eq!(on_disk, exp.keys().cloned().collect::<Vec<_>>());
}

#[test]
fn ac2_the_five_named_findings() {
    let manifest = stub();
    let one = |file: &str| findings(&model(&format!("bad/{file}")), &manifest);
    assert_eq!(one("cycle.json"), ["cycle n3 n3,n5,n7"]);
    assert_eq!(one("unknown_operator.json"), ["unknown_operator thermal unc.op.scale@9.9.9"]);
    assert_eq!(one("unsourced_input.json"), ["unsourced_input probe_r"]);
    assert_eq!(one("unit_m_to_rad.json"), ["unit_mismatch fit tilt tilt value"]);
    assert_eq!(one("correlation_indefinite.json"), ["invalid_correlation form indefinite"]);
    let all = one("all_five.json");
    assert_eq!(all.len(), 5);
    let codes: Vec<&str> = all.iter().map(|l| l.split(' ').next().unwrap()).collect();
    assert_eq!(codes, ["cycle", "unknown_operator", "unsourced_input", "unit_mismatch", "invalid_correlation"]);
    assert_eq!(one("all_five_shuffled.json"), all);
}

#[test]
fn findings_do_not_depend_on_document_order() {
    let manifest = stub();
    for base in ["all_five", "all_codes"] {
        let a = validate(&model(&format!("bad/{base}.json")), &manifest).unwrap_err();
        let b = validate(&model(&format!("bad/{base}_shuffled.json")), &manifest).unwrap_err();
        assert_eq!(a, b, "{base}");
    }
}

#[test]
fn an_operator_version_is_pinned_exactly() {
    // The stub lists unc.op.scale at 1.0.0 and 1.1.0 only.
    let manifest = stub();
    let ids: Vec<String> = manifest.core().algorithms.iter().map(|a| a.id.to_string()).collect();
    assert!(ids.contains(&"unc.op.scale@1.1.0".to_owned()) && !ids.contains(&"unc.op.scale@9.9.9".to_owned()));
    // Against a manifest without the circle fit, bore_ls fails on that operator alone.
    let core = fid_kci::manifest::ManifestCore {
        algorithms: manifest.core().algorithms.iter().filter(|a| a.id.module != "fit").cloned().collect(),
        ..manifest.core().clone()
    };
    let other = KernelManifest::new(core, manifest.artifacts().to_vec()).unwrap();
    assert_eq!(findings(&model("bore_ls.json"), &other), ["unknown_operator fit fit.circle.ls@1.0.0"]);
}

// ------------------------------------------------------------------ AC-3 (chain models)

/// splitmix64, for a seeded permutation of node IDs.
struct SplitMix(u64);

impl SplitMix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
}

const SEED: u64 = 0x4752_4150;
const NODES: usize = 1000;
/// REQ-UNC.MODEL.GRAPH.CHECK-006 / REQ-UNC.MODEL.GRAPH-008.
const BUDGET: Duration = Duration::from_millis(50);

fn at(node: &str, port: &str) -> Endpoint {
    Endpoint { node: node.into(), port: port.into() }
}

/// A chain input -> 998 operators -> output whose node IDs are a seeded permutation of
/// `n0000..n0999`, so ID order and chain order differ; the parts are also shuffled. Returns
/// the parts and the IDs in chain order. With `close`, the first operator's port takes a
/// second edge from the last operator, closing a ring of all 998 operators.
fn chain(close: bool) -> (ModelParts, Vec<String>) {
    let mut rng = SplitMix(SEED);
    let mut perm: Vec<usize> = (0..NODES).collect();
    for i in (1..NODES).rev() {
        let j = (rng.next() % (i as u64 + 1)) as usize;
        perm.swap(i, j);
    }
    let ids: Vec<String> = perm.iter().map(|p| format!("n{p:04}")).collect();
    let q = |v: f64| Quantity::new(v, UnitCode::Metre);
    let dist = Distribution::normal(q(1.0), q(0.001)).unwrap();
    let digest = fid_kci::kci::sha256_digest(b"chain source");
    let inputs = vec![InputNode {
        id: ids[0].clone(),
        quantity_kind: "length".into(),
        unit: UnitCode::Metre,
        distribution: dist,
        source: Some(SourceRef { kind: InputSourceKind::Declared, digest, author: "fixtures".into() }),
    }];
    let mut operators = Vec::new();
    for (k, id) in ids.iter().enumerate().take(NODES - 1).skip(1) {
        let arity = if close && k == 1 { 2 } else { 1 };
        operators.push(OperatorNode {
            id: id.clone(),
            algorithm_id: "unc.op.scale".into(),
            algorithm_version: "1.0.0".into(),
            params: Json::Obj(vec![("mode".into(), Json::Str("multiplicative".into()))]),
            ports_in: vec![InPort { name: "x".into(), quantity_kind: "length".into(), unit: UnitCode::Metre, arity }],
            ports_out: vec![OutPort { name: "y".into(), quantity_kind: "length".into(), unit: UnitCode::Metre }],
        });
    }
    let outputs = vec![OutputNode {
        id: ids[NODES - 1].clone(),
        quantity_kind: "length".into(),
        unit: UnitCode::Metre,
        characteristic: None,
    }];
    let out_port = |k: usize| if k == 0 { "value" } else { "y" };
    let in_port = |k: usize| if k == NODES - 1 { "value" } else { "x" };
    let mut edges: Vec<Edge> =
        (1..NODES).map(|k| Edge { from: at(&ids[k - 1], out_port(k - 1)), to: at(&ids[k], in_port(k)) }).collect();
    if close {
        edges.push(Edge { from: at(&ids[NODES - 2], "y"), to: at(&ids[1], "x") });
    }
    // Shuffle the edge list and the operator list with the same generator.
    for i in (1..edges.len()).rev() {
        edges.swap(i, (rng.next() % (i as u64 + 1)) as usize);
    }
    for i in (1..operators.len()).rev() {
        operators.swap(i, (rng.next() % (i as u64 + 1)) as usize);
    }
    let parts = ModelParts {
        name: "chain".into(),
        semver: "1.0.0".into(),
        inputs,
        operators,
        outputs,
        edges,
        correlations: Vec::new(),
    };
    (parts, ids)
}

#[test]
fn ac3_chain_of_1000_nodes_validates_within_budget() {
    let manifest = stub();
    let (parts, ids) = chain(false);
    let m = UncertaintyModel::new(parts).unwrap();
    assert_eq!(m.inputs().len() + m.operators().len() + m.outputs().len(), NODES);
    let mut best = Duration::MAX;
    let mut last = None;
    for _ in 0..3 {
        let t = Instant::now();
        let v = validate(&m, &manifest);
        best = best.min(t.elapsed());
        last = Some(v);
    }
    let v = last.unwrap().unwrap_or_else(|e| panic!("{e:?}"));
    // A chain has exactly one topological order: the chain order.
    assert_eq!(v.order(), ids.as_slice());
    // CI builds tests unoptimised (5x to 10x slower) and shares runners, so the time is held
    // to a guard against super-linear regressions: 40x the budget unoptimised, 10x optimised.
    let limit = if cfg!(debug_assertions) { BUDGET * 40 } else { BUDGET * 10 };
    println!("AC-3: 1000-node chain validated in {best:?} (budget {BUDGET:?}, guard {limit:?})");
    assert!(best < limit, "{best:?} >= {limit:?}");
}

#[test]
fn ac3_a_ring_of_998_operators_is_one_cycle_named_by_its_lowest_id() {
    let manifest = stub();
    let (parts, ids) = chain(true);
    let m = UncertaintyModel::new(parts).unwrap();
    let errs = validate(&m, &manifest).unwrap_err();
    let mut ring: Vec<String> = ids[1..NODES - 1].to_vec();
    ring.sort();
    assert_eq!(errs, vec![ModelError::Cycle { node: ring[0].clone(), members: ring }]);
}
