//! Block painters for tables, text, headers/footers, images, views, ballooned drawings and
//! form fields (spec node `RPT.OUTPUT.PDF.DRAW`).
//!
//! - Every numeric value is painted strictly from the resolved `ValueText` display string
//!   (with any nonconformance or tie markers visible next to the value), never formatted,
//!   rounded, or converted (`REQ-RPT.OUTPUT.PDF.DRAW-001`, `REQ-005`, CONSTITUTION C1).
//! - Before painting any page, [`PainterRegistry::check_document`] checks every block of the
//!   resolved report against the registry and fails with `report.unsupported_block` naming the
//!   first unsupported block ID before any output is produced (`REQ-002`, `AC-2`).
//! - Images and view snapshots are read from an [`ImageStore`] by SHA-256 digest and verified
//!   before decoding (`report.snapshot_missing` on absence or corruption, `REQ-003`, `AC-4`).
//!   JPEG bytes pass through as `/DCTDecode`; PNG bytes are decoded with bounded pure-Rust
//!   decoders and re-encoded with deterministic `/FlateDecode`.
//! - `drawing` blocks paint the sheet image and every balloon as a vector circle with its
//!   number centred at the exact stored sheet coordinate converted to page µm (`REQ-004`, `AC-3`).

use crate::image_codec::{decode_image, PdfImageObject};
use crate::layout::{
    BoxContent, CellStyleKind, LaidBalloon, LaidBox, LaidPage, LayoutError, PageLayout, ParsedBlock, RectUm,
    ResolvedDoc, ShapedLine, CELL_PAD_X_UM, CELL_PAD_Y_UM,
};
use crate::text::pdf::{fmt_milli, milli_points, text_object, MAX_CONTENT_BYTES};
use crate::text::{round_half_even, DefaultStyle, FontId, FontSet, ShapedRun, Subset, SubsetBuilder, TextError};
use fid_types::Digest;
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Default line width for table rules and borders (200 µm = 0.2 mm).
const RULE_WIDTH_UM: i64 = 200;
/// Line width for vector balloon circles (300 µm = 0.3 mm).
const BALLOON_STROKE_UM: i64 = 300;
/// Balloon label font size (2200 µm ~ 6.2 pt).
const BALLOON_FONT_UM: i64 = 2_200;
/// View placeholder label font size (3000 µm ~ 8.5 pt).
const PLACEHOLDER_FONT_UM: i64 = 3_000;

// ------------------------------------------------------------------ errors

/// Typed errors of block painting (`Design: Errors`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaintError {
    /// `report.unsupported_block`: a block kind has no registered painter (`REQ-002`, `AC-2`).
    UnsupportedBlock { block_id: String, kind: String },
    /// `report.snapshot_missing`: image or snapshot digest missing from CAS or digest mismatch (`REQ-003`, `AC-4`).
    SnapshotMissing { block_id: String, digest: Digest },
    /// `report.image_invalid`: image bytes are malformed or exceed decoder bounds.
    ImageInvalid { block_id: String, reason: &'static str },
    /// Error from text shaping or subsetting (`RPT.OUTPUT.PDF.TEXT`).
    Text(TextError),
    /// Error from layout (`RPT.OUTPUT.PDF.LAYOUT`).
    Layout(LayoutError),
}

impl PaintError {
    pub fn code(&self) -> &'static str {
        match self {
            PaintError::UnsupportedBlock { .. } => "report.unsupported_block",
            PaintError::SnapshotMissing { .. } => "report.snapshot_missing",
            PaintError::ImageInvalid { .. } => "report.image_invalid",
            PaintError::Text(e) => e.code(),
            PaintError::Layout(e) => e.code(),
        }
    }
}

impl fmt::Display for PaintError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PaintError::UnsupportedBlock { block_id, kind } => {
                write!(f, "report.unsupported_block: block {block_id} (kind '{kind}')")
            }
            PaintError::SnapshotMissing { block_id, digest } => {
                write!(f, "report.snapshot_missing: block {block_id} ({})", digest.as_str())
            }
            PaintError::ImageInvalid { block_id, reason } => {
                write!(f, "report.image_invalid: block {block_id} ({reason})")
            }
            PaintError::Text(e) => e.fmt(f),
            PaintError::Layout(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for PaintError {}

impl From<TextError> for PaintError {
    fn from(e: TextError) -> PaintError {
        PaintError::Text(e)
    }
}

impl From<LayoutError> for PaintError {
    fn from(e: LayoutError) -> PaintError {
        PaintError::Layout(e)
    }
}

// ------------------------------------------------------------------ CAS image store trait

/// Content-addressed read-only store for report images, view snapshots, and drawing sheets
/// (`REQ-RPT.OUTPUT.PDF.DRAW-003`).
pub trait ImageStore {
    /// Returns raw bytes stored under `digest`, or `Ok(None)` when absent.
    fn read_bytes(&self, digest: &Digest) -> Result<Option<Vec<u8>>, PaintError>;
}

/// Deterministic in-memory [`ImageStore`] backed by a [`BTreeMap`].
#[derive(Debug, Default, Clone)]
pub struct MemoryImageStore {
    objects: BTreeMap<Digest, Vec<u8>>,
}

impl MemoryImageStore {
    pub fn new() -> MemoryImageStore {
        MemoryImageStore { objects: BTreeMap::new() }
    }

    /// Inserts `bytes` under its exact SHA-256 digest and returns that digest.
    pub fn insert(&mut self, bytes: Vec<u8>) -> Digest {
        let digest = Digest::from_bytes(Sha256::digest(&bytes).into());
        self.objects.insert(digest, bytes);
        digest
    }

    /// Inserts `bytes` under an arbitrary digest (for corruption / digest-mismatch tests, `AC-4`).
    pub fn insert_under_digest(&mut self, digest: Digest, bytes: Vec<u8>) {
        self.objects.insert(digest, bytes);
    }
}

impl ImageStore for MemoryImageStore {
    fn read_bytes(&self, digest: &Digest) -> Result<Option<Vec<u8>>, PaintError> {
        Ok(self.objects.get(digest).cloned())
    }
}

// ------------------------------------------------------------------ painted output model

/// One painted PDF page ready for `RPT.OUTPUT.PDF.WRITE`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaintedPage {
    pub page_index: u32,
    pub size_um: (i64, i64),
    /// Deterministic PDF content-stream bytes (uncompressed; writer may Flate-compress).
    pub content_stream: Vec<u8>,
    /// Font resource names used on this page, in font-ID order.
    pub font_resources: BTreeSet<FontId>,
    /// Image digests referenced on this page, in digest order.
    pub image_digests: BTreeSet<Digest>,
}

/// Complete painted document with deterministic font subsets and verified image XObjects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaintedDocument {
    pub resolved_digest: Digest,
    pub pages: Vec<PaintedPage>,
    /// Embedded font subsets keyed by [`FontId`], ordered deterministically.
    pub subsets: BTreeMap<FontId, Subset>,
    /// Resource name (`/F1`, `/F2`, ...) for each font ID.
    pub font_names: BTreeMap<FontId, String>,
    /// Verified, deterministic image XObjects keyed by CAS [`Digest`], ordered deterministically.
    pub images: BTreeMap<Digest, PdfImageObject>,
    /// Resource name (`/Im1`, `/Im2`, ...) for each image digest.
    pub image_names: BTreeMap<Digest, String>,
}

// ------------------------------------------------------------------ PaintCtx

/// Mutable PDF content-stream context passed to each [`Painter`].
pub struct PaintCtx<'a> {
    page_height_um: i64,
    content: Vec<u8>,
    fonts: &'a FontSet,
    subsets: &'a BTreeMap<FontId, Subset>,
    font_names: &'a BTreeMap<FontId, String>,
    image_names: &'a BTreeMap<Digest, String>,
    used_fonts: BTreeSet<FontId>,
    used_images: BTreeSet<Digest>,
}

impl<'a> PaintCtx<'a> {
    fn push_bytes(&mut self, data: &[u8]) -> Result<(), PaintError> {
        if self.content.len().saturating_add(data.len()) > MAX_CONTENT_BYTES {
            return Err(PaintError::Text(TextError::Limit { what: "content stream exceeds MAX_CONTENT_BYTES" }));
        }
        self.content.extend_from_slice(data);
        Ok(())
    }

    fn push_str(&mut self, s: &str) -> Result<(), PaintError> {
        self.push_bytes(s.as_bytes())
    }

    /// Converts top-down layout `y_um` into bottom-up PDF `y_um` (`page_height_um - y_um`).
    pub fn pdf_y_um(&self, top_down_y_um: i64) -> i64 {
        self.page_height_um - top_down_y_um
    }

    /// Emits `q` (save graphics state).
    pub fn save_state(&mut self) -> Result<(), PaintError> {
        self.push_str("q\n")
    }

    /// Emits `Q` (restore graphics state).
    pub fn restore_state(&mut self) -> Result<(), PaintError> {
        self.push_str("Q\n")
    }

    /// Clips subsequent operations to `rect` (`x y w h re W n`).
    pub fn clip_rect_um(&mut self, rect: RectUm) -> Result<(), PaintError> {
        let pdf_y = self.pdf_y_um(rect.y_um + rect.height_um);
        let cmd = format!(
            "{} {} {} {} re W n\n",
            fmt_milli(milli_points(rect.x_um)),
            fmt_milli(milli_points(pdf_y)),
            fmt_milli(milli_points(rect.width_um)),
            fmt_milli(milli_points(rect.height_um)),
        );
        self.push_str(&cmd)
    }

    /// Sets non-stroking RGB color using integer thousandths (`0..=1000`, zero floats).
    pub fn set_fill_rgb_milli(&mut self, r: i64, g: i64, b: i64) -> Result<(), PaintError> {
        let cmd = format!("{} {} {} rg\n", fmt_milli(r), fmt_milli(g), fmt_milli(b));
        self.push_str(&cmd)
    }

    /// Sets stroking RGB color using integer thousandths (`0..=1000`, zero floats).
    pub fn set_stroke_rgb_milli(&mut self, r: i64, g: i64, b: i64) -> Result<(), PaintError> {
        let cmd = format!("{} {} {} RG\n", fmt_milli(r), fmt_milli(g), fmt_milli(b));
        self.push_str(&cmd)
    }

    /// Sets line stroke width in micrometres.
    pub fn set_line_width_um(&mut self, width_um: i64) -> Result<(), PaintError> {
        let cmd = format!("{} w\n", fmt_milli(milli_points(width_um)));
        self.push_str(&cmd)
    }

    /// Fills a rectangle in top-down layout µm.
    pub fn fill_rect_um(&mut self, rect: RectUm) -> Result<(), PaintError> {
        let pdf_y = self.pdf_y_um(rect.y_um + rect.height_um);
        let cmd = format!(
            "{} {} {} {} re f\n",
            fmt_milli(milli_points(rect.x_um)),
            fmt_milli(milli_points(pdf_y)),
            fmt_milli(milli_points(rect.width_um)),
            fmt_milli(milli_points(rect.height_um)),
        );
        self.push_str(&cmd)
    }

    /// Strokes a rectangle in top-down layout µm.
    pub fn stroke_rect_um(&mut self, rect: RectUm) -> Result<(), PaintError> {
        let pdf_y = self.pdf_y_um(rect.y_um + rect.height_um);
        let cmd = format!(
            "{} {} {} {} re S\n",
            fmt_milli(milli_points(rect.x_um)),
            fmt_milli(milli_points(pdf_y)),
            fmt_milli(milli_points(rect.width_um)),
            fmt_milli(milli_points(rect.height_um)),
        );
        self.push_str(&cmd)
    }

    /// Draws a straight vector line between two top-down µm points.
    pub fn draw_line_um(&mut self, x1: i64, y1: i64, x2: i64, y2: i64) -> Result<(), PaintError> {
        let py1 = self.pdf_y_um(y1);
        let py2 = self.pdf_y_um(y2);
        let cmd = format!(
            "{} {} m {} {} l S\n",
            fmt_milli(milli_points(x1)),
            fmt_milli(milli_points(py1)),
            fmt_milli(milli_points(x2)),
            fmt_milli(milli_points(py2)),
        );
        self.push_str(&cmd)
    }

    /// Draws an exact vector circle centred at top-down `(cx_um, cy_um)` with radius `r_um`
    /// using 4 symmetric cubic Bézier arcs (`kappa = round(r * 552285 / 1_000_000)`).
    pub fn draw_circle_um(
        &mut self,
        cx_um: i64,
        cy_um: i64,
        r_um: i64,
        fill_and_stroke: bool,
    ) -> Result<(), PaintError> {
        let py = self.pdf_y_um(cy_um);
        let k = round_half_even(i128::from(r_um) * 552_285, 1_000_000) as i64;
        let pt = |v: i64| fmt_milli(milli_points(v));

        // 4 cardinal points: E=(cx+r, py), N=(cx, py+r), W=(cx-r, py), S=(cx, py-r).
        let op = if fill_and_stroke { "B" } else { "S" };
        let cmd = format!(
            "{} {} m\n\
             {} {} {} {} {} {} c\n\
             {} {} {} {} {} {} c\n\
             {} {} {} {} {} {} c\n\
             {} {} {} {} {} {} c\n\
             h {op}\n",
            pt(cx_um + r_um),
            pt(py),
            // E -> N
            pt(cx_um + r_um),
            pt(py + k),
            pt(cx_um + k),
            pt(py + r_um),
            pt(cx_um),
            pt(py + r_um),
            // N -> W
            pt(cx_um - k),
            pt(py + r_um),
            pt(cx_um - r_um),
            pt(py + k),
            pt(cx_um - r_um),
            pt(py),
            // W -> S
            pt(cx_um - r_um),
            pt(py - k),
            pt(cx_um - k),
            pt(py - r_um),
            pt(cx_um),
            pt(py - r_um),
            // S -> E
            pt(cx_um + k),
            pt(py - r_um),
            pt(cx_um + r_um),
            pt(py - k),
            pt(cx_um + r_um),
            pt(py),
        );
        self.push_str(&cmd)
    }

    /// Paints a single [`ShapedRun`] at top-down `(x_um, baseline_y_um)`.
    pub fn draw_run(&mut self, x_um: i64, baseline_y_um: i64, run: &ShapedRun) -> Result<(), PaintError> {
        if run.glyphs.is_empty() {
            return Ok(());
        }
        let subset = self
            .subsets
            .get(&run.font)
            .ok_or_else(|| PaintError::Text(TextError::FontUnknown { font: run.font.as_str().to_string() }))?;
        let res_name = self
            .font_names
            .get(&run.font)
            .ok_or_else(|| PaintError::Text(TextError::FontUnknown { font: run.font.as_str().to_string() }))?;
        self.used_fonts.insert(run.font.clone());
        let pdf_y = self.pdf_y_um(baseline_y_um);
        let obj = text_object(res_name, subset, run, x_um, pdf_y)?;
        self.push_bytes(&obj)
    }

    /// Paints all shaped runs of a [`ShapedLine`] starting at top-down `(x_um, box_top_y_um)`.
    pub fn draw_shaped_line(&mut self, x_um: i64, box_top_y_um: i64, line: &ShapedLine) -> Result<(), PaintError> {
        let mut cursor_x = x_um;
        let baseline_y = box_top_y_um + line.baseline_offset_um;
        for run in &line.runs {
            self.draw_run(cursor_x, baseline_y, run)?;
            cursor_x += run.advance_um();
        }
        Ok(())
    }

    /// Draws an image XObject registered under `digest` into top-down `rect`.
    pub fn draw_image_xobject(&mut self, block_id: &str, digest: &Digest, rect: RectUm) -> Result<(), PaintError> {
        let res = self
            .image_names
            .get(digest)
            .ok_or_else(|| PaintError::SnapshotMissing { block_id: block_id.to_string(), digest: *digest })?
            .clone();
        self.used_images.insert(*digest);
        let pdf_y = self.pdf_y_um(rect.y_um + rect.height_um);
        let cmd = format!(
            "q\n{} 0 0 {} {} {} cm\n/{res} Do\nQ\n",
            fmt_milli(milli_points(rect.width_um)),
            fmt_milli(milli_points(rect.height_um)),
            fmt_milli(milli_points(rect.x_um)),
            fmt_milli(milli_points(pdf_y)),
        );
        self.push_str(&cmd)
    }

    /// Access to the loaded bundled font set (for auxiliary vector labels such as balloon numbers).
    pub fn fonts(&self) -> &'a FontSet {
        self.fonts
    }
}

// ------------------------------------------------------------------ Painter trait & registry

/// Per-block-kind PDF content-stream painter (`Design: trait Painter`).
pub trait Painter {
    /// Block kind identifier handled by this painter (e.g. `"table"`, `"drawing"`, `"chart"`).
    fn kind(&self) -> &str;
    /// Paints `laid` (and its original [`ParsedBlock`], if available) into `ctx`.
    fn paint(&self, laid: &LaidBox, block: Option<&ParsedBlock>, ctx: &mut PaintCtx<'_>) -> Result<(), PaintError>;
}

/// Deterministic registry mapping block kinds to [`Painter`] implementations.
pub struct PainterRegistry<'a> {
    by_kind: BTreeMap<String, &'a dyn Painter>,
}

impl<'a> PainterRegistry<'a> {
    /// Empty registry (useful for testing unsupported-block checks).
    pub fn empty() -> PainterRegistry<'a> {
        PainterRegistry { by_kind: BTreeMap::new() }
    }

    /// Registry populated with all 8 built-in `RPT.OUTPUT.PDF.DRAW` painters:
    /// `table`, `text`, `header`, `footer`, `image`, `view`, `drawing`, `form_field`.
    pub fn default_builtin() -> PainterRegistry<'static> {
        let mut reg = PainterRegistry::empty();
        reg.register(&TABLE_PAINTER);
        reg.register(&TEXT_PAINTER);
        reg.register(&HEADER_PAINTER);
        reg.register(&FOOTER_PAINTER);
        reg.register(&IMAGE_PAINTER);
        reg.register(&VIEW_PAINTER);
        reg.register(&DRAWING_PAINTER);
        reg.register(&FORM_FIELD_PAINTER);
        reg
    }

    /// Registers or overrides the painter for `painter.kind()`.
    pub fn register(&mut self, painter: &'a dyn Painter) {
        self.by_kind.insert(painter.kind().to_string(), painter);
    }

    /// Looks up a painter by kind.
    pub fn get(&self, kind: &str) -> Option<&'a dyn Painter> {
        self.by_kind.get(kind).copied()
    }

    /// Whole-document preflight check (`REQ-002`, `AC-2`): verifies every block in `doc` has a
    /// registered painter, returning `report.unsupported_block` naming the first unsupported
    /// block ID before any layout or painting occurs.
    pub fn check_document(&self, doc: &ResolvedDoc) -> Result<(), PaintError> {
        for page in &doc.pages {
            for block in &page.blocks {
                if !self.by_kind.contains_key(&block.kind) {
                    return Err(PaintError::UnsupportedBlock { block_id: block.id.clone(), kind: block.kind.clone() });
                }
            }
        }
        Ok(())
    }

    /// Checks every laid-out box in `layout` has a registered painter.
    pub fn check_layout(&self, layout: &PageLayout) -> Result<(), PaintError> {
        for page in &layout.pages {
            for b in &page.boxes {
                if !self.by_kind.contains_key(&b.kind) {
                    return Err(PaintError::UnsupportedBlock { block_id: b.block_id.clone(), kind: b.kind.clone() });
                }
            }
        }
        Ok(())
    }
}

// ------------------------------------------------------------------ built-in painters

struct TablePainter;
struct TextPainter(&'static str);
struct ImagePainter;
struct ViewPainter;
struct DrawingPainter;
struct FormFieldPainter;

static TABLE_PAINTER: TablePainter = TablePainter;
static TEXT_PAINTER: TextPainter = TextPainter("text");
static HEADER_PAINTER: TextPainter = TextPainter("header");
static FOOTER_PAINTER: TextPainter = TextPainter("footer");
static IMAGE_PAINTER: ImagePainter = ImagePainter;
static VIEW_PAINTER: ViewPainter = ViewPainter;
static DRAWING_PAINTER: DrawingPainter = DrawingPainter;
static FORM_FIELD_PAINTER: FormFieldPainter = FormFieldPainter;

impl Painter for TablePainter {
    fn kind(&self) -> &str {
        "table"
    }

    fn paint(&self, laid: &LaidBox, _block: Option<&ParsedBlock>, ctx: &mut PaintCtx<'_>) -> Result<(), PaintError> {
        let BoxContent::Table(tbl) = &laid.content else {
            return Ok(());
        };
        ctx.save_state()?;
        ctx.set_line_width_um(RULE_WIDTH_UM)?;

        for (row_idx, row) in tbl.rows.iter().enumerate() {
            let row_y = laid.rect_um.y_um + row.y_offset_um;
            for cell in &row.cells {
                let cell_rect = RectUm::new(laid.rect_um.x_um + cell.x_offset_um, row_y, cell.width_um, row.height_um);

                // P1a fixed default palette (Open questions: RPT.DESIGN.STYLE supplies custom palette later).
                let bg = match cell.style {
                    CellStyleKind::Header => Some((905, 929, 957)),
                    CellStyleKind::OutcomePass => Some((886, 965, 914)),
                    CellStyleKind::OutcomeFail => Some((988, 898, 898)),
                    CellStyleKind::OutcomeWarn => Some((996, 957, 855)),
                    CellStyleKind::Text | CellStyleKind::Numeric => {
                        if !row.is_header && row_idx % 2 == 0 {
                            Some((973, 976, 984))
                        } else {
                            None
                        }
                    }
                };
                if let Some((r, g, b)) = bg {
                    ctx.set_fill_rgb_milli(r, g, b)?;
                    ctx.fill_rect_um(cell_rect)?;
                }

                // Cell border.
                ctx.set_stroke_rgb_milli(780, 810, 850)?;
                ctx.stroke_rect_um(cell_rect)?;

                // Cell text (dark ink, verbatim shaped ValueText + markers, REQ-001, REQ-005).
                let (tr, tg, tb) = match cell.style {
                    CellStyleKind::OutcomeFail => (650, 100, 100),
                    CellStyleKind::OutcomePass => (80, 420, 200),
                    CellStyleKind::OutcomeWarn => (560, 360, 40),
                    _ => (90, 110, 140),
                };
                ctx.set_fill_rgb_milli(tr, tg, tb)?;
                let text_x = cell_rect.x_um + CELL_PAD_X_UM;
                let text_top_y = cell_rect.y_um + CELL_PAD_Y_UM;
                for line in &cell.lines {
                    ctx.draw_shaped_line(text_x, text_top_y, line)?;
                }
            }
        }

        ctx.restore_state()
    }
}

impl Painter for TextPainter {
    fn kind(&self) -> &str {
        self.0
    }

    fn paint(&self, laid: &LaidBox, _block: Option<&ParsedBlock>, ctx: &mut PaintCtx<'_>) -> Result<(), PaintError> {
        let BoxContent::Text(lines) = &laid.content else {
            return Ok(());
        };
        ctx.save_state()?;
        ctx.set_fill_rgb_milli(90, 110, 140)?;
        for line in lines {
            ctx.draw_shaped_line(laid.rect_um.x_um, laid.rect_um.y_um, line)?;
        }
        if self.0 == "header" {
            ctx.set_stroke_rgb_milli(780, 810, 850)?;
            ctx.set_line_width_um(RULE_WIDTH_UM)?;
            let y = laid.rect_um.y_um + laid.rect_um.height_um;
            ctx.draw_line_um(laid.rect_um.x_um, y, laid.rect_um.x_um + laid.rect_um.width_um, y)?;
        } else if self.0 == "footer" {
            ctx.set_stroke_rgb_milli(780, 810, 850)?;
            ctx.set_line_width_um(RULE_WIDTH_UM)?;
            let y = laid.rect_um.y_um;
            ctx.draw_line_um(laid.rect_um.x_um, y, laid.rect_um.x_um + laid.rect_um.width_um, y)?;
        }
        ctx.restore_state()
    }
}

impl Painter for ImagePainter {
    fn kind(&self) -> &str {
        "image"
    }

    fn paint(&self, laid: &LaidBox, _block: Option<&ParsedBlock>, ctx: &mut PaintCtx<'_>) -> Result<(), PaintError> {
        let BoxContent::Image { digest, .. } = &laid.content else {
            return Ok(());
        };
        ctx.draw_image_xobject(&laid.block_id, digest, laid.rect_um)
    }
}

impl Painter for ViewPainter {
    fn kind(&self) -> &str {
        "view"
    }

    fn paint(&self, laid: &LaidBox, _block: Option<&ParsedBlock>, ctx: &mut PaintCtx<'_>) -> Result<(), PaintError> {
        let BoxContent::View { snapshot_digest, .. } = &laid.content else {
            return Ok(());
        };
        ctx.save_state()?;
        if let Some(digest) = snapshot_digest {
            ctx.draw_image_xobject(&laid.block_id, digest, laid.rect_um)?;
            ctx.set_stroke_rgb_milli(700, 740, 800)?;
            ctx.set_line_width_um(RULE_WIDTH_UM)?;
            ctx.stroke_rect_um(laid.rect_um)?;
        } else {
            // Missing view snapshot: paint subtle frame + placeholder notice.
            ctx.set_fill_rgb_milli(960, 965, 975)?;
            ctx.fill_rect_um(laid.rect_um)?;
            ctx.set_stroke_rgb_milli(700, 740, 800)?;
            ctx.set_line_width_um(RULE_WIDTH_UM)?;
            ctx.stroke_rect_um(laid.rect_um)?;
            if let Some(font) = ctx.fonts().default_style(DefaultStyle::Body).cloned() {
                let shaped = ctx.fonts().shape(&font, "View snapshot missing", PLACEHOLDER_FONT_UM)?;
                ctx.set_fill_rgb_milli(400, 440, 500)?;
                let mut cx = laid.rect_um.x_um + 4_000;
                let cy = laid.rect_um.y_um + laid.rect_um.height_um / 2;
                for run in &shaped.runs {
                    ctx.draw_run(cx, cy, run)?;
                    cx += run.advance_um();
                }
            }
        }
        ctx.restore_state()
    }
}

impl Painter for DrawingPainter {
    fn kind(&self) -> &str {
        "drawing"
    }

    fn paint(&self, laid: &LaidBox, _block: Option<&ParsedBlock>, ctx: &mut PaintCtx<'_>) -> Result<(), PaintError> {
        let BoxContent::Drawing { sheet_digest, balloons, .. } = &laid.content else {
            return Ok(());
        };
        ctx.save_state()?;
        // 1. Sheet raster image XObject.
        ctx.draw_image_xobject(&laid.block_id, sheet_digest, laid.rect_um)?;
        ctx.set_stroke_rgb_milli(650, 690, 740)?;
        ctx.set_line_width_um(RULE_WIDTH_UM)?;
        ctx.stroke_rect_um(laid.rect_um)?;

        // 2. Vector balloon overlays at exact stored sheet coordinates (REQ-004, AC-3).
        let font = ctx
            .fonts()
            .default_style(DefaultStyle::Strong)
            .or_else(|| ctx.fonts().default_style(DefaultStyle::Body))
            .cloned();
        ctx.set_line_width_um(BALLOON_STROKE_UM)?;
        for balloon in balloons {
            paint_balloon(laid.rect_um, *balloon, font.as_ref(), ctx)?;
        }
        ctx.restore_state()
    }
}

fn paint_balloon(
    sheet_rect: RectUm,
    balloon: LaidBalloon,
    font: Option<&FontId>,
    ctx: &mut PaintCtx<'_>,
) -> Result<(), PaintError> {
    let cx = sheet_rect.x_um + balloon.cx_um;
    let cy = sheet_rect.y_um + balloon.cy_um;
    ctx.set_fill_rgb_milli(1000, 1000, 1000)?;
    ctx.set_stroke_rgb_milli(820, 120, 120)?;
    ctx.draw_circle_um(cx, cy, balloon.radius_um, true)?;

    if let Some(fid) = font {
        let label = balloon.number.to_string();
        let shaped = ctx.fonts().shape(fid, &label, BALLOON_FONT_UM)?;
        let w = shaped.advance_um();
        let text_x = cx - w / 2;
        let baseline_y = cy + BALLOON_FONT_UM / 3;
        ctx.set_fill_rgb_milli(820, 120, 120)?;
        let mut cursor_x = text_x;
        for run in &shaped.runs {
            ctx.draw_run(cursor_x, baseline_y, run)?;
            cursor_x += run.advance_um();
        }
    }
    Ok(())
}

impl Painter for FormFieldPainter {
    fn kind(&self) -> &str {
        "form_field"
    }

    fn paint(&self, laid: &LaidBox, _block: Option<&ParsedBlock>, ctx: &mut PaintCtx<'_>) -> Result<(), PaintError> {
        let BoxContent::FormField { lines, .. } = &laid.content else {
            return Ok(());
        };
        ctx.save_state()?;
        ctx.set_fill_rgb_milli(985, 988, 992)?;
        ctx.fill_rect_um(laid.rect_um)?;
        ctx.set_stroke_rgb_milli(740, 770, 810)?;
        ctx.set_line_width_um(RULE_WIDTH_UM)?;
        ctx.stroke_rect_um(laid.rect_um)?;

        ctx.set_fill_rgb_milli(90, 110, 140)?;
        let tx = laid.rect_um.x_um + CELL_PAD_X_UM;
        let ty = laid.rect_um.y_um + CELL_PAD_Y_UM;
        for line in lines {
            ctx.draw_shaped_line(tx, ty, line)?;
        }
        ctx.restore_state()
    }
}

// ------------------------------------------------------------------ paint orchestration

/// Paints every page of `layout` using `painters`, checking the entire `doc` against `painters`
/// first (`REQ-002`) and verifying every referenced image/snapshot against `images` (`REQ-003`).
pub fn paint_document(
    doc: &ResolvedDoc,
    layout: &PageLayout,
    fonts: &FontSet,
    images: &dyn ImageStore,
    painters: &PainterRegistry<'_>,
) -> Result<PaintedDocument, PaintError> {
    // REQ-002 / AC-2: check ALL blocks against the painter registry BEFORE doing any work.
    painters.check_document(doc)?;
    painters.check_layout(layout)?;

    let mut block_lookup: BTreeMap<String, &ParsedBlock> = BTreeMap::new();
    for page in &doc.pages {
        for block in &page.blocks {
            block_lookup.insert(block.id.clone(), block);
        }
    }

    // Pass 1: collect all glyphs per font (for deterministic subsetting) and all referenced
    // image digests (for deterministic CAS fetch, SHA-256 verification and resource ordering).
    let mut builders: BTreeMap<FontId, SubsetBuilder<'_>> = BTreeMap::new();
    let mut image_refs: BTreeMap<Digest, String> = BTreeMap::new();

    for page in &layout.pages {
        collect_page_resources(page, fonts, &mut builders, &mut image_refs)?;
    }

    let mut subsets: BTreeMap<FontId, Subset> = BTreeMap::new();
    let mut font_names: BTreeMap<FontId, String> = BTreeMap::new();
    for (idx, (fid, builder)) in builders.into_iter().enumerate() {
        subsets.insert(fid.clone(), builder.finish()?);
        font_names.insert(fid, format!("F{}", idx + 1));
    }

    let mut decoded_images: BTreeMap<Digest, PdfImageObject> = BTreeMap::new();
    let mut image_names: BTreeMap<Digest, String> = BTreeMap::new();
    for (idx, (digest, block_id)) in image_refs.into_iter().enumerate() {
        let Some(raw) = images.read_bytes(&digest)? else {
            return Err(PaintError::SnapshotMissing { block_id, digest });
        };
        let actual = Digest::from_bytes(Sha256::digest(&raw).into());
        if actual != digest {
            return Err(PaintError::SnapshotMissing { block_id, digest });
        }
        let img = decode_image(&raw).map_err(|reason| PaintError::ImageInvalid { block_id, reason })?;
        decoded_images.insert(digest, img);
        image_names.insert(digest, format!("Im{}", idx + 1));
    }

    // Pass 2: paint every page in document order.
    let mut painted_pages = Vec::with_capacity(layout.pages.len());
    for page in &layout.pages {
        let mut ctx = PaintCtx {
            page_height_um: page.size_um.1,
            content: Vec::new(),
            fonts,
            subsets: &subsets,
            font_names: &font_names,
            image_names: &image_names,
            used_fonts: BTreeSet::new(),
            used_images: BTreeSet::new(),
        };

        for laid in &page.boxes {
            let painter = painters.get(&laid.kind).ok_or_else(|| PaintError::UnsupportedBlock {
                block_id: laid.block_id.clone(),
                kind: laid.kind.clone(),
            })?;
            let orig = block_lookup.get(&laid.block_id).copied();
            if laid.clipped {
                ctx.save_state()?;
                ctx.clip_rect_um(laid.rect_um)?;
            }
            painter.paint(laid, orig, &mut ctx)?;
            if laid.clipped {
                ctx.restore_state()?;
            }
        }

        painted_pages.push(PaintedPage {
            page_index: page.page_index,
            size_um: page.size_um,
            content_stream: ctx.content,
            font_resources: ctx.used_fonts,
            image_digests: ctx.used_images,
        });
    }

    Ok(PaintedDocument {
        resolved_digest: doc.digest,
        pages: painted_pages,
        subsets,
        font_names,
        images: decoded_images,
        image_names,
    })
}

fn collect_page_resources<'a>(
    page: &LaidPage,
    fonts: &'a FontSet,
    builders: &mut BTreeMap<FontId, SubsetBuilder<'a>>,
    image_refs: &mut BTreeMap<Digest, String>,
) -> Result<(), PaintError> {
    for b in &page.boxes {
        match &b.content {
            BoxContent::Text(lines) | BoxContent::FormField { lines, .. } => {
                for line in lines {
                    add_line_to_builders(line, fonts, builders)?;
                }
            }
            BoxContent::Table(t) => {
                for row in &t.rows {
                    for cell in &row.cells {
                        for line in &cell.lines {
                            add_line_to_builders(line, fonts, builders)?;
                        }
                    }
                }
            }
            BoxContent::Image { digest, .. } => {
                image_refs.entry(*digest).or_insert_with(|| b.block_id.clone());
            }
            BoxContent::View { snapshot_digest, .. } => {
                if let Some(d) = snapshot_digest {
                    image_refs.entry(*d).or_insert_with(|| b.block_id.clone());
                } else if let Some(fid) = fonts.default_style(DefaultStyle::Body) {
                    let shaped = fonts.shape(fid, "View snapshot missing", PLACEHOLDER_FONT_UM)?;
                    for run in &shaped.runs {
                        add_run_to_builders(run, fonts, builders)?;
                    }
                }
            }
            BoxContent::Drawing { sheet_digest, balloons, .. } => {
                image_refs.entry(*sheet_digest).or_insert_with(|| b.block_id.clone());
                if let Some(fid) =
                    fonts.default_style(DefaultStyle::Strong).or_else(|| fonts.default_style(DefaultStyle::Body))
                {
                    for balloon in balloons {
                        let shaped = fonts.shape(fid, &balloon.number.to_string(), BALLOON_FONT_UM)?;
                        for run in &shaped.runs {
                            add_run_to_builders(run, fonts, builders)?;
                        }
                    }
                }
            }
            BoxContent::Paint { .. } => {}
        }
    }
    Ok(())
}

fn add_line_to_builders<'a>(
    line: &ShapedLine,
    fonts: &'a FontSet,
    builders: &mut BTreeMap<FontId, SubsetBuilder<'a>>,
) -> Result<(), PaintError> {
    for run in &line.runs {
        add_run_to_builders(run, fonts, builders)?;
    }
    Ok(())
}

fn add_run_to_builders<'a>(
    run: &ShapedRun,
    fonts: &'a FontSet,
    builders: &mut BTreeMap<FontId, SubsetBuilder<'a>>,
) -> Result<(), PaintError> {
    if !builders.contains_key(&run.font) {
        let sb = SubsetBuilder::new(fonts, &run.font)?;
        builders.insert(run.font.clone(), sb);
    }
    if let Some(sb) = builders.get_mut(&run.font) {
        sb.add_run(run)?;
    }
    Ok(())
}

/// Extracts every painted text run string across all pages of `doc` by decoding the hex glyph
/// strings in each page's PDF content stream through the embedded `ToUnicode` CMap.
pub fn extract_painted_strings(doc: &PaintedDocument) -> Vec<String> {
    let mut name_to_cmap: BTreeMap<String, BTreeMap<u16, String>> = BTreeMap::new();
    for (fid, res_name) in &doc.font_names {
        if let Some(sub) = doc.subsets.get(fid) {
            name_to_cmap.insert(res_name.clone(), parse_tounicode_cmap(sub.to_unicode.bytes()));
        }
    }

    let mut out = Vec::new();
    for page in &doc.pages {
        let text = String::from_utf8_lossy(&page.content_stream);
        let mut cur_font: Option<String> = None;
        for line in text.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix('/') {
                if trimmed.ends_with(" Tf") {
                    if let Some((name, _)) = rest.split_once(' ') {
                        cur_font = Some(name.to_string());
                    }
                }
            } else if let Some(inner) = trimmed.strip_prefix('<').and_then(|s| s.strip_suffix("> Tj")) {
                if let Some(cmap) = cur_font.as_ref().and_then(|f| name_to_cmap.get(f)) {
                    let mut decoded = String::new();
                    let bytes = inner.as_bytes();
                    for chunk in bytes.chunks_exact(4) {
                        if let Ok(hex) = std::str::from_utf8(chunk) {
                            if let Ok(gid) = u16::from_str_radix(hex, 16) {
                                if let Some(ch) = cmap.get(&gid) {
                                    decoded.push_str(ch);
                                }
                            }
                        }
                    }
                    out.push(decoded);
                }
            }
        }
    }
    out
}

fn parse_tounicode_cmap(bytes: &[u8]) -> BTreeMap<u16, String> {
    let mut map = BTreeMap::new();
    let s = String::from_utf8_lossy(bytes);
    let mut in_bfchar = false;
    let mut in_bfrange = false;
    for line in s.lines() {
        let t = line.trim();
        if t.ends_with("beginbfchar") {
            in_bfchar = true;
            continue;
        }
        if t == "endbfchar" {
            in_bfchar = false;
            continue;
        }
        if t.ends_with("beginbfrange") {
            in_bfrange = true;
            continue;
        }
        if t == "endbfrange" {
            in_bfrange = false;
            continue;
        }
        let hex_tokens: Vec<&str> =
            t.split('<').skip(1).filter_map(|part| part.split_once('>').map(|(h, _)| h)).collect();
        if in_bfchar && hex_tokens.len() >= 2 {
            if let (Ok(code), Some(uni)) = (u16::from_str_radix(hex_tokens[0], 16), decode_utf16be_hex(hex_tokens[1])) {
                map.insert(code, uni);
            }
        } else if in_bfrange && hex_tokens.len() >= 3 {
            if let (Ok(lo), Ok(hi), Ok(base)) = (
                u16::from_str_radix(hex_tokens[0], 16),
                u16::from_str_radix(hex_tokens[1], 16),
                u32::from_str_radix(hex_tokens[2], 16),
            ) {
                for k in lo..=hi {
                    if let Some(ch) = char::from_u32(base + u32::from(k - lo)) {
                        map.insert(k, ch.to_string());
                    }
                }
            }
        }
    }
    map
}

fn decode_utf16be_hex(hex: &str) -> Option<String> {
    if hex.len() % 4 != 0 {
        return None;
    }
    let mut units = Vec::with_capacity(hex.len() / 4);
    for chunk in hex.as_bytes().chunks_exact(4) {
        let s = std::str::from_utf8(chunk).ok()?;
        units.push(u16::from_str_radix(s, 16).ok()?);
    }
    String::from_utf16(&units).ok()
}
