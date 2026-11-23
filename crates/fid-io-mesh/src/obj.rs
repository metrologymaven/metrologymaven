//! Wavefront OBJ geometry reader and writer (spec node `IO.MESH.OBJ`).
//!
//! - Imports `v`, `vn`, `f` (all index forms `i`, `i/t`, `i//n`, `i/t/n`, including negative
//!   relative indices), plus `o` and `g` names as untrusted metadata (`REQ-001`, `REQ-002`).
//! - Fan-triangulates polygonal faces with `n > 3` vertices as `(0, i, i + 1)` and counts the
//!   number of faces triangulated (`REQ-001`).
//! - Counts every ignored statement by keyword (`vt`, `mtllib`, `usemtl`, `s`, `l`, `p`, ...)
//!   in the import report and NEVER opens files referenced by `mtllib` (`REQ-003`).
//! - Parses coordinates as `f64` with exact rounding (`SourcePrecision::F64`) and writes
//!   shortest round-trip decimals so export -> re-import is bit-identical (`REQ-004`).
//! - Bounded line tokenizer (`MAX_OBJ_LINE_BYTES = 4096`), zero panics, zero `unwrap`,
//!   zero `unsafe` (`CONSTITUTION §8`).
//! - Wired onto `fid-io-reg` import/export skeletons as `import.mesh_obj@1` (W2) and
//!   `export.mesh_obj@1` (W1, `fs_effect: write_root`).

use crate::builder::{BuiltMesh, MeshBuilder, SourcePrecision};
use crate::error::MeshBuildError;
use crate::stl::{encode_staged_mesh_payload, STL_ELEMENT_TYPE};
use fid_canon::CValue;
use fid_geo_frame::LengthUnit;
use fid_geo_types::{FrameRef, Mesh};
use fid_io_reg::detect::{FormatDescriptor, Origin, Phase, Registry, VersionRange};
use fid_io_reg::env::export::{self, ByteSink, ExportCommand, ExportError, Writer};
use fid_io_reg::env::import::{self, ImportCommand, ImportError, Reader, StageSink};
use fid_io_reg::prov::{to_metres, ReaderId, ReaderInfo, VersionToken};
use fid_io_reg::report::{Code, FormatId, ReportBuilder, ReportPos, TypeName};
use fid_io_reg::roots::Direction;
use fid_io_reg::{Budget, IoError, LimitKind, Position, UntrustedText};
use fid_registry::CommandDef;
use std::collections::BTreeMap;

/// Registered format ID (`mesh-obj` -> commands `import.mesh_obj@1` and `export.mesh_obj@1`).
pub const OBJ_FORMAT_ID: &str = "mesh-obj";
/// Maximum OBJ line length in bytes (Design: 4 KiB).
pub const MAX_OBJ_LINE_BYTES: usize = 4096;
/// Maximum vertices in a single polygonal face (defensive bound, CONSTITUTION §8).
pub const MAX_FACE_VERTICES: usize = 1_024;
/// Maximum stored object/group metadata names.
const MAX_META_NAMES: usize = 256;

/// Output of direct OBJ decoding into a GEO [`Mesh`].
#[derive(Debug, Clone, PartialEq)]
pub struct ObjImportResult {
    /// Validated GEO triangle mesh, source precision (`F64`) and exact defect counts.
    pub built: BuiltMesh,
    /// Number of input faces with `n > 3` vertices that were fan-triangulated (`REQ-001`).
    pub triangulated_faces: u64,
    /// Parsed `vn` vertex normals in file order.
    pub normals: Vec<[f64; 3]>,
    /// Object (`o`) names as untrusted metadata.
    pub object_names: Vec<UntrustedText>,
    /// Group (`g`) names as untrusted metadata.
    pub group_names: Vec<UntrustedText>,
    /// Counts of ignored OBJ statements keyed by lowercase statement keyword (`REQ-003`).
    pub ignored_statements: BTreeMap<String, u64>,
}

/// Export options for [`write_obj`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjExportOptions {
    /// Coordinate unit written into the OBJ file (default: metres).
    pub unit: LengthUnit,
    /// Emit per-face `vn` normals and `f v//n` references when `true`.
    pub normals: bool,
    /// Object name written in `o <name>`.
    pub object_name: String,
}

impl Default for ObjExportOptions {
    fn default() -> Self {
        ObjExportOptions { unit: LengthUnit::M, normals: false, object_name: "mesh".to_string() }
    }
}

/// Summary returned by [`write_obj`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjExportSummary {
    pub unit: LengthUnit,
    pub normals_written: bool,
    pub vertices: u64,
    pub triangles: u64,
    pub bytes: u64,
}

// ------------------------------------------------------------------ helpers

fn unit_scale_to_metres(unit: LengthUnit) -> f64 {
    to_metres(1.0, unit).unwrap_or(1.0)
}

fn parse_unit_symbol(s: &str) -> Option<LengthUnit> {
    LengthUnit::ALL.iter().copied().find(|u| u.symbol() == s)
}

fn map_build_err(err: MeshBuildError, at: Position) -> IoError {
    err.to_io_error(at)
}

fn parse_f64_coord(tok: &str, at: Position) -> Result<f64, IoError> {
    let v = tok.parse::<f64>().map_err(|_| IoError::Malformed { at, reason: "invalid_float" })?;
    if !v.is_finite() {
        return Err(IoError::Malformed { at, reason: "non_finite_coordinate" });
    }
    Ok(v)
}

/// Resolves a 1-based positive or negative (relative) OBJ index against `current_count`.
/// Fails with `IoError::Malformed` at `at` if zero or out of range (`REQ-002`).
fn resolve_obj_index(raw: &str, current_count: u64, at: Position) -> Result<u64, IoError> {
    let idx = raw.parse::<i64>().map_err(|_| IoError::Malformed { at, reason: "invalid_index" })?;
    if idx == 0 {
        return Err(IoError::Malformed { at, reason: "zero_index" });
    }
    if idx > 0 {
        let zero_based = u64::try_from(idx - 1).map_err(|_| IoError::Malformed { at, reason: "index_out_of_range" })?;
        if zero_based >= current_count {
            return Err(IoError::Malformed { at, reason: "index_out_of_range" });
        }
        Ok(zero_based)
    } else {
        // Negative relative index: -1 is the last vertex (`current_count - 1`).
        let back = idx.unsigned_abs();
        if back > current_count {
            return Err(IoError::Malformed { at, reason: "index_out_of_range" });
        }
        Ok(current_count - back)
    }
}

/// Parses one face corner token (`v`, `v/vt`, `v//vn`, `v/vt/vn`) and validates both vertex
/// index and optional normal index against current counts (`REQ-002`).
fn parse_face_corner(tok: &str, vertex_count: u64, normal_count: u64, at: Position) -> Result<u64, IoError> {
    let mut parts = tok.split('/');
    let Some(v_tok) = parts.next() else {
        return Err(IoError::Malformed { at, reason: "empty_face_vertex" });
    };
    let v_idx = resolve_obj_index(v_tok, vertex_count, at)?;
    let _vt_tok = parts.next(); // texture coordinate index ignored
    if let Some(vn_tok) = parts.next() {
        if !vn_tok.is_empty() {
            let _ = resolve_obj_index(vn_tok, normal_count, at)?;
        }
    }
    if parts.next().is_some() {
        return Err(IoError::Malformed { at, reason: "malformed_face_corner" });
    }
    Ok(v_idx)
}

fn sanitize_keyword_for_type_name(kw: &str) -> String {
    let mut out = String::new();
    for b in kw.bytes() {
        let low = b.to_ascii_lowercase();
        if low.is_ascii_lowercase() || low.is_ascii_digit() || low == b'_' {
            out.push(char::from(low));
        } else {
            out.push('_');
        }
        if out.len() >= 32 {
            break;
        }
    }
    if out.is_empty() || !out.as_bytes().first().is_some_and(u8::is_ascii_lowercase) {
        format!("k_{out}")
    } else {
        out
    }
}

fn sanitize_object_name(raw: &str) -> String {
    let cleaned: String =
        raw.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')).take(64).collect();
    if cleaned.is_empty() {
        "mesh".to_string()
    } else {
        cleaned
    }
}

// ------------------------------------------------------------------ reader

/// Decodes Wavefront OBJ bytes into a validated GEO [`Mesh`] in metres (`REQ-001..004`).
/// Never touches the filesystem and never opens files named by `mtllib`.
pub fn read_obj_bytes(
    bytes: &[u8],
    unit: LengthUnit,
    frame: FrameRef,
    budget: &mut Budget,
) -> Result<ObjImportResult, IoError> {
    let scale = unit_scale_to_metres(unit);
    let mut builder = MeshBuilder::new(SourcePrecision::F64);
    let mut normals: Vec<[f64; 3]> = Vec::new();
    let mut object_names: Vec<UntrustedText> = Vec::new();
    let mut group_names: Vec<UntrustedText> = Vec::new();
    let mut ignored_statements: BTreeMap<String, u64> = BTreeMap::new();
    let mut triangulated_faces: u64 = 0;

    let mut line_no: u32 = 0;
    let mut offset: usize = 0;
    let mut face_verts: Vec<u64> = Vec::with_capacity(8);

    while offset < bytes.len() {
        line_no = line_no.saturating_add(1);
        let line_start = offset;
        let rest = bytes.get(offset..).unwrap_or(&[]);
        let nl_rel = rest.iter().position(|&b| b == b'\n');
        let (raw_line, next_offset) = match nl_rel {
            Some(pos) => (rest.get(..pos).unwrap_or(&[]), offset + pos + 1),
            None => (rest, bytes.len()),
        };
        let line_bytes = raw_line.strip_suffix(b"\r").unwrap_or(raw_line);
        let at = Position::line_col(u64::try_from(line_start).unwrap_or(u64::MAX), line_no, 1);
        if line_bytes.len() > MAX_OBJ_LINE_BYTES {
            return Err(IoError::Malformed { at, reason: "line_too_long" });
        }
        let line = std::str::from_utf8(line_bytes).map_err(|_| IoError::Malformed { at, reason: "invalid_utf8" })?;
        offset = next_offset;

        // Strip `#` comment suffix and surrounding whitespace.
        let body = match line.find('#') {
            Some(pos) => line.get(..pos).unwrap_or(line),
            None => line,
        }
        .trim();
        if body.is_empty() {
            continue;
        }

        let mut iter = body.split_ascii_whitespace();
        let Some(kw) = iter.next() else {
            continue;
        };

        match kw {
            "v" => {
                let (Some(sx), Some(sy), Some(sz)) = (iter.next(), iter.next(), iter.next()) else {
                    return Err(IoError::Malformed { at, reason: "expected_three_vertex_coords" });
                };
                // Optional 4th homogeneous coordinate `w` is accepted if finite; >4 numbers refused.
                if let Some(sw) = iter.next() {
                    let _ = parse_f64_coord(sw, at)?;
                    if iter.next().is_some() {
                        return Err(IoError::Malformed { at, reason: "too_many_vertex_coords" });
                    }
                }
                let x = parse_f64_coord(sx, at)? * scale;
                let y = parse_f64_coord(sy, at)? * scale;
                let z = parse_f64_coord(sz, at)? * scale;
                builder.push_vertex([x, y, z], budget, at).map_err(|e| map_build_err(e, at))?;
            }
            "vn" => {
                let (Some(nx), Some(ny), Some(nz)) = (iter.next(), iter.next(), iter.next()) else {
                    return Err(IoError::Malformed { at, reason: "expected_three_normal_coords" });
                };
                if iter.next().is_some() {
                    return Err(IoError::Malformed { at, reason: "too_many_normal_coords" });
                }
                let n = [parse_f64_coord(nx, at)?, parse_f64_coord(ny, at)?, parse_f64_coord(nz, at)?];
                budget.charge_alloc(24, at)?;
                normals.try_reserve(1).map_err(|_| IoError::LimitExceeded {
                    limit: LimitKind::Alloc,
                    value: budget.limits().alloc(),
                    at,
                })?;
                normals.push(n);
            }
            "f" => {
                face_verts.clear();
                let v_count = builder.vertex_count();
                let n_count = u64::try_from(normals.len()).unwrap_or(u64::MAX);
                for tok in iter {
                    if face_verts.len() >= MAX_FACE_VERTICES {
                        return Err(IoError::Malformed { at, reason: "face_too_many_vertices" });
                    }
                    face_verts.push(parse_face_corner(tok, v_count, n_count, at)?);
                }
                if face_verts.len() < 3 {
                    return Err(IoError::Malformed { at, reason: "face_too_few_vertices" });
                }
                if face_verts.len() > 3 {
                    triangulated_faces = triangulated_faces.saturating_add(1);
                }
                // Fan triangulation `(0, i, i + 1)` (`REQ-001`).
                let Some(&v0) = face_verts.first() else {
                    return Err(IoError::Malformed { at, reason: "face_too_few_vertices" });
                };
                for i in 1..(face_verts.len() - 1) {
                    let (Some(&v1), Some(&v2)) = (face_verts.get(i), face_verts.get(i + 1)) else {
                        return Err(IoError::Malformed { at, reason: "face_index_error" });
                    };
                    builder.push_face_u64([v0, v1, v2], budget, at).map_err(|e| map_build_err(e, at))?;
                }
            }
            "o" => {
                let name = body.get(kw.len()..).unwrap_or("").trim();
                if !name.is_empty() && object_names.len() < MAX_META_NAMES {
                    object_names.push(UntrustedText::new(name));
                }
            }
            "g" => {
                let name = body.get(kw.len()..).unwrap_or("").trim();
                if !name.is_empty() && group_names.len() < MAX_META_NAMES {
                    group_names.push(UntrustedText::new(name));
                }
            }
            other => {
                // REQ-003: count every ignored statement by keyword; never open `mtllib` files.
                let key = other.to_ascii_lowercase();
                let entry = ignored_statements.entry(key).or_insert(0);
                *entry = entry.saturating_add(1);
            }
        }
    }

    let end_pos = Position::line_col(u64::try_from(bytes.len()).unwrap_or(u64::MAX), line_no.max(1), 1);
    let built = builder.build_mesh(frame, budget, end_pos).map_err(|e| map_build_err(e, end_pos))?;
    Ok(ObjImportResult { built, triangulated_faces, normals, object_names, group_names, ignored_statements })
}

// ------------------------------------------------------------------ writer

fn compute_face_normal_f64(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> [f64; 3] {
    let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let nx = ab[1] * ac[2] - ab[2] * ac[1];
    let ny = ab[2] * ac[0] - ab[0] * ac[2];
    let nz = ab[0] * ac[1] - ab[1] * ac[0];
    let len_sq = nx * nx + ny * ny + nz * nz;
    if !(len_sq.is_finite() && len_sq > 0.0) {
        return [0.0, 0.0, 0.0];
    }
    let len = len_sq.sqrt();
    if !(len.is_finite() && len > 0.0) {
        return [0.0, 0.0, 0.0];
    }
    [nx / len, ny / len, nz / len]
}

fn format_shortest_f64(v: f64) -> String {
    let clean = if v == 0.0 { 0.0 } else { v };
    format!("{clean}")
}

/// Writes `mesh` as Wavefront OBJ into `sink` using shortest round-trip `f64` decimals
/// (`REQ-004`). When `opts.normals` is true, emits per-face `vn` normals and `f i//n` indices.
pub fn write_obj(mesh: &Mesh, opts: &ObjExportOptions, sink: &mut dyn ByteSink) -> Result<ObjExportSummary, IoError> {
    let start_bytes = sink.bytes_written();
    let inv = unit_scale_to_metres(opts.unit);
    let name = sanitize_object_name(&opts.object_name);

    let mut out = String::with_capacity(64 * 1024);
    out.push_str("# fiducial wavefront obj\no ");
    out.push_str(&name);
    out.push('\n');

    let v_count = mesh.vertex_count();
    for i in 0..v_count {
        let p = mesh.vertex(i).ok_or(IoError::Malformed { at: Position::byte(0), reason: "vertex_missing" })?;
        let x = format_shortest_f64(p[0] / inv);
        let y = format_shortest_f64(p[1] / inv);
        let z = format_shortest_f64(p[2] / inv);
        use std::fmt::Write as _;
        let _ = writeln!(out, "v {x} {y} {z}");
        if out.len() >= 60 * 1024 {
            sink.write_bytes(out.as_bytes())?;
            out.clear();
        }
    }

    if opts.normals {
        for tri in mesh.triangles() {
            let a =
                mesh.vertex(tri[0]).ok_or(IoError::Malformed { at: Position::byte(0), reason: "vertex_missing" })?;
            let b =
                mesh.vertex(tri[1]).ok_or(IoError::Malformed { at: Position::byte(0), reason: "vertex_missing" })?;
            let c =
                mesh.vertex(tri[2]).ok_or(IoError::Malformed { at: Position::byte(0), reason: "vertex_missing" })?;
            let n = compute_face_normal_f64(a, b, c);
            use std::fmt::Write as _;
            let _ = writeln!(
                out,
                "vn {} {} {}",
                format_shortest_f64(n[0]),
                format_shortest_f64(n[1]),
                format_shortest_f64(n[2])
            );
            if out.len() >= 60 * 1024 {
                sink.write_bytes(out.as_bytes())?;
                out.clear();
            }
        }
    }

    for (idx, tri) in mesh.triangles().enumerate() {
        let i0 = tri[0].saturating_add(1);
        let i1 = tri[1].saturating_add(1);
        let i2 = tri[2].saturating_add(1);
        use std::fmt::Write as _;
        if opts.normals {
            let n_idx = u64::try_from(idx).unwrap_or(u64::MAX).saturating_add(1);
            let _ = writeln!(out, "f {i0}//{n_idx} {i1}//{n_idx} {i2}//{n_idx}");
        } else {
            let _ = writeln!(out, "f {i0} {i1} {i2}");
        }
        if out.len() >= 60 * 1024 {
            sink.write_bytes(out.as_bytes())?;
            out.clear();
        }
    }

    if !out.is_empty() {
        sink.write_bytes(out.as_bytes())?;
    }

    Ok(ObjExportSummary {
        unit: opts.unit,
        normals_written: opts.normals,
        vertices: v_count,
        triangles: mesh.triangle_count(),
        bytes: sink.bytes_written().saturating_sub(start_bytes),
    })
}

// ------------------------------------------------------------------ fid-io-reg integration

/// Returns the registered `FormatId` (`"mesh-obj"`).
pub fn obj_format_id() -> Result<FormatId, fid_io_reg::report::ReportError> {
    FormatId::new(OBJ_FORMAT_ID)
}

/// Format descriptor for OBJ import (extension-matched `.obj`; OBJ has no magic bytes).
pub fn obj_import_descriptor() -> Result<FormatDescriptor, fid_io_reg::DetectError> {
    let format =
        obj_format_id().map_err(|_| fid_io_reg::DetectError::Invalid { field: "format", reason: "bad format id" })?;
    let opts_schema = CValue::Obj(BTreeMap::from([
        ("additionalProperties".to_string(), CValue::Bool(false)),
        ("properties".to_string(), CValue::Obj(BTreeMap::new())),
        ("type".to_string(), CValue::Str("object".to_string())),
    ]));
    FormatDescriptor::new(
        format,
        VersionRange::new("1.0", "1.0")?,
        Vec::new(),
        vec!["obj".to_string()],
        opts_schema,
        Phase::new(1).ok_or(fid_io_reg::DetectError::Invalid { field: "phase", reason: "bad phase" })?,
        Origin::FirstParty,
        Direction::Import,
    )
}

/// Format descriptor for OBJ export (`normals: bool`, `unit: m|mm|um|in`).
pub fn obj_export_descriptor() -> Result<FormatDescriptor, fid_io_reg::DetectError> {
    let format =
        obj_format_id().map_err(|_| fid_io_reg::DetectError::Invalid { field: "format", reason: "bad format id" })?;
    let mut props = BTreeMap::new();
    props.insert(
        "normals".to_string(),
        CValue::Obj(BTreeMap::from([
            ("default".to_string(), CValue::Bool(false)),
            ("type".to_string(), CValue::Str("boolean".to_string())),
        ])),
    );
    props.insert(
        "unit".to_string(),
        CValue::Obj(BTreeMap::from([
            ("default".to_string(), CValue::Str("m".to_string())),
            (
                "enum".to_string(),
                CValue::Arr(LengthUnit::ALL.iter().map(|u| CValue::Str(u.symbol().to_string())).collect()),
            ),
            ("type".to_string(), CValue::Str("string".to_string())),
        ])),
    );
    let opts_schema = CValue::Obj(BTreeMap::from([
        ("additionalProperties".to_string(), CValue::Bool(false)),
        ("properties".to_string(), CValue::Obj(props)),
        ("type".to_string(), CValue::Str("object".to_string())),
    ]));
    FormatDescriptor::new(
        format,
        VersionRange::new("1.0", "1.0")?,
        Vec::new(),
        vec!["obj".to_string()],
        opts_schema,
        Phase::new(1).ok_or(fid_io_reg::DetectError::Invalid { field: "phase", reason: "bad phase" })?,
        Origin::FirstParty,
        Direction::Export,
    )
}

/// Registers both import and export descriptors for `mesh-obj` in `reg`.
pub fn register_obj(reg: &mut Registry) -> Result<(), IoError> {
    let imp =
        obj_import_descriptor().map_err(|_| IoError::Malformed { at: Position::byte(0), reason: "descriptor" })?;
    let exp =
        obj_export_descriptor().map_err(|_| IoError::Malformed { at: Position::byte(0), reason: "descriptor" })?;
    reg.register(imp)?;
    reg.register(exp)?;
    Ok(())
}

/// `fid-io-reg` import reader for `mesh-obj` (`import.mesh_obj@1`).
#[derive(Debug, Clone)]
pub struct ObjReader {
    format: FormatId,
    info: ReaderInfo,
}

impl ObjReader {
    /// Constructs the first-party OBJ reader (`fid-io-mesh-obj` v1.0.0).
    pub fn new() -> Result<Self, ImportError> {
        let format = obj_format_id().map_err(|_| ImportError::Internal("invalid mesh-obj format id"))?;
        let id = ReaderId::new("fid-io-mesh-obj").map_err(|_| ImportError::Internal("invalid obj reader id"))?;
        let version = VersionToken::new("1.0.0").map_err(|_| ImportError::Internal("invalid obj reader version"))?;
        Ok(ObjReader { format, info: ReaderInfo { id, version } })
    }
}

impl Reader for ObjReader {
    fn format(&self) -> &FormatId {
        &self.format
    }

    fn info(&self) -> &ReaderInfo {
        &self.info
    }

    fn options(&self, given: &CValue) -> Result<CValue, &'static str> {
        let CValue::Obj(o) = given else {
            return Err("options must be an object");
        };
        if !o.is_empty() {
            return Err("mesh-obj import takes no format options");
        }
        Ok(CValue::Obj(BTreeMap::new()))
    }

    fn read(&self, _opts: &CValue, sink: &mut StageSink<'_, '_>, report: &mut ReportBuilder) -> Result<(), IoError> {
        let mut raw = Vec::new();
        let mut scratch = vec![0u8; 64 * 1024];
        loop {
            let k = sink.source().read_bounded(&mut scratch)?;
            if k == 0 {
                break;
            }
            let at = Position::byte(u64::try_from(raw.len()).unwrap_or(u64::MAX));
            sink.source().budget_mut().charge_alloc(u64::try_from(k).unwrap_or(u64::MAX), at)?;
            raw.try_reserve(k).map_err(|_| IoError::LimitExceeded {
                limit: LimitKind::Alloc,
                value: sink.source().budget_mut().limits().alloc(),
                at,
            })?;
            raw.extend_from_slice(&scratch[..k]);
        }

        let frame = FrameRef::from_bytes([0u8; 16]);
        let decoded = read_obj_bytes(&raw, LengthUnit::M, frame, sink.source().budget_mut())?;
        drop(raw);

        if let Ok(ver) = VersionToken::new("1.0") {
            sink.declare_format_version(ver);
        }
        sink.declare("source_precision", UntrustedText::new(decoded.built.source_precision.as_str()));
        if let Some(first_obj) = decoded.object_names.first() {
            sink.declare("object", first_obj.clone());
        }
        if let Some(first_grp) = decoded.group_names.first() {
            sink.declare("group", first_grp.clone());
        }

        if let Ok(tn) = TypeName::new("mesh.face") {
            report.count(&tn, decoded.built.counts.faces);
        }
        if let Ok(tn) = TypeName::new("mesh.vertex") {
            report.count(&tn, decoded.built.counts.vertices);
        }
        if decoded.triangulated_faces > 0 {
            if let Ok(tn) = TypeName::new("mesh.triangulated_face") {
                report.count(&tn, decoded.triangulated_faces);
            }
        }
        // REQ-003: report ignored statements counted by keyword.
        for (kw, count) in &decoded.ignored_statements {
            let safe_kw = sanitize_keyword_for_type_name(kw);
            if let Ok(tn) = TypeName::new(&format!("obj.ignored.{safe_kw}")) {
                report.count(&tn, *count);
            }
            if let (Ok(tn), Ok(pos), Ok(code)) = (
                TypeName::new(&format!("obj.{safe_kw}")),
                ReportPos::new(Position::byte(0)),
                Code::new("ignored_statement"),
            ) {
                report.drop_item(&tn, pos, &code, &UntrustedText::new(&format!("{kw}:{count}")));
            }
        }

        let entity_name = decoded.object_names.first().cloned().unwrap_or_else(|| UntrustedText::new("obj-mesh"));
        let payload = encode_staged_mesh_payload(&decoded.built.mesh);
        sink.add_entity(STL_ELEMENT_TYPE, &entity_name, &payload, Position::byte(0), report)?;
        Ok(())
    }
}

/// `fid-io-reg` export writer for `mesh-obj` (`export.mesh_obj@1`).
#[derive(Debug, Clone)]
pub struct ObjWriter {
    format: FormatId,
    version: VersionToken,
}

impl ObjWriter {
    /// Constructs the first-party OBJ writer (`1.0.0`).
    pub fn new() -> Result<Self, ExportError> {
        let format = obj_format_id().map_err(|_| ExportError::Internal("invalid mesh-obj format id"))?;
        let version = VersionToken::new("1.0.0").map_err(|_| ExportError::Internal("invalid obj writer version"))?;
        Ok(ObjWriter { format, version })
    }
}

fn parse_export_opts(v: &CValue) -> Result<ObjExportOptions, &'static str> {
    let CValue::Obj(o) = v else {
        return Err("options must be an object");
    };
    if o.keys().any(|k| !matches!(k.as_str(), "normals" | "unit")) {
        return Err("unknown mesh-obj export option");
    }
    let normals = match o.get("normals") {
        None => false,
        Some(CValue::Bool(b)) => *b,
        Some(_) => return Err("normals must be a boolean"),
    };
    let unit = match o.get("unit") {
        None => LengthUnit::M,
        Some(CValue::Str(s)) => parse_unit_symbol(s).ok_or("unknown length unit")?,
        Some(_) => return Err("unit must be a string"),
    };
    Ok(ObjExportOptions { unit, normals, object_name: "mesh".to_string() })
}

impl Writer for ObjWriter {
    type Source = Mesh;

    fn format(&self) -> &FormatId {
        &self.format
    }

    fn version(&self) -> &VersionToken {
        &self.version
    }

    fn options(&self, given: &CValue) -> Result<CValue, &'static str> {
        let parsed = parse_export_opts(given)?;
        Ok(CValue::Obj(BTreeMap::from([
            ("normals".to_string(), CValue::Bool(parsed.normals)),
            ("unit".to_string(), CValue::Str(parsed.unit.symbol().to_string())),
        ])))
    }

    fn write(
        &self,
        src: &Mesh,
        opts: &CValue,
        sink: &mut dyn ByteSink,
        _report: &mut ReportBuilder,
    ) -> Result<(), IoError> {
        let parsed = parse_export_opts(opts)
            .map_err(|_| IoError::Malformed { at: Position::byte(0), reason: "invalid_export_options" })?;
        let _ = write_obj(src, &parsed, sink)?;
        Ok(())
    }
}

/// Builds the `import.mesh_obj@1` command on the `fid-io-reg` import skeleton.
pub fn import_mesh_obj_command() -> Result<ImportCommand<ObjReader>, ImportError> {
    ImportCommand::new(ObjReader::new()?, &[])
}

/// Builds the `export.mesh_obj@1` command on the `fid-io-reg` export skeleton.
pub fn export_mesh_obj_command() -> Result<ExportCommand<ObjWriter>, ExportError> {
    ExportCommand::new(ObjWriter::new()?, &[])
}

/// Builds the registry [`CommandDef`]s for `import.mesh_obj@1` and `export.mesh_obj@1`.
pub fn obj_command_defs(reg: &Registry) -> Result<Vec<CommandDef>, String> {
    let fmt = obj_format_id().map_err(|e| e.to_string())?;
    let imp = import::command::format_def(reg, &fmt, &[]).map_err(|e| e.to_string())?;
    let exp = export::command::format_def(reg, &fmt, &[]).map_err(|e| e.to_string())?;
    Ok(vec![imp, exp])
}
