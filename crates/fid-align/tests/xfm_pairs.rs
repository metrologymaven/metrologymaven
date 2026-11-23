//! Acceptance tests for `ALN.XFM.PAIRS` (`align.run_point_pairs@1`): AC-1..AC-6.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_align::{
    invoke_point_pairs_cli, invoke_point_pairs_mcp, replay_point_pairs, run_point_pairs, run_point_pairs_command_def,
    AlignmentModel, PointPairInput, PrincipalKind, ResidualStore, RunPointPairsParams, CLI_RUN_POINT_PAIRS,
};
use fid_align_solve::constr::Dof;
use fid_canon::CValue;
use fid_dep::EdgeIndex;
use fid_geo_frame::FrameTree;
use fid_project::{Bits, IdService, Observation, ReplaySource};
use fid_uuid::Uuid;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn fix(sub: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/aln/xfm").join(sub)
}

fn obj(v: &CValue) -> &BTreeMap<String, CValue> {
    match v {
        CValue::Obj(o) => o,
        _ => panic!("expected object"),
    }
}

fn arr(v: &CValue) -> &[CValue] {
    match v {
        CValue::Arr(a) => a,
        _ => panic!("expected array"),
    }
}

fn num(v: &CValue) -> f64 {
    match v {
        CValue::Num(n) => *n,
        _ => panic!("expected number"),
    }
}

fn vec3(v: &CValue) -> [f64; 3] {
    let a = arr(v);
    [num(&a[0]), num(&a[1]), num(&a[2])]
}

fn det3(r: [[f64; 3]; 3]) -> f64 {
    ((r[0][0] * ((r[1][1] * r[2][2]) - (r[1][2] * r[2][1]))) - (r[0][1] * ((r[1][0] * r[2][2]) - (r[1][2] * r[2][0]))))
        + (r[0][2] * ((r[1][0] * r[2][1]) - (r[1][1] * r[2][0])))
}

fn make_src(n: usize) -> ReplaySource {
    let mut obs = Vec::with_capacity(n * 2);
    for i in 0..n {
        obs.push(Observation::NowMs(1_763_700_000_000 + i as u64));
        obs.push(Observation::RandomBits(Bits::new(74, 0x200 + i as u128).unwrap()));
    }
    ReplaySource::new(obs)
}

#[test]
fn ac1_ac3_ac6_known_pairs_corpus_locked_and_exact_replay() {
    let def = run_point_pairs_command_def().unwrap();
    assert_eq!(def.id, "align.run_point_pairs");
    assert_eq!(def.major, 1);

    let doc = fid_canon::parse(&std::fs::read(fix("pairs_known.json")).unwrap()).unwrap();
    let root = obj(&doc);
    let cases = arr(&root["cases"]);
    assert_eq!(cases.len(), 20);

    let mut model = AlignmentModel::new();
    let mut edges = EdgeIndex::new();
    let mut reports = ResidualStore::new();
    let tree = FrameTree::new();
    let mut src = make_src(30);
    let existing = BTreeSet::new();
    let mut ids = IdService::new(Uuid::from_u128(1), &mut src, &existing);
    let mut journal: Vec<Vec<u8>> = Vec::new();

    // AC-1: all 20 known transforms (3, 4, 10, 1000 pairs including 179 deg & 10 m offsets).
    for (idx, c) in cases.iter().enumerate() {
        let co = obj(c);
        let meas: Vec<[f64; 3]> = arr(&co["measured"]).iter().map(vec3).collect();
        let refr: Vec<[f64; 3]> = arr(&co["reference"]).iter().map(vec3).collect();
        let pairs: Vec<PointPairInput> = meas
            .into_iter()
            .zip(refr)
            .map(|(m, r)| PointPairInput { measured: m, reference: r, weight: None })
            .collect();
        let params = RunPointPairsParams {
            pairs,
            dof_locks: Vec::new(),
            lock_frame: None,
            start_pose: None,
            parent: None,
            principal: "alice".to_owned(),
            principal_kind: PrincipalKind::Human,
        };
        let committed = run_point_pairs(&mut model, &mut edges, &mut reports, &tree, &mut ids, &params).unwrap();
        assert!(committed.solution.max_len_m <= 1e-12, "case {idx}: max_len={}", committed.solution.max_len_m);
        assert!((det3(committed.solution.transform.r()) - 1.0).abs() <= 1e-12, "case {idx}: det != +1");
        if idx < 4 {
            journal.push(committed.event.to_canonical_bytes().unwrap());
        }
    }

    // AC-3: 10-pair locked {rx, ry} case via command layer.
    let lo = obj(&root["locked_rx_ry"]);
    let meas_l: Vec<[f64; 3]> = arr(&lo["measured"]).iter().map(vec3).collect();
    let ref_l: Vec<[f64; 3]> = arr(&lo["reference"]).iter().map(vec3).collect();
    let pairs_l: Vec<PointPairInput> = meas_l
        .into_iter()
        .zip(ref_l)
        .map(|(m, r)| PointPairInput { measured: m, reference: r, weight: None })
        .collect();
    let locked_params = RunPointPairsParams {
        pairs: pairs_l,
        dof_locks: vec![Dof::Rx, Dof::Ry],
        lock_frame: None,
        start_pose: None,
        parent: None,
        principal: "alice".to_owned(),
        principal_kind: PrincipalKind::Human,
    };
    let c_lock = run_point_pairs(&mut model, &mut edges, &mut reports, &tree, &mut ids, &locked_params).unwrap();
    assert!(c_lock.solution.max_len_m <= 1e-10, "locked max_len={}", c_lock.solution.max_len_m);
    journal.push(c_lock.event.to_canonical_bytes().unwrap());

    // CLI + MCP invocation parity check.
    let raw = br#"{"pairs":[{"measured":[0,0,0],"reference":[1,0,0]},{"measured":[1,0,0],"reference":[1,1,0]},{"measured":[0,1,0],"reference":[0,0,0]}]}"#;
    let c_cli = invoke_point_pairs_cli(
        &mut model,
        &mut edges,
        &mut reports,
        &tree,
        &mut ids,
        ("alice", PrincipalKind::Human),
        &[CLI_RUN_POINT_PAIRS, "--params-json", std::str::from_utf8(raw).unwrap(), "--json"],
    )
    .unwrap();
    let c_mcp = invoke_point_pairs_mcp(
        &mut model,
        &mut edges,
        &mut reports,
        &tree,
        &mut ids,
        ("alice", PrincipalKind::Human),
        raw,
    )
    .unwrap();
    assert_eq!(c_cli.alignment.transform(), c_mcp.alignment.transform());
    journal.push(c_cli.event.to_canonical_bytes().unwrap());

    // AC-6: exact replay reproduces identical alignments and digests.
    let mut r_model = AlignmentModel::new();
    let mut r_edges = EdgeIndex::new();
    let mut r_reports = ResidualStore::new();
    for ev_bytes in &journal {
        let rc = replay_point_pairs(&mut r_model, &mut r_edges, &mut r_reports, &tree, ev_bytes).unwrap();
        let orig = model.get(&rc.alignment.id()).unwrap();
        assert_eq!(rc.alignment.digest(), orig.digest());
        assert_eq!(rc.alignment.transform(), orig.transform());
    }
    assert_eq!(r_model.len(), journal.len());
}

#[test]
fn ac4_collinear_error_journals_nothing() {
    let mut model = AlignmentModel::new();
    let mut edges = EdgeIndex::new();
    let mut reports = ResidualStore::new();
    let tree = FrameTree::new();
    let mut src = make_src(2);
    let existing = BTreeSet::new();
    let mut ids = IdService::new(Uuid::from_u128(1), &mut src, &existing);

    let raw = br#"{"pairs":[{"measured":[0,0,0],"reference":[0,0,0]},{"measured":[1,0,0],"reference":[1,0,0]},{"measured":[2,0,0],"reference":[2,0,0]}]}"#;
    let err = invoke_point_pairs_mcp(
        &mut model,
        &mut edges,
        &mut reports,
        &tree,
        &mut ids,
        ("alice", PrincipalKind::Human),
        raw,
    )
    .unwrap_err();
    assert_eq!(err.code(), "collinear_pairs");
    assert!(model.is_empty());
}
