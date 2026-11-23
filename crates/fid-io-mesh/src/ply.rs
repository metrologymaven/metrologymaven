//! PLY reader and writer: ASCII, binary little-endian and binary big-endian, meshes and
//! point sets (spec node `IO.MESH.PLY`).
//!
//! - Parses bounded PLY headers (<= 64 KiB, <= 256 properties) supporting all standard scalar
//!   types and aliases (`char`/`int8` through `double`/`float64`) and `list` properties
//!   (`REQ-001`).
//! - Charges declared element counts to `Budget` BEFORE allocating (`limit_exceeded`, `AC-6`).
//! - Imports double-precision coordinates without loss (`SourcePrecision::F64`) and widens
//!   single-precision coordinates losslessly (`SourcePrecision::F32`) (`REQ-002`).
//! - Preserves all vertex properties with their declared scalar types (standard normals &
//!   RGB colour plus custom columns in [`ScalarColumn`]) (`REQ-001`, `AC-2`).
//! - Produces a GEO [`PointSet`] when no `face` element is present or face count is zero,
//!   and a GEO [`Mesh`] otherwise with fan triangulation `(0, i, i + 1)` counted in the
//!   report (`REQ-003`).
//! - Skips elements other than `vertex` and `face` and reports them with counts (`REQ-006`).
//! - Fails with `truncated_file` when the body ends before all declared elements (`REQ-004`).
//! - Exports meshes and point sets to ASCII or binary little-endian PLY with `float` or
//!   `double` precision; double binary round-trip is bit-identical (`REQ-005`).
//! - Wired onto `fid-io-reg` skeletons as `import.mesh_ply@1` (W2) and `export.mesh_ply@1`
//!   (W1, `fs_effect: write_root`).

use crate::attrs::{CustomScalarType, ScalarColumn};
use crate::builder::{BuiltGeometry, MeshBuilder, SourcePrecision};
use crate::error::MeshBuildError;
use crate::precision::f32_rounding_report;
use crate::stl::{encode_staged_mesh_payload, STL_ELEMENT_TYPE};
use fid_canon::CValue;
use fid_geo_frame::LengthUnit;
use fid_geo_types::{FrameRef, PointSet};
use fid_io_reg::detect::{FormatDescriptor, Matcher, Origin, Phase, Registry, Signature, VersionRange};
use fid_io_reg::env::export::{self, ByteSink, ExportCommand, ExportError, Writer};
use fid_io_reg::env::import::{self, ImportCommand, ImportError, Reader, StageSink};
use fid_io_reg::prov::{to_metres, ReaderId, ReaderInfo, VersionToken};
use fid_io_reg::report::{Code, FormatId, ReportBuilder, ReportPos, TypeName};
use fid_io_reg::roots::Direction;
use fid_io_reg::{Budget, IoError, LimitKind, Position, UntrustedText};
use fid_registry::CommandDef;
use std::collections::{BTreeMap, BTreeSet};

/// Registered format ID (`mesh-ply` -> commands `import.mesh_ply@1` and `export.mesh_ply@1`).
pub const PLY_FORMAT_ID: &str = "mesh-ply";
/// Element type ID staged for vertex-only PLY point sets (`REQ-003`).
pub const PLY_POINT_SET_ELEMENT_TYPE: &str = "geo.point_set";
/// Maximum PLY header length in bytes (Design: 64 KiB).
pub const MAX_PLY_HEADER_BYTES: usize = 64 * 1024;
/// Maximum total properties across all elements in a PLY header (Design: 256).
pub const MAX_PLY_PROPERTIES: usize = 256;
/// Maximum elements declared in one PLY header.
pub const MAX_PLY_ELEMENTS: usize = 64;
/// Maximum line length inside ASCII PLY body.
pub const MAX_PLY_LINE_BYTES: usize = 16 * 1024;
/// Maximum entries in one PLY list property (e.g. polygon vertex indices).
pub const MAX_PLY_LIST_LEN: usize = 1_024;

// ------------------------------------------------------------------ header model

/// PLY wire encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlyFormat {
    Ascii,
    BinaryLittleEndian,
    BinaryBigEndian,
}

impl PlyFormat {
    pub const fn as_str(self) -> &'static str {
        match self {
            PlyFormat::Ascii => "ascii",
            PlyFormat::BinaryLittleEndian => "binary_little_endian",
            PlyFormat::BinaryBigEndian => "binary_big_endian",
        }
    }
}

/// Export encoding (`ascii` or `binary_le`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlyExportEncoding {
    #[default]
    BinaryLe,
    Ascii,
}

impl PlyExportEncoding {
    pub const fn as_str(self) -> &'static str {
        match self {
            PlyExportEncoding::BinaryLe => "binary_le",
            PlyExportEncoding::Ascii => "ascii",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "binary_le" | "binary_little_endian" => Some(PlyExportEncoding::BinaryLe),
            "ascii" => Some(PlyExportEncoding::Ascii),
            _ => None,
        }
    }
}

/// Export coordinate precision (`float` or `double`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlyExportPrecision {
    #[default]
    Double,
    Float,
}

impl PlyExportPrecision {
    pub const fn as_str(self) -> &'static str {
        match self {
            PlyExportPrecision::Double => "double",
            PlyExportPrecision::Float => "float",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "double" | "f64" => Some(PlyExportPrecision::Double),
            "float" | "f32" => Some(PlyExportPrecision::Float),
            _ => None,
        }
    }
}

/// PLY scalar property type (all standard names and aliases, `REQ-001`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlyScalarType {
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    F32,
    F64,
}

impl PlyScalarType {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "char" | "int8" => Some(PlyScalarType::I8),
            "uchar" | "uint8" => Some(PlyScalarType::U8),
            "short" | "int16" => Some(PlyScalarType::I16),
            "ushort" | "uint16" => Some(PlyScalarType::U16),
            "int" | "int32" => Some(PlyScalarType::I32),
            "uint" | "uint32" => Some(PlyScalarType::U32),
            "float" | "float32" => Some(PlyScalarType::F32),
            "double" | "float64" => Some(PlyScalarType::F64),
            _ => None,
        }
    }

    pub const fn byte_width(self) -> usize {
        match self {
            PlyScalarType::I8 | PlyScalarType::U8 => 1,
            PlyScalarType::I16 | PlyScalarType::U16 => 2,
            PlyScalarType::I32 | PlyScalarType::U32 | PlyScalarType::F32 => 4,
            PlyScalarType::F64 => 8,
        }
    }

    pub const fn ply_name(self) -> &'static str {
        match self {
            PlyScalarType::I8 => "char",
            PlyScalarType::U8 => "uchar",
            PlyScalarType::I16 => "short",
            PlyScalarType::U16 => "ushort",
            PlyScalarType::I32 => "int",
            PlyScalarType::U32 => "uint",
            PlyScalarType::F32 => "float",
            PlyScalarType::F64 => "double",
        }
    }

    pub const fn to_custom_scalar(self) -> CustomScalarType {
        match self {
            PlyScalarType::I8 => CustomScalarType::I8,
            PlyScalarType::U8 => CustomScalarType::U8,
            PlyScalarType::I16 => CustomScalarType::I16,
            PlyScalarType::U16 => CustomScalarType::U16,
            PlyScalarType::I32 => CustomScalarType::I32,
            PlyScalarType::U32 => CustomScalarType::U32,
            PlyScalarType::F32 => CustomScalarType::F32,
            PlyScalarType::F64 => CustomScalarType::F64,
        }
    }

    pub const fn from_custom_scalar(c: CustomScalarType) -> Option<Self> {
        match c {
            CustomScalarType::I8 => Some(PlyScalarType::I8),
            CustomScalarType::U8 => Some(PlyScalarType::U8),
            CustomScalarType::I16 => Some(PlyScalarType::I16),
            CustomScalarType::U16 => Some(PlyScalarType::U16),
            CustomScalarType::I32 => Some(PlyScalarType::I32),
            CustomScalarType::U32 => Some(PlyScalarType::U32),
            CustomScalarType::F32 => Some(PlyScalarType::F32),
            CustomScalarType::F64 => Some(PlyScalarType::F64),
            CustomScalarType::I64 | CustomScalarType::U64 => None,
        }
    }

    pub const fn is_integer(self) -> bool {
        !matches!(self, PlyScalarType::F32 | PlyScalarType::F64)
    }
}

/// Property declaration inside a PLY element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlyProperty {
    Scalar { name: String, ty: PlyScalarType },
    List { name: String, count_ty: PlyScalarType, item_ty: PlyScalarType },
}

/// Element declaration inside a [`PlyHeader`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementDecl {
    pub name: String,
    pub count: u64,
    pub properties: Vec<PlyProperty>,
}

/// Parsed PLY header (`Design: PlyHeader { format, elements: Vec<ElementDecl> }`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlyHeader {
    pub format: PlyFormat,
    pub elements: Vec<ElementDecl>,
    pub comments: Vec<UntrustedText>,
    pub obj_info: Vec<UntrustedText>,
    pub body_offset: usize,
}

/// Output of direct PLY decoding into either a GEO [`Mesh`] or GEO [`PointSet`].
#[derive(Debug, Clone, PartialEq)]
pub struct PlyImportResult {
    pub header: PlyHeader,
    pub geometry: BuiltGeometry,
    /// Number of input polygon faces with `n > 3` vertices fan-triangulated (`REQ-001`).
    pub triangulated_faces: u64,
    /// Skipped elements other than `vertex` and `face`, mapped to their declared counts (`REQ-006`).
    pub dropped_elements: BTreeMap<String, u64>,
}

/// Export options for [`write_ply`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlyExportOptions {
    pub encoding: PlyExportEncoding,
    pub precision: PlyExportPrecision,
    pub unit: LengthUnit,
    /// Property filter (`empty` = export all available standard + custom vertex properties).
    pub properties: Vec<String>,
}

impl Default for PlyExportOptions {
    fn default() -> Self {
        PlyExportOptions {
            encoding: PlyExportEncoding::BinaryLe,
            precision: PlyExportPrecision::Double,
            unit: LengthUnit::M,
            properties: Vec::new(),
        }
    }
}

/// Summary returned by [`write_ply`].
#[derive(Debug, Clone, PartialEq)]
pub struct PlyExportSummary {
    pub encoding: PlyExportEncoding,
    pub precision: PlyExportPrecision,
    pub unit: LengthUnit,
    pub vertices: u64,
    pub triangles: u64,
    pub bytes: u64,
    pub max_abs_rounding_m: f64,
}

// ------------------------------------------------------------------ typed scalar values

#[derive(Debug, Clone, Copy)]
enum ScalarVal {
    I8(i8),
    U8(u8),
    I16(i16),
    U16(u16),
    I32(i32),
    U32(u32),
    F32(f32),
    F64(f64),
}

impl ScalarVal {
    fn as_f64_coord(self, at: Position) -> Result<f64, IoError> {
        let v = match self {
            ScalarVal::I8(x) => f64::from(x),
            ScalarVal::U8(x) => f64::from(x),
            ScalarVal::I16(x) => f64::from(x),
            ScalarVal::U16(x) => f64::from(x),
            ScalarVal::I32(x) => f64::from(x),
            ScalarVal::U32(x) => f64::from(x),
            ScalarVal::F32(x) => f64::from(x),
            ScalarVal::F64(x) => x,
        };
        if !v.is_finite() {
            return Err(IoError::Malformed { at, reason: "non_finite_coordinate" });
        }
        Ok(v)
    }

    fn as_u64_index(self, at: Position) -> Result<u64, IoError> {
        match self {
            ScalarVal::I8(x) => u64::try_from(x).map_err(|_| IoError::Malformed { at, reason: "negative_index" }),
            ScalarVal::U8(x) => Ok(u64::from(x)),
            ScalarVal::I16(x) => u64::try_from(x).map_err(|_| IoError::Malformed { at, reason: "negative_index" }),
            ScalarVal::U16(x) => Ok(u64::from(x)),
            ScalarVal::I32(x) => u64::try_from(x).map_err(|_| IoError::Malformed { at, reason: "negative_index" }),
            ScalarVal::U32(x) => Ok(u64::from(x)),
            ScalarVal::F32(_) | ScalarVal::F64(_) => Err(IoError::Malformed { at, reason: "non_integer_index" }),
        }
    }

    fn as_u8_color(self) -> Option<u8> {
        match self {
            ScalarVal::U8(x) => Some(x),
            _ => None,
        }
    }
}

// ------------------------------------------------------------------ header parser

/// Parses a PLY header with strict byte (`64 KiB`) and property (`256`) bounds.
pub fn parse_ply_header(bytes: &[u8]) -> Result<PlyHeader, IoError> {
    let mut offset = 0usize;
    let mut line_no: u32 = 0;
    let mut saw_magic = false;
    let mut format: Option<PlyFormat> = None;
    let mut elements: Vec<ElementDecl> = Vec::new();
    let mut comments: Vec<UntrustedText> = Vec::new();
    let mut obj_info: Vec<UntrustedText> = Vec::new();
    let mut total_props = 0usize;

    while offset < bytes.len() {
        if offset > MAX_PLY_HEADER_BYTES {
            return Err(IoError::LimitExceeded {
                limit: LimitKind::Bytes,
                value: MAX_PLY_HEADER_BYTES as u64,
                at: Position::byte(offset as u64),
            });
        }
        line_no = line_no.saturating_add(1);
        let line_start = offset;
        let rest = bytes.get(offset..).unwrap_or(&[]);
        let Some(nl_rel) = rest.iter().position(|&b| b == b'\n') else {
            return Err(IoError::TruncatedFile { at: Position::byte(u64::try_from(bytes.len()).unwrap_or(u64::MAX)) });
        };
        let raw_line = rest.get(..nl_rel).unwrap_or(&[]);
        let line_bytes = raw_line.strip_suffix(b"\r").unwrap_or(raw_line);
        offset = line_start + nl_rel + 1;
        if offset > MAX_PLY_HEADER_BYTES {
            return Err(IoError::LimitExceeded {
                limit: LimitKind::Bytes,
                value: MAX_PLY_HEADER_BYTES as u64,
                at: Position::line_col(line_start as u64, line_no, 1),
            });
        }
        let at = Position::line_col(u64::try_from(line_start).unwrap_or(u64::MAX), line_no, 1);
        let line = std::str::from_utf8(line_bytes).map_err(|_| IoError::Malformed { at, reason: "invalid_utf8" })?;
        let trimmed = line.trim();

        if !saw_magic {
            if trimmed != "ply" {
                return Err(IoError::Malformed { at, reason: "expected_ply_magic" });
            }
            saw_magic = true;
            continue;
        }
        if trimmed.is_empty() {
            continue;
        }

        let tokens: Vec<&str> = trimmed.split_ascii_whitespace().collect();
        let Some(&kw) = tokens.first() else {
            continue;
        };
        match kw {
            "end_header" => {
                let Some(fmt) = format else {
                    return Err(IoError::Malformed { at, reason: "missing_ply_format" });
                };
                return Ok(PlyHeader { format: fmt, elements, comments, obj_info, body_offset: offset });
            }
            "format" => {
                if format.is_some() {
                    return Err(IoError::Malformed { at, reason: "duplicate_format" });
                }
                let [_, fmt_s, ver_s] = tokens.as_slice() else {
                    return Err(IoError::Malformed { at, reason: "invalid_format_line" });
                };
                if *ver_s != "1.0" {
                    return Err(IoError::Malformed { at, reason: "unsupported_ply_version" });
                }
                format = Some(match *fmt_s {
                    "ascii" => PlyFormat::Ascii,
                    "binary_little_endian" => PlyFormat::BinaryLittleEndian,
                    "binary_big_endian" => PlyFormat::BinaryBigEndian,
                    _ => return Err(IoError::Malformed { at, reason: "unknown_ply_format" }),
                });
            }
            "comment" => {
                let text = trimmed.get(kw.len()..).unwrap_or("").trim();
                if comments.len() < 128 {
                    comments.push(UntrustedText::new(text));
                }
            }
            "obj_info" => {
                let text = trimmed.get(kw.len()..).unwrap_or("").trim();
                if obj_info.len() < 128 {
                    obj_info.push(UntrustedText::new(text));
                }
            }
            "element" => {
                if elements.len() >= MAX_PLY_ELEMENTS {
                    return Err(IoError::LimitExceeded {
                        limit: LimitKind::Elements,
                        value: MAX_PLY_ELEMENTS as u64,
                        at,
                    });
                }
                let [_, name, count_s] = tokens.as_slice() else {
                    return Err(IoError::Malformed { at, reason: "invalid_element_line" });
                };
                let count =
                    count_s.parse::<u64>().map_err(|_| IoError::Malformed { at, reason: "invalid_element_count" })?;
                elements.push(ElementDecl { name: (*name).to_string(), count, properties: Vec::new() });
            }
            "property" => {
                let Some(cur_el) = elements.last_mut() else {
                    return Err(IoError::Malformed { at, reason: "property_before_element" });
                };
                total_props += 1;
                if total_props > MAX_PLY_PROPERTIES {
                    return Err(IoError::LimitExceeded {
                        limit: LimitKind::Elements,
                        value: MAX_PLY_PROPERTIES as u64,
                        at,
                    });
                }
                if tokens.get(1) == Some(&"list") {
                    let [_, _, cnt_s, item_s, name] = tokens.as_slice() else {
                        return Err(IoError::Malformed { at, reason: "invalid_list_property" });
                    };
                    let count_ty =
                        PlyScalarType::parse(cnt_s).ok_or(IoError::Malformed { at, reason: "unknown_scalar_type" })?;
                    if !count_ty.is_integer() {
                        return Err(IoError::Malformed { at, reason: "list_count_must_be_integer" });
                    }
                    let item_ty =
                        PlyScalarType::parse(item_s).ok_or(IoError::Malformed { at, reason: "unknown_scalar_type" })?;
                    cur_el.properties.push(PlyProperty::List { name: (*name).to_string(), count_ty, item_ty });
                } else {
                    let [_, ty_s, name] = tokens.as_slice() else {
                        return Err(IoError::Malformed { at, reason: "invalid_scalar_property" });
                    };
                    let ty =
                        PlyScalarType::parse(ty_s).ok_or(IoError::Malformed { at, reason: "unknown_scalar_type" })?;
                    cur_el.properties.push(PlyProperty::Scalar { name: (*name).to_string(), ty });
                }
            }
            _ => return Err(IoError::Malformed { at, reason: "unknown_header_keyword" }),
        }
    }

    Err(IoError::TruncatedFile { at: Position::byte(u64::try_from(bytes.len()).unwrap_or(u64::MAX)) })
}

// ------------------------------------------------------------------ binary & ASCII value readers

struct BinCursor<'a> {
    bytes: &'a [u8],
    pos: usize,
    le: bool,
}

impl<'a> BinCursor<'a> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N], IoError> {
        let end = self.pos.checked_add(N).ok_or(IoError::TruncatedFile { at: Position::byte(self.pos as u64) })?;
        let slice = self.bytes.get(self.pos..end).ok_or(IoError::TruncatedFile {
            at: Position::byte(u64::try_from(self.bytes.len()).unwrap_or(u64::MAX)),
        })?;
        self.pos = end;
        slice.try_into().map_err(|_| IoError::TruncatedFile { at: Position::byte(self.pos as u64) })
    }

    fn read_scalar(&mut self, ty: PlyScalarType) -> Result<ScalarVal, IoError> {
        let val = match ty {
            PlyScalarType::I8 => ScalarVal::I8(i8::from_le_bytes(self.take::<1>()?)),
            PlyScalarType::U8 => ScalarVal::U8(u8::from_le_bytes(self.take::<1>()?)),
            PlyScalarType::I16 => {
                let b = self.take::<2>()?;
                ScalarVal::I16(if self.le { i16::from_le_bytes(b) } else { i16::from_be_bytes(b) })
            }
            PlyScalarType::U16 => {
                let b = self.take::<2>()?;
                ScalarVal::U16(if self.le { u16::from_le_bytes(b) } else { u16::from_be_bytes(b) })
            }
            PlyScalarType::I32 => {
                let b = self.take::<4>()?;
                ScalarVal::I32(if self.le { i32::from_le_bytes(b) } else { i32::from_be_bytes(b) })
            }
            PlyScalarType::U32 => {
                let b = self.take::<4>()?;
                ScalarVal::U32(if self.le { u32::from_le_bytes(b) } else { u32::from_be_bytes(b) })
            }
            PlyScalarType::F32 => {
                let b = self.take::<4>()?;
                ScalarVal::F32(if self.le { f32::from_le_bytes(b) } else { f32::from_be_bytes(b) })
            }
            PlyScalarType::F64 => {
                let b = self.take::<8>()?;
                ScalarVal::F64(if self.le { f64::from_le_bytes(b) } else { f64::from_be_bytes(b) })
            }
        };
        Ok(val)
    }

    fn pos_info(&self) -> Position {
        Position::byte(u64::try_from(self.pos).unwrap_or(u64::MAX))
    }
}

fn parse_ascii_scalar(tok: &str, ty: PlyScalarType, at: Position) -> Result<ScalarVal, IoError> {
    let bad_int = |_: std::num::ParseIntError| IoError::Malformed { at, reason: "invalid_ascii_scalar" };
    let bad_flt = |_: std::num::ParseFloatError| IoError::Malformed { at, reason: "invalid_ascii_scalar" };
    let val = match ty {
        PlyScalarType::I8 => ScalarVal::I8(tok.parse::<i8>().map_err(bad_int)?),
        PlyScalarType::U8 => ScalarVal::U8(tok.parse::<u8>().map_err(bad_int)?),
        PlyScalarType::I16 => ScalarVal::I16(tok.parse::<i16>().map_err(bad_int)?),
        PlyScalarType::U16 => ScalarVal::U16(tok.parse::<u16>().map_err(bad_int)?),
        PlyScalarType::I32 => ScalarVal::I32(tok.parse::<i32>().map_err(bad_int)?),
        PlyScalarType::U32 => ScalarVal::U32(tok.parse::<u32>().map_err(bad_int)?),
        PlyScalarType::F32 => ScalarVal::F32(tok.parse::<f32>().map_err(bad_flt)?),
        PlyScalarType::F64 => ScalarVal::F64(tok.parse::<f64>().map_err(bad_flt)?),
    };
    Ok(val)
}

struct AsciiLines<'a> {
    bytes: &'a [u8],
    offset: usize,
    line_no: u32,
}

impl<'a> AsciiLines<'a> {
    fn next_non_empty(&mut self) -> Result<Option<(&'a str, Position)>, IoError> {
        while self.offset < self.bytes.len() {
            self.line_no = self.line_no.saturating_add(1);
            let start = self.offset;
            let rest = self.bytes.get(start..).unwrap_or(&[]);
            let (raw, next_off) = match rest.iter().position(|&b| b == b'\n') {
                Some(p) => (rest.get(..p).unwrap_or(&[]), start + p + 1),
                None => (rest, self.bytes.len()),
            };
            self.offset = next_off;
            let line_bytes = raw.strip_suffix(b"\r").unwrap_or(raw);
            let at = Position::line_col(u64::try_from(start).unwrap_or(u64::MAX), self.line_no, 1);
            if line_bytes.len() > MAX_PLY_LINE_BYTES {
                return Err(IoError::Malformed { at, reason: "line_too_long" });
            }
            let s = std::str::from_utf8(line_bytes).map_err(|_| IoError::Malformed { at, reason: "invalid_utf8" })?;
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                return Ok(Some((trimmed, at)));
            }
        }
        Ok(None)
    }

    fn eof_pos(&self) -> Position {
        Position::byte(u64::try_from(self.bytes.len()).unwrap_or(u64::MAX))
    }
}

// ------------------------------------------------------------------ custom scalar column helpers

fn push_custom_scalar(
    custom: &mut BTreeMap<String, ScalarColumn>,
    name: &str,
    val: ScalarVal,
    budget: &mut Budget,
    at: Position,
) -> Result<(), IoError> {
    let col = custom.entry(name.to_string()).or_insert_with(|| {
        let kind = match val {
            ScalarVal::I8(_) => CustomScalarType::I8,
            ScalarVal::U8(_) => CustomScalarType::U8,
            ScalarVal::I16(_) => CustomScalarType::I16,
            ScalarVal::U16(_) => CustomScalarType::U16,
            ScalarVal::I32(_) => CustomScalarType::I32,
            ScalarVal::U32(_) => CustomScalarType::U32,
            ScalarVal::F32(_) => CustomScalarType::F32,
            ScalarVal::F64(_) => CustomScalarType::F64,
        };
        ScalarColumn::empty(kind)
    });
    let width = col.scalar_type().byte_width();
    macro_rules! push_typed {
        ($vec:expr, $x:expr) => {{
            if $vec.len() == $vec.capacity() {
                let grow = if $vec.capacity() == 0 { 16 } else { $vec.capacity() };
                let bytes = u64::try_from(grow).unwrap_or(u64::MAX).saturating_mul(width);
                budget.charge_alloc(bytes, at)?;
                $vec.try_reserve(grow).map_err(|_| IoError::LimitExceeded {
                    limit: LimitKind::Alloc,
                    value: budget.limits().alloc(),
                    at,
                })?;
            }
            $vec.push($x);
        }};
    }
    match (col, val) {
        (ScalarColumn::I8(v), ScalarVal::I8(x)) => push_typed!(v, x),
        (ScalarColumn::U8(v), ScalarVal::U8(x)) => push_typed!(v, x),
        (ScalarColumn::I16(v), ScalarVal::I16(x)) => push_typed!(v, x),
        (ScalarColumn::U16(v), ScalarVal::U16(x)) => push_typed!(v, x),
        (ScalarColumn::I32(v), ScalarVal::I32(x)) => push_typed!(v, x),
        (ScalarColumn::U32(v), ScalarVal::U32(x)) => push_typed!(v, x),
        (ScalarColumn::F32(v), ScalarVal::F32(x)) => push_typed!(v, x),
        (ScalarColumn::F64(v), ScalarVal::F64(x)) => push_typed!(v, x),
        _ => return Err(IoError::Malformed { at, reason: "attr_type_mismatch" }),
    }
    Ok(())
}

// ------------------------------------------------------------------ reader

fn unit_scale_to_metres(unit: LengthUnit) -> f64 {
    to_metres(1.0, unit).unwrap_or(1.0)
}

fn parse_unit_symbol(s: &str) -> Option<LengthUnit> {
    LengthUnit::ALL.iter().copied().find(|u| u.symbol() == s)
}

fn map_build_err(err: MeshBuildError, at: Position) -> IoError {
    err.to_io_error(at)
}

fn detect_vertex_precision(vertex_el: &ElementDecl) -> Result<SourcePrecision, IoError> {
    let mut x_ty = None;
    let mut y_ty = None;
    let mut z_ty = None;
    for prop in &vertex_el.properties {
        if let PlyProperty::Scalar { name, ty } = prop {
            match name.as_str() {
                "x" => x_ty = Some(*ty),
                "y" => y_ty = Some(*ty),
                "z" => z_ty = Some(*ty),
                _ => {}
            }
        }
    }
    let (Some(tx), Some(ty), Some(tz)) = (x_ty, y_ty, z_ty) else {
        return Err(IoError::Malformed { at: Position::byte(0), reason: "vertex_missing_xyz" });
    };
    if matches!(tx, PlyScalarType::F64) || matches!(ty, PlyScalarType::F64) || matches!(tz, PlyScalarType::F64) {
        Ok(SourcePrecision::F64)
    } else {
        Ok(SourcePrecision::F32)
    }
}

fn has_rgb_uchar(vertex_el: &ElementDecl) -> bool {
    let has = |want: &str| {
        vertex_el
            .properties
            .iter()
            .any(|p| matches!(p, PlyProperty::Scalar { name, ty: PlyScalarType::U8 } if name == want))
    };
    has("red") && has("green") && has("blue")
}

fn has_normals(vertex_el: &ElementDecl) -> bool {
    let has =
        |want: &str| vertex_el.properties.iter().any(|p| matches!(p, PlyProperty::Scalar { name, .. } if name == want));
    has("nx") && has("ny") && has("nz")
}

/// Decodes PLY bytes (ASCII, binary LE, or binary BE) into a GEO [`Mesh`] or [`PointSet`]
/// (`REQ-001..006`). Element counts are charged to `budget` before allocating (`AC-6`).
pub fn read_ply_bytes(
    bytes: &[u8],
    unit: LengthUnit,
    frame: FrameRef,
    budget: &mut Budget,
) -> Result<PlyImportResult, IoError> {
    let header = parse_ply_header(bytes)?;
    let hdr_pos = Position::byte(u64::try_from(header.body_offset).unwrap_or(u64::MAX));

    // AC-6 / Design: charge ALL header element counts to the budget BEFORE allocating.
    let mut total_elements = 0u64;
    let mut vertex_count = 0u64;
    let mut face_count = 0u64;
    let mut vertex_precision = SourcePrecision::F64;
    let mut saw_vertex = false;
    for el in &header.elements {
        total_elements = total_elements.checked_add(el.count).ok_or(IoError::LimitExceeded {
            limit: LimitKind::Elements,
            value: budget.limits().elements(),
            at: hdr_pos,
        })?;
        if el.name == "vertex" {
            if saw_vertex {
                return Err(IoError::Malformed { at: hdr_pos, reason: "duplicate_vertex_element" });
            }
            saw_vertex = true;
            vertex_count = el.count;
            vertex_precision = detect_vertex_precision(el)?;
        } else if el.name == "face" {
            face_count = face_count.saturating_add(el.count);
        }
    }
    if !saw_vertex {
        return Err(IoError::Malformed { at: hdr_pos, reason: "missing_vertex_element" });
    }
    // Check total declared elements against budget element limit before any reservation.
    let other_elements = total_elements.saturating_sub(vertex_count).saturating_sub(face_count);
    if other_elements > 0 {
        budget.charge_elements(other_elements, hdr_pos)?;
    }

    let mut builder = MeshBuilder::new(vertex_precision);
    builder.reserve_vertices(vertex_count, budget, hdr_pos).map_err(|e| map_build_err(e, hdr_pos))?;
    if face_count > 0 {
        builder.reserve_faces(face_count, budget, hdr_pos).map_err(|e| map_build_err(e, hdr_pos))?;
    }

    let scale = unit_scale_to_metres(unit);
    let is_metres = unit == LengthUnit::M;
    let mut triangulated_faces = 0u64;
    let mut dropped_elements: BTreeMap<String, u64> = BTreeMap::new();

    match header.format {
        PlyFormat::Ascii => {
            let mut lines = AsciiLines { bytes, offset: header.body_offset, line_no: 0 };
            for el in &header.elements {
                match el.name.as_str() {
                    "vertex" => {
                        let keep_rgb = has_rgb_uchar(el);
                        let keep_n = has_normals(el);
                        for _ in 0..el.count {
                            let Some((line, at)) = lines.next_non_empty()? else {
                                return Err(IoError::TruncatedFile { at: lines.eof_pos() });
                            };
                            let mut toks = line.split_ascii_whitespace();
                            let (mut vx, mut vy, mut vz) = (0.0_f64, 0.0_f64, 0.0_f64);
                            let (mut nx, mut ny, mut nz) = (0.0_f64, 0.0_f64, 0.0_f64);
                            let (mut cr, mut cg, mut cb) = (0u8, 0u8, 0u8);
                            for prop in &el.properties {
                                match prop {
                                    PlyProperty::Scalar { name, ty } => {
                                        let tok = toks
                                            .next()
                                            .ok_or(IoError::Malformed { at, reason: "short_vertex_line" })?;
                                        let sv = parse_ascii_scalar(tok, *ty, at)?;
                                        match name.as_str() {
                                            "x" => vx = sv.as_f64_coord(at)?,
                                            "y" => vy = sv.as_f64_coord(at)?,
                                            "z" => vz = sv.as_f64_coord(at)?,
                                            "nx" if keep_n => nx = sv.as_f64_coord(at)?,
                                            "ny" if keep_n => ny = sv.as_f64_coord(at)?,
                                            "nz" if keep_n => nz = sv.as_f64_coord(at)?,
                                            "red" if keep_rgb => cr = sv.as_u8_color().unwrap_or(0),
                                            "green" if keep_rgb => cg = sv.as_u8_color().unwrap_or(0),
                                            "blue" if keep_rgb => cb = sv.as_u8_color().unwrap_or(0),
                                            other => {
                                                push_custom_scalar(
                                                    &mut builder.attrs_mut().custom,
                                                    other,
                                                    sv,
                                                    budget,
                                                    at,
                                                )?;
                                            }
                                        }
                                    }
                                    PlyProperty::List { count_ty, item_ty, .. } => {
                                        let ctok =
                                            toks.next().ok_or(IoError::Malformed { at, reason: "short_list" })?;
                                        let n = parse_ascii_scalar(ctok, *count_ty, at)?.as_u64_index(at)?;
                                        let nu = usize::try_from(n)
                                            .map_err(|_| IoError::Malformed { at, reason: "list_too_long" })?;
                                        if nu > MAX_PLY_LIST_LEN {
                                            return Err(IoError::Malformed { at, reason: "list_too_long" });
                                        }
                                        for _ in 0..nu {
                                            let itok =
                                                toks.next().ok_or(IoError::Malformed { at, reason: "short_list" })?;
                                            let _ = parse_ascii_scalar(itok, *item_ty, at)?;
                                        }
                                    }
                                }
                            }
                            let p = if is_metres { [vx, vy, vz] } else { [vx * scale, vy * scale, vz * scale] };
                            builder.push_vertex(p, budget, at).map_err(|e| map_build_err(e, at))?;
                            if keep_n {
                                builder
                                    .attrs_mut()
                                    .push_normal([nx, ny, nz], budget, at)
                                    .map_err(|e| map_build_err(e, at))?;
                            }
                            if keep_rgb {
                                builder
                                    .attrs_mut()
                                    .push_colour([cr, cg, cb], budget, at)
                                    .map_err(|e| map_build_err(e, at))?;
                            }
                        }
                    }
                    "face" => {
                        let mut corners = Vec::with_capacity(8);
                        for _ in 0..el.count {
                            let Some((line, at)) = lines.next_non_empty()? else {
                                return Err(IoError::TruncatedFile { at: lines.eof_pos() });
                            };
                            let mut toks = line.split_ascii_whitespace();
                            corners.clear();
                            for prop in &el.properties {
                                match prop {
                                    PlyProperty::List { name, count_ty, item_ty } => {
                                        let ctok =
                                            toks.next().ok_or(IoError::Malformed { at, reason: "short_face_line" })?;
                                        let n = parse_ascii_scalar(ctok, *count_ty, at)?.as_u64_index(at)?;
                                        let nu = usize::try_from(n)
                                            .map_err(|_| IoError::Malformed { at, reason: "face_too_many_vertices" })?;
                                        if nu > MAX_PLY_LIST_LEN {
                                            return Err(IoError::Malformed { at, reason: "face_too_many_vertices" });
                                        }
                                        let is_indices = name == "vertex_indices" || name == "vertex_index";
                                        for _ in 0..nu {
                                            let itok = toks
                                                .next()
                                                .ok_or(IoError::Malformed { at, reason: "short_face_list" })?;
                                            let sv = parse_ascii_scalar(itok, *item_ty, at)?;
                                            if is_indices {
                                                corners.push(sv.as_u64_index(at)?);
                                            }
                                        }
                                    }
                                    PlyProperty::Scalar { ty, .. } => {
                                        let tok =
                                            toks.next().ok_or(IoError::Malformed { at, reason: "short_face_line" })?;
                                        let _ = parse_ascii_scalar(tok, *ty, at)?;
                                    }
                                }
                            }
                            triangulated_faces = push_ply_face(&mut builder, &corners, triangulated_faces, budget, at)?;
                        }
                    }
                    other => {
                        for _ in 0..el.count {
                            let Some((line, at)) = lines.next_non_empty()? else {
                                return Err(IoError::TruncatedFile { at: lines.eof_pos() });
                            };
                            let mut toks = line.split_ascii_whitespace();
                            for prop in &el.properties {
                                match prop {
                                    PlyProperty::Scalar { ty, .. } => {
                                        let tok = toks
                                            .next()
                                            .ok_or(IoError::Malformed { at, reason: "short_element_line" })?;
                                        let _ = parse_ascii_scalar(tok, *ty, at)?;
                                    }
                                    PlyProperty::List { count_ty, item_ty, .. } => {
                                        let ctok =
                                            toks.next().ok_or(IoError::Malformed { at, reason: "short_list" })?;
                                        let n = parse_ascii_scalar(ctok, *count_ty, at)?.as_u64_index(at)?;
                                        let nu = usize::try_from(n)
                                            .map_err(|_| IoError::Malformed { at, reason: "list_too_long" })?;
                                        if nu > MAX_PLY_LIST_LEN {
                                            return Err(IoError::Malformed { at, reason: "list_too_long" });
                                        }
                                        for _ in 0..nu {
                                            let itok =
                                                toks.next().ok_or(IoError::Malformed { at, reason: "short_list" })?;
                                            let _ = parse_ascii_scalar(itok, *item_ty, at)?;
                                        }
                                    }
                                }
                            }
                        }
                        if el.count > 0 {
                            let entry = dropped_elements.entry(other.to_string()).or_insert(0);
                            *entry = entry.saturating_add(el.count);
                        }
                    }
                }
            }
        }
        PlyFormat::BinaryLittleEndian | PlyFormat::BinaryBigEndian => {
            let mut cur =
                BinCursor { bytes, pos: header.body_offset, le: header.format == PlyFormat::BinaryLittleEndian };
            for el in &header.elements {
                match el.name.as_str() {
                    "vertex" => {
                        let keep_rgb = has_rgb_uchar(el);
                        let keep_n = has_normals(el);
                        for _ in 0..el.count {
                            let at = cur.pos_info();
                            let (mut vx, mut vy, mut vz) = (0.0_f64, 0.0_f64, 0.0_f64);
                            let (mut nx, mut ny, mut nz) = (0.0_f64, 0.0_f64, 0.0_f64);
                            let (mut cr, mut cg, mut cb) = (0u8, 0u8, 0u8);
                            for prop in &el.properties {
                                match prop {
                                    PlyProperty::Scalar { name, ty } => {
                                        let sv = cur.read_scalar(*ty)?;
                                        match name.as_str() {
                                            "x" => vx = sv.as_f64_coord(at)?,
                                            "y" => vy = sv.as_f64_coord(at)?,
                                            "z" => vz = sv.as_f64_coord(at)?,
                                            "nx" if keep_n => nx = sv.as_f64_coord(at)?,
                                            "ny" if keep_n => ny = sv.as_f64_coord(at)?,
                                            "nz" if keep_n => nz = sv.as_f64_coord(at)?,
                                            "red" if keep_rgb => cr = sv.as_u8_color().unwrap_or(0),
                                            "green" if keep_rgb => cg = sv.as_u8_color().unwrap_or(0),
                                            "blue" if keep_rgb => cb = sv.as_u8_color().unwrap_or(0),
                                            other => {
                                                push_custom_scalar(
                                                    &mut builder.attrs_mut().custom,
                                                    other,
                                                    sv,
                                                    budget,
                                                    at,
                                                )?;
                                            }
                                        }
                                    }
                                    PlyProperty::List { count_ty, item_ty, .. } => {
                                        let n = cur.read_scalar(*count_ty)?.as_u64_index(at)?;
                                        let nu = usize::try_from(n)
                                            .map_err(|_| IoError::Malformed { at, reason: "list_too_long" })?;
                                        if nu > MAX_PLY_LIST_LEN {
                                            return Err(IoError::Malformed { at, reason: "list_too_long" });
                                        }
                                        for _ in 0..nu {
                                            let _ = cur.read_scalar(*item_ty)?;
                                        }
                                    }
                                }
                            }
                            let p = if is_metres { [vx, vy, vz] } else { [vx * scale, vy * scale, vz * scale] };
                            builder.push_vertex(p, budget, at).map_err(|e| map_build_err(e, at))?;
                            if keep_n {
                                builder
                                    .attrs_mut()
                                    .push_normal([nx, ny, nz], budget, at)
                                    .map_err(|e| map_build_err(e, at))?;
                            }
                            if keep_rgb {
                                builder
                                    .attrs_mut()
                                    .push_colour([cr, cg, cb], budget, at)
                                    .map_err(|e| map_build_err(e, at))?;
                            }
                        }
                    }
                    "face" => {
                        let mut corners = Vec::with_capacity(8);
                        for _ in 0..el.count {
                            let at = cur.pos_info();
                            corners.clear();
                            for prop in &el.properties {
                                match prop {
                                    PlyProperty::List { name, count_ty, item_ty } => {
                                        let n = cur.read_scalar(*count_ty)?.as_u64_index(at)?;
                                        let nu = usize::try_from(n)
                                            .map_err(|_| IoError::Malformed { at, reason: "face_too_many_vertices" })?;
                                        if nu > MAX_PLY_LIST_LEN {
                                            return Err(IoError::Malformed { at, reason: "face_too_many_vertices" });
                                        }
                                        let is_indices = name == "vertex_indices" || name == "vertex_index";
                                        for _ in 0..nu {
                                            let sv = cur.read_scalar(*item_ty)?;
                                            if is_indices {
                                                corners.push(sv.as_u64_index(at)?);
                                            }
                                        }
                                    }
                                    PlyProperty::Scalar { ty, .. } => {
                                        let _ = cur.read_scalar(*ty)?;
                                    }
                                }
                            }
                            triangulated_faces = push_ply_face(&mut builder, &corners, triangulated_faces, budget, at)?;
                        }
                    }
                    other => {
                        for _ in 0..el.count {
                            let at = cur.pos_info();
                            for prop in &el.properties {
                                match prop {
                                    PlyProperty::Scalar { ty, .. } => {
                                        let _ = cur.read_scalar(*ty)?;
                                    }
                                    PlyProperty::List { count_ty, item_ty, .. } => {
                                        let n = cur.read_scalar(*count_ty)?.as_u64_index(at)?;
                                        let nu = usize::try_from(n)
                                            .map_err(|_| IoError::Malformed { at, reason: "list_too_long" })?;
                                        if nu > MAX_PLY_LIST_LEN {
                                            return Err(IoError::Malformed { at, reason: "list_too_long" });
                                        }
                                        for _ in 0..nu {
                                            let _ = cur.read_scalar(*item_ty)?;
                                        }
                                    }
                                }
                            }
                        }
                        if el.count > 0 {
                            let entry = dropped_elements.entry(other.to_string()).or_insert(0);
                            *entry = entry.saturating_add(el.count);
                        }
                    }
                }
            }
        }
    }

    let end_pos = Position::byte(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
    let geometry = builder.build(frame, budget, end_pos).map_err(|e| map_build_err(e, end_pos))?;
    Ok(PlyImportResult { header, geometry, triangulated_faces, dropped_elements })
}

fn push_ply_face(
    builder: &mut MeshBuilder,
    corners: &[u64],
    mut triangulated_faces: u64,
    budget: &mut Budget,
    at: Position,
) -> Result<u64, IoError> {
    if corners.len() < 3 {
        return Err(IoError::Malformed { at, reason: "face_too_few_vertices" });
    }
    if corners.len() > 3 {
        triangulated_faces = triangulated_faces.saturating_add(1);
    }
    let Some(&v0) = corners.first() else {
        return Err(IoError::Malformed { at, reason: "face_too_few_vertices" });
    };
    for i in 1..(corners.len() - 1) {
        let (Some(&v1), Some(&v2)) = (corners.get(i), corners.get(i + 1)) else {
            return Err(IoError::Malformed { at, reason: "face_index_error" });
        };
        builder.push_face_u64([v0, v1, v2], budget, at).map_err(|e| map_build_err(e, at))?;
    }
    Ok(triangulated_faces)
}

// ------------------------------------------------------------------ writer

fn point_set_f32_max_rounding(ps: &PointSet) -> f64 {
    let mut max_abs = 0.0_f64;
    for i in 0..ps.len() {
        if let Some(p) = ps.point(i) {
            for c in p {
                let d = (c - f64::from(c as f32)).abs();
                if d > max_abs {
                    max_abs = d;
                }
            }
        }
    }
    max_abs
}

fn want_prop(filter: &BTreeSet<&str>, name: &str) -> bool {
    filter.is_empty() || filter.contains(name)
}

fn format_shortest_f64(v: f64) -> String {
    let clean = if v == 0.0 { 0.0 } else { v };
    format!("{clean}")
}

/// Writes a [`BuiltGeometry`] (GEO [`Mesh`] or [`PointSet`] plus custom scalar columns) as
/// ASCII or binary little-endian PLY (`REQ-005`).
pub fn write_ply(
    geom: &BuiltGeometry,
    opts: &PlyExportOptions,
    sink: &mut dyn ByteSink,
) -> Result<PlyExportSummary, IoError> {
    let start_bytes = sink.bytes_written();
    let inv = unit_scale_to_metres(opts.unit);
    let is_metres = opts.unit == LengthUnit::M;
    let filter: BTreeSet<&str> = opts.properties.iter().map(String::as_str).collect();

    let (v_count, tri_count, attrs, custom, max_round) = match geom {
        BuiltGeometry::Mesh(bm) => (
            bm.mesh.vertex_count(),
            bm.mesh.triangle_count(),
            bm.mesh.vertex_attrs(),
            &bm.custom_attrs,
            if opts.precision == PlyExportPrecision::Float { f32_rounding_report(&bm.mesh).max_abs_m } else { 0.0 },
        ),
        BuiltGeometry::PointSet(bp) => (
            bp.points.len(),
            0u64,
            bp.points.attributes(),
            &bp.custom_attrs,
            if opts.precision == PlyExportPrecision::Float { point_set_f32_max_rounding(&bp.points) } else { 0.0 },
        ),
    };

    let include_normals = attrs.normal.is_some()
        && (filter.is_empty() || (filter.contains("nx") && filter.contains("ny") && filter.contains("nz")));
    let include_colour = attrs.colour.is_some()
        && (filter.is_empty() || (filter.contains("red") && filter.contains("green") && filter.contains("blue")));
    let custom_cols: Vec<(&String, &ScalarColumn, PlyScalarType)> = custom
        .iter()
        .filter(|(name, _)| want_prop(&filter, name.as_str()))
        .filter_map(|(name, col)| PlyScalarType::from_custom_scalar(col.scalar_type()).map(|t| (name, col, t)))
        .collect();

    let coord_ty = match opts.precision {
        PlyExportPrecision::Double => "double",
        PlyExportPrecision::Float => "float",
    };

    // Build deterministic PLY header.
    let mut header = String::new();
    use std::fmt::Write as _;
    header.push_str("ply\n");
    let fmt_line = match opts.encoding {
        PlyExportEncoding::BinaryLe => "format binary_little_endian 1.0\n",
        PlyExportEncoding::Ascii => "format ascii 1.0\n",
    };
    header.push_str(fmt_line);
    let _ = writeln!(header, "element vertex {v_count}");
    let _ = writeln!(header, "property {coord_ty} x");
    let _ = writeln!(header, "property {coord_ty} y");
    let _ = writeln!(header, "property {coord_ty} z");
    if include_normals {
        let _ = writeln!(header, "property {coord_ty} nx");
        let _ = writeln!(header, "property {coord_ty} ny");
        let _ = writeln!(header, "property {coord_ty} nz");
    }
    for (name, _, ply_ty) in &custom_cols {
        let _ = writeln!(header, "property {} {name}", ply_ty.ply_name());
    }
    if include_colour {
        header.push_str("property uchar red\nproperty uchar green\nproperty uchar blue\n");
    }
    if tri_count > 0 {
        let _ = writeln!(header, "element face {tri_count}");
        header.push_str("property list uchar uint vertex_indices\n");
    }
    header.push_str("end_header\n");
    sink.write_bytes(header.as_bytes())?;

    let get_xyz = |i: u64| -> Result<[f64; 3], IoError> {
        let p = match geom {
            BuiltGeometry::Mesh(bm) => bm.mesh.vertex(i),
            BuiltGeometry::PointSet(bp) => bp.points.point(i),
        }
        .ok_or(IoError::Malformed { at: Position::byte(0), reason: "vertex_missing" })?;
        Ok(if is_metres { p } else { [p[0] / inv, p[1] / inv, p[2] / inv] })
    };

    match opts.encoding {
        PlyExportEncoding::BinaryLe => {
            let mut buf = Vec::with_capacity(64 * 1024);
            for i in 0..v_count {
                let idx = usize::try_from(i).unwrap_or(usize::MAX);
                let p = get_xyz(i)?;
                match opts.precision {
                    PlyExportPrecision::Double => {
                        for c in p {
                            buf.extend_from_slice(&c.to_le_bytes());
                        }
                    }
                    PlyExportPrecision::Float => {
                        for c in p {
                            buf.extend_from_slice(&(c as f32).to_le_bytes());
                        }
                    }
                }
                if include_normals {
                    if let Some(ncols) = &attrs.normal {
                        let n = [
                            ncols[0].get(idx).copied().unwrap_or(0.0),
                            ncols[1].get(idx).copied().unwrap_or(0.0),
                            ncols[2].get(idx).copied().unwrap_or(0.0),
                        ];
                        match opts.precision {
                            PlyExportPrecision::Double => {
                                for c in n {
                                    buf.extend_from_slice(&c.to_le_bytes());
                                }
                            }
                            PlyExportPrecision::Float => {
                                for c in n {
                                    buf.extend_from_slice(&(c as f32).to_le_bytes());
                                }
                            }
                        }
                    }
                }
                for (_, col, _) in &custom_cols {
                    write_custom_bin_le(col, idx, &mut buf);
                }
                if include_colour {
                    if let Some(ccol) = &attrs.colour {
                        let rgb = ccol.get(idx).copied().unwrap_or([0, 0, 0]);
                        buf.extend_from_slice(&rgb);
                    }
                }
                if buf.len() >= 60 * 1024 {
                    sink.write_bytes(&buf)?;
                    buf.clear();
                }
            }
            if let BuiltGeometry::Mesh(bm) = geom {
                for tri in bm.mesh.triangles() {
                    let i0 = u32::try_from(tri[0]).map_err(|_| IoError::LimitExceeded {
                        limit: LimitKind::Elements,
                        value: u64::from(u32::MAX),
                        at: Position::byte(0),
                    })?;
                    let i1 = u32::try_from(tri[1]).map_err(|_| IoError::LimitExceeded {
                        limit: LimitKind::Elements,
                        value: u64::from(u32::MAX),
                        at: Position::byte(0),
                    })?;
                    let i2 = u32::try_from(tri[2]).map_err(|_| IoError::LimitExceeded {
                        limit: LimitKind::Elements,
                        value: u64::from(u32::MAX),
                        at: Position::byte(0),
                    })?;
                    buf.push(3u8);
                    buf.extend_from_slice(&i0.to_le_bytes());
                    buf.extend_from_slice(&i1.to_le_bytes());
                    buf.extend_from_slice(&i2.to_le_bytes());
                    if buf.len() >= 60 * 1024 {
                        sink.write_bytes(&buf)?;
                        buf.clear();
                    }
                }
            }
            if !buf.is_empty() {
                sink.write_bytes(&buf)?;
            }
        }
        PlyExportEncoding::Ascii => {
            let mut out = String::with_capacity(64 * 1024);
            for i in 0..v_count {
                let idx = usize::try_from(i).unwrap_or(usize::MAX);
                let p = get_xyz(i)?;
                match opts.precision {
                    PlyExportPrecision::Double => {
                        let _ = write!(
                            out,
                            "{} {} {}",
                            format_shortest_f64(p[0]),
                            format_shortest_f64(p[1]),
                            format_shortest_f64(p[2])
                        );
                    }
                    PlyExportPrecision::Float => {
                        let _ = write!(out, "{} {} {}", p[0] as f32, p[1] as f32, p[2] as f32);
                    }
                }
                if include_normals {
                    if let Some(ncols) = &attrs.normal {
                        let n = [
                            ncols[0].get(idx).copied().unwrap_or(0.0),
                            ncols[1].get(idx).copied().unwrap_or(0.0),
                            ncols[2].get(idx).copied().unwrap_or(0.0),
                        ];
                        match opts.precision {
                            PlyExportPrecision::Double => {
                                let _ = write!(
                                    out,
                                    " {} {} {}",
                                    format_shortest_f64(n[0]),
                                    format_shortest_f64(n[1]),
                                    format_shortest_f64(n[2])
                                );
                            }
                            PlyExportPrecision::Float => {
                                let _ = write!(out, " {} {} {}", n[0] as f32, n[1] as f32, n[2] as f32);
                            }
                        }
                    }
                }
                for (_, col, _) in &custom_cols {
                    out.push(' ');
                    write_custom_ascii(col, idx, &mut out);
                }
                if include_colour {
                    if let Some(ccol) = &attrs.colour {
                        let rgb = ccol.get(idx).copied().unwrap_or([0, 0, 0]);
                        let _ = write!(out, " {} {} {}", rgb[0], rgb[1], rgb[2]);
                    }
                }
                out.push('\n');
                if out.len() >= 60 * 1024 {
                    sink.write_bytes(out.as_bytes())?;
                    out.clear();
                }
            }
            if let BuiltGeometry::Mesh(bm) = geom {
                for tri in bm.mesh.triangles() {
                    let _ = writeln!(out, "3 {} {} {}", tri[0], tri[1], tri[2]);
                    if out.len() >= 60 * 1024 {
                        sink.write_bytes(out.as_bytes())?;
                        out.clear();
                    }
                }
            }
            if !out.is_empty() {
                sink.write_bytes(out.as_bytes())?;
            }
        }
    }

    Ok(PlyExportSummary {
        encoding: opts.encoding,
        precision: opts.precision,
        unit: opts.unit,
        vertices: v_count,
        triangles: tri_count,
        bytes: sink.bytes_written().saturating_sub(start_bytes),
        max_abs_rounding_m: max_round,
    })
}

fn write_custom_bin_le(col: &ScalarColumn, idx: usize, buf: &mut Vec<u8>) {
    match col {
        ScalarColumn::I8(v) => buf.extend_from_slice(&v.get(idx).copied().unwrap_or(0).to_le_bytes()),
        ScalarColumn::U8(v) => buf.extend_from_slice(&v.get(idx).copied().unwrap_or(0).to_le_bytes()),
        ScalarColumn::I16(v) => buf.extend_from_slice(&v.get(idx).copied().unwrap_or(0).to_le_bytes()),
        ScalarColumn::U16(v) => buf.extend_from_slice(&v.get(idx).copied().unwrap_or(0).to_le_bytes()),
        ScalarColumn::I32(v) => buf.extend_from_slice(&v.get(idx).copied().unwrap_or(0).to_le_bytes()),
        ScalarColumn::U32(v) => buf.extend_from_slice(&v.get(idx).copied().unwrap_or(0).to_le_bytes()),
        ScalarColumn::I64(v) => buf.extend_from_slice(&v.get(idx).copied().unwrap_or(0).to_le_bytes()),
        ScalarColumn::U64(v) => buf.extend_from_slice(&v.get(idx).copied().unwrap_or(0).to_le_bytes()),
        ScalarColumn::F32(v) => buf.extend_from_slice(&v.get(idx).copied().unwrap_or(0.0).to_le_bytes()),
        ScalarColumn::F64(v) => buf.extend_from_slice(&v.get(idx).copied().unwrap_or(0.0).to_le_bytes()),
    }
}

fn write_custom_ascii(col: &ScalarColumn, idx: usize, out: &mut String) {
    use std::fmt::Write as _;
    match col {
        ScalarColumn::I8(v) => {
            let _ = write!(out, "{}", v.get(idx).copied().unwrap_or(0));
        }
        ScalarColumn::U8(v) => {
            let _ = write!(out, "{}", v.get(idx).copied().unwrap_or(0));
        }
        ScalarColumn::I16(v) => {
            let _ = write!(out, "{}", v.get(idx).copied().unwrap_or(0));
        }
        ScalarColumn::U16(v) => {
            let _ = write!(out, "{}", v.get(idx).copied().unwrap_or(0));
        }
        ScalarColumn::I32(v) => {
            let _ = write!(out, "{}", v.get(idx).copied().unwrap_or(0));
        }
        ScalarColumn::U32(v) => {
            let _ = write!(out, "{}", v.get(idx).copied().unwrap_or(0));
        }
        ScalarColumn::I64(v) => {
            let _ = write!(out, "{}", v.get(idx).copied().unwrap_or(0));
        }
        ScalarColumn::U64(v) => {
            let _ = write!(out, "{}", v.get(idx).copied().unwrap_or(0));
        }
        ScalarColumn::F32(v) => {
            let _ = write!(out, "{}", v.get(idx).copied().unwrap_or(0.0));
        }
        ScalarColumn::F64(v) => {
            let _ = write!(out, "{}", format_shortest_f64(v.get(idx).copied().unwrap_or(0.0)));
        }
    }
}

fn encode_staged_point_set_payload(ps: &PointSet) -> Vec<u8> {
    let n = ps.len();
    let cap = 16usize.saturating_add(usize::try_from(n).unwrap_or(0).saturating_mul(24));
    let mut out = Vec::with_capacity(cap);
    out.extend_from_slice(b"FIDPTS01");
    out.extend_from_slice(&n.to_le_bytes());
    for i in 0..n {
        if let Some(p) = ps.point(i) {
            out.extend_from_slice(&p[0].to_le_bytes());
            out.extend_from_slice(&p[1].to_le_bytes());
            out.extend_from_slice(&p[2].to_le_bytes());
        }
    }
    out
}

// ------------------------------------------------------------------ fid-io-reg integration

/// Returns the registered `FormatId` (`"mesh-ply"`).
pub fn ply_format_id() -> Result<FormatId, fid_io_reg::report::ReportError> {
    FormatId::new(PLY_FORMAT_ID)
}

/// Format descriptor for PLY import (magic `"ply\n"` / `"ply\r\n"`, extension `.ply`).
pub fn ply_import_descriptor() -> Result<FormatDescriptor, fid_io_reg::DetectError> {
    let format =
        ply_format_id().map_err(|_| fid_io_reg::DetectError::Invalid { field: "format", reason: "bad format id" })?;
    let sigs = vec![
        Signature::new(0, Matcher::Bytes(b"ply\n".to_vec()), 90)?,
        Signature::new(0, Matcher::Bytes(b"ply\r\n".to_vec()), 90)?,
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
        vec!["ply".to_string()],
        opts_schema,
        Phase::new(1).ok_or(fid_io_reg::DetectError::Invalid { field: "phase", reason: "bad phase" })?,
        Origin::FirstParty,
        Direction::Import,
    )
}

/// Format descriptor for PLY export (`encoding`, `precision`, `unit`, `properties`).
pub fn ply_export_descriptor() -> Result<FormatDescriptor, fid_io_reg::DetectError> {
    let format =
        ply_format_id().map_err(|_| fid_io_reg::DetectError::Invalid { field: "format", reason: "bad format id" })?;
    let mut props = BTreeMap::new();
    props.insert(
        "encoding".to_string(),
        CValue::Obj(BTreeMap::from([
            ("default".to_string(), CValue::Str("binary_le".to_string())),
            (
                "enum".to_string(),
                CValue::Arr(vec![CValue::Str("binary_le".to_string()), CValue::Str("ascii".to_string())]),
            ),
            ("type".to_string(), CValue::Str("string".to_string())),
        ])),
    );
    props.insert(
        "precision".to_string(),
        CValue::Obj(BTreeMap::from([
            ("default".to_string(), CValue::Str("double".to_string())),
            (
                "enum".to_string(),
                CValue::Arr(vec![CValue::Str("double".to_string()), CValue::Str("float".to_string())]),
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
    props.insert(
        "properties".to_string(),
        CValue::Obj(BTreeMap::from([
            (
                "items".to_string(),
                CValue::Obj(BTreeMap::from([("type".to_string(), CValue::Str("string".to_string()))])),
            ),
            ("type".to_string(), CValue::Str("array".to_string())),
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
        vec!["ply".to_string()],
        opts_schema,
        Phase::new(1).ok_or(fid_io_reg::DetectError::Invalid { field: "phase", reason: "bad phase" })?,
        Origin::FirstParty,
        Direction::Export,
    )
}

/// Registers both import and export descriptors for `mesh-ply` in `reg`.
pub fn register_ply(reg: &mut Registry) -> Result<(), IoError> {
    let imp =
        ply_import_descriptor().map_err(|_| IoError::Malformed { at: Position::byte(0), reason: "descriptor" })?;
    let exp =
        ply_export_descriptor().map_err(|_| IoError::Malformed { at: Position::byte(0), reason: "descriptor" })?;
    reg.register(imp)?;
    reg.register(exp)?;
    Ok(())
}

fn sanitize_element_name(raw: &str) -> String {
    let mut out = String::new();
    for b in raw.bytes() {
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
        format!("el_{out}")
    } else {
        out
    }
}

/// `fid-io-reg` import reader for `mesh-ply` (`import.mesh_ply@1`).
#[derive(Debug, Clone)]
pub struct PlyReader {
    format: FormatId,
    info: ReaderInfo,
}

impl PlyReader {
    /// Constructs the first-party PLY reader (`fid-io-mesh-ply` v1.0.0).
    pub fn new() -> Result<Self, ImportError> {
        let format = ply_format_id().map_err(|_| ImportError::Internal("invalid mesh-ply format id"))?;
        let id = ReaderId::new("fid-io-mesh-ply").map_err(|_| ImportError::Internal("invalid ply reader id"))?;
        let version = VersionToken::new("1.0.0").map_err(|_| ImportError::Internal("invalid ply reader version"))?;
        Ok(PlyReader { format, info: ReaderInfo { id, version } })
    }
}

impl Reader for PlyReader {
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
            return Err("mesh-ply import takes no format options");
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
        let decoded = read_ply_bytes(&raw, LengthUnit::M, frame, sink.source().budget_mut())?;
        drop(raw);

        if let Ok(ver) = VersionToken::new("1.0") {
            sink.declare_format_version(ver);
        }
        sink.declare("encoding", UntrustedText::new(decoded.header.format.as_str()));

        let (prec, counts, type_id, payload) = match &decoded.geometry {
            BuiltGeometry::Mesh(bm) => {
                (bm.source_precision, bm.counts, STL_ELEMENT_TYPE, encode_staged_mesh_payload(&bm.mesh))
            }
            BuiltGeometry::PointSet(bp) => (
                bp.source_precision,
                bp.counts,
                PLY_POINT_SET_ELEMENT_TYPE,
                encode_staged_point_set_payload(&bp.points),
            ),
        };
        sink.declare("source_precision", UntrustedText::new(prec.as_str()));

        if let Ok(tn) = TypeName::new("mesh.vertex") {
            report.count(&tn, counts.vertices);
        }
        if counts.faces > 0 {
            if let Ok(tn) = TypeName::new("mesh.face") {
                report.count(&tn, counts.faces);
            }
        }
        if decoded.triangulated_faces > 0 {
            if let Ok(tn) = TypeName::new("mesh.triangulated_face") {
                report.count(&tn, decoded.triangulated_faces);
            }
        }
        // REQ-006: report skipped non-vertex/face elements with counts.
        for (el_name, count) in &decoded.dropped_elements {
            let safe = sanitize_element_name(el_name);
            if let Ok(tn) = TypeName::new(&format!("ply.dropped.{safe}")) {
                report.count(&tn, *count);
            }
            if let (Ok(tn), Ok(pos), Ok(code)) =
                (TypeName::new(&format!("ply.{safe}")), ReportPos::new(Position::byte(0)), Code::new("element_dropped"))
            {
                report.drop_item(&tn, pos, &code, &UntrustedText::new(&format!("{el_name} x {count}")));
            }
        }

        let name = if type_id == PLY_POINT_SET_ELEMENT_TYPE { "ply-point-set" } else { "ply-mesh" };
        sink.add_entity(type_id, &UntrustedText::new(name), &payload, Position::byte(0), report)?;
        Ok(())
    }
}

/// `fid-io-reg` export writer for `mesh-ply` (`export.mesh_ply@1`).
#[derive(Debug, Clone)]
pub struct PlyWriter {
    format: FormatId,
    version: VersionToken,
}

impl PlyWriter {
    /// Constructs the first-party PLY writer (`1.0.0`).
    pub fn new() -> Result<Self, ExportError> {
        let format = ply_format_id().map_err(|_| ExportError::Internal("invalid mesh-ply format id"))?;
        let version = VersionToken::new("1.0.0").map_err(|_| ExportError::Internal("invalid ply writer version"))?;
        Ok(PlyWriter { format, version })
    }
}

fn parse_export_opts(v: &CValue) -> Result<PlyExportOptions, &'static str> {
    let CValue::Obj(o) = v else {
        return Err("options must be an object");
    };
    if o.keys().any(|k| !matches!(k.as_str(), "encoding" | "precision" | "unit" | "properties")) {
        return Err("unknown mesh-ply export option");
    }
    let encoding = match o.get("encoding") {
        None => PlyExportEncoding::BinaryLe,
        Some(CValue::Str(s)) => PlyExportEncoding::parse(s).ok_or("encoding must be binary_le or ascii")?,
        Some(_) => return Err("encoding must be a string"),
    };
    let precision = match o.get("precision") {
        None => PlyExportPrecision::Double,
        Some(CValue::Str(s)) => PlyExportPrecision::parse(s).ok_or("precision must be double or float")?,
        Some(_) => return Err("precision must be a string"),
    };
    let unit = match o.get("unit") {
        None => LengthUnit::M,
        Some(CValue::Str(s)) => parse_unit_symbol(s).ok_or("unknown length unit")?,
        Some(_) => return Err("unit must be a string"),
    };
    let properties = match o.get("properties") {
        None => Vec::new(),
        Some(CValue::Arr(arr)) => {
            let mut out = Vec::with_capacity(arr.len());
            for item in arr {
                let CValue::Str(s) = item else {
                    return Err("properties must be an array of strings");
                };
                if s.is_empty() || s.len() > 64 {
                    return Err("invalid property name");
                }
                out.push(s.clone());
            }
            out
        }
        Some(_) => return Err("properties must be an array"),
    };
    Ok(PlyExportOptions { encoding, precision, unit, properties })
}

impl Writer for PlyWriter {
    type Source = BuiltGeometry;

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
            ("precision".to_string(), CValue::Str(parsed.precision.as_str().to_string())),
            ("properties".to_string(), CValue::Arr(parsed.properties.into_iter().map(CValue::Str).collect())),
            ("unit".to_string(), CValue::Str(parsed.unit.symbol().to_string())),
        ])))
    }

    fn write(
        &self,
        src: &BuiltGeometry,
        opts: &CValue,
        sink: &mut dyn ByteSink,
        report: &mut ReportBuilder,
    ) -> Result<(), IoError> {
        let parsed = parse_export_opts(opts)
            .map_err(|_| IoError::Malformed { at: Position::byte(0), reason: "invalid_export_options" })?;
        let summary = write_ply(src, &parsed, sink)?;
        if summary.max_abs_rounding_m > 0.0 {
            if let Ok(tn) = TypeName::new("f64_precision") {
                report.lossy(&tn);
            }
            if let Ok(code) = Code::new("f32_rounding") {
                report.warn(&code, None, &UntrustedText::new(&format!("max_abs_m={}", summary.max_abs_rounding_m)));
            }
        }
        Ok(())
    }
}

/// Builds the `import.mesh_ply@1` command on the `fid-io-reg` import skeleton.
pub fn import_mesh_ply_command() -> Result<ImportCommand<PlyReader>, ImportError> {
    ImportCommand::new(PlyReader::new()?, &[])
}

/// Builds the `export.mesh_ply@1` command on the `fid-io-reg` export skeleton.
pub fn export_mesh_ply_command() -> Result<ExportCommand<PlyWriter>, ExportError> {
    ExportCommand::new(PlyWriter::new()?, &[])
}

/// Builds the registry [`CommandDef`]s for `import.mesh_ply@1` and `export.mesh_ply@1`.
pub fn ply_command_defs(reg: &Registry) -> Result<Vec<CommandDef>, String> {
    let fmt = ply_format_id().map_err(|e| e.to_string())?;
    let imp = import::command::format_def(reg, &fmt, &[]).map_err(|e| e.to_string())?;
    let exp = export::command::format_def(reg, &fmt, &[]).map_err(|e| e.to_string())?;
    Ok(vec![imp, exp])
}
