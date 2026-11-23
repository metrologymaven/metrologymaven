//! Integration tests for `IO.MESH.STL` (AC-1 through AC-8 + `fid-io-reg` skeleton & conformance).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{CValue, Digest as CanonDigest};
use fid_geo_frame::LengthUnit;
use fid_geo_types::{FrameRef, Mesh};
use fid_io_mesh::stl::{
    decode_staged_mesh_payload, detect_stl_encoding, export_mesh_stl_command, import_mesh_stl_command, read_stl_bytes,
    read_stl_stream, register_stl, stl_command_defs, stl_format_id, write_stl, StlEncoding, StlExportOptions,
    STL_ELEMENT_TYPE,
};
use fid_io_mesh::SourcePrecision;
use fid_io_reg::detect::Registry;
use fid_io_reg::env::export::{
    conformance as export_conformance, ByteSink, ExportEnv, ExportParams, ExportRun, SourceRef,
};
use fid_io_reg::env::import::{
    self, import_file_id, register_import_events, BlobSink, ImportEnv, ImportError, ImportParams, ImportRun,
    MemoryBlobs,
};
use fid_io_reg::prov::{PrincipalCommit, PrincipalKind};
use fid_io_reg::report::TypeName;
use fid_io_reg::roots::{RootAccess, RootId, RootSet};
use fid_io_reg::{Budget, IoError, LimitKind, LimitRequest, Principal};
use fid_project::{ElementTypeDef, ElementTypeRegistry, Tree, Uuid, STAGE_ADDED};
use fid_replay::ReplayOutcome;
use fid_types::Digest;
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::Arc;

const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/io/mesh/stl/");

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
        let base = std::env::temp_dir().join(format!("fid-io-mesh-stl-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("in")).unwrap();
        fs::create_dir_all(base.join("out")).unwrap();
        for name in
            ["cube_bin.stl", "cube_bin_solid_header.stl", "cube_ascii.stl", "cube_bin_truncated.stl", "bad_vertex.stl"]
        {
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
    register_stl(&mut reg).unwrap();
    reg
}

fn setup_project() -> (Tree, Uuid) {
    let ns = Uuid::parse("11111111-2222-5333-8444-555555555555").unwrap();
    let mut reg = ElementTypeRegistry::new();
    let schema = CValue::Obj(BTreeMap::from([("type".to_string(), CValue::Str("object".to_string()))]));
    reg.register(ElementTypeDef::new(STL_ELEMENT_TYPE, schema, "mesh").unwrap()).unwrap();
    let imp_cmd = import_mesh_stl_command().unwrap();
    let imp_id = imp_cmd.command_id().unwrap();
    register_import_events(&mut reg, &[&import_file_id().unwrap(), &imp_id]).unwrap();
    let mut tree = Tree::new(ns, Arc::new(reg)).unwrap();
    let run = fid_project::uuid_v5(&ns, b"stl-test-run").to_string();
    let s1 = fid_project::uuid_v5(&ns, b"stl-test-s1").to_string();
    let s2 = fid_project::uuid_v5(&ns, b"stl-test-s2").to_string();
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
    PrincipalCommit { kind: PrincipalKind::Agent, commit: CanonDigest([0x3c; 32]), on_behalf_of_commit: None }
}

fn mesh_vertices(mesh: &Mesh) -> Vec<[f64; 3]> {
    (0..mesh.vertex_count()).map(|i| mesh.vertex(i).unwrap()).collect()
}

fn mesh_faces(mesh: &Mesh) -> Vec<[u64; 3]> {
    mesh.triangles().collect()
}

fn cval_str<'a>(v: &'a CValue, path: &[&str]) -> &'a str {
    let mut cur = v;
    for k in path {
        match cur {
            CValue::Obj(m) => cur = m.get(*k).unwrap_or_else(|| panic!("missing key {k}")),
            other => panic!("expected object at {k}, got {other:?}"),
        }
    }
    match cur {
        CValue::Str(s) => s.as_str(),
        other => panic!("expected string, got {other:?}"),
    }
}

#[test]
fn ac1_ac2_ac3_binary_solid_header_and_ascii_import_match() {
    let bin_bytes = fixture_bytes("cube_bin.stl");
    let solid_bin_bytes = fixture_bytes("cube_bin_solid_header.stl");
    let ascii_bytes = fixture_bytes("cube_ascii.stl");

    // REQ-001: size consistency wins over leading `solid` header.
    assert_eq!(detect_stl_encoding(&bin_bytes).unwrap(), StlEncoding::Binary);
    assert_eq!(detect_stl_encoding(&solid_bin_bytes).unwrap(), StlEncoding::Binary);
    assert_eq!(detect_stl_encoding(&ascii_bytes).unwrap(), StlEncoding::Ascii);

    let mut b1 = default_budget();
    let r_bin = read_stl_bytes(&bin_bytes, LengthUnit::Mm, frame0(), &mut b1).unwrap();
    assert_eq!(r_bin.encoding, StlEncoding::Binary);
    assert_eq!(r_bin.built.counts.faces, 12);
    assert_eq!(r_bin.built.counts.vertices, 36);
    assert_eq!(r_bin.built.counts.degenerate_faces, 0);
    assert_eq!(r_bin.built.counts.non_manifold_edges, 0);
    assert_eq!(r_bin.built.source_precision, SourcePrecision::F32);
    // 10 mm -> 0.01 m exact f32->f64 widening * scale.
    let verts_bin = mesh_vertices(&r_bin.built.mesh);
    assert!(verts_bin.iter().any(|v| (v[0] - 0.01).abs() < 1e-15));
    assert!(verts_bin.iter().all(|v| v.iter().all(|&c| (c == 0.0) || (c - 0.01).abs() < 1e-15)));

    // AC-2: binary with `solid cube` header yields identical geometry & counts.
    let mut b2 = default_budget();
    let r_solid = read_stl_bytes(&solid_bin_bytes, LengthUnit::Mm, frame0(), &mut b2).unwrap();
    assert_eq!(r_solid.encoding, StlEncoding::Binary);
    assert_eq!(r_solid.built.counts, r_bin.built.counts);
    assert_eq!(r_solid.built.source_precision, SourcePrecision::F32);
    assert_eq!(mesh_vertices(&r_solid.built.mesh), verts_bin);
    assert_eq!(mesh_faces(&r_solid.built.mesh), mesh_faces(&r_bin.built.mesh));

    // AC-3: ASCII STL yields 12 faces with coordinates equal to AC-1's.
    let mut b3 = default_budget();
    let r_ascii = read_stl_bytes(&ascii_bytes, LengthUnit::Mm, frame0(), &mut b3).unwrap();
    assert_eq!(r_ascii.encoding, StlEncoding::Ascii);
    assert_eq!(r_ascii.built.counts, r_bin.built.counts);
    assert_eq!(r_ascii.built.source_precision, SourcePrecision::F32);
    assert_eq!(mesh_vertices(&r_ascii.built.mesh), verts_bin);
    assert_eq!(mesh_faces(&r_ascii.built.mesh), mesh_faces(&r_bin.built.mesh));

    // Full import.mesh_stl@1 command check for AC-1..AC-3.
    let reg = setup_registry();
    let tmp = TmpRoots::new("ac123");
    let roots = tmp.roots();
    let cmd = import_mesh_stl_command().unwrap();

    for (file, exp_enc) in
        [("cube_bin.stl", "binary"), ("cube_bin_solid_header.stl", "binary"), ("cube_ascii.stl", "ascii")]
    {
        let (mut tree, ns) = setup_project();
        let mut blobs = MemoryBlobs::default();
        let env = ImportEnv::new(&reg, &roots, Principal::Agent, principal_commit(), &tree, ns);
        let mut params = ImportParams::new(&format!("root://in/{file}"));
        params.unit = Some(LengthUnit::Mm);

        let prepared = cmd.prepare(&env, &params, Some(&mut blobs as &mut dyn BlobSink)).unwrap();
        let out = prepared.commit(&mut tree).unwrap();

        let counts = out.report.entity_counts();
        assert_eq!(counts.get(&TypeName::new("mesh.face").unwrap()).copied(), Some(12));
        assert_eq!(counts.get(&TypeName::new("mesh.vertex").unwrap()).copied(), Some(36));
        assert_eq!(counts.get(&TypeName::new(STL_ELEMENT_TYPE).unwrap()).copied(), Some(1));
        assert_eq!(out.event.unit, Some(LengthUnit::Mm));
        assert_eq!(cval_str(&out.event.provenance, &["declared", "source_precision"]), "f32");
        assert_eq!(cval_str(&out.event.provenance, &["declared", "encoding"]), exp_enc);
        assert_eq!(cval_str(&out.event.provenance, &["unit"]), "mm");

        // Verify staged blob materializes exact AC-1 mesh in metres.
        let el = tree.element(&out.event.entities[0]).unwrap();
        let payload_digest = el.slots().get(&tree.active_stage()).unwrap().digest.unwrap();
        let payload = blobs.blobs.get(&payload_digest).unwrap();
        let mut bm = default_budget();
        let mat = decode_staged_mesh_payload(payload, LengthUnit::Mm, frame0(), &mut bm).unwrap();
        assert_eq!(mesh_vertices(&mat), verts_bin);
        assert_eq!(mesh_faces(&mat), mesh_faces(&r_bin.built.mesh));
    }
}

#[test]
fn unit_is_required_for_stl_import() {
    let reg = setup_registry();
    let tmp = TmpRoots::new("unit_req");
    let roots = tmp.roots();
    let (tree, ns) = setup_project();
    let env = ImportEnv::new(&reg, &roots, Principal::Agent, principal_commit(), &tree, ns);
    let cmd = import_mesh_stl_command().unwrap();
    let mut blobs = MemoryBlobs::default();
    let params = ImportParams::new("root://in/cube_bin.stl"); // unit = None
    let err = cmd.prepare(&env, &params, Some(&mut blobs as &mut dyn BlobSink)).unwrap_err();
    assert_eq!(err.code(), "unit_required");
}

#[test]
fn ac4_truncated_binary_fails_and_journals_nothing() {
    let bytes = fixture_bytes("cube_bin_truncated.stl");
    let mut b = default_budget();
    let err = read_stl_bytes(&bytes, LengthUnit::Mm, frame0(), &mut b).unwrap_err();
    assert!(matches!(err, IoError::TruncatedFile { .. }), "expected truncated_file, got {err:?}");

    let reg = setup_registry();
    let tmp = TmpRoots::new("ac4");
    let roots = tmp.roots();
    let (tree, ns) = setup_project();
    let before = tree.state_digest().unwrap();
    let env = ImportEnv::new(&reg, &roots, Principal::Agent, principal_commit(), &tree, ns);
    let cmd = import_mesh_stl_command().unwrap();
    let mut blobs = MemoryBlobs::default();
    let mut params = ImportParams::new("root://in/cube_bin_truncated.stl");
    params.unit = Some(LengthUnit::Mm);

    let err = cmd.prepare(&env, &params, Some(&mut blobs as &mut dyn BlobSink)).unwrap_err();
    assert_eq!(err.code(), "truncated_file");
    assert!(blobs.blobs.is_empty(), "truncated import must store no blobs");
    assert_eq!(tree.state_digest().unwrap(), before);
}

#[test]
fn ac5_malformed_ascii_reports_line_4() {
    let bytes = fixture_bytes("bad_vertex.stl");
    let mut b = default_budget();
    let err = read_stl_bytes(&bytes, LengthUnit::Mm, frame0(), &mut b).unwrap_err();
    match err {
        IoError::Malformed { at, .. } => assert_eq!(at.line, Some(4), "expected line 4, got {at:?}"),
        other => panic!("expected Malformed at line 4, got {other:?}"),
    }

    let reg = setup_registry();
    let tmp = TmpRoots::new("ac5");
    let roots = tmp.roots();
    let (tree, ns) = setup_project();
    let env = ImportEnv::new(&reg, &roots, Principal::Agent, principal_commit(), &tree, ns);
    let cmd = import_mesh_stl_command().unwrap();
    let mut params = ImportParams::new("root://in/bad_vertex.stl");
    params.unit = Some(LengthUnit::Mm);

    let err = cmd.prepare(&env, &params, None).unwrap_err();
    assert_eq!(err.code(), "malformed");
    match err {
        ImportError::Reader(IoError::Malformed { at, .. }) => assert_eq!(at.line, Some(4)),
        other => panic!("expected Reader(Malformed line 4), got {other:?}"),
    }
}

#[test]
fn ac6_binary_and_ascii_export_round_trip_bit_identical_and_precision_report() {
    let bin_bytes = fixture_bytes("cube_bin.stl");
    let mut b = default_budget();
    let imported = read_stl_bytes(&bin_bytes, LengthUnit::Mm, frame0(), &mut b).unwrap();

    // Binary export -> re-import in mm: bit-identical coordinates.
    let mut bin_sink = VecSink(Vec::new());
    let bin_opts = StlExportOptions { encoding: StlEncoding::Binary, unit: LengthUnit::Mm, solid_name: "cube".into() };
    let bin_summary = write_stl(&imported.built.mesh, &bin_opts, &mut bin_sink).unwrap();
    assert_eq!(bin_summary.triangles, 12);
    assert_eq!(bin_summary.bytes, 84 + 50 * 12);

    let mut b2 = default_budget();
    let re_bin = read_stl_bytes(&bin_sink.0, LengthUnit::Mm, frame0(), &mut b2).unwrap();
    assert_eq!(re_bin.built.mesh.vertex_count(), imported.built.mesh.vertex_count());
    for i in 0..imported.built.mesh.vertex_count() {
        let orig = imported.built.mesh.vertex(i).unwrap();
        let back = re_bin.built.mesh.vertex(i).unwrap();
        assert_eq!(orig[0].to_bits(), back[0].to_bits(), "vertex {i}.x bits differ");
        assert_eq!(orig[1].to_bits(), back[1].to_bits(), "vertex {i}.y bits differ");
        assert_eq!(orig[2].to_bits(), back[2].to_bits(), "vertex {i}.z bits differ");
    }

    // ASCII export -> re-import in mm: bit-identical coordinates (shortest round-trip f32).
    let mut ascii_sink = VecSink(Vec::new());
    let ascii_opts = StlExportOptions { encoding: StlEncoding::Ascii, unit: LengthUnit::Mm, solid_name: "cube".into() };
    let ascii_summary = write_stl(&imported.built.mesh, &ascii_opts, &mut ascii_sink).unwrap();
    assert_eq!(ascii_summary.triangles, 12);
    let mut b3 = default_budget();
    let re_ascii = read_stl_bytes(&ascii_sink.0, LengthUnit::Mm, frame0(), &mut b3).unwrap();
    for i in 0..imported.built.mesh.vertex_count() {
        let orig = imported.built.mesh.vertex(i).unwrap();
        let back = re_ascii.built.mesh.vertex(i).unwrap();
        assert_eq!(orig[0].to_bits(), back[0].to_bits(), "ascii vertex {i}.x bits differ");
        assert_eq!(orig[1].to_bits(), back[1].to_bits(), "ascii vertex {i}.y bits differ");
        assert_eq!(orig[2].to_bits(), back[2].to_bits(), "ascii vertex {i}.z bits differ");
    }

    // Verify f32 rounding report detects f64->f32 loss when coordinates are not exact f32.
    let mut lossy_builder = fid_io_mesh::MeshBuilder::new(SourcePrecision::F64);
    let mut bl = default_budget();
    let pos = fid_io_reg::Position::byte(0);
    lossy_builder.push_vertex([0.123_456_789_012_345, 0.0, 0.0], &mut bl, pos).unwrap();
    lossy_builder.push_vertex([1.0, 0.0, 0.0], &mut bl, pos).unwrap();
    lossy_builder.push_vertex([0.0, 1.0, 0.0], &mut bl, pos).unwrap();
    lossy_builder.push_face_u64([0, 1, 2], &mut bl, pos).unwrap();
    let lossy_mesh = lossy_builder.build_mesh(frame0(), &mut bl, pos).unwrap().mesh;
    let mut s_lossy = VecSink(Vec::new());
    let lossy_summary = write_stl(&lossy_mesh, &StlExportOptions::default(), &mut s_lossy).unwrap();
    assert!(lossy_summary.precision.max_abs_m > 0.0);
    assert!(lossy_summary.precision.max_abs_m < 1e-7);
}

#[test]
fn export_command_passes_shared_conformance_kit() {
    let bin_bytes = fixture_bytes("cube_bin.stl");
    let mut b = default_budget();
    let mesh = read_stl_bytes(&bin_bytes, LengthUnit::Mm, frame0(), &mut b).unwrap().built.mesh;

    // Compute expected binary default export SHA-256 independently via write_stl.
    let mut sink = VecSink(Vec::new());
    write_stl(&mesh, &StlExportOptions::default(), &mut sink).unwrap();
    let sha = Digest::from_bytes(Sha256::digest(&sink.0).into());

    let reg = setup_registry();
    let tmp = TmpRoots::new("exp_kit");
    let roots = tmp.roots();
    let out_dir = tmp.out_dir();
    let fmt = stl_format_id().unwrap();
    let cmd = export_mesh_stl_command().unwrap();
    let defs = stl_command_defs(&reg).unwrap();
    assert_eq!(defs.len(), 2);
    assert_eq!(format!("{}@{}", defs[0].id, defs[0].major), "import.mesh_stl@1");
    assert_eq!(format!("{}@{}", defs[1].id, defs[1].major), "export.mesh_stl@1");

    let source = SourceRef::One("11111111-2222-5333-8444-555555555555".to_string());
    let default_opts = CValue::Obj(BTreeMap::new());
    let variants = vec![
        fid_canon::parse(b"{\"encoding\":\"ascii\"}").unwrap(),
        fid_canon::parse(b"{\"unit\":\"mm\"}").unwrap(),
        fid_canon::parse(b"{\"encoding\":\"yaml\"}").unwrap(),
        fid_canon::parse(b"{\"unknown\":1}").unwrap(),
    ];
    let fx = export_conformance::Fixture {
        registry: &reg,
        roots: &roots,
        root_id: "out",
        root_dir: &out_dir,
        format: &fmt,
        file_name: "cube.stl",
        source: &source,
        options: &default_opts,
        expected_sha256: &sha,
        option_variants: &variants,
        fail_after: 100,
    };
    export_conformance::check(&cmd, &defs[1], &mesh, &fx).unwrap();

    // Also verify ExportEnv direct ASCII export with precision loss warning works.
    let exp_env = ExportEnv::new(&reg, &roots, Principal::Agent);
    let mut ep = ExportParams::new(source, None, "root://out/cube_ascii.stl");
    ep.options = fid_canon::parse(b"{\"encoding\":\"ascii\",\"unit\":\"mm\"}").unwrap();
    let prep = cmd.prepare(&exp_env, &ep, &mesh).unwrap();
    let placed = prep.place().unwrap();
    assert_eq!(placed.report.format(), &fmt);
    let written = fs::read(out_dir.join("cube_ascii.stl")).unwrap();
    assert!(written.starts_with(b"solid "));
}

#[test]
fn ac8_exact_replay_is_identical() {
    let reg = setup_registry();
    let tmp = TmpRoots::new("ac8");
    let roots = tmp.roots();
    let (tree_before, ns) = setup_project();
    let mut tree_after = tree_before.clone();
    let cmd = import_mesh_stl_command().unwrap();
    let mut blobs = MemoryBlobs::default();

    let env = ImportEnv::new(&reg, &roots, Principal::Agent, principal_commit(), &tree_before, ns);
    let mut params = ImportParams::new("root://in/cube_bin.stl");
    params.unit = Some(LengthUnit::Mm);

    let prepared = cmd.prepare(&env, &params, Some(&mut blobs as &mut dyn BlobSink)).unwrap();
    let recorded_events = prepared.events().to_vec();
    let out = prepared.commit(&mut tree_after).unwrap();

    let replay_env = ImportEnv::new(&reg, &roots, Principal::Agent, principal_commit(), &tree_before, ns);
    let outcome = cmd.replay(&replay_env, &out.event, &recorded_events).unwrap();
    assert_eq!(outcome, ReplayOutcome::Identical);

    // Import definition check (W2, read_root, D0, head_move, source path_like + x-extensions=["stl"]).
    let defs = stl_command_defs(&reg).unwrap();
    let def_v = defs[0].to_value();
    assert_eq!(cval_str(&def_v, &["side_effect"]), "W2");
    assert_eq!(cval_str(&def_v, &["fs_effect"]), "read_root");
    assert_eq!(cval_str(&def_v, &["determinism"]), "D0");
    assert_eq!(cval_str(&def_v, &["undo"]), "head_move");

    // Also verify dry_run leaves tree and blobs untouched.
    let mut dry_blobs = MemoryBlobs::default();
    let mut dry_params = ImportParams::new("root://in/cube_bin.stl");
    dry_params.unit = Some(LengthUnit::Mm);
    dry_params.dry_run = true;
    let dry_prep = cmd.prepare(&env, &dry_params, Some(&mut dry_blobs as &mut dyn BlobSink)).unwrap();
    assert!(dry_prep.report().dry_run());
    assert!(dry_prep.events().is_empty());
    assert!(dry_blobs.blobs.is_empty());
    let mut tree_dry = tree_before.clone();
    let _ = dry_prep.commit(&mut tree_dry).unwrap();
    assert_eq!(tree_dry.state_digest().unwrap(), tree_before.state_digest().unwrap());

    // Double import into same stage is refused (id_collision) and changes nothing.
    let env_after = ImportEnv::new(&reg, &roots, Principal::Agent, principal_commit(), &tree_after, ns);
    let dup = cmd.prepare(&env_after, &params, None);
    assert!(dup.is_err());

    // Confirm import::prepare_import also works via generic import.file@1 with content detection.
    let mut gen_params = ImportParams::new("root://in/cube_bin_solid_header.stl");
    gen_params.unit = Some(LengthUnit::Mm);
    let mut gen_blobs = MemoryBlobs::default();
    let gen_prep = import::prepare_import(
        &env,
        &import_file_id().unwrap(),
        cmd.reader(),
        &gen_params,
        Some(&mut gen_blobs as &mut dyn BlobSink),
    )
    .unwrap();
    assert_eq!(gen_prep.report().entity_counts().get(&TypeName::new("mesh.face").unwrap()).copied(), Some(12));
}

#[test]
fn streaming_binary_reader_handles_chunk_boundaries_and_limits() {
    // 5,000 triangles (> 3 chunks of 1,310 records) streamed through read_stl_stream.
    let n: u32 = 5_000;
    let total_len = 84u64 + 50u64 * u64::from(n);
    let mut data = Vec::with_capacity(usize::try_from(total_len).unwrap());
    let mut header = [0u8; 80];
    header[..5].copy_from_slice(b"solid"); // binary with `solid` prefix
    data.extend_from_slice(&header);
    data.extend_from_slice(&n.to_le_bytes());
    for i in 0..n {
        let z = (i as f32) * 0.25;
        let rec: [f32; 12] = [0.0, 0.0, 1.0, 0.0, 0.0, z, 1.0, 0.0, z, 0.0, 1.0, z];
        for f in rec {
            data.extend_from_slice(&f.to_le_bytes());
        }
        data.extend_from_slice(&0u16.to_le_bytes());
    }

    let mut b = default_budget();
    let mut cursor = Cursor::new(data.as_slice());
    let res = read_stl_stream(&mut cursor, Some(total_len), LengthUnit::Mm, frame0(), &mut b).unwrap();
    assert_eq!(res.encoding, StlEncoding::Binary);
    assert_eq!(res.built.counts.faces, 5_000);
    assert_eq!(res.built.counts.vertices, 15_000);
    assert_eq!(res.built.source_precision, SourcePrecision::F32);

    // Budget element cap fails cleanly with limit_exceeded.
    let tight = LimitRequest { elements: Some(1_000), ..LimitRequest::default() };
    let mut b_tight = Budget::new(Principal::Human, &tight).unwrap();
    let mut cur2 = Cursor::new(data.as_slice());
    let err = read_stl_stream(&mut cur2, Some(total_len), LengthUnit::Mm, frame0(), &mut b_tight).unwrap_err();
    assert!(matches!(err, IoError::LimitExceeded { limit: LimitKind::Elements, .. }));
}

#[test]
#[ignore = "AC-7 benchmark: 10^7 triangles (< 5 s on reference hardware); run with --release -- --ignored"]
fn ac7_ten_million_triangles_benchmark() {
    use std::io::{self, Read};
    use std::time::Instant;

    struct SynthBinaryStl {
        pos: u64,
        total_bytes: u64,
        header: [u8; 84],
        record: [u8; 50],
    }

    impl SynthBinaryStl {
        fn new(total_tris: u32) -> Self {
            let mut header = [0u8; 84];
            header[..12].copy_from_slice(b"seeded bench");
            header[80..84].copy_from_slice(&total_tris.to_le_bytes());
            let mut record = [0u8; 50];
            let vals: [f32; 12] = [0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0];
            for (idx, f) in vals.into_iter().enumerate() {
                record[idx * 4..idx * 4 + 4].copy_from_slice(&f.to_le_bytes());
            }
            SynthBinaryStl { pos: 0, total_bytes: 84 + 50 * u64::from(total_tris), header, record }
        }
    }

    impl Read for SynthBinaryStl {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let rem = self.total_bytes.saturating_sub(self.pos);
            if rem == 0 {
                return Ok(0);
            }
            let mut written = 0usize;
            let limit = usize::try_from(rem.min(buf.len() as u64)).unwrap_or(buf.len());
            while written < limit {
                if self.pos < 84 {
                    let off = self.pos as usize;
                    let n = (84 - off).min(limit - written);
                    buf[written..written + n].copy_from_slice(&self.header[off..off + n]);
                    written += n;
                    self.pos += n as u64;
                } else {
                    let off = ((self.pos - 84) % 50) as usize;
                    let n = (50 - off).min(limit - written);
                    buf[written..written + n].copy_from_slice(&self.record[off..off + n]);
                    written += n;
                    self.pos += n as u64;
                }
            }
            Ok(written)
        }
    }

    let n = 10_000_000u32;
    let total_bytes = 84 + 50 * u64::from(n);
    let req = LimitRequest {
        bytes: Some(total_bytes + 1024),
        alloc: Some(4 * 1024 * 1024 * 1024),
        elements: Some(50_000_000),
        wall_ms: Some(60_000),
        ..LimitRequest::default()
    };
    let mut budget = Budget::new(Principal::Human, &req).unwrap();
    let mut synth = SynthBinaryStl::new(n);
    let t0 = Instant::now();
    let res = read_stl_stream(&mut synth, Some(total_bytes), LengthUnit::Mm, frame0(), &mut budget).unwrap();
    let elapsed = t0.elapsed();
    assert_eq!(res.built.counts.faces, u64::from(n));
    assert_eq!(res.built.counts.vertices, 3 * u64::from(n));
    assert!(elapsed.as_secs_f64() < 5.0, "10M binary STL took {:.3}s (> 5 s)", elapsed.as_secs_f64());
}
