//! Integration tests for `IO.MESH.PLY` (AC-1 through AC-7 + `fid-io-reg` skeleton, conformance & replay).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{CValue, Digest as CanonDigest};
use fid_geo_frame::LengthUnit;
use fid_geo_types::{FrameRef, Mesh, PointSet};
use fid_io_mesh::attrs::ScalarColumn;
use fid_io_mesh::ply::{
    export_mesh_ply_command, import_mesh_ply_command, ply_command_defs, ply_format_id, read_ply_bytes, register_ply,
    write_ply, PlyExportEncoding, PlyExportOptions, PlyExportPrecision, PlyFormat, PLY_POINT_SET_ELEMENT_TYPE,
};
use fid_io_mesh::stl::STL_ELEMENT_TYPE;
use fid_io_mesh::{BuiltGeometry, MeshBuilder, SourcePrecision};
use fid_io_reg::detect::Registry;
use fid_io_reg::env::export::{
    conformance as export_conformance, ByteSink, ExportEnv, ExportParams, ExportRun, SourceRef,
};
use fid_io_reg::env::import::{
    import_file_id, register_import_events, BlobSink, ImportEnv, ImportError, ImportParams, ImportRun, MemoryBlobs,
};
use fid_io_reg::prov::{PrincipalCommit, PrincipalKind};
use fid_io_reg::report::TypeName;
use fid_io_reg::roots::{RootAccess, RootId, RootSet};
use fid_io_reg::{Budget, IoError, LimitKind, LimitRequest, Position, Principal};
use fid_project::{ElementTypeDef, ElementTypeRegistry, Tree, Uuid, STAGE_ADDED};
use fid_replay::ReplayOutcome;
use fid_types::Digest;
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/io/mesh/ply/");

fn fixture_bytes(name: &str) -> Vec<u8> {
    fs::read(format!("{FIXTURE_DIR}{name}")).unwrap_or_else(|e| panic!("missing fixture {name}: {e}"))
}

fn frame0() -> FrameRef {
    FrameRef::from_bytes([0u8; 16])
}

fn default_budget() -> Budget {
    Budget::new(Principal::Human, &LimitRequest::default()).unwrap()
}

struct VecSink(Vec<u8>);

impl ByteSink for VecSink {
    fn write_bytes(&mut self, buf: &[u8]) -> Result<(), IoError> {
        self.0.extend_from_slice(buf);
        Ok(())
    }

    fn bytes_written(&self) -> u64 {
        u64::try_from(self.0.len()).unwrap_or(u64::MAX)
    }
}

struct TmpRoots {
    base: PathBuf,
}

impl TmpRoots {
    fn new(tag: &str) -> Self {
        let base = std::env::temp_dir().join(format!("fid-io-mesh-ply-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("in")).unwrap();
        fs::create_dir_all(base.join("out")).unwrap();
        for name in [
            "cube_ascii.ply",
            "cube_bin_le.ply",
            "cube_bin_be.ply",
            "points_double_custom.ply",
            "truncated.ply",
            "with_edges.ply",
        ] {
            fs::copy(format!("{FIXTURE_DIR}{name}"), base.join("in").join(name)).unwrap();
        }
        TmpRoots { base }
    }

    fn roots(&self) -> RootSet {
        let mut s = RootSet::new();
        s.insert(RootId::new("in").unwrap(), RootAccess::Import, self.base.join("in"));
        s.insert(RootId::new("out").unwrap(), RootAccess::Export, self.base.join("out"));
        s
    }

    fn out_dir(&self) -> PathBuf {
        self.base.join("out")
    }
}

impl Drop for TmpRoots {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

fn setup_registry() -> Registry {
    let mut reg = Registry::new();
    register_ply(&mut reg).unwrap();
    reg
}

fn setup_project() -> (Tree, Uuid) {
    let ns = Uuid::parse("33333333-4444-5555-8666-777777777777").unwrap();
    let mut reg = ElementTypeRegistry::new();
    let schema = CValue::Obj(BTreeMap::from([("type".to_string(), CValue::Str("object".to_string()))]));
    reg.register(ElementTypeDef::new(STL_ELEMENT_TYPE, schema.clone(), "mesh").unwrap()).unwrap();
    reg.register(ElementTypeDef::new(PLY_POINT_SET_ELEMENT_TYPE, schema, "point_set").unwrap()).unwrap();
    let imp_cmd = import_mesh_ply_command().unwrap();
    let imp_id = imp_cmd.command_id().unwrap();
    register_import_events(&mut reg, &[&import_file_id().unwrap(), &imp_id]).unwrap();
    let mut tree = Tree::new(ns, Arc::new(reg)).unwrap();
    let run = fid_project::uuid_v5(&ns, b"ply-test-run").to_string();
    let s1 = fid_project::uuid_v5(&ns, b"ply-test-s1").to_string();
    let s2 = fid_project::uuid_v5(&ns, b"ply-test-s2").to_string();
    let default_stage = tree.active_stage().to_string();
    let add_s1 = fid_canon::parse(
        format!(
            r#"{{"adopts":"{default_stage}","id":"{s1}","label":"S1","metadata":{{}},"run":"{run}","serial":null}}"#
        )
        .as_bytes(),
    )
    .unwrap();
    let add_s2 = fid_canon::parse(
        format!(r#"{{"adopts":null,"id":"{s2}","label":"S2","metadata":{{}},"run":"{run}","serial":null}}"#).as_bytes(),
    )
    .unwrap();
    tree.apply(STAGE_ADDED, &add_s1).unwrap();
    tree.apply(STAGE_ADDED, &add_s2).unwrap();
    (tree, ns)
}

fn principal_commit() -> PrincipalCommit {
    PrincipalCommit { kind: PrincipalKind::Agent, commit: CanonDigest([0x8b; 32]), on_behalf_of_commit: None }
}

fn mesh_vertices(mesh: &Mesh) -> Vec<[f64; 3]> {
    (0..mesh.vertex_count()).map(|i| mesh.vertex(i).unwrap()).collect()
}

fn mesh_faces(mesh: &Mesh) -> Vec<[u64; 3]> {
    mesh.triangles().collect()
}

fn point_set_coords(ps: &PointSet) -> Vec<[f64; 3]> {
    (0..ps.len()).map(|i| ps.point(i).unwrap()).collect()
}

#[test]
fn ac1_ascii_bin_le_and_bin_be_cubes_match_identically() {
    let cases = [
        ("cube_ascii.ply", PlyFormat::Ascii),
        ("cube_bin_le.ply", PlyFormat::BinaryLittleEndian),
        ("cube_bin_be.ply", PlyFormat::BinaryBigEndian),
    ];

    let mut ref_verts: Option<Vec<[f64; 3]>> = None;
    let mut ref_faces: Option<Vec<[u64; 3]>> = None;

    for (name, expected_fmt) in cases {
        let bytes = fixture_bytes(name);
        let mut b = default_budget();
        let res = read_ply_bytes(&bytes, LengthUnit::Mm, frame0(), &mut b).unwrap();
        assert_eq!(res.header.format, expected_fmt);
        assert_eq!(res.triangulated_faces, 6, "{name}");
        assert!(res.dropped_elements.is_empty(), "{name}");
        let BuiltGeometry::Mesh(bm) = res.geometry else {
            panic!("{name} should produce a Mesh");
        };
        assert_eq!(bm.counts.vertices, 8);
        assert_eq!(bm.counts.faces, 12);
        assert_eq!(bm.source_precision, SourcePrecision::F32);
        let verts = mesh_vertices(&bm.mesh);
        let faces = mesh_faces(&bm.mesh);
        if let Some(rv) = &ref_verts {
            assert_eq!(&verts, rv, "{name} vertices differ");
            assert_eq!(&faces, ref_faces.as_ref().unwrap(), "{name} faces differ");
        } else {
            ref_verts = Some(verts);
            ref_faces = Some(faces);
        }
    }

    // Verify import.mesh_ply@1 enforces unit_required and records triangulated_faces: 6.
    let reg = setup_registry();
    let tmp = TmpRoots::new("ac1");
    let roots = tmp.roots();
    let (mut tree, ns) = setup_project();
    let cmd = import_mesh_ply_command().unwrap();

    {
        let env = ImportEnv::new(&reg, &roots, Principal::Agent, principal_commit(), &tree, ns);
        let err = cmd.prepare(&env, &ImportParams::new("root://in/cube_bin_le.ply"), None).unwrap_err();
        assert_eq!(err.code(), "unit_required");
    }

    let mut blobs = MemoryBlobs::default();
    let env = ImportEnv::new(&reg, &roots, Principal::Agent, principal_commit(), &tree, ns);
    let mut params = ImportParams::new("root://in/cube_bin_le.ply");
    params.unit = Some(LengthUnit::Mm);
    let out = cmd.prepare(&env, &params, Some(&mut blobs as &mut dyn BlobSink)).unwrap().commit(&mut tree).unwrap();
    let counts = out.report.entity_counts();
    assert_eq!(counts.get(&TypeName::new("mesh.vertex").unwrap()).copied(), Some(8));
    assert_eq!(counts.get(&TypeName::new("mesh.face").unwrap()).copied(), Some(12));
    assert_eq!(counts.get(&TypeName::new("mesh.triangulated_face").unwrap()).copied(), Some(6));
}

#[test]
fn ac2_and_ac3_points_double_custom_bit_identical_and_round_trip() {
    let raw = fixture_bytes("points_double_custom.ply");
    let mut b = default_budget();
    let res = read_ply_bytes(&raw, LengthUnit::M, frame0(), &mut b).unwrap();
    assert_eq!(res.header.format, PlyFormat::BinaryLittleEndian);
    assert_eq!(res.triangulated_faces, 0);
    assert!(res.dropped_elements.is_empty());

    let BuiltGeometry::PointSet(ref bp) = res.geometry else {
        panic!("vertex-only PLY must produce a PointSet");
    };
    assert_eq!(bp.counts.vertices, 1_000);
    assert_eq!(bp.counts.faces, 0);
    assert_eq!(bp.source_precision, SourcePrecision::F64);

    // Check quality (f32) and RGB (u8 x 3) attributes and exact bit-level double coordinates
    // against the raw binary body of `points_double_custom.ply`.
    let body = &raw[res.header.body_offset..];
    assert_eq!(body.len(), 1_000 * (24 + 4 + 3));
    let Some(ScalarColumn::F32(ref qual)) = bp.custom_attrs.get("quality") else {
        panic!("missing float quality column");
    };
    assert_eq!(qual.len(), 1_000);
    let colours = bp.points.attributes().colour.as_ref().expect("missing RGB colour column");
    assert_eq!(colours.len(), 1_000);

    for i in 0..1_000usize {
        let rec = &body[i * 31..(i + 1) * 31];
        let ex = f64::from_le_bytes(rec[0..8].try_into().unwrap());
        let ey = f64::from_le_bytes(rec[8..16].try_into().unwrap());
        let ez = f64::from_le_bytes(rec[16..24].try_into().unwrap());
        let eq = f32::from_le_bytes(rec[24..28].try_into().unwrap());
        let ergb = [rec[28], rec[29], rec[30]];

        let got = bp.points.point(i as u64).unwrap();
        assert_eq!(got[0].to_bits(), ex.to_bits(), "x[{i}] bits mismatch");
        assert_eq!(got[1].to_bits(), ey.to_bits(), "y[{i}] bits mismatch");
        assert_eq!(got[2].to_bits(), ez.to_bits(), "z[{i}] bits mismatch");
        assert_eq!(qual[i].to_bits(), eq.to_bits(), "quality[{i}] bits mismatch");
        assert_eq!(colours[i], ergb, "rgb[{i}] mismatch");
    }

    // AC-3: Export with precision=double (binary_le) and re-import -> bit-identical coordinates & attributes.
    let mut sink = VecSink(Vec::new());
    let exp_opts = PlyExportOptions {
        encoding: PlyExportEncoding::BinaryLe,
        precision: PlyExportPrecision::Double,
        unit: LengthUnit::M,
        properties: Vec::new(),
    };
    let summary = write_ply(&res.geometry, &exp_opts, &mut sink).unwrap();
    assert_eq!(summary.vertices, 1_000);
    assert_eq!(summary.triangles, 0);
    assert_eq!(summary.max_abs_rounding_m, 0.0);

    let mut b2 = default_budget();
    let re = read_ply_bytes(&sink.0, LengthUnit::M, frame0(), &mut b2).unwrap();
    let BuiltGeometry::PointSet(ref bp2) = re.geometry else {
        panic!("re-imported point set should still be a PointSet");
    };
    let orig_pts = point_set_coords(&bp.points);
    let back_pts = point_set_coords(&bp2.points);
    assert_eq!(orig_pts.len(), back_pts.len());
    for (i, (a, b_pt)) in orig_pts.iter().zip(back_pts.iter()).enumerate() {
        assert_eq!(a[0].to_bits(), b_pt[0].to_bits(), "round-trip x[{i}]");
        assert_eq!(a[1].to_bits(), b_pt[1].to_bits(), "round-trip y[{i}]");
        assert_eq!(a[2].to_bits(), b_pt[2].to_bits(), "round-trip z[{i}]");
    }
    assert_eq!(bp2.points.attributes().colour, bp.points.attributes().colour);
    let Some(ScalarColumn::F32(ref qual2)) = bp2.custom_attrs.get("quality") else {
        panic!("missing round-trip quality column");
    };
    for (i, (a, b_q)) in qual.iter().zip(qual2.iter()).enumerate() {
        assert_eq!(a.to_bits(), b_q.to_bits(), "round-trip quality[{i}]");
    }

    // Also verify ASCII export round-trip with precision=double stays bit-identical.
    let mut ascii_sink = VecSink(Vec::new());
    let ascii_opts = PlyExportOptions {
        encoding: PlyExportEncoding::Ascii,
        precision: PlyExportPrecision::Double,
        unit: LengthUnit::M,
        properties: Vec::new(),
    };
    write_ply(&res.geometry, &ascii_opts, &mut ascii_sink).unwrap();
    let mut b3 = default_budget();
    let re_ascii = read_ply_bytes(&ascii_sink.0, LengthUnit::M, frame0(), &mut b3).unwrap();
    let BuiltGeometry::PointSet(ref bp3) = re_ascii.geometry else {
        panic!("re-imported ASCII point set should still be a PointSet");
    };
    for i in 0..1_000u64 {
        let a = bp.points.point(i).unwrap();
        let c = bp3.points.point(i).unwrap();
        assert_eq!(a[0].to_bits(), c[0].to_bits(), "ascii round-trip x[{i}]");
        assert_eq!(a[1].to_bits(), c[1].to_bits(), "ascii round-trip y[{i}]");
        assert_eq!(a[2].to_bits(), c[2].to_bits(), "ascii round-trip z[{i}]");
    }
}

#[test]
fn ac4_truncated_body_fails_truncated_file_and_journals_nothing() {
    let raw = fixture_bytes("truncated.ply");
    let mut b = default_budget();
    let err = read_ply_bytes(&raw, LengthUnit::Mm, frame0(), &mut b).unwrap_err();
    assert!(matches!(err, IoError::TruncatedFile { .. }), "expected TruncatedFile, got {err:?}");

    let reg = setup_registry();
    let tmp = TmpRoots::new("ac4");
    let roots = tmp.roots();
    let (tree, ns) = setup_project();
    let before = tree.state_digest().unwrap();
    let cmd = import_mesh_ply_command().unwrap();
    let env = ImportEnv::new(&reg, &roots, Principal::Agent, principal_commit(), &tree, ns);
    let mut params = ImportParams::new("root://in/truncated.ply");
    params.unit = Some(LengthUnit::Mm);
    let err = cmd.prepare(&env, &params, None).unwrap_err();
    assert_eq!(err.code(), "truncated_file");
    assert!(matches!(err, ImportError::Reader(IoError::TruncatedFile { .. })));
    assert_eq!(tree.state_digest().unwrap(), before);
}

#[test]
fn ac5_with_edges_succeeds_and_reports_dropped_edges() {
    let raw = fixture_bytes("with_edges.ply");
    let mut b = default_budget();
    let res = read_ply_bytes(&raw, LengthUnit::Mm, frame0(), &mut b).unwrap();
    assert_eq!(res.dropped_elements.get("edge").copied(), Some(12));
    assert_eq!(res.triangulated_faces, 6);

    let reg = setup_registry();
    let tmp = TmpRoots::new("ac5");
    let roots = tmp.roots();
    let (mut tree, ns) = setup_project();
    let cmd = import_mesh_ply_command().unwrap();
    let mut blobs = MemoryBlobs::default();
    let env = ImportEnv::new(&reg, &roots, Principal::Agent, principal_commit(), &tree, ns);
    let mut params = ImportParams::new("root://in/with_edges.ply");
    params.unit = Some(LengthUnit::Mm);
    let out = cmd.prepare(&env, &params, Some(&mut blobs as &mut dyn BlobSink)).unwrap().commit(&mut tree).unwrap();

    let counts = out.report.entity_counts();
    assert_eq!(counts.get(&TypeName::new("ply.dropped.edge").unwrap()).copied(), Some(12));
    assert_eq!(out.report.dropped_total(), 1);
    assert_eq!(out.report.dropped()[0].detail.char_len(), "edge x 12".len());
}

#[test]
fn ac6_huge_vertex_count_fails_limit_exceeded_before_allocating() {
    let huge = b"ply\nformat binary_little_endian 1.0\nelement vertex 99999999999\nproperty float x\nproperty float y\nproperty float z\nend_header\n";
    let mut b = default_budget();
    let err = read_ply_bytes(huge, LengthUnit::Mm, frame0(), &mut b).unwrap_err();
    match err {
        IoError::LimitExceeded { limit, .. } => {
            assert!(matches!(limit, LimitKind::Elements | LimitKind::Alloc), "unexpected limit kind {limit:?}");
        }
        other => panic!("expected LimitExceeded before allocating, got {other:?}"),
    }
}

#[test]
fn ac7_replay_identical_and_export_conformance_kit() {
    // AC-7: Exact replay of AC-2 (`points_double_custom.ply`) -> IDENTICAL.
    let reg = setup_registry();
    let tmp = TmpRoots::new("ac7");
    let roots = tmp.roots();
    let out_dir = tmp.out_dir();
    let (mut tree, ns) = setup_project();
    let imp_cmd = import_mesh_ply_command().unwrap();
    let mut blobs = MemoryBlobs::default();
    let env = ImportEnv::new(&reg, &roots, Principal::Agent, principal_commit(), &tree, ns);
    let mut params = ImportParams::new("root://in/points_double_custom.ply");
    params.unit = Some(LengthUnit::M);
    let prepared = imp_cmd.prepare(&env, &params, Some(&mut blobs as &mut dyn BlobSink)).unwrap();
    let recorded = prepared.events().to_vec();
    let tree_before = tree.clone();
    let out = prepared.commit(&mut tree).unwrap();
    assert_eq!(out.event.entities.len(), 1);

    // Check staged element type is `geo.point_set`.
    let el = tree.element(&out.event.entities[0]).unwrap();
    assert_eq!(el.type_id(), PLY_POINT_SET_ELEMENT_TYPE);

    let renv = ImportEnv::new(&reg, &roots, Principal::Agent, principal_commit(), &tree_before, ns);
    assert_eq!(imp_cmd.replay(&renv, &out.event, &recorded).unwrap(), ReplayOutcome::Identical);

    // Export conformance kit on `export.mesh_ply@1` using AC-2 point set.
    let mut bm = default_budget();
    let geom =
        read_ply_bytes(&fixture_bytes("points_double_custom.ply"), LengthUnit::M, frame0(), &mut bm).unwrap().geometry;
    let mut expected_sink = VecSink(Vec::new());
    write_ply(&geom, &PlyExportOptions::default(), &mut expected_sink).unwrap();
    let sha = Digest::from_bytes(Sha256::digest(&expected_sink.0).into());

    let fmt = ply_format_id().unwrap();
    let exp_cmd = export_mesh_ply_command().unwrap();
    let defs = ply_command_defs(&reg).unwrap();
    assert_eq!(defs.len(), 2);
    assert_eq!(format!("{}@{}", defs[0].id, defs[0].major), "import.mesh_ply@1");
    assert_eq!(format!("{}@{}", defs[1].id, defs[1].major), "export.mesh_ply@1");

    let source = SourceRef::One("33333333-4444-5555-8666-777777777777".to_string());
    let default_opts = CValue::Obj(BTreeMap::new());
    let variants = vec![
        fid_canon::parse(b"{\"encoding\":\"ascii\",\"precision\":\"double\"}").unwrap(),
        fid_canon::parse(b"{\"encoding\":\"binary_le\",\"precision\":\"float\",\"properties\":[\"quality\"]}").unwrap(),
        fid_canon::parse(b"{\"unit\":\"mm\"}").unwrap(),
        fid_canon::parse(b"{\"encoding\":\"binary_be\"}").unwrap(),
        fid_canon::parse(b"{\"precision\":\"half\"}").unwrap(),
        fid_canon::parse(b"{\"unknown\":1}").unwrap(),
    ];
    let fx = export_conformance::Fixture {
        registry: &reg,
        roots: &roots,
        root_id: "out",
        root_dir: &out_dir,
        format: &fmt,
        file_name: "points.ply",
        source: &source,
        options: &default_opts,
        expected_sha256: &sha,
        option_variants: &variants,
        fail_after: 64,
    };
    export_conformance::check(&exp_cmd, &defs[1], &geom, &fx).unwrap();

    // Verify float precision export records f32_rounding lossy warning on double-precision points.
    let exp_env = ExportEnv::new(&reg, &roots, Principal::Agent);
    let mut ep = ExportParams::new(source, None, "root://out/points_f32.ply");
    ep.options =
        fid_canon::parse(b"{\"encoding\":\"binary_le\",\"precision\":\"float\",\"properties\":[\"quality\"]}").unwrap();
    let exp_out = exp_cmd.prepare(&exp_env, &ep, &geom).unwrap().place().unwrap();
    assert!(!exp_out.report.warnings().is_empty(), "expected f32_rounding warning when downcasting double to float");
}

#[test]
fn export_overflow_fails_atomically_without_writing_header() {
    let mut b_huge = MeshBuilder::new(SourcePrecision::F64);
    let mut budget = default_budget();
    let v0 = b_huge.push_vertex([1e305, 0.0, 0.0], &mut budget, Position::byte(0)).unwrap();
    let v1 = b_huge.push_vertex([0.0, 1.0, 0.0], &mut budget, Position::byte(0)).unwrap();
    let v2 = b_huge.push_vertex([0.0, 0.0, 1.0], &mut budget, Position::byte(0)).unwrap();
    b_huge.push_face_u64([v0, v1, v2], &mut budget, Position::byte(0)).unwrap();
    let geom_huge = b_huge.build(frame0(), &mut budget, Position::byte(0)).unwrap();

    let mut sink_um = VecSink(Vec::new());
    let opts_um = PlyExportOptions {
        encoding: PlyExportEncoding::Ascii,
        precision: PlyExportPrecision::Double,
        unit: LengthUnit::Um,
        properties: Vec::new(),
    };
    let err = write_ply(&geom_huge, &opts_um, &mut sink_um).unwrap_err();
    assert!(matches!(err, IoError::Malformed { reason: "non_finite_coordinate", .. }), "unexpected error: {err:?}");
    assert!(sink_um.0.is_empty(), "failed PLY export must write 0 bytes, wrote {}", sink_um.0.len());

    let mut b_f32 = MeshBuilder::new(SourcePrecision::F64);
    let v0 = b_f32.push_vertex([1e39, 0.0, 0.0], &mut budget, Position::byte(0)).unwrap();
    let v1 = b_f32.push_vertex([0.0, 1.0, 0.0], &mut budget, Position::byte(0)).unwrap();
    let v2 = b_f32.push_vertex([0.0, 0.0, 1.0], &mut budget, Position::byte(0)).unwrap();
    b_f32.push_face_u64([v0, v1, v2], &mut budget, Position::byte(0)).unwrap();
    let geom_f32_ovf = b_f32.build(frame0(), &mut budget, Position::byte(0)).unwrap();
    for encoding in [PlyExportEncoding::BinaryLe, PlyExportEncoding::Ascii] {
        let mut sink = VecSink(Vec::new());
        let opts = PlyExportOptions {
            encoding,
            precision: PlyExportPrecision::Float,
            unit: LengthUnit::M,
            properties: Vec::new(),
        };
        let err = write_ply(&geom_f32_ovf, &opts, &mut sink).unwrap_err();
        assert!(matches!(err, IoError::Malformed { reason: "non_finite_coordinate", .. }));
        assert!(sink.0.is_empty(), "failed PLY float export must write 0 bytes");
    }

    let mut b_pts = MeshBuilder::new(SourcePrecision::F64);
    b_pts.push_vertex([1e305, 0.0, 0.0], &mut budget, Position::byte(0)).unwrap();
    let geom_pts = b_pts.build(frame0(), &mut budget, Position::byte(0)).unwrap();
    let mut sink_pts = VecSink(Vec::new());
    let err_pts = write_ply(&geom_pts, &opts_um, &mut sink_pts).unwrap_err();
    assert!(matches!(err_pts, IoError::Malformed { reason: "non_finite_coordinate", .. }));
    assert!(sink_pts.0.is_empty());
}
