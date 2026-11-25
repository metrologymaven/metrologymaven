//! Acceptance tests for `ALN.XFM.MATRIX` (`align.set_matrix@1`): AC-1..AC-7.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_align::{
    invoke_set_matrix_cli, invoke_set_matrix_mcp, replay_set_matrix, set_matrix_command_def, AlignmentModel,
    PrincipalKind, ResidualStore, SetMatrixError, CLI_SET_MATRIX, MCP_SET_MATRIX,
};
use fid_canon::CValue;
use fid_dep::EdgeIndex;
use fid_geo_frame::Transform;
use fid_project::{Bits, IdService, Observation, ReplaySource};
use fid_uuid::Uuid;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const TOL: f64 = 1e-14;

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

fn st(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        _ => panic!("expected string"),
    }
}

fn num(v: &CValue) -> f64 {
    match v {
        CValue::Num(n) => *n,
        _ => panic!("expected number"),
    }
}

fn mat3(v: &CValue) -> [[f64; 3]; 3] {
    let rows = arr(v);
    let mut out = [[0.0; 3]; 3];
    for i in 0..3 {
        let cols = arr(&rows[i]);
        for j in 0..3 {
            out[i][j] = num(&cols[j]);
        }
    }
    out
}

fn vec3(v: &CValue) -> [f64; 3] {
    let a = arr(v);
    [num(&a[0]), num(&a[1]), num(&a[2])]
}

fn assert_transform_close(got: &Transform, exp_r: [[f64; 3]; 3], exp_t: [f64; 3], label: &str) {
    let gr = got.rotation();
    let gt = got.translation();
    for i in 0..3 {
        for j in 0..3 {
            assert!((gr[i][j] - exp_r[i][j]).abs() <= TOL, "{label}: R[{i}][{j}] got={} exp={}", gr[i][j], exp_r[i][j]);
        }
        assert!((gt[i] - exp_t[i]).abs() <= TOL, "{label}: t[{i}] got={} exp={}", gt[i], exp_t[i]);
    }
}

fn make_src(n: usize) -> ReplaySource {
    let mut obs = Vec::with_capacity(n * 2);
    for i in 0..n {
        obs.push(Observation::NowMs(1_763_600_000_000 + i as u64));
        obs.push(Observation::RandomBits(Bits::new(74, 0x100 + i as u128).unwrap()));
    }
    ReplaySource::new(obs)
}

#[test]
fn ac1_ac2_ac7_every_form_and_all_24_euler_conventions_plus_exact_replay() {
    let def = set_matrix_command_def().unwrap();
    assert_eq!(def.id, "align.set_matrix");
    assert_eq!(def.major, 1);

    let doc = fid_canon::parse(&std::fs::read(fix("matrices.json")).unwrap()).unwrap();
    let root = obj(&doc);

    let mut model = AlignmentModel::new();
    let mut edges = EdgeIndex::new();
    let mut reports = ResidualStore::new();
    let mut src = make_src(40);
    let existing = BTreeSet::new();
    let mut ids = IdService::new(Uuid::from_u128(1), &mut src, &existing);
    let mut journal: Vec<Vec<u8>> = Vec::new();

    // 1. Matrix4 row-major via CLI.
    let m_row = obj(&root["matrix4_row_major"]);
    let row_csv: String = arr(&m_row["entries"]).iter().map(st).collect::<Vec<_>>().join(",");
    let c_row = invoke_set_matrix_cli(
        &mut model,
        &mut edges,
        &mut reports,
        &mut ids,
        "alice",
        PrincipalKind::Human,
        &[
            CLI_SET_MATRIX,
            "--form",
            "matrix4",
            "--order",
            "row_major",
            "--matrix",
            &row_csv,
            "--length-unit",
            "mm",
            "--direction",
            "data_to_reference",
            "--source",
            "CMM fixture plate #1",
            "--json",
        ],
    )
    .unwrap();
    assert_transform_close(
        c_row.alignment.transform(),
        mat3(&m_row["expected_r"]),
        vec3(&m_row["expected_t_m"]),
        "row_major",
    );
    assert_eq!(c_row.event.conversions.len(), 3);
    assert_eq!(c_row.event.source_untrusted, "CMM fixture plate #1");
    journal.push(c_row.event.to_canonical_bytes().unwrap());

    // 2. Matrix4 column-major via MCP.
    let m_col = obj(&root["matrix4_col_major"]);
    let col_entries: Vec<String> = arr(&m_col["entries"]).iter().map(|v| format!(r#""{}""#, st(v))).collect();
    let col_json = format!(
        r#"{{"direction":"data_to_reference","form":"matrix4","length_unit":"mm","matrix":[{}],"order":"column_major","source":"col_major"}}"#,
        col_entries.join(",")
    );
    let c_col = invoke_set_matrix_mcp(
        &mut model,
        &mut edges,
        &mut reports,
        &mut ids,
        ("alice", PrincipalKind::Human),
        MCP_SET_MATRIX,
        col_json.as_bytes(),
    )
    .unwrap();
    assert_transform_close(
        c_col.alignment.transform(),
        mat3(&m_col["expected_r"]),
        vec3(&m_col["expected_t_m"]),
        "col_major",
    );
    journal.push(c_col.event.to_canonical_bytes().unwrap());

    // 3. Quaternion via CLI.
    let q = obj(&root["quaternion"]);
    let q_csv = format!("{},{},{},{}", st(&q["w"]), st(&q["x"]), st(&q["y"]), st(&q["z"]));
    let q_t: String = arr(&q["translation_mm"]).iter().map(st).collect::<Vec<_>>().join(",");
    let c_q = invoke_set_matrix_cli(
        &mut model,
        &mut edges,
        &mut reports,
        &mut ids,
        "alice",
        PrincipalKind::Human,
        &[
            CLI_SET_MATRIX,
            "--form",
            "trs",
            "--translation",
            &q_t,
            "--length-unit",
            "mm",
            "--rotation-kind",
            "quaternion",
            "--quat",
            &q_csv,
            "--direction",
            "data_to_reference",
            "--json",
        ],
    )
    .unwrap();
    assert_transform_close(c_q.alignment.transform(), mat3(&q["expected_r"]), vec3(&q["expected_t_m"]), "quaternion");
    journal.push(c_q.event.to_canonical_bytes().unwrap());

    // 4. Axis-angle via CLI (mm + deg -> 4 conversion records).
    let aa = obj(&root["axis_angle"]);
    let aa_ax: String = arr(&aa["axis"]).iter().map(st).collect::<Vec<_>>().join(",");
    let aa_t: String = arr(&aa["translation_mm"]).iter().map(st).collect::<Vec<_>>().join(",");
    let c_aa = invoke_set_matrix_cli(
        &mut model,
        &mut edges,
        &mut reports,
        &mut ids,
        "alice",
        PrincipalKind::Human,
        &[
            CLI_SET_MATRIX,
            "--form",
            "trs",
            "--translation",
            &aa_t,
            "--length-unit",
            "mm",
            "--rotation-kind",
            "axis_angle",
            "--axis",
            &aa_ax,
            "--angle",
            st(&aa["angle_deg"]),
            "--angle-unit",
            "deg",
            "--direction",
            "data_to_reference",
            "--json",
        ],
    )
    .unwrap();
    assert_transform_close(
        c_aa.alignment.transform(),
        mat3(&aa["expected_r"]),
        vec3(&aa["expected_t_m"]),
        "axis_angle",
    );
    assert_eq!(c_aa.event.conversions.len(), 4);
    journal.push(c_aa.event.to_canonical_bytes().unwrap());

    // 5. All 24 Euler cases (12 sequences × intrinsic/extrinsic) + AC-2 unit/conversion recording.
    for case in arr(&root["euler_cases"]) {
        let eo = obj(case);
        let name = st(&eo["name"]);
        let seq = st(&eo["sequence"]);
        let ek = st(&eo["euler_kind"]);
        let angs: String = arr(&eo["angles_deg"]).iter().map(st).collect::<Vec<_>>().join(",");
        let tr: String = arr(&eo["translation_mm"]).iter().map(st).collect::<Vec<_>>().join(",");
        let c_e = invoke_set_matrix_cli(
            &mut model,
            &mut edges,
            &mut reports,
            &mut ids,
            "alice",
            PrincipalKind::Human,
            &[
                CLI_SET_MATRIX,
                "--form",
                "trs",
                "--translation",
                &tr,
                "--length-unit",
                "mm",
                "--rotation-kind",
                "euler",
                "--euler-seq",
                seq,
                "--euler-kind",
                ek,
                "--angles",
                &angs,
                "--angle-unit",
                "deg",
                "--direction",
                "data_to_reference",
                "--json",
            ],
        )
        .unwrap();
        assert_transform_close(c_e.alignment.transform(), mat3(&eo["expected_r"]), vec3(&eo["expected_t_m"]), name);
        // AC-2: 3 mm translations + 3 deg angles = 6 single-rounding conversion records.
        assert_eq!(c_e.event.conversions.len(), 6, "{name}");
        assert_eq!(c_e.event.length_unit, "mm");
        assert_eq!(c_e.event.angle_unit.as_deref(), Some("deg"));
        journal.push(c_e.event.to_canonical_bytes().unwrap());
    }

    // AC-7: exact replay of every committed event reproduces identical alignments and digests.
    let mut r_model = AlignmentModel::new();
    let mut r_edges = EdgeIndex::new();
    let mut r_reports = ResidualStore::new();
    for raw in &journal {
        let rc = replay_set_matrix(&mut r_model, &mut r_edges, &mut r_reports, raw).unwrap();
        let orig = model.get(&rc.alignment.id()).unwrap();
        assert_eq!(rc.alignment.digest(), orig.digest());
        assert_eq!(rc.alignment.transform(), orig.transform());
    }
    assert_eq!(r_model.len(), journal.len());
}

#[test]
fn ac3_ac4_error_paths_journal_nothing() {
    let mut model = AlignmentModel::new();
    let mut edges = EdgeIndex::new();
    let mut reports = ResidualStore::new();
    let mut src = make_src(5);
    let existing = BTreeSet::new();
    let mut ids = IdService::new(Uuid::from_u128(1), &mut src, &existing);

    // AC-3: orthonormality error of 1e-9 -> not_rigid, draws no UUID, mutates nothing.
    let e_ortho = invoke_set_matrix_cli(
        &mut model,
        &mut edges,
        &mut reports,
        &mut ids,
        "alice",
        PrincipalKind::Human,
        &[
            CLI_SET_MATRIX,
            "--form",
            "matrix4",
            "--order",
            "row_major",
            "--matrix",
            "1,1e-9,0,0,0,1,0,0,0,0,1,0,0,0,0,1",
            "--direction",
            "data_to_reference",
        ],
    )
    .unwrap_err();
    assert_eq!(e_ortho.code(), "not_rigid");
    assert!(matches!(e_ortho, SetMatrixError::NotRigid { .. }));

    // AC-4: reflection matrix (det = -1) -> not_rigid.
    let e_refl = invoke_set_matrix_cli(
        &mut model,
        &mut edges,
        &mut reports,
        &mut ids,
        "alice",
        PrincipalKind::Human,
        &[
            CLI_SET_MATRIX,
            "--form",
            "matrix4",
            "--order",
            "row_major",
            "--matrix",
            "-1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1",
            "--direction",
            "data_to_reference",
        ],
    )
    .unwrap_err();
    assert_eq!(e_refl.code(), "not_rigid");

    // REQ-004: bottom row not (0, 0, 0, 1) -> not_homogeneous.
    let e_hom = invoke_set_matrix_cli(
        &mut model,
        &mut edges,
        &mut reports,
        &mut ids,
        "alice",
        PrincipalKind::Human,
        &[
            CLI_SET_MATRIX,
            "--form",
            "matrix4",
            "--order",
            "row_major",
            "--matrix",
            "1,0,0,0,0,1,0,0,0,0,1,0,0,0,1e-9,1",
            "--direction",
            "data_to_reference",
        ],
    )
    .unwrap_err();
    assert_eq!(e_hom.code(), "not_homogeneous");

    // REQ-001: missing direction -> convention_unspecified.
    let e_conv = invoke_set_matrix_cli(
        &mut model,
        &mut edges,
        &mut reports,
        &mut ids,
        "alice",
        PrincipalKind::Human,
        &[CLI_SET_MATRIX, "--form", "matrix4", "--order", "row_major", "--matrix", "1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1"],
    )
    .unwrap_err();
    assert_eq!(e_conv.code(), "convention_unspecified");

    assert!(model.is_empty());
}

#[test]
fn ac5_ac6_uniform_scale_and_reference_to_data_inverse() {
    let doc = fid_canon::parse(&std::fs::read(fix("matrices.json")).unwrap()).unwrap();
    let root = obj(&doc);
    let sc = obj(&root["scaled_matrix4"]);
    let sc_csv: String = arr(&sc["entries"]).iter().map(st).collect::<Vec<_>>().join(",");

    let mut model = AlignmentModel::new();
    let mut edges = EdgeIndex::new();
    let mut reports = ResidualStore::new();
    let mut src = make_src(5);
    let existing = BTreeSet::new();
    let mut ids = IdService::new(Uuid::from_u128(1), &mut src, &existing);

    // AC-5: uniform scale 1.0001 with --scale.
    let c_sc = invoke_set_matrix_cli(
        &mut model,
        &mut edges,
        &mut reports,
        &mut ids,
        "alice",
        PrincipalKind::Human,
        &[
            CLI_SET_MATRIX,
            "--form",
            "matrix4",
            "--order",
            "row_major",
            "--matrix",
            &sc_csv,
            "--scale",
            "--direction",
            "data_to_reference",
            "--json",
        ],
    )
    .unwrap();
    assert!((c_sc.alignment.transform().scale() - 1.0001).abs() / 1.0001 <= 1e-14);
    assert_transform_close(c_sc.alignment.transform(), mat3(&sc["expected_r"]), vec3(&sc["expected_t_m"]), "scaled");

    // AC-6: direction reference_to_data composes with forward transform to identity within 1e-15.
    let m_row = obj(&root["matrix4_row_major"]);
    let row_csv: String = arr(&m_row["entries"]).iter().map(st).collect::<Vec<_>>().join(",");
    let c_fwd = invoke_set_matrix_cli(
        &mut model,
        &mut edges,
        &mut reports,
        &mut ids,
        "alice",
        PrincipalKind::Human,
        &[
            CLI_SET_MATRIX,
            "--form",
            "matrix4",
            "--order",
            "row_major",
            "--matrix",
            &row_csv,
            "--length-unit",
            "mm",
            "--direction",
            "data_to_reference",
        ],
    )
    .unwrap();
    let c_inv = invoke_set_matrix_cli(
        &mut model,
        &mut edges,
        &mut reports,
        &mut ids,
        "alice",
        PrincipalKind::Human,
        &[
            CLI_SET_MATRIX,
            "--form",
            "matrix4",
            "--order",
            "row_major",
            "--matrix",
            &row_csv,
            "--length-unit",
            "mm",
            "--direction",
            "reference_to_data",
        ],
    )
    .unwrap();
    let prod = c_inv.alignment.transform().compose(c_fwd.alignment.transform()).unwrap();
    assert!(prod.approx_eq(&Transform::IDENTITY, 1e-15), "inverse * forward must be identity");
}
