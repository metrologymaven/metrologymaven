//! STL reader and writer, ASCII and binary (spec node `IO.MESH.STL`).
//!
//! - Detection (`REQ-IO.MESH.STL-001`): binary iff `byte_len == 84 + 50 * n` where `n` is
//!   the little-endian `u32` at bytes `80..84`; ASCII otherwise, regardless of whether the
//!   binary header begins with `solid`.
//! - Truncated binary files (`len < 84 + 50 * n` for plausible `n`) fail with
//!   `truncated_file`; malformed ASCII lines fail with `malformed` carrying the 1-based line
//!   number (`REQ-IO.MESH.STL-002`).
//! - Coordinates are parsed as `f32`, widened losslessly to `f64` before unit conversion,
//!   and recorded with `SourcePrecision::F32` (`REQ-IO.MESH.STL-003`).
//! - Export produces binary STL by default or ASCII when `encoding = "ascii"`, computing
//!   and reporting the `f32` rounding error via [`crate::f32_rounding_report`]
//!   (`REQ-IO.MESH.STL-004`).
//! - `import.mesh_stl@1` (W2) and `export.mesh_stl@1` (W1, `fs_effect: write_root`) are
//!   wired on the `fid-io-reg` import/export skeletons (`StlReader`, `StlWriter`).

use crate::builder::{BuiltMesh, MeshBuilder, SourcePrecision};
use crate::error::MeshBuildError;
use crate::precision::{f32_rounding_report, PrecisionReport};
use fid_canon::CValue;
use fid_geo_frame::LengthUnit;
use fid_geo_types::{FrameRef, Mesh};
use fid_io_reg::detect::{FormatDescriptor, Matcher, Origin, Phase, Predicate, Registry, Signature, VersionRange};
use fid_io_reg::env::export::{self, ByteSink, ExportCommand, ExportError, Writer};
use fid_io_reg::env::import::{self, ImportCommand, ImportError, Reader, StageSink};
use fid_io_reg::prov::{to_metres, ReaderId, ReaderInfo, VersionToken};
use fid_io_reg::report::{Code, FormatId, ReportBuilder, TypeName};
use fid_io_reg::roots::Direction;
use fid_io_reg::{Budget, IoError, LimitKind, Position, UntrustedText};
use fid_registry::CommandDef;
use std::collections::BTreeMap;
use std::io::Read;

/// Registered format ID (`mesh-stl` -> commands `import.mesh_stl@1` and `export.mesh_stl@1`).
pub const STL_FORMAT_ID: &str = "mesh-stl";
/// Element type ID staged for imported meshes.
pub const STL_ELEMENT_TYPE: &str = "geo.mesh";
/// Binary STL header size in bytes.
pub const BINARY_HEADER_LEN: usize = 80;
/// Binary STL header (80) + little-endian `u32` triangle count (4).
pub const BINARY_PREAMBLE_LEN: usize = 84;
/// Binary STL per-triangle record size in bytes (12 normal + 36 vertices + 2 attr).
pub const BINARY_RECORD_LEN: usize = 50;
/// Maximum ASCII STL line length in bytes (Design: 4 KiB).
pub const MAX_ASCII_LINE_BYTES: usize = 4096;
/// Number of 50-byte binary records read/written per ~64 KiB chunk (65,500 bytes).
const CHUNK_RECORDS: usize = 1310;
/// Maximum triangle count treated as a plausible truncated binary STL (100M triangles ~ 5 GB).
const MAX_PLAUSIBLE_TRIANGLES: u64 = 100_000_000;
/// Small-file threshold (16 MiB) below which streaming `Read` buffers bytes to check exact total length.
const BUFFER_DETECT_LIMIT: usize = 16 * 1024 * 1024;
/// Deterministic binary header written by the exporter (never starts with `"solid"`).
const EXPORT_BINARY_HEADER: &[u8] = b"fiducial binary stl";

/// STL wire encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StlEncoding {
    /// Little-endian binary STL (default for export).
    #[default]
    Binary,
    /// Human-readable ASCII STL.
    Ascii,
}

impl StlEncoding {
    /// Stable lowercase wire string (`"binary"` or `"ascii"`).
    pub const fn as_str(self) -> &'static str {
        match self {
            StlEncoding::Binary => "binary",
            StlEncoding::Ascii => "ascii",
        }
    }

    /// Parses `"binary"` or `"ascii"`.
    pub fn parse(s: &str) -> Option<StlEncoding> {
        match s {
            "binary" => Some(StlEncoding::Binary),
            "ascii" => Some(StlEncoding::Ascii),
            _ => None,
        }
    }
}

/// Output of direct STL decoding into a GEO [`Mesh`].
#[derive(Debug, Clone, PartialEq)]
pub struct StlImportResult {
    /// Validated GEO triangle mesh, source precision (`F32`) and exact defect counts.
    pub built: BuiltMesh,
    /// Detected encoding (`Binary` or `Ascii`).
    pub encoding: StlEncoding,
    /// Untrusted 80-byte binary header or ASCII solid name.
    pub header_text: UntrustedText,
}

/// Export options for [`write_stl`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StlExportOptions {
    /// Binary (default) or ASCII encoding.
    pub encoding: StlEncoding,
    /// Coordinate unit written into the STL file (default: metres).
    pub unit: LengthUnit,
    /// Solid name emitted in ASCII STL (`solid <name>`); sanitized to ASCII tokens.
    pub solid_name: String,
}

impl Default for StlExportOptions {
    fn default() -> Self {
        StlExportOptions { encoding: StlEncoding::Binary, unit: LengthUnit::M, solid_name: "mesh".to_string() }
    }
}

/// Summary returned by [`write_stl`].
#[derive(Debug, Clone, PartialEq)]
pub struct StlExportSummary {
    /// Encoding written.
    pub encoding: StlEncoding,
    /// Unit written.
    pub unit: LengthUnit,
    /// Total bytes emitted to the sink.
    pub bytes: u64,
    /// Number of triangles emitted.
    pub triangles: u64,
    /// Single-precision rounding error report in metres (`REQ-IO.MESH.STL-004`).
    pub precision: PrecisionReport,
}

// ------------------------------------------------------------------ unit helpers

fn unit_scale_to_metres(unit: LengthUnit) -> f64 {
    to_metres(1.0, unit).unwrap_or(1.0)
}

fn parse_unit_symbol(s: &str) -> Option<LengthUnit> {
    LengthUnit::ALL.iter().copied().find(|u| u.symbol() == s)
}

// ------------------------------------------------------------------ detection

fn is_ascii_solid_prefix(bytes: &[u8]) -> bool {
    let inspect = bytes.get(..bytes.len().min(512)).unwrap_or(bytes);
    // Any NUL byte in the leading prefix marks a binary file (including binary headers
    // that begin with "solid ...").
    if inspect.contains(&0) {
        return false;
    }
    let trimmed = match inspect.iter().position(|b| !b.is_ascii_whitespace()) {
        Some(pos) => inspect.get(pos..).unwrap_or(&[]),
        None => return false,
    };
    let Some(kw) = trimmed.get(..5) else {
        return false;
    };
    if !kw.eq_ignore_ascii_case(b"solid") {
        return false;
    }
    match trimmed.get(5) {
        None => true,
        Some(&b) => b.is_ascii_whitespace(),
    }
}

fn binary_expected_len(header84: &[u8]) -> Option<(u32, u64)> {
    let count_bytes: [u8; 4] = header84.get(80..84)?.try_into().ok()?;
    let n = u32::from_le_bytes(count_bytes);
    let expected = (BINARY_PREAMBLE_LEN as u64).checked_add(u64::from(n).checked_mul(BINARY_RECORD_LEN as u64)?)?;
    Some((n, expected))
}

/// Determines whether `bytes` is binary STL (`len == 84 + 50 * n`) or ASCII STL
/// (`REQ-IO.MESH.STL-001`), or returns `truncated_file` / `malformed` (`REQ-002`).
pub fn detect_stl_encoding(bytes: &[u8]) -> Result<StlEncoding, IoError> {
    let len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if bytes.len() >= BINARY_PREAMBLE_LEN {
        if let Some((_n, expected)) = binary_expected_len(bytes) {
            if expected == len {
                return Ok(StlEncoding::Binary);
            }
        }
    }
    if is_ascii_solid_prefix(bytes) {
        return Ok(StlEncoding::Ascii);
    }
    if bytes.is_empty() {
        return Err(IoError::TruncatedFile { at: Position::byte(0) });
    }
    if bytes.len() < BINARY_PREAMBLE_LEN {
        return Err(IoError::TruncatedFile { at: Position::byte(len) });
    }
    if let Some((n, expected)) = binary_expected_len(bytes) {
        let n64 = u64::from(n);
        if n64 > 0 && n64 <= MAX_PLAUSIBLE_TRIANGLES && len < expected {
            return Err(IoError::TruncatedFile { at: Position::byte(len) });
        }
    }
    Err(IoError::Malformed { at: Position::byte(0), reason: "stl_size_mismatch" })
}

// ------------------------------------------------------------------ binary reader

fn decode_header_text(header80: &[u8]) -> UntrustedText {
    let trimmed = match header80.iter().position(|&b| b == 0) {
        Some(pos) => header80.get(..pos).unwrap_or(header80),
        None => header80,
    };
    let text = String::from_utf8_lossy(trimmed);
    UntrustedText::new(text.trim())
}

fn read_le_f32(rec: &[u8], off: usize, at: Position) -> Result<f32, IoError> {
    let raw: [u8; 4] = rec.get(off..off + 4).and_then(|s| s.try_into().ok()).ok_or(IoError::TruncatedFile { at })?;
    let val = f32::from_le_bytes(raw);
    if !val.is_finite() {
        return Err(IoError::Malformed { at, reason: "non_finite_coordinate" });
    }
    Ok(val)
}

fn map_build_err(err: MeshBuildError, at: Position) -> IoError {
    err.to_io_error(at)
}

/// Parses binary STL from `reader` after the 84-byte preamble has been read, streaming
/// 50-byte records in ~64 KiB chunks (`CHUNK_RECORDS = 1310`).
fn parse_binary_body<R: Read>(
    header80: &[u8; BINARY_HEADER_LEN],
    tri_count: u32,
    reader: &mut R,
    unit: LengthUnit,
    frame: FrameRef,
    budget: &mut Budget,
) -> Result<StlImportResult, IoError> {
    let n64 = u64::from(tri_count);
    let vert_count = n64.checked_mul(3).ok_or(IoError::LimitExceeded {
        limit: LimitKind::Elements,
        value: budget.limits().elements(),
        at: Position::byte(BINARY_PREAMBLE_LEN as u64),
    })?;

    let mut builder = MeshBuilder::new(SourcePrecision::F32);
    let preamble_pos = Position::byte(BINARY_PREAMBLE_LEN as u64);
    builder.reserve_vertices(vert_count, budget, preamble_pos).map_err(|e| map_build_err(e, preamble_pos))?;
    builder.reserve_faces(n64, budget, preamble_pos).map_err(|e| map_build_err(e, preamble_pos))?;

    let scale = unit_scale_to_metres(unit);
    let chunk_bytes = CHUNK_RECORDS * BINARY_RECORD_LEN;
    let mut chunk = vec![0u8; chunk_bytes];
    let mut remaining = n64;
    let mut byte_pos = BINARY_PREAMBLE_LEN as u64;
    let mut next_v = 0u64;

    while remaining > 0 {
        let batch = usize::try_from(remaining.min(CHUNK_RECORDS as u64)).unwrap_or(CHUNK_RECORDS);
        let need = batch * BINARY_RECORD_LEN;
        let buf = chunk.get_mut(..need).ok_or(IoError::Malformed { at: Position::byte(byte_pos), reason: "chunk" })?;
        read_exact_io(reader, buf, byte_pos)?;

        for rec in buf.chunks_exact(BINARY_RECORD_LEN) {
            let rec_pos = Position::byte(byte_pos);
            let v0 = [read_le_f32(rec, 12, rec_pos)?, read_le_f32(rec, 16, rec_pos)?, read_le_f32(rec, 20, rec_pos)?];
            let v1 = [read_le_f32(rec, 24, rec_pos)?, read_le_f32(rec, 28, rec_pos)?, read_le_f32(rec, 32, rec_pos)?];
            let v2 = [read_le_f32(rec, 36, rec_pos)?, read_le_f32(rec, 40, rec_pos)?, read_le_f32(rec, 44, rec_pos)?];
            builder.push_vertex_f32(v0, scale, budget, rec_pos).map_err(|e| map_build_err(e, rec_pos))?;
            builder.push_vertex_f32(v1, scale, budget, rec_pos).map_err(|e| map_build_err(e, rec_pos))?;
            builder.push_vertex_f32(v2, scale, budget, rec_pos).map_err(|e| map_build_err(e, rec_pos))?;
            builder
                .push_face_u64([next_v, next_v + 1, next_v + 2], budget, rec_pos)
                .map_err(|e| map_build_err(e, rec_pos))?;
            next_v += 3;
            byte_pos = byte_pos.saturating_add(BINARY_RECORD_LEN as u64);
        }
        remaining -= batch as u64;
    }

    let built = builder
        .build_mesh(frame, budget, Position::byte(byte_pos))
        .map_err(|e| map_build_err(e, Position::byte(byte_pos)))?;
    Ok(StlImportResult { built, encoding: StlEncoding::Binary, header_text: decode_header_text(header80) })
}

fn read_exact_io<R: Read>(reader: &mut R, mut dst: &mut [u8], mut at: u64) -> Result<(), IoError> {
    while !dst.is_empty() {
        match reader.read(dst) {
            Ok(0) => return Err(IoError::TruncatedFile { at: Position::byte(at) }),
            Ok(k) => {
                at = at.saturating_add(u64::try_from(k).unwrap_or(u64::MAX));
                dst = dst.get_mut(k..).unwrap_or(&mut []);
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(IoError::from_io(e, at)),
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ ASCII reader

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AsciiState {
    ExpectSolid,
    InSolid,
    InFacet,
    InLoop(u8),
    AfterLoop,
    Done,
}

fn parse_ascii_f32(tok: &str, at: Position) -> Result<f32, IoError> {
    let v = tok.parse::<f32>().map_err(|_| IoError::Malformed { at, reason: "invalid_float" })?;
    if !v.is_finite() {
        return Err(IoError::Malformed { at, reason: "non_finite_coordinate" });
    }
    Ok(v)
}

fn parse_ascii_bytes(
    bytes: &[u8],
    unit: LengthUnit,
    frame: FrameRef,
    budget: &mut Budget,
) -> Result<StlImportResult, IoError> {
    let scale = unit_scale_to_metres(unit);
    let mut builder = MeshBuilder::new(SourcePrecision::F32);
    let mut state = AsciiState::ExpectSolid;
    let mut solid_name = String::new();
    let mut has_solid_name = false;
    let mut line_no: u32 = 0;
    let mut offset: usize = 0;
    let mut tri_verts: [[f32; 3]; 3] = [[0.0; 3]; 3];

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
        if line_bytes.len() > MAX_ASCII_LINE_BYTES {
            return Err(IoError::Malformed { at, reason: "line_too_long" });
        }
        let line = std::str::from_utf8(line_bytes).map_err(|_| IoError::Malformed { at, reason: "invalid_utf8" })?;
        let trimmed = line.trim();
        offset = next_offset;

        if trimmed.is_empty() {
            continue;
        }
        let tokens: Vec<&str> = trimmed.split_ascii_whitespace().collect();
        let Some(&kw) = tokens.first() else {
            continue;
        };

        match state {
            AsciiState::ExpectSolid | AsciiState::Done => {
                if kw.eq_ignore_ascii_case("solid") {
                    if !has_solid_name {
                        let rest_name = trimmed.get(kw.len()..).unwrap_or("").trim();
                        solid_name = rest_name.to_string();
                        has_solid_name = true;
                    }
                    state = AsciiState::InSolid;
                } else {
                    return Err(IoError::Malformed { at, reason: "expected_solid" });
                }
            }
            AsciiState::InSolid => {
                if kw.eq_ignore_ascii_case("facet") {
                    // `facet normal nx ny nz`
                    if tokens.len() != 5 || !tokens.get(1).is_some_and(|t| t.eq_ignore_ascii_case("normal")) {
                        return Err(IoError::Malformed { at, reason: "expected_facet_normal" });
                    }
                    for &coord_tok in tokens.get(2..5).unwrap_or(&[]) {
                        let _ = parse_ascii_f32(coord_tok, at)?;
                    }
                    state = AsciiState::InFacet;
                } else if kw.eq_ignore_ascii_case("endsolid") {
                    state = AsciiState::Done;
                } else {
                    return Err(IoError::Malformed { at, reason: "expected_facet_or_endsolid" });
                }
            }
            AsciiState::InFacet => {
                if tokens.len() == 2
                    && kw.eq_ignore_ascii_case("outer")
                    && tokens.get(1).is_some_and(|t| t.eq_ignore_ascii_case("loop"))
                {
                    state = AsciiState::InLoop(0);
                } else {
                    return Err(IoError::Malformed { at, reason: "expected_outer_loop" });
                }
            }
            AsciiState::InLoop(count) => {
                if count < 3 {
                    if !kw.eq_ignore_ascii_case("vertex") {
                        return Err(IoError::Malformed { at, reason: "expected_vertex" });
                    }
                    let [_, sx, sy, sz] = tokens.as_slice() else {
                        return Err(IoError::Malformed { at, reason: "expected_three_vertex_coords" });
                    };
                    let x = parse_ascii_f32(sx, at)?;
                    let y = parse_ascii_f32(sy, at)?;
                    let z = parse_ascii_f32(sz, at)?;
                    if let Some(slot) = tri_verts.get_mut(usize::from(count)) {
                        *slot = [x, y, z];
                    }
                    state = AsciiState::InLoop(count + 1);
                } else if tokens.len() == 1 && kw.eq_ignore_ascii_case("endloop") {
                    let v0 =
                        builder.push_vertex_f32(tri_verts[0], scale, budget, at).map_err(|e| map_build_err(e, at))?;
                    let v1 =
                        builder.push_vertex_f32(tri_verts[1], scale, budget, at).map_err(|e| map_build_err(e, at))?;
                    let v2 =
                        builder.push_vertex_f32(tri_verts[2], scale, budget, at).map_err(|e| map_build_err(e, at))?;
                    builder.push_face_u64([v0, v1, v2], budget, at).map_err(|e| map_build_err(e, at))?;
                    state = AsciiState::AfterLoop;
                } else {
                    return Err(IoError::Malformed { at, reason: "expected_endloop" });
                }
            }
            AsciiState::AfterLoop => {
                if tokens.len() == 1 && kw.eq_ignore_ascii_case("endfacet") {
                    state = AsciiState::InSolid;
                } else {
                    return Err(IoError::Malformed { at, reason: "expected_endfacet" });
                }
            }
        }
    }

    let end_pos = Position::line_col(u64::try_from(bytes.len()).unwrap_or(u64::MAX), line_no.max(1), 1);
    if state != AsciiState::Done {
        return Err(IoError::TruncatedFile { at: end_pos });
    }

    let built = builder.build_mesh(frame, budget, end_pos).map_err(|e| map_build_err(e, end_pos))?;
    Ok(StlImportResult { built, encoding: StlEncoding::Ascii, header_text: UntrustedText::new(&solid_name) })
}

// ------------------------------------------------------------------ public read entry points

/// Decodes an STL byte slice (binary or ASCII) into a validated GEO [`Mesh`] in metres.
pub fn read_stl_bytes(
    bytes: &[u8],
    unit: LengthUnit,
    frame: FrameRef,
    budget: &mut Budget,
) -> Result<StlImportResult, IoError> {
    let enc = detect_stl_encoding(bytes)?;
    match enc {
        StlEncoding::Binary => {
            let header80: [u8; BINARY_HEADER_LEN] = bytes
                .get(..BINARY_HEADER_LEN)
                .and_then(|s| s.try_into().ok())
                .ok_or(IoError::TruncatedFile { at: Position::byte(u64::try_from(bytes.len()).unwrap_or(u64::MAX)) })?;
            let (tri_count, _) = binary_expected_len(bytes).ok_or(IoError::TruncatedFile { at: Position::byte(0) })?;
            let mut body = bytes.get(BINARY_PREAMBLE_LEN..).unwrap_or(&[]);
            parse_binary_body(&header80, tri_count, &mut body, unit, frame, budget)
        }
        StlEncoding::Ascii => parse_ascii_bytes(bytes, unit, frame, budget),
    }
}

/// Decodes an STL stream into a validated GEO [`Mesh`], streaming large binary STLs in
/// 64 KiB chunks without buffering the whole file in memory.
pub fn read_stl_stream<R: Read>(
    reader: &mut R,
    total_len: Option<u64>,
    unit: LengthUnit,
    frame: FrameRef,
    budget: &mut Budget,
) -> Result<StlImportResult, IoError> {
    // Fast streaming path when total_len is known and matches 84 + 50 * n.
    let mut preamble = [0u8; BINARY_PREAMBLE_LEN];
    let mut got = 0usize;
    while got < BINARY_PREAMBLE_LEN {
        match reader.read(&mut preamble[got..]) {
            Ok(0) => break,
            Ok(k) => got += k,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(IoError::from_io(e, got as u64)),
        }
    }

    if got == BINARY_PREAMBLE_LEN {
        if let Some((tri_count, expected_len)) = binary_expected_len(&preamble) {
            let stream_binary = match total_len {
                Some(len) => len == expected_len,
                None => expected_len > BUFFER_DETECT_LIMIT as u64 && !is_ascii_solid_prefix(&preamble),
            };
            if stream_binary {
                let mut header80 = [0u8; BINARY_HEADER_LEN];
                header80.copy_from_slice(&preamble[..BINARY_HEADER_LEN]);
                let res = parse_binary_body(&header80, tri_count, reader, unit, frame, budget)?;
                // Verify no unexpected trailing bytes if total_len wasn't known beforehand.
                if total_len.is_none() {
                    let mut extra = [0u8; 1];
                    match reader.read(&mut extra) {
                        Ok(0) => {}
                        Ok(_) => {
                            return Err(IoError::Malformed {
                                at: Position::byte(expected_len),
                                reason: "stl_trailing_bytes",
                            });
                        }
                        Err(e) => return Err(IoError::from_io(e, expected_len)),
                    }
                }
                return Ok(res);
            }
        }
    }

    // Buffer the remainder under budget alloc limits for exact detection and ASCII parsing.
    let mut buf = Vec::new();
    let initial_bytes = u64::try_from(got).unwrap_or(u64::MAX);
    budget.charge_alloc(initial_bytes, Position::byte(0))?;
    buf.try_reserve(got).map_err(|_| IoError::LimitExceeded {
        limit: LimitKind::Alloc,
        value: budget.limits().alloc(),
        at: Position::byte(0),
    })?;
    buf.extend_from_slice(&preamble[..got]);

    let mut scratch = vec![0u8; 64 * 1024];
    loop {
        match reader.read(&mut scratch) {
            Ok(0) => break,
            Ok(k) => {
                let at = Position::byte(u64::try_from(buf.len()).unwrap_or(u64::MAX));
                budget.charge_alloc(u64::try_from(k).unwrap_or(u64::MAX), at)?;
                buf.try_reserve(k).map_err(|_| IoError::LimitExceeded {
                    limit: LimitKind::Alloc,
                    value: budget.limits().alloc(),
                    at,
                })?;
                buf.extend_from_slice(&scratch[..k]);
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(IoError::from_io(e, u64::try_from(buf.len()).unwrap_or(u64::MAX))),
        }
    }

    read_stl_bytes(&buf, unit, frame, budget)
}

// ------------------------------------------------------------------ writer

fn compute_face_normal_f32(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> [f32; 3] {
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
    let out = [(nx / len) as f32, (ny / len) as f32, (nz / len) as f32];
    if out[0].is_finite() && out[1].is_finite() && out[2].is_finite() {
        out
    } else {
        [0.0, 0.0, 0.0]
    }
}

fn coord_to_f32(v_m: [f64; 3], unit: LengthUnit) -> Result<[f32; 3], IoError> {
    let inv = unit_scale_to_metres(unit);
    let out = [(v_m[0] / inv) as f32, (v_m[1] / inv) as f32, (v_m[2] / inv) as f32];
    if out.iter().all(|c| c.is_finite()) {
        Ok(out)
    } else {
        Err(IoError::Malformed { at: Position::byte(0), reason: "non_finite_coordinate" })
    }
}

fn precheck_stl_triangles(mesh: &Mesh, unit: LengthUnit) -> Result<(), IoError> {
    for tri in mesh.triangles() {
        for &vid in &tri {
            let p = mesh.vertex(vid).ok_or(IoError::Malformed { at: Position::byte(0), reason: "vertex_missing" })?;
            let _ = coord_to_f32(p, unit)?;
        }
    }
    Ok(())
}

fn sanitize_solid_name(raw: &str) -> String {
    let cleaned: String =
        raw.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')).take(64).collect();
    if cleaned.is_empty() {
        "mesh".to_string()
    } else {
        cleaned
    }
}

/// Writes `mesh` as little-endian binary STL into `sink` and returns the export summary
/// including the `f32` precision report (`REQ-IO.MESH.STL-004`).
pub fn write_stl_binary(mesh: &Mesh, unit: LengthUnit, sink: &mut dyn ByteSink) -> Result<StlExportSummary, IoError> {
    let tri_count = mesh.triangle_count();
    let count_u32 = u32::try_from(tri_count).map_err(|_| IoError::LimitExceeded {
        limit: LimitKind::Elements,
        value: u64::from(u32::MAX),
        at: Position::byte(0),
    })?;
    precheck_stl_triangles(mesh, unit)?;
    let precision = f32_rounding_report(mesh);
    let start_bytes = sink.bytes_written();

    let mut preamble = [0u8; BINARY_PREAMBLE_LEN];
    preamble[..EXPORT_BINARY_HEADER.len()].copy_from_slice(EXPORT_BINARY_HEADER);
    preamble[BINARY_HEADER_LEN..BINARY_PREAMBLE_LEN].copy_from_slice(&count_u32.to_le_bytes());
    sink.write_bytes(&preamble)?;

    let mut buf = Vec::with_capacity(CHUNK_RECORDS * BINARY_RECORD_LEN);
    for tri in mesh.triangles() {
        let a = mesh.vertex(tri[0]).ok_or(IoError::Malformed { at: Position::byte(0), reason: "vertex_missing" })?;
        let b = mesh.vertex(tri[1]).ok_or(IoError::Malformed { at: Position::byte(0), reason: "vertex_missing" })?;
        let c = mesh.vertex(tri[2]).ok_or(IoError::Malformed { at: Position::byte(0), reason: "vertex_missing" })?;
        let n = compute_face_normal_f32(a, b, c);
        let va = coord_to_f32(a, unit)?;
        let vb = coord_to_f32(b, unit)?;
        let vc = coord_to_f32(c, unit)?;

        for f in [n[0], n[1], n[2], va[0], va[1], va[2], vb[0], vb[1], vb[2], vc[0], vc[1], vc[2]] {
            buf.extend_from_slice(&f.to_le_bytes());
        }
        buf.extend_from_slice(&0u16.to_le_bytes());

        if buf.len() >= CHUNK_RECORDS * BINARY_RECORD_LEN {
            sink.write_bytes(&buf)?;
            buf.clear();
        }
    }
    if !buf.is_empty() {
        sink.write_bytes(&buf)?;
    }

    Ok(StlExportSummary {
        encoding: StlEncoding::Binary,
        unit,
        bytes: sink.bytes_written().saturating_sub(start_bytes),
        triangles: tri_count,
        precision,
    })
}

/// Writes `mesh` as ASCII STL into `sink` using shortest round-trip `f32` formatting
/// (`REQ-IO.MESH.STL-004`).
pub fn write_stl_ascii(
    mesh: &Mesh,
    unit: LengthUnit,
    solid_name: &str,
    sink: &mut dyn ByteSink,
) -> Result<StlExportSummary, IoError> {
    precheck_stl_triangles(mesh, unit)?;
    let precision = f32_rounding_report(mesh);
    let start_bytes = sink.bytes_written();
    let name = sanitize_solid_name(solid_name);

    let mut out = String::with_capacity(64 * 1024);
    out.push_str("solid ");
    out.push_str(&name);
    out.push('\n');

    for tri in mesh.triangles() {
        let a = mesh.vertex(tri[0]).ok_or(IoError::Malformed { at: Position::byte(0), reason: "vertex_missing" })?;
        let b = mesh.vertex(tri[1]).ok_or(IoError::Malformed { at: Position::byte(0), reason: "vertex_missing" })?;
        let c = mesh.vertex(tri[2]).ok_or(IoError::Malformed { at: Position::byte(0), reason: "vertex_missing" })?;
        let n = compute_face_normal_f32(a, b, c);
        let va = coord_to_f32(a, unit)?;
        let vb = coord_to_f32(b, unit)?;
        let vc = coord_to_f32(c, unit)?;

        use std::fmt::Write as _;
        let _ = writeln!(out, "  facet normal {} {} {}", n[0], n[1], n[2]);
        out.push_str("    outer loop\n");
        let _ = writeln!(out, "      vertex {} {} {}", va[0], va[1], va[2]);
        let _ = writeln!(out, "      vertex {} {} {}", vb[0], vb[1], vb[2]);
        let _ = writeln!(out, "      vertex {} {} {}", vc[0], vc[1], vc[2]);
        out.push_str("    endloop\n");
        out.push_str("  endfacet\n");

        if out.len() >= 60 * 1024 {
            sink.write_bytes(out.as_bytes())?;
            out.clear();
        }
    }
    out.push_str("endsolid ");
    out.push_str(&name);
    out.push('\n');
    sink.write_bytes(out.as_bytes())?;

    Ok(StlExportSummary {
        encoding: StlEncoding::Ascii,
        unit,
        bytes: sink.bytes_written().saturating_sub(start_bytes),
        triangles: mesh.triangle_count(),
        precision,
    })
}

/// Writes `mesh` to `sink` using `opts` (binary by default, ASCII when `encoding == Ascii`).
pub fn write_stl(mesh: &Mesh, opts: &StlExportOptions, sink: &mut dyn ByteSink) -> Result<StlExportSummary, IoError> {
    match opts.encoding {
        StlEncoding::Binary => write_stl_binary(mesh, opts.unit, sink),
        StlEncoding::Ascii => write_stl_ascii(mesh, opts.unit, &opts.solid_name, sink),
    }
}

// ------------------------------------------------------------------ fid-io-reg integration

/// Returns the registered `FormatId` (`"mesh-stl"`).
pub fn stl_format_id() -> Result<FormatId, fid_io_reg::report::ReportError> {
    FormatId::new(STL_FORMAT_ID)
}

/// Format descriptor for STL import (content detection prefers `StlBinarySize` over `solid`).
pub fn stl_import_descriptor() -> Result<FormatDescriptor, fid_io_reg::DetectError> {
    let format =
        stl_format_id().map_err(|_| fid_io_reg::DetectError::Invalid { field: "format", reason: "bad format id" })?;
    let sigs = vec![
        Signature::new(0, Matcher::Predicate(Predicate::StlBinarySize), 90)?,
        Signature::new(0, Matcher::Bytes(b"solid".to_vec()), 40)?,
        Signature::new(0, Matcher::Bytes(b"SOLID".to_vec()), 40)?,
    ];
    let opts_schema = CValue::Obj(BTreeMap::from([
        ("additionalProperties".to_string(), CValue::Bool(false)),
        ("properties".to_string(), CValue::Obj(BTreeMap::new())),
        ("type".to_string(), CValue::Str("object".to_string())),
    ]));
    FormatDescriptor::new(
        format,
        VersionRange::new("1.0", "1.0")?,
        sigs,
        vec!["stl".to_string()],
        opts_schema,
        Phase::new(1).ok_or(fid_io_reg::DetectError::Invalid { field: "phase", reason: "bad phase" })?,
        Origin::FirstParty,
        Direction::Import,
    )
}

/// Format descriptor for STL export (`encoding: binary|ascii`, `unit: m|mm|um|in`).
pub fn stl_export_descriptor() -> Result<FormatDescriptor, fid_io_reg::DetectError> {
    let format =
        stl_format_id().map_err(|_| fid_io_reg::DetectError::Invalid { field: "format", reason: "bad format id" })?;
    let mut props = BTreeMap::new();
    props.insert(
        "encoding".to_string(),
        CValue::Obj(BTreeMap::from([
            ("default".to_string(), CValue::Str("binary".to_string())),
            (
                "enum".to_string(),
                CValue::Arr(vec![CValue::Str("binary".to_string()), CValue::Str("ascii".to_string())]),
            ),
            ("type".to_string(), CValue::Str("string".to_string())),
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
        vec!["stl".to_string()],
        opts_schema,
        Phase::new(1).ok_or(fid_io_reg::DetectError::Invalid { field: "phase", reason: "bad phase" })?,
        Origin::FirstParty,
        Direction::Export,
    )
}

/// Registers both import and export descriptors for `mesh-stl` in `reg`.
pub fn register_stl(reg: &mut Registry) -> Result<(), IoError> {
    let imp =
        stl_import_descriptor().map_err(|_| IoError::Malformed { at: Position::byte(0), reason: "descriptor" })?;
    let exp =
        stl_export_descriptor().map_err(|_| IoError::Malformed { at: Position::byte(0), reason: "descriptor" })?;
    reg.register(imp)?;
    reg.register(exp)?;
    Ok(())
}

/// Canonical deterministic binary mesh payload stored in the CAS blob by [`StlReader`].
/// Layout: magic `"FIDSTL01"` (8B) + vertex count (u64 LE) + face count (u64 LE) +
/// vertices (`3 * f64` LE each, exact `f32`-widened raw coordinates) + faces (`3 * u64` LE each).
pub fn encode_staged_mesh_payload(mesh: &Mesh) -> Vec<u8> {
    let v_count = mesh.vertex_count();
    let f_count = mesh.triangle_count();
    let cap = 24usize
        .saturating_add(usize::try_from(v_count).unwrap_or(0).saturating_mul(24))
        .saturating_add(usize::try_from(f_count).unwrap_or(0).saturating_mul(24));
    let mut out = Vec::with_capacity(cap);
    out.extend_from_slice(b"FIDSTL01");
    out.extend_from_slice(&v_count.to_le_bytes());
    out.extend_from_slice(&f_count.to_le_bytes());
    for i in 0..v_count {
        if let Some(p) = mesh.vertex(i) {
            out.extend_from_slice(&p[0].to_le_bytes());
            out.extend_from_slice(&p[1].to_le_bytes());
            out.extend_from_slice(&p[2].to_le_bytes());
        }
    }
    for tri in mesh.triangles() {
        out.extend_from_slice(&tri[0].to_le_bytes());
        out.extend_from_slice(&tri[1].to_le_bytes());
        out.extend_from_slice(&tri[2].to_le_bytes());
    }
    out
}

/// Materializes a staged CAS mesh payload written by [`StlReader`] into a GEO [`Mesh`] in
/// metres using the resolved `unit` recorded in `SourceProvenance`.
pub fn decode_staged_mesh_payload(
    bytes: &[u8],
    unit: LengthUnit,
    frame: FrameRef,
    budget: &mut Budget,
) -> Result<Mesh, IoError> {
    let at0 = Position::byte(0);
    if bytes.get(..8) != Some(b"FIDSTL01".as_slice()) {
        return Err(IoError::Malformed { at: at0, reason: "bad_staged_mesh_magic" });
    }
    let read_u64 = |off: usize| -> Result<u64, IoError> {
        let raw: [u8; 8] = bytes
            .get(off..off + 8)
            .and_then(|s| s.try_into().ok())
            .ok_or(IoError::TruncatedFile { at: Position::byte(u64::try_from(off).unwrap_or(u64::MAX)) })?;
        Ok(u64::from_le_bytes(raw))
    };
    let read_f64 = |off: usize| -> Result<f64, IoError> {
        let raw: [u8; 8] = bytes
            .get(off..off + 8)
            .and_then(|s| s.try_into().ok())
            .ok_or(IoError::TruncatedFile { at: Position::byte(u64::try_from(off).unwrap_or(u64::MAX)) })?;
        let v = f64::from_le_bytes(raw);
        if !v.is_finite() {
            return Err(IoError::Malformed {
                at: Position::byte(u64::try_from(off).unwrap_or(u64::MAX)),
                reason: "non_finite_coordinate",
            });
        }
        Ok(v)
    };
    let v_count = read_u64(8)?;
    let f_count = read_u64(16)?;
    let v_bytes = v_count.checked_mul(24).ok_or(IoError::Malformed { at: at0, reason: "vertex_count_overflow" })?;
    let f_bytes = f_count.checked_mul(24).ok_or(IoError::Malformed { at: at0, reason: "face_count_overflow" })?;
    let total = 24u64
        .checked_add(v_bytes)
        .and_then(|n| n.checked_add(f_bytes))
        .ok_or(IoError::Malformed { at: at0, reason: "payload_len_overflow" })?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) != total {
        return Err(IoError::TruncatedFile { at: Position::byte(u64::try_from(bytes.len()).unwrap_or(u64::MAX)) });
    }
    let scale = unit_scale_to_metres(unit);
    let mut builder = MeshBuilder::new(SourcePrecision::F32);
    builder.reserve_vertices(v_count, budget, at0).map_err(|e| map_build_err(e, at0))?;
    builder.reserve_faces(f_count, budget, at0).map_err(|e| map_build_err(e, at0))?;
    let mut off = 24usize;
    for _ in 0..v_count {
        let x = read_f64(off)? * scale;
        let y = read_f64(off + 8)? * scale;
        let z = read_f64(off + 16)? * scale;
        let pos = Position::byte(u64::try_from(off).unwrap_or(u64::MAX));
        builder.push_vertex([x, y, z], budget, pos).map_err(|e| map_build_err(e, pos))?;
        off += 24;
    }
    for _ in 0..f_count {
        let i0 = read_u64(off)?;
        let i1 = read_u64(off + 8)?;
        let i2 = read_u64(off + 16)?;
        let pos = Position::byte(u64::try_from(off).unwrap_or(u64::MAX));
        builder.push_face_u64([i0, i1, i2], budget, pos).map_err(|e| map_build_err(e, pos))?;
        off += 24;
    }
    let built = builder.build_mesh(frame, budget, Position::byte(total)).map_err(|e| map_build_err(e, at0))?;
    Ok(built.mesh)
}

/// `fid-io-reg` import reader for `mesh-stl` (`import.mesh_stl@1`).
#[derive(Debug, Clone)]
pub struct StlReader {
    format: FormatId,
    info: ReaderInfo,
}

impl StlReader {
    /// Constructs the first-party STL reader (`fid-io-mesh-stl` v1.0.0).
    pub fn new() -> Result<Self, ImportError> {
        let format = stl_format_id().map_err(|_| ImportError::Internal("invalid mesh-stl format id"))?;
        let id = ReaderId::new("fid-io-mesh-stl").map_err(|_| ImportError::Internal("invalid stl reader id"))?;
        let version = VersionToken::new("1.0.0").map_err(|_| ImportError::Internal("invalid stl reader version"))?;
        Ok(StlReader { format, info: ReaderInfo { id, version } })
    }
}

impl Reader for StlReader {
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
            return Err("mesh-stl import takes no format options");
        }
        Ok(CValue::Obj(BTreeMap::new()))
    }

    fn read(&self, _opts: &CValue, sink: &mut StageSink<'_, '_>, report: &mut ReportBuilder) -> Result<(), IoError> {
        // Read bytes through sink.source() (HashingSource over BoundedSource) under the
        // single import budget. We buffer in bounded 64 KiB chunks so exact binary size
        // consistency (`len == 84 + 50 * n`) holds without requiring Seek.
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

        // Decode with exact f32 -> f64 widening (unit factor 1.0); the skeleton enforces
        // unit_required (since STL declares no unit) and records the resolved LengthUnit in
        // SourceProvenance. Callers materialize metres via decode_staged_mesh_payload.
        let frame = FrameRef::from_bytes([0u8; 16]);
        let decoded = read_stl_bytes(&raw, LengthUnit::M, frame, sink.source().budget_mut())?;
        drop(raw);

        if let Ok(ver) = VersionToken::new("1.0") {
            sink.declare_format_version(ver);
        }
        sink.declare("encoding", UntrustedText::new(decoded.encoding.as_str()));
        sink.declare("source_precision", UntrustedText::new(decoded.built.source_precision.as_str()));
        if decoded.header_text.len() > 0 {
            sink.declare("header", decoded.header_text.clone());
        }

        if let Ok(tn) = TypeName::new("mesh.face") {
            report.count(&tn, decoded.built.counts.faces);
        }
        if let Ok(tn) = TypeName::new("mesh.vertex") {
            report.count(&tn, decoded.built.counts.vertices);
        }
        if decoded.built.counts.degenerate_faces > 0 {
            if let Ok(code) = Code::new("degenerate_faces") {
                report.warn(&code, None, &UntrustedText::new(&decoded.built.counts.degenerate_faces.to_string()));
            }
        }
        if decoded.built.counts.non_manifold_edges > 0 {
            if let Ok(code) = Code::new("non_manifold_edges") {
                report.warn(&code, None, &UntrustedText::new(&decoded.built.counts.non_manifold_edges.to_string()));
            }
        }

        let entity_name =
            if decoded.header_text.len() == 0 { UntrustedText::new("stl-mesh") } else { decoded.header_text.clone() };
        let payload = encode_staged_mesh_payload(&decoded.built.mesh);
        sink.add_entity(STL_ELEMENT_TYPE, &entity_name, &payload, Position::byte(0), report)?;
        Ok(())
    }
}

/// `fid-io-reg` export writer for `mesh-stl` (`export.mesh_stl@1`).
#[derive(Debug, Clone)]
pub struct StlWriter {
    format: FormatId,
    version: VersionToken,
}

impl StlWriter {
    /// Constructs the first-party STL writer (`1.0.0`).
    pub fn new() -> Result<Self, ExportError> {
        let format = stl_format_id().map_err(|_| ExportError::Internal("invalid mesh-stl format id"))?;
        let version = VersionToken::new("1.0.0").map_err(|_| ExportError::Internal("invalid stl writer version"))?;
        Ok(StlWriter { format, version })
    }
}

fn parse_export_opts(v: &CValue) -> Result<StlExportOptions, &'static str> {
    let CValue::Obj(o) = v else {
        return Err("options must be an object");
    };
    if o.keys().any(|k| !matches!(k.as_str(), "encoding" | "unit")) {
        return Err("unknown mesh-stl export option");
    }
    let encoding = match o.get("encoding") {
        None => StlEncoding::Binary,
        Some(CValue::Str(s)) => StlEncoding::parse(s).ok_or("encoding must be binary or ascii")?,
        Some(_) => return Err("encoding must be a string"),
    };
    let unit = match o.get("unit") {
        None => LengthUnit::M,
        Some(CValue::Str(s)) => parse_unit_symbol(s).ok_or("unknown length unit")?,
        Some(_) => return Err("unit must be a string"),
    };
    Ok(StlExportOptions { encoding, unit, solid_name: "mesh".to_string() })
}

impl Writer for StlWriter {
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
            ("encoding".to_string(), CValue::Str(parsed.encoding.as_str().to_string())),
            ("unit".to_string(), CValue::Str(parsed.unit.symbol().to_string())),
        ])))
    }

    fn write(
        &self,
        src: &Mesh,
        opts: &CValue,
        sink: &mut dyn ByteSink,
        report: &mut ReportBuilder,
    ) -> Result<(), IoError> {
        let parsed = parse_export_opts(opts)
            .map_err(|_| IoError::Malformed { at: Position::byte(0), reason: "invalid_export_options" })?;
        let summary = write_stl(src, &parsed, sink)?;
        if summary.precision.max_abs_m > 0.0 {
            if let Ok(tn) = TypeName::new("f64_precision") {
                report.lossy(&tn);
            }
            if let Ok(code) = Code::new("f32_rounding") {
                report.warn(&code, None, &UntrustedText::new(&format!("max_abs_m={}", summary.precision.max_abs_m)));
            }
        }
        Ok(())
    }
}

/// Builds the `import.mesh_stl@1` command on the `fid-io-reg` import skeleton.
pub fn import_mesh_stl_command() -> Result<ImportCommand<StlReader>, ImportError> {
    ImportCommand::new(StlReader::new()?, &[])
}

/// Builds the `export.mesh_stl@1` command on the `fid-io-reg` export skeleton.
pub fn export_mesh_stl_command() -> Result<ExportCommand<StlWriter>, ExportError> {
    ExportCommand::new(StlWriter::new()?, &[])
}

/// Builds the registry [`CommandDef`]s for `import.mesh_stl@1` and `export.mesh_stl@1`.
pub fn stl_command_defs(reg: &Registry) -> Result<Vec<CommandDef>, String> {
    let fmt = stl_format_id().map_err(|e| e.to_string())?;
    let imp = import::command::format_def(reg, &fmt, &[]).map_err(|e| e.to_string())?;
    let exp = export::command::format_def(reg, &fmt, &[]).map_err(|e| e.to_string())?;
    Ok(vec![imp, exp])
}
