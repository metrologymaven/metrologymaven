//! Bounded first-party 2D drawing reader for PDF page structure, PNG, and TIFF
//! (spec node `IO.REG.PLUGIN.DRAW`).
//!
//! - Implements [`fid_io_plugin::abi::FormatPluginReader`] for formats `pdf-drawing` and
//!   `raster-drawing` (`REQ-IO.REG.PLUGIN.DRAW-001`): receives only the input byte stream and
//!   options, with zero file, network, clock, or document access.
//! - PDF (`pdf`): bounded xref tables/streams, ObjStm, iterative cycle-safe page-tree walk,
//!   `CropBox` else `MediaBox`, `/Rotate`, `/UserUnit`, integer `0.01 mm` dimensions; rejects
//!   encrypted PDFs (`pdf_encrypted`); never executes actions or JavaScript, reporting their
//!   presence plus embedded files and forms as warnings (`REQ-002`, `REQ-004`, `REQ-005`).
//! - PNG & TIFF (`png`, `tiff`): bounded pure-Rust decoders (all standard PNG color types &
//!   `pHYs` DPI; multi-page TIFF baseline, CCITT G3/G4, LZW, Deflate, PackBits & resolution
//!   tags), outputting `Gray8` or `Rgba8` (`REQ-003`).
//! - Limits: 200 pages, 500 MiB input, 400 Mpx per raster page (`limit_exceeded`, `REQ-006`).
//! - CONSTITUTION §8: `#![forbid(unsafe_code)]`, zero panics, zero `unwrap`/`expect` in library code.

pub(crate) mod flate;
pub mod pdf;
pub mod png;
pub mod tiff;

use fid_canon::CValue;
use fid_io_plugin::abi::{
    ByteSignature, ContractVersion, DrawingPageRecord, FormatPluginReader, KvEntry, PluginDescriptor, PluginError,
    PluginFormatSpec, ReadRequest, Record, RequestedCaps, WarningRecord, HOST_CONTRACT_VERSION,
};
use fid_io_reg::{Direction, FormatId, Phase, ReaderId, UntrustedText, VersionToken};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use std::fmt;
use std::io::Read;

/// Maximum pages accepted in one PDF or multi-page TIFF (`REQ-IO.REG.PLUGIN.DRAW-006`, `AC-7`).
pub const MAX_PAGES: usize = 200;
/// Maximum input stream size accepted (500 MiB, `REQ-006`).
pub const MAX_INPUT_BYTES: usize = 500 * 1024 * 1024;
/// Maximum pixel count per raster page (400 Mpx, `REQ-006`).
pub const MAX_RASTER_PIXELS: u64 = 400_000_000;
/// Registered format ID for vector PDF page-structure drawings (`Design: Registry commands`).
pub const FORMAT_PDF_DRAWING: &str = "pdf-drawing";
/// Registered format ID for PNG and TIFF raster drawings (`Design: Registry commands`).
pub const FORMAT_RASTER_DRAWING: &str = "raster-drawing";
/// First-party reader/plugin identifier.
pub const DRAW_PLUGIN_ID: &str = "fid.io.draw";
/// First-party reader version token.
pub const DRAW_PLUGIN_VERSION: &str = "1.0.0";
/// Warning code emitted when PDF JavaScript is encountered and ignored (`REQ-005`, `AC-6`).
pub const WARN_JS_IGNORED: &str = "pdf_javascript_ignored";
/// Warning code emitted when PDF actions are encountered and ignored (`REQ-005`).
pub const WARN_ACTION_IGNORED: &str = "pdf_action_ignored";
/// Warning code emitted when PDF embedded files are encountered and ignored (`REQ-005`).
pub const WARN_EMBEDDED_FILES_IGNORED: &str = "pdf_embedded_files_ignored";
/// Warning code emitted when PDF AcroForm / interactive forms are encountered and ignored (`REQ-005`).
pub const WARN_FORMS_IGNORED: &str = "pdf_forms_ignored";

/// `n / d` rounded half to even for `d > 0`.
pub(crate) fn round_half_even(n: i128, d: i128) -> i128 {
    if d <= 0 {
        return 0;
    }
    let q = n.div_euclid(d);
    let r = n.rem_euclid(d);
    match (2 * r).cmp(&d) {
        std::cmp::Ordering::Greater => q + 1,
        std::cmp::Ordering::Less => q,
        std::cmp::Ordering::Equal => q + (q & 1),
    }
}

// ------------------------------------------------------------------ errors & warnings

/// Typed errors of the bounded 2D drawing reader (`Design: Errors`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DrawError {
    /// `pdf_encrypted`: PDF carries an `/Encrypt` dictionary (`REQ-004`, `AC-6`).
    PdfEncrypted,
    /// `limit_exceeded`: page count (>200), byte size (>500 MiB), or raster pixels (>400 Mpx) exceeded (`REQ-006`).
    LimitExceeded { limit: &'static str, detail: String },
    /// `malformed`: structurally invalid input with byte offset (`REQ-007`).
    Malformed { detail: String, at: u64 },
    /// `truncated_file`: unexpected EOF with byte offset (`REQ-007`).
    TruncatedFile { detail: String, at: u64 },
    /// `format_unknown`: bytes do not match PDF, PNG, or TIFF signatures.
    FormatUnknown,
}

impl DrawError {
    pub fn pdf_encrypted() -> Self {
        DrawError::PdfEncrypted
    }

    pub fn limit_exceeded(limit: &'static str, detail: impl Into<String>) -> Self {
        DrawError::LimitExceeded { limit, detail: detail.into() }
    }

    pub fn malformed(detail: impl Into<String>, at: u64) -> Self {
        DrawError::Malformed { detail: detail.into(), at }
    }

    pub fn truncated(detail: impl Into<String>, at: u64) -> Self {
        DrawError::TruncatedFile { detail: detail.into(), at }
    }

    pub fn code(&self) -> &'static str {
        match self {
            DrawError::PdfEncrypted => "pdf_encrypted",
            DrawError::LimitExceeded { .. } => "limit_exceeded",
            DrawError::Malformed { .. } => "malformed",
            DrawError::TruncatedFile { .. } => "truncated_file",
            DrawError::FormatUnknown => "format_unknown",
        }
    }

    pub fn byte_offset(&self) -> Option<u64> {
        match self {
            DrawError::Malformed { at, .. } | DrawError::TruncatedFile { at, .. } => Some(*at),
            _ => None,
        }
    }

    pub fn into_plugin_error(self) -> PluginError {
        let code = self.code().to_string();
        let byte_offset = self.byte_offset();
        let detail = UntrustedText::from_string(self.to_string());
        PluginError { code, detail, byte_offset }
    }
}

impl fmt::Display for DrawError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DrawError::PdfEncrypted => f.write_str("pdf_encrypted: encrypted PDF drawings are not supported"),
            DrawError::LimitExceeded { limit, detail } => write!(f, "limit_exceeded ({limit}): {detail}"),
            DrawError::Malformed { detail, at } => write!(f, "malformed at byte {at}: {detail}"),
            DrawError::TruncatedFile { detail, at } => write!(f, "truncated_file at byte {at}: {detail}"),
            DrawError::FormatUnknown => f.write_str("format_unknown: expected PDF, PNG, or TIFF drawing"),
        }
    }
}

impl std::error::Error for DrawError {}

/// Non-fatal diagnostic warning recorded during drawing import (`REQ-005`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawWarning {
    pub code: String,
    pub detail: String,
    pub byte_offset: Option<u64>,
}

// ------------------------------------------------------------------ drawing page model

/// PDF page box selected for geometry (`REQ-002`, `AC-3`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxUsed {
    Crop,
    Media,
}

impl BoxUsed {
    pub const fn as_str(self) -> &'static str {
        match self {
            BoxUsed::Crop => "crop",
            BoxUsed::Media => "media",
        }
    }
}

/// Decoded raster pixel encoding (`Scope`: RGBA8 or Gray8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    Gray8,
    Rgba8,
}

impl PixelFormat {
    pub const fn as_str(self) -> &'static str {
        match self {
            PixelFormat::Gray8 => "gray8",
            PixelFormat::Rgba8 => "rgba8",
        }
    }
}

/// Decoded raster page attached to a [`DrawingPage`] for PNG and TIFF inputs (`REQ-003`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RasterPage {
    pub width: u32,
    pub height: u32,
    pub format: PixelFormat,
    /// Stated resolution `(x_dpi, y_dpi)`, or `None` (`resolution: none`) when unstated (`AC-4`).
    pub dpi: Option<(u32, u32)>,
    /// Raw decoded pixels (`Gray8`: `width * height` bytes; `Rgba8`: `width * height * 4` bytes).
    pub pixels: Vec<u8>,
}

impl RasterPage {
    /// SHA-256 digest of the decoded pixel buffer (for deterministic checksums, `AC-5`).
    pub fn pixels_sha256(&self) -> fid_types::Digest {
        fid_types::Digest::from_bytes(Sha256::digest(&self.pixels).into())
    }

    /// Computes page dimensions in integer `0.01 mm` from stated DPI, or `0` when `dpi` is `None`.
    pub fn dimensions_cmm(&self) -> (i64, i64) {
        match self.dpi {
            Some((dx, dy)) if dx > 0 && dy > 0 => {
                let w = round_half_even(i128::from(self.width) * 2540, i128::from(dx)) as i64;
                let h = round_half_even(i128::from(self.height) * 2540, i128::from(dy)) as i64;
                (w, h)
            }
            _ => (0, 0),
        }
    }
}

/// One 2D drawing page record emitted by `IO.REG.PLUGIN.DRAW` (`Design: Data structures`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawingPage {
    /// 0-based page index in document order.
    pub index: u32,
    /// Page width in integer `0.01 mm` units (`0` for raster images with `resolution: none`).
    pub width_cmm: i64,
    /// Page height in integer `0.01 mm` units (`0` for raster images with `resolution: none`).
    pub height_cmm: i64,
    /// Normalized page rotation in degrees (`0`, `90`, `180`, `270`).
    pub rotation: u16,
    /// Box used for PDF geometry (`Crop` or `Media`).
    pub box_used: BoxUsed,
    /// Decoded raster image when the source was PNG or TIFF (`None` for vector PDFs).
    pub raster: Option<RasterPage>,
}

/// Complete typed result of reading a PDF, PNG, or TIFF drawing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawReadResult {
    pub format_id: &'static str,
    pub pages: Vec<DrawingPage>,
    pub warnings: Vec<DrawWarning>,
}

/// Reads drawing bytes directly (auto-detecting PDF, PNG, or TIFF, or validating `format_hint`).
pub fn read_drawing_bytes(format_hint: Option<&str>, bytes: &[u8]) -> Result<DrawReadResult, DrawError> {
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(DrawError::limit_exceeded("bytes", format!("input {} exceeds 500 MiB", bytes.len())));
    }

    let is_pdf = bytes.len() >= 5 && bytes[..bytes.len().min(1024)].windows(5).any(|w| w == b"%PDF-");
    let is_png = bytes.starts_with(b"\x89PNG\r\n\x1a\n");
    let is_tiff = bytes.starts_with(b"II\x2A\x00") || bytes.starts_with(b"MM\x00\x2A");

    match format_hint {
        Some(FORMAT_PDF_DRAWING) => {
            if !is_pdf {
                return Err(if bytes.is_empty() {
                    DrawError::truncated("empty input", 0)
                } else {
                    DrawError::FormatUnknown
                });
            }
            let st = pdf::read_pdf_structure(bytes)?;
            Ok(DrawReadResult { format_id: FORMAT_PDF_DRAWING, pages: st.pages, warnings: st.warnings })
        }
        Some(FORMAT_RASTER_DRAWING) => {
            if is_png {
                let r = png::decode_png(bytes)?;
                let (w_cmm, h_cmm) = r.dimensions_cmm();
                Ok(DrawReadResult {
                    format_id: FORMAT_RASTER_DRAWING,
                    pages: vec![DrawingPage {
                        index: 0,
                        width_cmm: w_cmm,
                        height_cmm: h_cmm,
                        rotation: 0,
                        box_used: BoxUsed::Media,
                        raster: Some(r),
                    }],
                    warnings: Vec::new(),
                })
            } else if is_tiff {
                let rasters = tiff::decode_tiff(bytes)?;
                let pages = rasters
                    .into_iter()
                    .enumerate()
                    .map(|(i, r)| {
                        let (w_cmm, h_cmm) = r.dimensions_cmm();
                        DrawingPage {
                            index: i as u32,
                            width_cmm: w_cmm,
                            height_cmm: h_cmm,
                            rotation: 0,
                            box_used: BoxUsed::Media,
                            raster: Some(r),
                        }
                    })
                    .collect();
                Ok(DrawReadResult { format_id: FORMAT_RASTER_DRAWING, pages, warnings: Vec::new() })
            } else {
                Err(DrawError::FormatUnknown)
            }
        }
        None => {
            if is_pdf {
                read_drawing_bytes(Some(FORMAT_PDF_DRAWING), bytes)
            } else if is_png || is_tiff {
                read_drawing_bytes(Some(FORMAT_RASTER_DRAWING), bytes)
            } else {
                Err(DrawError::FormatUnknown)
            }
        }
        Some(_) => Err(DrawError::FormatUnknown),
    }
}

// ------------------------------------------------------------------ FormatPluginReader contract

/// First-party 2D drawing reader implementing the `IO.REG.PLUGIN.ABI` contract (`REQ-001`).
#[derive(Debug, Clone)]
pub struct DrawPluginReader {
    descriptor: PluginDescriptor,
}

impl DrawPluginReader {
    /// Constructs the first-party `fid.io.draw` reader with a validated [`PluginDescriptor`] for
    /// `pdf-drawing` and `raster-drawing`.
    pub fn new() -> Result<DrawPluginReader, fid_io_plugin::abi::AbiError> {
        let invalid = |field: &str, detail: &str| fid_io_plugin::abi::AbiError::DescriptorInvalid {
            field: field.to_string(),
            detail: detail.to_string(),
        };
        let phase = Phase::new(1).ok_or_else(|| invalid("formats.phase", "invalid P1 phase"))?;
        let empty_schema = CValue::Obj(BTreeMap::new());
        let pdf_spec = PluginFormatSpec {
            format_id: FormatId::new(FORMAT_PDF_DRAWING)
                .map_err(|_| invalid("formats.pdf-drawing", "invalid format ID"))?,
            direction: Direction::Import,
            version_min: "1.0".to_string(),
            version_max: "2.0".to_string(),
            signatures: vec![ByteSignature { offset: 0, bytes: b"%PDF-".to_vec() }],
            extensions: vec!["pdf".to_string()],
            options_schema: empty_schema.clone(),
            phase,
        };
        let raster_spec = PluginFormatSpec {
            format_id: FormatId::new(FORMAT_RASTER_DRAWING)
                .map_err(|_| invalid("formats.raster-drawing", "invalid format ID"))?,
            direction: Direction::Import,
            version_min: "1.0".to_string(),
            version_max: "6.0".to_string(),
            signatures: vec![
                ByteSignature { offset: 0, bytes: b"\x89PNG\r\n\x1a\n".to_vec() },
                ByteSignature { offset: 0, bytes: b"II\x2A\x00".to_vec() },
                ByteSignature { offset: 0, bytes: b"MM\x00\x2A".to_vec() },
            ],
            extensions: vec!["png".to_string(), "tif".to_string(), "tiff".to_string()],
            options_schema: empty_schema,
            phase,
        };
        let descriptor = PluginDescriptor {
            plugin_id: ReaderId::new(DRAW_PLUGIN_ID).map_err(|_| invalid("plugin_id", "invalid plugin ID"))?,
            version: VersionToken::new(DRAW_PLUGIN_VERSION).map_err(|_| invalid("version", "invalid version"))?,
            contract: ContractVersion::new(HOST_CONTRACT_VERSION.major, HOST_CONTRACT_VERSION.minor),
            formats: vec![pdf_spec, raster_spec],
            requested_caps: RequestedCaps { max_memory_bytes: 512 * 1024 * 1024, max_wall_ms: 30_000 },
        };
        descriptor.validate()?;
        Ok(DrawPluginReader { descriptor })
    }

    /// Registers `pdf-drawing` and `raster-drawing` into `registry` (`IO.REG.DETECT`) with
    /// `Origin::FirstParty`.
    pub fn register_formats(
        &self,
        registry: &mut fid_io_reg::Registry,
    ) -> Result<Vec<fid_io_reg::FormatDescriptor>, fid_io_plugin::abi::AbiError> {
        fid_io_plugin::abi::adapter::register_descriptor(
            registry,
            &self.descriptor,
            fid_io_reg::Origin::FirstParty,
            HOST_CONTRACT_VERSION,
        )
    }
}

impl FormatPluginReader for DrawPluginReader {
    fn descriptor(&self) -> &PluginDescriptor {
        &self.descriptor
    }

    fn read(&self, req: &mut ReadRequest<'_>) -> Result<Vec<Record>, PluginError> {
        let mut buf = Vec::new();
        let cap = MAX_INPUT_BYTES + 1;
        req.stream.take(cap as u64).read_to_end(&mut buf).map_err(|e| {
            DrawError::malformed(format!("stream read failed: {}", e.kind()), buf.len() as u64).into_plugin_error()
        })?;
        if buf.len() > MAX_INPUT_BYTES {
            return Err(DrawError::limit_exceeded("bytes", "input stream exceeds 500 MiB").into_plugin_error());
        }

        let res = read_drawing_bytes(Some(req.format_id.as_str()), &buf).map_err(DrawError::into_plugin_error)?;
        let mut records = Vec::with_capacity(res.pages.len() + res.warnings.len());

        for page in res.pages {
            let mut metadata = vec![
                KvEntry::new("width_cmm", page.width_cmm.to_string()),
                KvEntry::new("height_cmm", page.height_cmm.to_string()),
                KvEntry::new("rotation", page.rotation.to_string()),
                KvEntry::new("box_used", page.box_used.as_str()),
            ];
            let (dpi, media_type, image_bytes, width_mm, height_mm) = match page.raster {
                Some(r) => {
                    metadata.push(KvEntry::new("raster_width_px", r.width.to_string()));
                    metadata.push(KvEntry::new("raster_height_px", r.height.to_string()));
                    metadata.push(KvEntry::new("pixel_format", r.format.as_str()));
                    metadata.push(KvEntry::new("pixels_sha256", r.pixels_sha256().as_str()));
                    let (dpi_val, w_mm, h_mm) = match r.dpi {
                        Some((dx, dy)) => {
                            metadata.push(KvEntry::new("resolution", format!("{dx}x{dy}dpi")));
                            (dx, (page.width_cmm as f64) / 100.0, (page.height_cmm as f64) / 100.0)
                        }
                        None => {
                            metadata.push(KvEntry::new("resolution", "none"));
                            // PluginReaderAdapter enforces width_mm > 0 && height_mm > 0; report
                            // nominal 96 dpi display size while metadata + dpi=0 preserve `none`.
                            (0, (f64::from(r.width) * 25.4) / 96.0, (f64::from(r.height) * 25.4) / 96.0)
                        }
                    };
                    (dpi_val, format!("image/x-raw-{}", r.format.as_str()), r.pixels, w_mm, h_mm)
                }
                None => {
                    metadata.push(KvEntry::new("resolution", "vector"));
                    (
                        0,
                        "application/pdf".to_string(),
                        Vec::new(),
                        (page.width_cmm as f64) / 100.0,
                        (page.height_cmm as f64) / 100.0,
                    )
                }
            };
            records.push(Record::DrawingPage(DrawingPageRecord {
                name: UntrustedText::from_string(format!("page-{}", page.index + 1)),
                page_index: page.index,
                width_mm,
                height_mm,
                dpi,
                media_type: UntrustedText::from_string(media_type),
                image_bytes,
                metadata,
            }));
        }

        for w in res.warnings {
            records.push(Record::Warning(WarningRecord {
                code: UntrustedText::from_string(w.code),
                detail: UntrustedText::from_string(w.detail),
                byte_offset: w.byte_offset,
            }));
        }

        Ok(records)
    }
}

/// Fuzzing entry point (`REQ-007`, `AC-8`): exercises auto-detect, PDF, PNG, and TIFF paths on
/// arbitrary untrusted bytes without ever panicking or looping without bound.
pub fn fuzz_read_all(data: &[u8]) {
    let _ = read_drawing_bytes(None, data);
    let _ = pdf::read_pdf_structure(data);
    let _ = png::decode_png(data);
    let _ = tiff::decode_tiff(data);
}
