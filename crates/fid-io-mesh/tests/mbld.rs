#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Acceptance and edge-case tests for `IO.MESH.MBLD` (`fid-io-mesh`).

use fid_canon::{parse, CValue};
use fid_geo_types::{digest_mesh, FrameRef};
use fid_io_mesh::{
    count_non_manifold_edges, f32_rounding_report, f32_rounding_report_point_set, BuiltGeometry, CustomScalarType,
    MeshBuildError, MeshBuilder, MeshImportCounts, PrecisionReport, ScalarColumn, SourcePrecision,
};
use fid_io_reg::{Budget, IoError, LimitKind, LimitRequest, Position, Principal};
use std::path::PathBuf;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/io/mesh/mbld").join(name)
}

fn load_json(name: &str) -> CValue {
    let bytes = std::fs::read(fixture_path(name)).expect("read fixture");
    parse(&bytes).expect("parse fixture json")
}

fn field<'a>(v: &'a CValue, key: &str) -> &'a CValue {
    match v {
        CValue::Obj(map) => map.get(key).unwrap_or_else(|| panic!("missing field {key}")),
        other => panic!("expected object, got {other:?}"),
    }
}

fn as_array(v: &CValue) -> &[CValue] {
    match v {
        CValue::Arr(items) => items,
        other => panic!("expected array, got {other:?}"),
    }
}

fn as_f64(v: &CValue) -> f64 {
    match v {
        CValue::Num(n) => *n,
        other => panic!("expected number, got {other:?}"),
    }
}

fn as_u64(v: &CValue) -> u64 {
    let f = as_f64(v);
    assert!(f >= 0.0 && f.fract() == 0.0, "expected non-negative integer, got {f}");
    f as u64
}

fn load_fixture_into_builder(name: &str, budget: &mut Budget) -> (MeshBuilder, Vec<[f64; 3]>, Vec<[u64; 3]>) {
    let doc = load_json(name);
    let precision = match field(&doc, "source_precision") {
        CValue::Str(s) if s == "f32" => SourcePrecision::F32,
        CValue::Str(s) if s == "f64" => SourcePrecision::F64,
        other => panic!("unexpected source_precision: {other:?}"),
    };
    let mut builder = MeshBuilder::new(precision);

    let mut verts = Vec::new();
    for item in as_array(field(&doc, "vertices_m")) {
        let coords = as_array(item);
        let p = [as_f64(&coords[0]), as_f64(&coords[1]), as_f64(&coords[2])];
        builder.push_vertex(p, budget, Position::byte(0)).expect("push vertex");
        verts.push(p);
    }

    let mut faces = Vec::new();
    for item in as_array(field(&doc, "faces")) {
        let idxs = as_array(item);
        let f = [as_u64(&idxs[0]), as_u64(&idxs[1]), as_u64(&idxs[2])];
        builder.push_face_u64(f, budget, Position::byte(0)).expect("push face");
        faces.push(f);
    }

    (builder, verts, faces)
}

fn test_frame() -> FrameRef {
    FrameRef::from_bytes([0x42; 16])
}

#[test]
fn ac1_tetra_plus_degenerate_keeps_order_and_counts_defects() {
    let mut budget = Budget::new(Principal::Human, &LimitRequest::default()).unwrap();
    let (builder, verts, faces) = load_fixture_into_builder("tetra_plus_degenerate.json", &mut budget);

    let standalone_nm = count_non_manifold_edges(&faces, &mut budget, Position::byte(0)).unwrap();
    let built = builder.build_mesh(test_frame(), &mut budget, Position::byte(0)).unwrap();

    assert_eq!(built.source_precision, SourcePrecision::F64);
    assert_eq!(
        built.counts,
        MeshImportCounts {
            vertices: 5,
            faces: 6,
            degenerate_faces: 1,
            non_manifold_edges: 1,
            unreferenced_vertices: 0,
        }
    );
    assert_eq!(standalone_nm, built.counts.non_manifold_edges);
    assert_eq!(built.mesh.facts().nonmanifold_edges, 1);

    // REQ-IO.MESH.MBLD-001: exact vertex and face order preserved, nothing repaired or dropped.
    let got_verts: Vec<[f64; 3]> = (0..built.mesh.vertex_count()).map(|i| built.mesh.vertex(i).unwrap()).collect();
    let got_faces: Vec<[u64; 3]> = built.mesh.triangles().collect();
    assert_eq!(got_verts, verts);
    assert_eq!(got_faces, faces);
}

#[test]
fn ac1_open_tetra_five_faces_companion_counts() {
    let mut budget = Budget::new(Principal::Human, &LimitRequest::default()).unwrap();
    let (builder, verts, faces) = load_fixture_into_builder("open_tetra_five_faces.json", &mut budget);
    let built = builder.build_mesh(test_frame(), &mut budget, Position::byte(0)).unwrap();

    assert_eq!(
        built.counts,
        MeshImportCounts {
            vertices: 5,
            faces: 5,
            degenerate_faces: 1,
            non_manifold_edges: 1,
            unreferenced_vertices: 0,
        }
    );
    assert_eq!(built.mesh.triangles().collect::<Vec<_>>(), faces);
    assert_eq!(built.mesh.vertex_count(), verts.len() as u64);
}

#[test]
fn ac1_zero_area_collinear_and_unreferenced_vertices() {
    let mut budget = Budget::new(Principal::Human, &LimitRequest::default()).unwrap();
    let (mut builder, _verts, faces) = load_fixture_into_builder("zero_area_and_unreferenced.json", &mut budget);

    // Attach standard colours and a custom f64 scalar property for all 6 vertices.
    for i in 0..6u8 {
        builder.attrs_mut().push_colour([i * 10, i * 20, i * 30], &mut budget, Position::byte(0)).unwrap();
        builder
            .attrs_mut()
            .push_custom_f64("temperature_c", 20.0 + f64::from(i) * 0.25, &mut budget, Position::byte(0))
            .unwrap();
    }

    let built = builder.build_mesh(test_frame(), &mut budget, Position::byte(0)).unwrap();
    assert_eq!(
        built.counts,
        MeshImportCounts {
            vertices: 6,
            faces: 2,
            degenerate_faces: 1, // collinear face [0, 1, 2] has exact zero cross product
            non_manifold_edges: 0,
            unreferenced_vertices: 2, // vertices 4 and 5
        }
    );
    assert_eq!(built.mesh.triangles().collect::<Vec<_>>(), faces);
    assert_eq!(built.mesh.vertex_attrs().colour.as_ref().map(Vec::len), Some(6));
    assert_eq!(
        built.custom_attrs.get("temperature_c"),
        Some(&ScalarColumn::F64(vec![20.0, 20.25, 20.5, 20.75, 21.0, 21.25]))
    );
    assert_eq!(built.custom_attrs["temperature_c"].scalar_type(), CustomScalarType::F64);
}

#[test]
fn ac2_out_of_range_vertex_index_fails_malformed_naming_face() {
    let mut budget = Budget::new(Principal::Human, &LimitRequest::default()).unwrap();
    let (builder, _, _) = load_fixture_into_builder("bad_index_face4.json", &mut budget);

    let err = builder.build_mesh(test_frame(), &mut budget, Position::byte(128)).unwrap_err();
    assert_eq!(err, MeshBuildError::MalformedFace { face: 4, vertex_index: 9, vertex_count: 4 });
    assert_eq!(err.code(), "malformed");
    assert_eq!(err.face(), Some(4));
    assert!(err.to_string().contains("malformed{face: 4}"), "display was: {err}");
    assert_eq!(
        err.to_io_error(Position::byte(128)),
        IoError::Malformed { at: Position::byte(128), reason: "face_index_out_of_range" }
    );
}

#[test]
fn ac3_f32_rounding_report_exact_in_f64() {
    let mut budget = Budget::new(Principal::Human, &LimitRequest::default()).unwrap();
    let (builder, _, _) = load_fixture_into_builder("precision_vertex.json", &mut budget);

    let geom = builder.build(test_frame(), &mut budget, Position::byte(0)).unwrap();
    let BuiltGeometry::PointSet(built_ps) = geom else {
        panic!("expected vertex-only fixture to build a PointSet");
    };
    let report = f32_rounding_report_point_set(&built_ps.points);
    let expected = (f64::from(5.000000123_f64 as f32) - 5.000000123_f64).abs();
    assert_eq!(report, PrecisionReport { max_abs_m: expected });
    assert_eq!(report.max_abs_m.to_bits(), expected.to_bits());
    assert!(report.max_abs_m > 0.0);

    // Also verify on a Mesh containing the same vertex plus exact f32 coordinates.
    let mut b2 = MeshBuilder::new(SourcePrecision::F64);
    b2.push_vertex([5.000000123, 0.0, 0.0], &mut budget, Position::byte(0)).unwrap();
    b2.push_vertex([1.0, 0.0, 0.0], &mut budget, Position::byte(0)).unwrap();
    b2.push_vertex([0.0, 2.0, 0.0], &mut budget, Position::byte(0)).unwrap();
    b2.push_face([0, 1, 2], &mut budget, Position::byte(0)).unwrap();
    let built_mesh = b2.build_mesh(test_frame(), &mut budget, Position::byte(0)).unwrap();
    assert_eq!(f32_rounding_report(&built_mesh.mesh).max_abs_m.to_bits(), expected.to_bits());

    // Empty mesh has 0.0 rounding error.
    let empty_mesh =
        MeshBuilder::new(SourcePrecision::F32).build_mesh(test_frame(), &mut budget, Position::byte(0)).unwrap();
    assert_eq!(f32_rounding_report(&empty_mesh.mesh), PrecisionReport { max_abs_m: 0.0 });
}

/// Deterministic seeded grid generator (LCG seed 7) for AC-4.
fn feed_seeded_grid(
    builder: &mut MeshBuilder,
    grid_side: u32,
    seed: u64,
    budget: &mut Budget,
) -> Result<(), MeshBuildError> {
    let mut state = seed;
    let mut next_jitter = || -> f64 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((state >> 40) as f64) * 1e-10
    };

    let v_side = grid_side + 1;
    for iy in 0..v_side {
        for ix in 0..v_side {
            builder.push_vertex(
                [f64::from(ix) * 1e-3, f64::from(iy) * 1e-3, next_jitter()],
                budget,
                Position::byte(0),
            )?;
        }
    }
    for iy in 0..grid_side {
        for ix in 0..grid_side {
            let v00 = iy * v_side + ix;
            let v10 = v00 + 1;
            let v01 = v00 + v_side;
            let v11 = v01 + 1;
            builder.push_face([v00, v10, v11], budget, Position::byte(0))?;
            builder.push_face([v00, v11, v01], budget, Position::byte(0))?;
        }
    }
    Ok(())
}

#[test]
fn ac4_million_face_grid_hits_element_limit() {
    // 708 x 708 cells x 2 triangles = 1,002,528 faces (> 10^6 faces), seed 7.
    let req = LimitRequest { elements: Some(100_000), ..LimitRequest::default() };

    // Path 1: incremental streaming push stops cleanly at limit_exceeded{limit: elements}.
    let mut budget = Budget::new(Principal::Human, &req).unwrap();
    let mut builder = MeshBuilder::new(SourcePrecision::F32);
    let err = feed_seeded_grid(&mut builder, 708, 7, &mut budget).unwrap_err();
    match err {
        MeshBuildError::Io(IoError::LimitExceeded { limit, value, .. }) => {
            assert_eq!(limit, LimitKind::Elements);
            assert_eq!(value, 100_000);
        }
        other => panic!("expected limit_exceeded(elements), got {other:?}"),
    }
    assert_eq!(err.code(), "limit_exceeded");

    // Path 2: upfront header count reservation (10^6 faces) fails before allocating.
    let mut budget2 = Budget::new(Principal::Human, &req).unwrap();
    let mut builder2 = MeshBuilder::new(SourcePrecision::F32);
    let err2 = builder2.reserve_faces(1_000_000, &mut budget2, Position::byte(84)).unwrap_err();
    match err2 {
        MeshBuildError::Io(IoError::LimitExceeded { limit, value, at }) => {
            assert_eq!(limit, LimitKind::Elements);
            assert_eq!(value, 100_000);
            assert_eq!(at, Position::byte(84));
        }
        other => panic!("expected limit_exceeded(elements), got {other:?}"),
    }
}

#[test]
fn f32_widening_before_unit_conversion_and_determinism() {
    let mut b1 = Budget::new(Principal::Human, &LimitRequest::default()).unwrap();
    let mut b2 = Budget::new(Principal::Human, &LimitRequest::default()).unwrap();

    let build_once = |budget: &mut Budget| {
        let mut builder = MeshBuilder::new(SourcePrecision::F32);
        builder.reserve_vertices(3, budget, Position::byte(84)).unwrap();
        builder.reserve_faces(1, budget, Position::byte(84)).unwrap();
        let scale_mm = 1e-3_f64;
        builder.push_vertex_f32([10.5_f32, 0.0, 0.0], scale_mm, budget, Position::byte(84)).unwrap();
        builder.push_vertex_f32([0.0, 20.25_f32, 0.0], scale_mm, budget, Position::byte(96)).unwrap();
        builder.push_vertex_f32([0.0, 0.0, 30.125_f32], scale_mm, budget, Position::byte(108)).unwrap();
        builder.push_face([0, 1, 2], budget, Position::byte(120)).unwrap();
        builder.build_mesh(test_frame(), budget, Position::byte(134)).unwrap()
    };

    let m1 = build_once(&mut b1);
    let m2 = build_once(&mut b2);

    assert_eq!(m1.source_precision, SourcePrecision::F32);
    assert_eq!(m1.source_precision.as_str(), "f32");
    assert_eq!(m1.mesh.vertex(0), Some([f64::from(10.5_f32) * 1e-3, 0.0, 0.0]));
    assert_eq!(digest_mesh(&m1.mesh), digest_mesh(&m2.mesh));
    // Pre-charged 3 vertices + 1 face must not be double-charged.
    assert_eq!(b1.elements_used(), 4);
}

#[test]
fn error_paths_non_finite_attr_mismatch_and_alloc_limit() {
    let mut budget = Budget::new(Principal::Human, &LimitRequest::default()).unwrap();
    let mut builder = MeshBuilder::new(SourcePrecision::F64);
    assert_eq!(
        builder.push_vertex([f64::NAN, 0.0, 0.0], &mut budget, Position::byte(0)),
        Err(MeshBuildError::NonFiniteVertex { vertex: 0 })
    );

    builder.push_vertex([0.0, 0.0, 0.0], &mut budget, Position::byte(0)).unwrap();
    builder.push_vertex([1.0, 0.0, 0.0], &mut budget, Position::byte(0)).unwrap();
    builder.attrs_mut().push_custom_f64("roughness", 0.5, &mut budget, Position::byte(0)).unwrap();
    assert_eq!(
        builder.attrs_mut().push_custom_f32("roughness", 0.5, &mut budget, Position::byte(0)),
        Err(MeshBuildError::AttrTypeMismatch { name: "roughness".to_string() })
    );
    let err = builder.build_point_set(test_frame()).unwrap_err();
    assert_eq!(err, MeshBuildError::AttrLengthMismatch { name: "roughness".to_string(), expected: 2, found: 1 });

    // Alloc budget limit triggers cleanly before large reservation.
    let tiny_alloc = LimitRequest { alloc: Some(128), ..LimitRequest::default() };
    let mut small_budget = Budget::new(Principal::Human, &tiny_alloc).unwrap();
    let mut b3 = MeshBuilder::new(SourcePrecision::F64);
    let err = b3.reserve_vertices(100, &mut small_budget, Position::byte(0)).unwrap_err();
    match err {
        MeshBuildError::Io(IoError::LimitExceeded { limit, .. }) => assert_eq!(limit, LimitKind::Alloc),
        other => panic!("expected alloc limit_exceeded, got {other:?}"),
    }
}
