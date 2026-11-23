//! Acceptance tests for `ALN.RPS.CLASSIC` command `align.run_321@1` in `fid-align`
//! (`AC-1`..`AC-6`, CLI/MCP parity, atomic fail-closed journaling, exact replay, and schema validation).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_align::{
    invoke_321_cli, invoke_321_mcp, replay_321, run_321, run_321_command_def, AlignmentModel, PrincipalKind,
    ResidualStore, Rps321Elements, Run321Mode, Run321Params, RUN_321,
};
use fid_align_solve::{Rps321Tolerances, RpsElement, SignedAxis};
use fid_bus::error::{BusError, Detail, ValidationReason};
use fid_bus::unit::Normalised;
use fid_bus::validate::{Compiled, Hooks, RefLookup};
use fid_canon::{parse, CValue};
use fid_dep::EdgeIndex;
use fid_project::{Bits, IdService, Observation, ReplaySource};
use fid_uuid::Uuid;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

struct NoHooks;

impl Hooks for NoHooks {
    fn literal(&self, _: &str, _: &CValue) -> Result<Normalised, BusError> {
        Err(BusError::Validation(ValidationReason::UnitMismatch, Detail::new("no quantity literals")))
    }

    fn entity(&self, _: &str) -> RefLookup {
        RefLookup::Missing
    }
}

fn load_fixture() -> BTreeMap<String, CValue> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/aln/rps/block_321.json");
    let raw = std::fs::read(&path).unwrap();
    match parse(&raw).unwrap() {
        CValue::Obj(o) => o,
        _ => panic!("fixture not JSON object"),
    }
}

fn num_vec3(v: &CValue) -> [f64; 3] {
    let CValue::Arr(a) = v else { panic!("not arr") };
    let f = |x: &CValue| match x {
        CValue::Num(n) => *n,
        _ => panic!("not num"),
    };
    [f(&a[0]), f(&a[1]), f(&a[2])]
}

fn obj<'a>(m: &'a BTreeMap<String, CValue>, k: &str) -> &'a BTreeMap<String, CValue> {
    match m.get(k).unwrap() {
        CValue::Obj(o) => o,
        _ => panic!("field {k} not object"),
    }
}

fn mat3(v: &CValue) -> [[f64; 3]; 3] {
    let CValue::Arr(rows) = v else { panic!("not mat") };
    [num_vec3(&rows[0]), num_vec3(&rows[1]), num_vec3(&rows[2])]
}

fn corners(v: &CValue) -> Vec<[f64; 3]> {
    let CValue::Arr(arr) = v else { panic!("not corners") };
    arr.iter().map(num_vec3).collect()
}

fn fixture_store(fix: &BTreeMap<String, CValue>) -> Rps321Elements {
    let top = obj(fix, "top_plane");
    let front = obj(fix, "front_line");
    let end = obj(fix, "end_point");
    let mut store = Rps321Elements::new();
    store
        .insert("top", RpsElement::Plane { point: num_vec3(&top["point"]), normal: num_vec3(&top["normal"]) })
        .unwrap();
    store
        .insert(
            "front",
            RpsElement::Line { point: num_vec3(&front["point"]), direction: num_vec3(&front["direction"]) },
        )
        .unwrap();
    store.insert("end", RpsElement::Point { position: num_vec3(&end["position"]) }).unwrap();
    store.insert("parallel", RpsElement::Line { point: [0.0, 0.0, 0.0], direction: num_vec3(&top["normal"]) }).unwrap();
    store.insert("on_front", RpsElement::Point { position: num_vec3(&front["point"]) }).unwrap();
    store
}

fn replay_source(seed: u64) -> ReplaySource {
    let mut obs = Vec::new();
    for i in 0..16_u64 {
        let rand = (u128::from(seed.wrapping_add(i.wrapping_mul(0x9e37_79b9))) << 16) | u128::from(i + 1);
        obs.push(Observation::NowMs(1_764_000_000_000 + i * 10));
        obs.push(Observation::RandomBits(Bits::new(74, rand & ((1_u128 << 74) - 1)).unwrap()));
    }
    ReplaySource::new(obs)
}

fn apply(r: &[[f64; 3]; 3], t: [f64; 3], p: [f64; 3]) -> [f64; 3] {
    [
        r[0][0] * p[0] + r[0][1] * p[1] + r[0][2] * p[2] + t[0],
        r[1][0] * p[0] + r[1][1] * p[1] + r[1][2] * p[2] + t[1],
        r[2][0] * p[0] + r[2][1] * p[1] + r[2][2] * p[2] + t[2],
    ]
}

fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let dz = a[2] - b[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}

#[test]
fn ac1_ac2_ac6_cli_mcp_parity_and_exact_replay() {
    let fix = load_fixture();
    let store = fixture_store(&fix);
    let cw = corners(&fix["corners_world"]);
    let cp = corners(&fix["corners_part"]);
    let exp_y_negx = obj(&fix, "expected_y_negx");
    let exp_r2 = mat3(&exp_y_negx["r"]);
    let exp_t2 = num_vec3(&exp_y_negx["t"]);
    let empty: BTreeSet<Uuid> = BTreeSet::new();

    // AC-1 via CLI (`fid align run-321 --primary top --secondary front --tertiary end --json`).
    let mut m1 = AlignmentModel::new();
    let mut e1 = EdgeIndex::new();
    let mut r1 = ResidualStore::new();
    let mut src1 = replay_source(8);
    let mut ids1 = IdService::new(Uuid::from_u128(1), &mut src1, &empty);
    let cli_commit = invoke_321_cli(
        &mut m1,
        &mut e1,
        &mut r1,
        &store,
        &mut ids1,
        ("operator:alice", PrincipalKind::Human),
        &["run-321", "--primary", "top", "--secondary", "front", "--tertiary", "end", "--json"],
    )
    .unwrap();
    let rot = cli_commit.solution.transform.r();
    let tr = cli_commit.solution.transform.t();
    for (pw, pp) in cw.iter().zip(cp.iter()) {
        assert!(dist(apply(&rot, tr, *pw), *pp) <= 1e-12);
    }
    assert_eq!(m1.len(), 1);
    assert!(r1.get(&cli_commit.alignment.report().id).is_some());

    // MCP parity produces identical canonical bytes with the same ID source.
    let mut m2 = AlignmentModel::new();
    let mut e2 = EdgeIndex::new();
    let mut r2 = ResidualStore::new();
    let mut src2 = replay_source(8);
    let mut ids2 = IdService::new(Uuid::from_u128(1), &mut src2, &empty);
    let mcp_commit = invoke_321_mcp(
        &mut m2,
        &mut e2,
        &mut r2,
        &store,
        &mut ids2,
        ("operator:alice", PrincipalKind::Human),
        br#"{"primary":"top","secondary":"front","tertiary":"end"}"#,
    )
    .unwrap();
    assert_eq!(cli_commit.to_canonical_bytes().unwrap(), mcp_commit.to_canonical_bytes().unwrap());
    assert_eq!(cli_commit.event.to_canonical_bytes().unwrap(), mcp_commit.event.to_canonical_bytes().unwrap());

    // AC-6: exact replay in a fresh store reproduces bit-identical event and alignment digest.
    let journal_bytes = cli_commit.event.to_canonical_bytes().unwrap();
    let mut mr = AlignmentModel::new();
    let mut er = EdgeIndex::new();
    let mut rr = ResidualStore::new();
    let replayed = replay_321(&mut mr, &mut er, &mut rr, &store, &journal_bytes).unwrap();
    assert_eq!(replayed.alignment.digest(), cli_commit.alignment.digest());
    assert_eq!(replayed.event.to_canonical_bytes().unwrap(), journal_bytes);

    // AC-2: primary +Y, secondary -X via CLI.
    let c2 = invoke_321_cli(
        &mut m1,
        &mut e1,
        &mut r1,
        &store,
        &mut ids1,
        ("operator:alice", PrincipalKind::Human),
        &[
            "run-321",
            "--primary",
            "top",
            "--secondary",
            "front",
            "--tertiary",
            "end",
            "--primary-axis",
            "+Y",
            "--secondary-axis",
            "-X",
            "--json",
        ],
    )
    .unwrap();
    let rot2 = c2.solution.transform.r();
    let tr2 = c2.solution.transform.t();
    for pw in &cw {
        assert!(dist(apply(&rot2, tr2, *pw), apply(&exp_r2, exp_t2, *pw)) <= 1e-12);
    }
}

#[test]
fn ac3_ac4_ac5_errors_journal_nothing_and_three_points() {
    let fix = load_fixture();
    let store = fixture_store(&fix);
    let cw = corners(&fix["corners_world"]);
    let cp = corners(&fix["corners_part"]);
    let pts = corners(&fix["three_points_world"]);
    let empty: BTreeSet<Uuid> = BTreeSet::new();

    let mut m = AlignmentModel::new();
    let mut e = EdgeIndex::new();
    let mut r = ResidualStore::new();
    let mut src = replay_source(99);
    let mut ids = IdService::new(Uuid::from_u128(2), &mut src, &empty);

    // AC-3: secondary parallel to primary fails & mutates nothing.
    let err = invoke_321_cli(
        &mut m,
        &mut e,
        &mut r,
        &store,
        &mut ids,
        ("operator:bob", PrincipalKind::Human),
        &["run-321", "--primary", "top", "--secondary", "parallel", "--tertiary", "end", "--json"],
    )
    .unwrap_err();
    assert_eq!(err.code(), "secondary_parallel_to_primary");
    assert_eq!(m.len(), 0);

    // AC-4: tertiary on secondary line fails & mutates nothing.
    let err = invoke_321_cli(
        &mut m,
        &mut e,
        &mut r,
        &store,
        &mut ids,
        ("operator:bob", PrincipalKind::Human),
        &["run-321", "--primary", "top", "--secondary", "front", "--tertiary", "on_front", "--json"],
    )
    .unwrap_err();
    assert_eq!(err.code(), "tertiary_on_secondary");
    assert_eq!(m.len(), 0);

    // AC-5: three_points mode via CLI.
    let pts_arg = format!(
        "{},{},{};{},{},{};{},{},{}",
        pts[0][0], pts[0][1], pts[0][2], pts[1][0], pts[1][1], pts[1][2], pts[2][0], pts[2][1], pts[2][2]
    );
    let c3 = invoke_321_cli(
        &mut m,
        &mut e,
        &mut r,
        &store,
        &mut ids,
        ("operator:bob", PrincipalKind::Human),
        &["run-321", "--mode", "three_points", "--points", &pts_arg, "--json"],
    )
    .unwrap();
    for (pw, pp) in cw.iter().zip(cp.iter()) {
        assert!(dist(apply(&c3.solution.transform.r(), c3.solution.transform.t(), *pw), *pp) <= 1e-12);
    }

    // Collinear three_points fails with `collinear_points` and mutates nothing.
    let before = m.len();
    let err = run_321(
        &mut m,
        &mut e,
        &mut r,
        &store,
        &mut ids,
        &Run321Params {
            mode: Run321Mode::ThreePoints { points: [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]] },
            primary_axis: SignedAxis::PosX,
            secondary_axis: SignedAxis::PosY,
            tolerances: Rps321Tolerances::default(),
            parent: None,
            principal: "operator:bob".to_owned(),
            principal_kind: PrincipalKind::Human,
        },
    )
    .unwrap_err();
    assert_eq!(err.code(), "collinear_points");
    assert_eq!(m.len(), before);

    // CommandDef schema validates the emitted result value.
    let def = run_321_command_def().unwrap();
    assert_eq!(format!("{}@{}", def.id, def.major), RUN_321);
    let compiled_result = Compiled::compile(&def.result_schema).unwrap();
    compiled_result.evaluate(&c3.to_value(), &NoHooks).unwrap();
}
