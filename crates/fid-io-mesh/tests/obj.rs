//! Integration tests for `IO.MESH.OBJ` (AC-1 through AC-5 + `fid-io-reg` skeleton, conformance & replay).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{CValue, Digest as CanonDigest};
use fid_geo_frame::LengthUnit;
use fid_geo_types::{FrameRef, Mesh};
use fid_io_mesh::obj::{
    export_mesh_obj_command, import_mesh_obj_command, obj_command_defs, obj_format_id, read_obj_bytes, register_obj,
    write_obj, ObjExportOptions,
};
use fid_io_mesh::stl::{decode_staged_mesh_payload, STL_ELEMENT_TYPE};
use fid_io_mesh::SourcePrecision;
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
use fid_io_reg::{Budget, IoError, LimitRequest, Principal};
use fid_project::{ElementTypeDef, ElementTypeRegistry, Tree, Uuid, STAGE_ADDED};
use fid_replay::ReplayOutcome;
use fid_types::Digest;
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/io/mesh/obj/");

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
        let base = std::env::temp_dir().join(format!("fid-io-mesh-obj-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("in")).unwrap();
        fs::create_dir_all(base.join("out")).unwrap();
        for name in ["cube_quads.obj", "negative_idx.obj", "with_mtl.obj", "bad_index.obj"] {
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
    register_obj(&mut reg).unwrap();
    reg
}

fn setup_project() -> (Tree, Uuid) {
    let ns = Uuid::parse("22222222-3333-5444-8555-666666666666").unwrap();
    let mut reg = ElementTypeRegistry::new();
    let schema = CValue::Obj(BTreeMap::from([("type".to_string(), CValue::Str("object".to_string()))]));
    reg.register(ElementTypeDef::new(STL_ELEMENT_TYPE, schema, "mesh").unwrap()).unwrap();
    let imp_cmd = import_mesh_obj_command().unwrap();
    let imp_id = imp_cmd.command_id().unwrap();
    register_import_events(&mut reg, &[&import_file_id().unwrap(), &imp_id]).unwrap();
    let mut tree = Tree::new(ns, Arc::new(reg)).unwrap();
    let run = fid_project::uuid_v5(&ns, b"obj-test-run").to_string();
    let s1 = fid_project::uuid_v5(&ns, b"obj-test-s1").to_string();
    let s2 = fid_project::uuid_v5(&ns, b"obj-test-s2").to_string();
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
    PrincipalCommit { kind: PrincipalKind::Agent, commit: CanonDigest([0x7a; 32]), on_behalf_of_commit: None }
}

fn mesh_vertices(mesh: &Mesh) -> Vec<[f64; 3]> {
    (0..mesh.vertex_count()).map(|i| mesh.vertex(i).unwrap()).collect()
}

fn mesh_faces(mesh: &Mesh) -> Vec<[u64; 3]> {
    mesh.triangles().collect()
}

#[test]
fn ac1_and_ac2_quad_triangulation_and_negative_indices_match() {
    let quad_bytes = fixture_bytes("cube_quads.obj");
    let neg_bytes = fixture_bytes("negative_idx.obj");

    let mut b1 = default_budget();
    let r1 = read_obj_bytes(&quad_bytes, LengthUnit::Mm, frame0(), &mut b1).unwrap();
    assert_eq!(r1.built.counts.vertices, 8);
    assert_eq!(r1.built.counts.faces, 12);
    assert_eq!(r1.triangulated_faces, 6);
    assert_eq!(r1.built.source_precision, SourcePrecision::F64);
    assert!(r1.ignored_statements.is_empty());

    // AC-2: negative relative indices yield identical vertices and triangles.
    let mut b2 = default_budget();
    let r2 = read_obj_bytes(&neg_bytes, LengthUnit::Mm, frame0(), &mut b2).unwrap();
    assert_eq!(r2.built.counts, r1.built.counts);
    assert_eq!(r2.triangulated_faces, 6);
    assert_eq!(mesh_vertices(&r2.built.mesh), mesh_vertices(&r1.built.mesh));
    assert_eq!(mesh_faces(&r2.built.mesh), mesh_faces(&r1.built.mesh));

    // Also verify import.mesh_obj@1 reports triangulated_faces = 6 and requires unit.
    let reg = setup_registry();
    let tmp = TmpRoots::new("ac12");
    let roots = tmp.roots();
    let (mut tree, ns) = setup_project();
    let cmd = import_mesh_obj_command().unwrap();

    // Missing unit fails unit_required.
    {
        let env = ImportEnv::new(&reg, &roots, Principal::Agent, principal_commit(), &tree, ns);
        let err = cmd.prepare(&env, &ImportParams::new("root://in/cube_quads.obj"), None).unwrap_err();
        assert_eq!(err.code(), "unit_required");
    }

    let mut blobs = MemoryBlobs::default();
    let env = ImportEnv::new(&reg, &roots, Principal::Agent, principal_commit(), &tree, ns);
    let mut params = ImportParams::new("root://in/cube_quads.obj");
    params.unit = Some(LengthUnit::Mm);
    let prepared = cmd.prepare(&env, &params, Some(&mut blobs as &mut dyn BlobSink)).unwrap();
    let recorded = prepared.events().to_vec();
    let tree_before = tree.clone();
    let out = prepared.commit(&mut tree).unwrap();

    let counts = out.report.entity_counts();
    assert_eq!(counts.get(&TypeName::new("mesh.face").unwrap()).copied(), Some(12));
    assert_eq!(counts.get(&TypeName::new("mesh.vertex").unwrap()).copied(), Some(8));
    assert_eq!(counts.get(&TypeName::new("mesh.triangulated_face").unwrap()).copied(), Some(6));

    // Verify exact replay is IDENTICAL.
    let renv = ImportEnv::new(&reg, &roots, Principal::Agent, principal_commit(), &tree_before, ns);
    assert_eq!(cmd.replay(&renv, &out.event, &recorded).unwrap(), ReplayOutcome::Identical);

    // Staged blob decodes to identical vertices in metres.
    let el = tree.element(&out.event.entities[0]).unwrap();
    let digest = el.slots().get(&tree.active_stage()).unwrap().digest.unwrap();
    let payload = blobs.blobs.get(&digest).unwrap();
    let mut bm = default_budget();
    let mat = decode_staged_mesh_payload(payload, LengthUnit::Mm, frame0(), &mut bm).unwrap();
    assert_eq!(mesh_vertices(&mat), mesh_vertices(&r1.built.mesh));
    assert_eq!(mesh_faces(&mat), mesh_faces(&r1.built.mesh));
}

#[test]
fn ac3_with_mtl_counts_ignored_keywords_and_never_opens_mtl() {
    let bytes = fixture_bytes("with_mtl.obj");
    let mut b = default_budget();
    // Pure in-memory decode cannot open any file; `missing.mtl` does not exist anywhere.
    let r = read_obj_bytes(&bytes, LengthUnit::Mm, frame0(), &mut b).unwrap();
    assert_eq!(r.built.counts.faces, 12);
    assert_eq!(r.triangulated_faces, 6);
    assert_eq!(r.ignored_statements.get("mtllib").copied(), Some(1));
    assert_eq!(r.ignored_statements.get("usemtl").copied(), Some(2));
    assert_eq!(r.ignored_statements.get("vt").copied(), Some(24));

    // Also run through import.mesh_obj@1 in a temp root that does NOT contain missing.mtl.
    let reg = setup_registry();
    let tmp = TmpRoots::new("ac3");
    assert!(!tmp.base.join("in/missing.mtl").exists());
    let roots = tmp.roots();
    let (mut tree, ns) = setup_project();
    let cmd = import_mesh_obj_command().unwrap();
    let mut blobs = MemoryBlobs::default();
    let env = ImportEnv::new(&reg, &roots, Principal::Agent, principal_commit(), &tree, ns);
    let mut params = ImportParams::new("root://in/with_mtl.obj");
    params.unit = Some(LengthUnit::Mm);
    let out = cmd.prepare(&env, &params, Some(&mut blobs as &mut dyn BlobSink)).unwrap().commit(&mut tree).unwrap();

    let counts = out.report.entity_counts();
    assert_eq!(counts.get(&TypeName::new("obj.ignored.mtllib").unwrap()).copied(), Some(1));
    assert_eq!(counts.get(&TypeName::new("obj.ignored.usemtl").unwrap()).copied(), Some(2));
    assert_eq!(counts.get(&TypeName::new("obj.ignored.vt").unwrap()).copied(), Some(24));
    assert_eq!(out.report.dropped_total(), 3);
}

#[test]
fn ac4_bad_index_fails_malformed_line_10_and_journals_nothing() {
    let bytes = fixture_bytes("bad_index.obj");
    let mut b = default_budget();
    let err = read_obj_bytes(&bytes, LengthUnit::Mm, frame0(), &mut b).unwrap_err();
    match err {
        IoError::Malformed { at, .. } => assert_eq!(at.line, Some(10), "expected line 10, got {at:?}"),
        other => panic!("expected Malformed line 10, got {other:?}"),
    }

    let reg = setup_registry();
    let tmp = TmpRoots::new("ac4");
    let roots = tmp.roots();
    let (tree, ns) = setup_project();
    let before = tree.state_digest().unwrap();
    let cmd = import_mesh_obj_command().unwrap();
    let env = ImportEnv::new(&reg, &roots, Principal::Agent, principal_commit(), &tree, ns);
    let mut params = ImportParams::new("root://in/bad_index.obj");
    params.unit = Some(LengthUnit::Mm);
    let err = cmd.prepare(&env, &params, None).unwrap_err();
    assert_eq!(err.code(), "malformed");
    match err {
        ImportError::Reader(IoError::Malformed { at, .. }) => assert_eq!(at.line, Some(10)),
        other => panic!("expected Reader(Malformed line 10), got {other:?}"),
    }
    assert_eq!(tree.state_digest().unwrap(), before);
}

#[test]
fn ac5_export_round_trip_bit_identical_and_conformance_kit() {
    // Include high-precision f64 coordinates that would lose bits in f32, verifying bit-identical
    // shortest round-trip decimal formatting (REQ-004, AC-5).
    let mut builder = fid_io_mesh::MeshBuilder::new(SourcePrecision::F64);
    let mut b0 = default_budget();
    let pos = fid_io_reg::Position::byte(0);
    let pts = [
        [0.123_456_789_012_345_68, -0.000_000_123_456_789, 1.0 / 3.0],
        [10.987_654_321_098_765, 2.5, -7.125],
        [0.0, 1.0, 0.0],
    ];
    for p in pts {
        builder.push_vertex(p, &mut b0, pos).unwrap();
    }
    builder.push_face_u64([0, 1, 2], &mut b0, pos).unwrap();
    let precise_mesh = builder.build_mesh(frame0(), &mut b0, pos).unwrap().mesh;

    // Export in metres without normals and with normals; re-import is bit-identical.
    for with_normals in [false, true] {
        let mut sink = VecSink(Vec::new());
        let opts = ObjExportOptions { unit: LengthUnit::M, normals: with_normals, object_name: "precise".into() };
        let summary = write_obj(&precise_mesh, &opts, &mut sink).unwrap();
        assert_eq!(summary.vertices, 3);
        assert_eq!(summary.triangles, 1);
        assert_eq!(summary.normals_written, with_normals);

        let mut br = default_budget();
        let back = read_obj_bytes(&sink.0, LengthUnit::M, frame0(), &mut br).unwrap();
        assert_eq!(back.built.mesh.vertex_count(), precise_mesh.vertex_count());
        assert_eq!(mesh_faces(&back.built.mesh), mesh_faces(&precise_mesh));
        if with_normals {
            assert_eq!(back.normals.len(), 1);
        }
        for i in 0..precise_mesh.vertex_count() {
            let orig = precise_mesh.vertex(i).unwrap();
            let got = back.built.mesh.vertex(i).unwrap();
            assert_eq!(orig[0].to_bits(), got[0].to_bits(), "v[{i}].x bits differ (normals={with_normals})");
            assert_eq!(orig[1].to_bits(), got[1].to_bits(), "v[{i}].y bits differ (normals={with_normals})");
            assert_eq!(orig[2].to_bits(), got[2].to_bits(), "v[{i}].z bits differ (normals={with_normals})");
        }
    }

    // AC-1 cube exported and re-imported in mm -> bit-identical.
    let mut bc = default_budget();
    let cube_mesh =
        read_obj_bytes(&fixture_bytes("cube_quads.obj"), LengthUnit::Mm, frame0(), &mut bc).unwrap().built.mesh;
    let mut cube_sink = VecSink(Vec::new());
    write_obj(&cube_mesh, &ObjExportOptions::default(), &mut cube_sink).unwrap();
    let sha = Digest::from_bytes(Sha256::digest(&cube_sink.0).into());

    // Pass the shared fid-io-reg export conformance kit!
    let reg = setup_registry();
    let tmp = TmpRoots::new("exp_kit");
    let roots = tmp.roots();
    let out_dir = tmp.out_dir();
    let fmt = obj_format_id().unwrap();
    let cmd = export_mesh_obj_command().unwrap();
    let defs = obj_command_defs(&reg).unwrap();
    assert_eq!(defs.len(), 2);
    assert_eq!(format!("{}@{}", defs[0].id, defs[0].major), "import.mesh_obj@1");
    assert_eq!(format!("{}@{}", defs[1].id, defs[1].major), "export.mesh_obj@1");

    let source = SourceRef::One("22222222-3333-5444-8555-666666666666".to_string());
    let default_opts = CValue::Obj(BTreeMap::new());
    let variants = vec![
        fid_canon::parse(b"{\"normals\":true}").unwrap(),
        fid_canon::parse(b"{\"unit\":\"mm\"}").unwrap(),
        fid_canon::parse(b"{\"normals\":\"yes\"}").unwrap(),
        fid_canon::parse(b"{\"unknown\":1}").unwrap(),
    ];
    let fx = export_conformance::Fixture {
        registry: &reg,
        roots: &roots,
        root_id: "out",
        root_dir: &out_dir,
        format: &fmt,
        file_name: "cube.obj",
        source: &source,
        options: &default_opts,
        expected_sha256: &sha,
        option_variants: &variants,
        fail_after: 64,
    };
    export_conformance::check(&cmd, &defs[1], &cube_mesh, &fx).unwrap();

    // Verify ExportEnv with normals=true emits vn and f i//n lines.
    let exp_env = ExportEnv::new(&reg, &roots, Principal::Agent);
    let mut ep = ExportParams::new(source, None, "root://out/cube_normals.obj");
    ep.options = fid_canon::parse(b"{\"normals\":true,\"unit\":\"mm\"}").unwrap();
    let _ = cmd.prepare(&exp_env, &ep, &cube_mesh).unwrap().place().unwrap();
    let written = fs::read_to_string(out_dir.join("cube_normals.obj")).unwrap();
    assert!(written.contains("vn "));
    assert!(written.contains("//1"));
}
