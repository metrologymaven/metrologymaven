//! Bounded pure-integer PDF content-stream extractor for `RPT.BALLOON.AUTO.TEXT`.
//!
//! Extracts visible vector text runs (`BT`/`ET`, `Tf`, `Tm`, `Td`, `TD`, `T*`, `TL`, `Tc`, `Tw`,
//! `Tz`, `Tr`, `Ts`, `Tj`, `TJ`, `'`, `"`, `q`/`Q`/`cm`, `ToUnicode` CMap) and vector callout
//! frame boxes (`m`, `l`, `re`, `h`, `S`, `s`, `f`, `F`, `B`, `b`, `n`) into top-left origin
//! integer `0.01 mm` `SheetRect` bounds.
//!
//! - Zero floating-point types or operations (RPT float rule, `i128` fixed-point micro-units).
//! - Zero `HashMap` (`BTreeMap`/`BTreeSet` only), zero `unwrap`/`expect`/`panic`.
//! - Never executes PDF actions, JavaScript, forms, or external references (`REQ-002`).
//! - Hard caps: 50,000 runs, 10 MiB text, 500,000 operators, 64 gstate depth, 16 MiB streams (`AC-3`).

use super::flate::{decompress_zlib, MAX_DECODED_STREAM_BYTES};
use super::model::{
    AutoTextError, DrawingTextMap, PageKind, SheetRect, TextRun, MAX_TEXT_BYTES_PER_SHEET, MAX_TEXT_RUNS_PER_SHEET,
};
use super::tile::SheetSegment;
use crate::balloon::sheet::SheetId;
use crate::doc::text::Untrusted;
use std::collections::{BTreeMap, BTreeSet};

const MICRO: i128 = 1_000_000;
const MAX_XREF_OBJECTS: usize = 200_000;
const MAX_NESTING: usize = 64;
const MAX_OPERATORS: usize = 500_000;
const MAX_GSTATE_DEPTH: usize = 64;
const MAX_FRAME_BOXES: usize = 50_000;
const MAX_SEGMENTS: usize = 100_000;

/// Full extraction result including vector path segments for display-tile rendering (`REQ-005`).
#[derive(Debug, Clone)]
pub struct ExtractedPage {
    pub map: DrawingTextMap,
    pub segments: Vec<SheetSegment>,
    pub sheet_bounds_cmm: (i64, i64),
}

/// Pure-integer fixed-point number scaled by `1_000_000`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Fix(i128);

impl Fix {
    const ZERO: Fix = Fix(0);
    const ONE: Fix = Fix(MICRO);

    fn from_int(v: i64) -> Fix {
        Fix(i128::from(v).saturating_mul(MICRO))
    }

    fn mul(self, rhs: Fix) -> Fix {
        Fix(self.0.saturating_mul(rhs.0) / MICRO)
    }

    fn add(self, rhs: Fix) -> Fix {
        Fix(self.0.saturating_add(rhs.0))
    }

    fn sub(self, rhs: Fix) -> Fix {
        Fix(self.0.saturating_sub(rhs.0))
    }

    fn abs(self) -> Fix {
        Fix(self.0.saturating_abs())
    }
}

/// 2D affine matrix `[a, b, c, d, e, f]` matching PDF row-vector convention:
/// `x' = a*x + c*y + e`, `y' = b*x + d*y + f`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Mat([Fix; 6]);

impl Mat {
    const ID: Mat = Mat([Fix::ONE, Fix::ZERO, Fix::ZERO, Fix::ONE, Fix::ZERO, Fix::ZERO]);

    fn concat(self, rhs: Mat) -> Mat {
        // Row-vector multiplication: self * rhs
        let [a1, b1, c1, d1, e1, f1] = self.0;
        let [a2, b2, c2, d2, e2, f2] = rhs.0;
        Mat([
            a1.mul(a2).add(b1.mul(c2)),
            a1.mul(b2).add(b1.mul(d2)),
            c1.mul(a2).add(d1.mul(c2)),
            c1.mul(b2).add(d1.mul(d2)),
            e1.mul(a2).add(f1.mul(c2)).add(e2),
            e1.mul(b2).add(f1.mul(d2)).add(f2),
        ])
    }

    fn apply(self, x: Fix, y: Fix) -> (Fix, Fix) {
        let [a, b, c, d, e, f] = self.0;
        (a.mul(x).add(c.mul(y)).add(e), b.mul(x).add(d.mul(y)).add(f))
    }
}

/// Extracts visible text runs, frame boxes, page classification and vector segments from page
/// `page_index` (0-based) of raw PDF `bytes`.
pub fn extract_pdf_page(
    sheet: SheetId,
    bytes: &[u8],
    page_index: u32,
    include_frame_boxes: bool,
) -> Result<ExtractedPage, AutoTextError> {
    let doc = PdfDoc::parse(bytes)?;
    let mut warnings = Vec::new();
    let catalog_ref = doc
        .trailer_root
        .ok_or_else(|| AutoTextError::PdfMalformed { at: 0, detail: "missing /Root in PDF trailer".into() })?;
    let (catalog_val, _) = doc.resolve_obj(catalog_ref, 0)?;
    let catalog = catalog_val
        .as_dict()
        .ok_or_else(|| AutoTextError::PdfMalformed { at: 0, detail: "/Root is not a dictionary".into() })?;

    if catalog.contains_key("OpenAction") || catalog.contains_key("AA") {
        warnings.push("pdf_action_ignored".to_string());
    }
    if catalog.contains_key("AcroForm") {
        warnings.push("pdf_forms_ignored".to_string());
    }
    if let Some(names_val) = catalog.get("Names") {
        let names = doc.resolve_val(names_val, 0)?;
        if let Some(nd) = names.as_dict() {
            if nd.contains_key("JavaScript") {
                warnings.push("pdf_javascript_ignored".to_string());
            }
            if nd.contains_key("EmbeddedFiles") {
                warnings.push("pdf_embedded_files_ignored".to_string());
            }
        }
    }

    let pages_ref = catalog
        .get("Pages")
        .and_then(PdfVal::as_ref)
        .ok_or_else(|| AutoTextError::PdfMalformed { at: 0, detail: "missing /Pages in /Catalog".into() })?;

    let pages = collect_pages(&doc, pages_ref)?;
    let page = pages.get(page_index as usize).ok_or(AutoTextError::NotFound("page_index"))?;

    // Determine page geometry (CropBox else MediaBox), Rotate, and UserUnit.
    let box_rect = page
        .crop_box
        .or(page.media_box)
        .ok_or_else(|| AutoTextError::PdfMalformed { at: 0, detail: "page has neither CropBox nor MediaBox".into() })?;
    let user_unit = page.user_unit.unwrap_or(Fix::ONE);
    let rotation = page.rotation;
    let mapper = SheetMapper::new(box_rect, rotation, user_unit)?;

    // Resolve resources: /Font (ToUnicode CMaps) and /XObject (Image presence).
    let mut font_cmaps: BTreeMap<String, FontCMap> = BTreeMap::new();
    let mut image_xobjects: BTreeSet<String> = BTreeSet::new();
    if let Some(res_dict) = &page.resources {
        if let Some(fonts_val) = res_dict.get("Font") {
            let fonts_resolved = doc.resolve_val(fonts_val, 0)?;
            if let Some(fdict) = fonts_resolved.as_dict() {
                for (fname, fval) in fdict {
                    let fobj = doc.resolve_val(fval, 0)?;
                    if let Some(fd) = fobj.as_dict() {
                        if let Some(tu) = fd.get("ToUnicode").and_then(PdfVal::as_ref) {
                            if let Ok(cmap_bytes) = doc.decode_stream(tu, 0) {
                                font_cmaps.insert(fname.clone(), FontCMap::parse(&cmap_bytes));
                            }
                        }
                    }
                }
            }
        }
        if let Some(xobj_val) = res_dict.get("XObject") {
            let xobj_resolved = doc.resolve_val(xobj_val, 0)?;
            if let Some(xdict) = xobj_resolved.as_dict() {
                for (xname, xval) in xdict {
                    let xobj = doc.resolve_val(xval, 0)?;
                    if let Some(xd) = xobj.as_dict() {
                        if xd.get("Subtype").and_then(PdfVal::as_name) == Some("Image") {
                            image_xobjects.insert(xname.clone());
                        }
                    }
                }
            }
        }
    }

    // Collect page content streams (single ref or array of refs).
    let mut combined_stream = Vec::new();
    if let Some(contents_val) = &page.contents {
        let mut stream_refs = Vec::new();
        match contents_val {
            PdfVal::Ref(num, _) => stream_refs.push(*num),
            PdfVal::Arr(arr) => {
                for item in arr {
                    if let Some(r) = item.as_ref() {
                        stream_refs.push(r);
                    }
                }
            }
            _ => {}
        }
        for sr in stream_refs {
            let decoded = doc.decode_stream(sr, 0)?;
            if combined_stream.len().saturating_add(decoded.len()) > MAX_DECODED_STREAM_BYTES {
                return Err(AutoTextError::LimitExceeded {
                    limit: "stream_bytes",
                    detail: "combined page content streams exceed 16 MiB".into(),
                });
            }
            combined_stream.extend_from_slice(&decoded);
            combined_stream.push(b'\n');
        }
    }

    let mut interp = StreamInterpreter::new(&mapper, &font_cmaps, &image_xobjects, include_frame_boxes);
    interp.run(&combined_stream)?;

    // Sort runs deterministically by (y0, x0, original_order) and assign 1-based sequential IDs.
    let mut raw_runs = interp.runs;
    raw_runs.sort_by(|a, b| {
        a.bbox.y0.cmp(&b.bbox.y0).then_with(|| a.bbox.x0.cmp(&b.bbox.x0)).then_with(|| a.order.cmp(&b.order))
    });
    let runs: Vec<TextRun> = raw_runs
        .into_iter()
        .enumerate()
        .map(|(i, r)| TextRun {
            id: (i as u32) + 1,
            text: r.text,
            bbox: r.bbox,
            font_h_cmm: r.font_h_cmm,
            angle_mdeg: r.angle_mdeg,
        })
        .collect();

    let mut frame_boxes = interp.frame_boxes;
    frame_boxes.sort();
    frame_boxes.dedup();

    let has_text = !runs.is_empty();
    let has_raster = interp.saw_image_do || (!has_text && !image_xobjects.is_empty());
    let page_kind = match (has_text, has_raster) {
        (true, false) => PageKind::Vector,
        (true, true) => PageKind::Mixed,
        (false, true) => PageKind::RasterOnly,
        (false, false) => PageKind::Vector,
    };

    Ok(ExtractedPage {
        map: DrawingTextMap { sheet, page_kind, runs, frame_boxes, warnings },
        segments: interp.segments,
        sheet_bounds_cmm: (mapper.sheet_w_cmm, mapper.sheet_h_cmm),
    })
}

// ------------------------------------------------------------------ coordinate mapping (integer 0.01 mm)

struct SheetMapper {
    x0: Fix,
    y0: Fix,
    w_pt: Fix,
    h_pt: Fix,
    rotation: u16,
    user_unit: Fix,
    sheet_w_cmm: i64,
    sheet_h_cmm: i64,
}

impl SheetMapper {
    fn new(box_rect: [Fix; 4], rotation: u16, user_unit: Fix) -> Result<Self, AutoTextError> {
        let x0 = Fix(box_rect[0].0.min(box_rect[2].0));
        let x1 = Fix(box_rect[0].0.max(box_rect[2].0));
        let y0 = Fix(box_rect[1].0.min(box_rect[3].0));
        let y1 = Fix(box_rect[1].0.max(box_rect[3].0));
        let w_pt = x1.sub(x0);
        let h_pt = y1.sub(y0);
        if w_pt.0 <= 0 || h_pt.0 <= 0 || user_unit.0 <= 0 {
            return Err(AutoTextError::PdfMalformed { at: 0, detail: "non-positive page box".into() });
        }
        let rot = rotation % 360;
        let (eff_w, eff_h) = if rot == 90 || rot == 270 { (h_pt, w_pt) } else { (w_pt, h_pt) };
        let sheet_w_cmm = pt_to_cmm(eff_w.mul(user_unit));
        let sheet_h_cmm = pt_to_cmm(eff_h.mul(user_unit));
        Ok(SheetMapper { x0, y0, w_pt, h_pt, rotation: rot, user_unit, sheet_w_cmm, sheet_h_cmm })
    }

    /// Maps a PDF user-space point `(px, py)` (bottom-left origin, PDF points) to top-left origin
    /// integer `0.01 mm` sheet coordinates with single half-even rounding.
    fn point_to_sheet_cmm(&self, px: Fix, py: Fix) -> (i64, i64) {
        let rx = px.sub(self.x0);
        let ry = py.sub(self.y0);
        // Apply /Rotate around the page box, then invert Y to top-left origin.
        let (sx_pt, sy_top_pt) = match self.rotation {
            90 => (self.h_pt.sub(ry), self.w_pt.sub(rx)),
            180 => (self.w_pt.sub(rx), ry),
            270 => (ry, rx),
            _ => (rx, self.h_pt.sub(ry)),
        };
        (pt_to_cmm(sx_pt.mul(self.user_unit)), pt_to_cmm(sy_top_pt.mul(self.user_unit)))
    }

    fn length_to_cmm(&self, len_pt: Fix) -> i64 {
        pt_to_cmm(len_pt.abs().mul(self.user_unit))
    }
}

/// Converts PDF points (scaled by `MICRO`) to integer `0.01 mm`: `pt * 2540 / 72`, half-even.
fn pt_to_cmm(pt: Fix) -> i64 {
    let neg = pt.0 < 0;
    let mag = pt.0.unsigned_abs();
    let num = mag.saturating_mul(2540);
    let den = (MICRO as u128) * 72;
    let q = num / den;
    let r = num % den;
    let rounded = match (2 * r).cmp(&den) {
        std::cmp::Ordering::Greater => q + 1,
        std::cmp::Ordering::Less => q,
        std::cmp::Ordering::Equal => q + (q & 1),
    };
    let val = rounded.min(i64::MAX as u128) as i64;
    if neg {
        -val
    } else {
        val
    }
}

/// Integer Euclidean norm `sqrt(x^2 + y^2)` in `Fix` micro-units via exact Newton iteration.
fn fix_hypot(x: Fix, y: Fix) -> Fix {
    let a = x.0.unsigned_abs();
    let b = y.0.unsigned_abs();
    let sum = a.saturating_mul(a).saturating_add(b.saturating_mul(b));
    Fix(isqrt_u128(sum) as i128)
}

fn isqrt_u128(n: u128) -> u128 {
    if n == 0 {
        return 0;
    }
    let mut x = 1u128 << ((128 - n.leading_zeros()).div_ceil(2));
    loop {
        let next = (x + n / x) / 2;
        if next >= x {
            return x;
        }
        x = next;
    }
}

/// Pure-integer counter-clockwise angle in millidegrees (`0..360_000`) from vector `(dx, dy)` in
/// PDF user space (Y-up), adjusted for page `/Rotate`.
fn angle_mdeg_from_vec(dx: Fix, dy: Fix, page_rotation: u16) -> i32 {
    if dx.0 == 0 && dy.0 == 0 {
        return 0;
    }
    let raw = if dy.0 == 0 {
        if dx.0 >= 0 {
            0
        } else {
            180_000
        }
    } else if dx.0 == 0 {
        if dy.0 >= 0 {
            90_000
        } else {
            270_000
        }
    } else {
        // First-octant integer atan approximation: atan(t) ≈ (45° * t) + 15.65° * t * (1 - t)
        // Accurate to ~0.2° across all angles and exact at 0°/45°/90°, zero floats.
        let ax = dx.0.unsigned_abs();
        let ay = dy.0.unsigned_abs();
        let oct = if ay <= ax {
            let t = (ay.saturating_mul(1_000_000)) / ax; // 0..=1_000_000
            let lin = (45_000u128 * t) / 1_000_000;
            let corr = (15_650u128 * t / 1_000_000) * (1_000_000 - t) / 1_000_000;
            (lin + corr) as i32
        } else {
            let t = (ax.saturating_mul(1_000_000)) / ay;
            let lin = (45_000u128 * t) / 1_000_000;
            let corr = (15_650u128 * t / 1_000_000) * (1_000_000 - t) / 1_000_000;
            90_000 - ((lin + corr) as i32)
        };
        match (dx.0 >= 0, dy.0 >= 0) {
            (true, true) => oct,
            (false, true) => 180_000 - oct,
            (false, false) => 180_000 + oct,
            (true, false) => 360_000 - oct,
        }
    };
    let rot_adj = i32::from(page_rotation) * 1000;
    (raw - rot_adj).rem_euclid(360_000)
}

// ------------------------------------------------------------------ ToUnicode CMap

#[derive(Debug, Clone, Default)]
struct FontCMap {
    map: BTreeMap<u32, String>,
}

impl FontCMap {
    fn parse(bytes: &[u8]) -> Self {
        let mut map = BTreeMap::new();
        let text = String::from_utf8_lossy(bytes);
        let tokens: Vec<&str> = text.split_whitespace().collect();
        let mut i = 0usize;
        while i < tokens.len() {
            if tokens[i] == "beginbfchar" {
                i += 1;
                while i + 1 < tokens.len() && tokens[i] != "endbfchar" {
                    if let (Some(src), Some(dst)) = (parse_hex_u32(tokens[i]), parse_hex_utf16(tokens[i + 1])) {
                        map.insert(src, dst);
                    }
                    i += 2;
                }
            } else if tokens[i] == "beginbfrange" {
                i += 1;
                while i + 2 < tokens.len() && tokens[i] != "endbfrange" {
                    if let (Some(lo), Some(hi)) = (parse_hex_u32(tokens[i]), parse_hex_u32(tokens[i + 1])) {
                        let span = hi.saturating_sub(lo).min(4096);
                        if let Some(dst_start) = parse_hex_u32(tokens[i + 2]) {
                            for off in 0..=span {
                                if let Some(ch) = char::from_u32(dst_start.saturating_add(off)) {
                                    map.insert(lo + off, ch.to_string());
                                }
                            }
                        }
                    }
                    i += 3;
                }
            } else {
                i += 1;
            }
        }
        FontCMap { map }
    }

    fn decode(&self, raw: &[u8]) -> String {
        let mut out = String::with_capacity(raw.len());
        for &b in raw {
            if let Some(s) = self.map.get(&u32::from(b)) {
                out.push_str(s);
            } else {
                out.push(win_ansi_char(b));
            }
        }
        out
    }
}

fn parse_hex_u32(tok: &str) -> Option<u32> {
    let inner = tok.strip_prefix('<')?.strip_suffix('>')?;
    u32::from_str_radix(inner, 16).ok()
}

fn parse_hex_utf16(tok: &str) -> Option<String> {
    let inner = tok.strip_prefix('<')?.strip_suffix('>')?;
    if inner.is_empty() || inner.len() % 2 != 0 {
        return None;
    }
    let mut bytes = Vec::with_capacity(inner.len() / 2);
    for chunk in inner.as_bytes().chunks_exact(2) {
        let hi = hex_nibble(chunk[0])?;
        let lo = hex_nibble(chunk[1])?;
        bytes.push((hi << 4) | lo);
    }
    if bytes.len() % 2 == 0 {
        let units: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
        Some(String::from_utf16_lossy(&units))
    } else {
        Some(bytes.into_iter().map(char::from).collect())
    }
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn win_ansi_char(b: u8) -> char {
    match b {
        0x20..=0x7E | 0xA0..=0xFF => char::from(b),
        0x80 => '\u{20AC}',
        0x85 => '\u{2026}',
        0x91 => '\u{2018}',
        0x92 => '\u{2019}',
        0x93 => '\u{201C}',
        0x94 => '\u{201D}',
        0x95 => '\u{2022}',
        0x96 => '\u{2013}',
        0x97 => '\u{2014}',
        0x99 => '\u{2122}',
        _ => ' ',
    }
}

// ------------------------------------------------------------------ content stream interpreter

#[derive(Debug, Clone)]
struct RawRun {
    order: u32,
    text: Untrusted<crate::doc::text::Text>,
    bbox: SheetRect,
    font_h_cmm: i64,
    angle_mdeg: i32,
}

#[derive(Debug, Clone)]
struct GState {
    ctm: Mat,
    font: String,
    font_size: Fix,
    leading: Fix,
    char_space: Fix,
    word_space: Fix,
    h_scale: Fix,
    rise: Fix,
    render_mode: i64,
}

impl Default for GState {
    fn default() -> Self {
        GState {
            ctm: Mat::ID,
            font: String::new(),
            font_size: Fix::from_int(12),
            leading: Fix::ZERO,
            char_space: Fix::ZERO,
            word_space: Fix::ZERO,
            h_scale: Fix::from_int(100),
            rise: Fix::ZERO,
            render_mode: 0,
        }
    }
}

#[derive(Debug, Clone)]
enum CsTok {
    Fixed(Fix),
    Name(String),
    Str(Vec<u8>),
    Arr(Vec<CsTok>),
    Op(String),
}

struct StreamInterpreter<'a> {
    mapper: &'a SheetMapper,
    cmaps: &'a BTreeMap<String, FontCMap>,
    image_xobjects: &'a BTreeSet<String>,
    include_frame_boxes: bool,
    gstack: Vec<GState>,
    gs: GState,
    in_text: bool,
    tm: Mat,
    tlm: Mat,
    path_pts: Vec<(Fix, Fix)>,
    path_cur: Option<(Fix, Fix)>,
    path_start: Option<(Fix, Fix)>,
    runs: Vec<RawRun>,
    total_text_bytes: usize,
    frame_boxes: Vec<SheetRect>,
    segments: Vec<SheetSegment>,
    saw_image_do: bool,
    op_count: usize,
}

impl<'a> StreamInterpreter<'a> {
    fn new(
        mapper: &'a SheetMapper,
        cmaps: &'a BTreeMap<String, FontCMap>,
        image_xobjects: &'a BTreeSet<String>,
        include_frame_boxes: bool,
    ) -> Self {
        StreamInterpreter {
            mapper,
            cmaps,
            image_xobjects,
            include_frame_boxes,
            gstack: Vec::new(),
            gs: GState::default(),
            in_text: false,
            tm: Mat::ID,
            tlm: Mat::ID,
            path_pts: Vec::new(),
            path_cur: None,
            path_start: None,
            runs: Vec::new(),
            total_text_bytes: 0,
            frame_boxes: Vec::new(),
            segments: Vec::new(),
            saw_image_do: false,
            op_count: 0,
        }
    }

    fn run(&mut self, stream: &[u8]) -> Result<(), AutoTextError> {
        let mut pos = 0usize;
        let mut operands: Vec<CsTok> = Vec::new();
        while let Some(tok) = next_cs_tok(stream, &mut pos, 0)? {
            match tok {
                CsTok::Op(op) => {
                    self.op_count += 1;
                    if self.op_count > MAX_OPERATORS {
                        return Err(AutoTextError::LimitExceeded {
                            limit: "operators",
                            detail: format!("content stream exceeds {MAX_OPERATORS} operators"),
                        });
                    }
                    self.exec_op(&op, &operands)?;
                    operands.clear();
                }
                other => {
                    if operands.len() < 4096 {
                        operands.push(other);
                    }
                }
            }
        }
        Ok(())
    }

    fn exec_op(&mut self, op: &str, ops: &[CsTok]) -> Result<(), AutoTextError> {
        match op {
            "q" => {
                if self.gstack.len() >= MAX_GSTATE_DEPTH {
                    return Err(AutoTextError::LimitExceeded {
                        limit: "gstate_depth",
                        detail: "graphics state stack depth > 64".into(),
                    });
                }
                self.gstack.push(self.gs.clone());
            }
            "Q" => {
                if let Some(prev) = self.gstack.pop() {
                    self.gs = prev;
                }
            }
            "cm" => {
                if ops.len() >= 6 {
                    let m = Mat([
                        as_fix(&ops[ops.len() - 6]),
                        as_fix(&ops[ops.len() - 5]),
                        as_fix(&ops[ops.len() - 4]),
                        as_fix(&ops[ops.len() - 3]),
                        as_fix(&ops[ops.len() - 2]),
                        as_fix(&ops[ops.len() - 1]),
                    ]);
                    self.gs.ctm = m.concat(self.gs.ctm);
                }
            }
            "BT" => {
                self.in_text = true;
                self.tm = Mat::ID;
                self.tlm = Mat::ID;
            }
            "ET" => {
                self.in_text = false;
            }
            "Tf" => {
                if ops.len() >= 2 {
                    if let CsTok::Name(n) = &ops[ops.len() - 2] {
                        self.gs.font = n.clone();
                    }
                    self.gs.font_size = as_fix(&ops[ops.len() - 1]);
                }
            }
            "TL" => {
                if let Some(v) = ops.last() {
                    self.gs.leading = as_fix(v);
                }
            }
            "Tc" => {
                if let Some(v) = ops.last() {
                    self.gs.char_space = as_fix(v);
                }
            }
            "Tw" => {
                if let Some(v) = ops.last() {
                    self.gs.word_space = as_fix(v);
                }
            }
            "Tz" => {
                if let Some(v) = ops.last() {
                    self.gs.h_scale = as_fix(v);
                }
            }
            "Tr" => {
                if let Some(v) = ops.last() {
                    self.gs.render_mode = (as_fix(v).0 / MICRO) as i64;
                }
            }
            "Ts" => {
                if let Some(v) = ops.last() {
                    self.gs.rise = as_fix(v);
                }
            }
            "Tm" => {
                if ops.len() >= 6 {
                    let m = Mat([
                        as_fix(&ops[ops.len() - 6]),
                        as_fix(&ops[ops.len() - 5]),
                        as_fix(&ops[ops.len() - 4]),
                        as_fix(&ops[ops.len() - 3]),
                        as_fix(&ops[ops.len() - 2]),
                        as_fix(&ops[ops.len() - 1]),
                    ]);
                    self.tm = m;
                    self.tlm = m;
                }
            }
            "Td" => {
                if ops.len() >= 2 {
                    let tx = as_fix(&ops[ops.len() - 2]);
                    let ty = as_fix(&ops[ops.len() - 1]);
                    let trans = Mat([Fix::ONE, Fix::ZERO, Fix::ZERO, Fix::ONE, tx, ty]);
                    self.tlm = trans.concat(self.tlm);
                    self.tm = self.tlm;
                }
            }
            "TD" => {
                if ops.len() >= 2 {
                    let tx = as_fix(&ops[ops.len() - 2]);
                    let ty = as_fix(&ops[ops.len() - 1]);
                    self.gs.leading = Fix(-ty.0);
                    let trans = Mat([Fix::ONE, Fix::ZERO, Fix::ZERO, Fix::ONE, tx, ty]);
                    self.tlm = trans.concat(self.tlm);
                    self.tm = self.tlm;
                }
            }
            "T*" => {
                let trans = Mat([Fix::ONE, Fix::ZERO, Fix::ZERO, Fix::ONE, Fix::ZERO, Fix(-self.gs.leading.0)]);
                self.tlm = trans.concat(self.tlm);
                self.tm = self.tlm;
            }
            "Tj" => {
                if let Some(CsTok::Str(bytes)) = ops.last() {
                    self.emit_text_run(bytes)?;
                }
            }
            "'" => {
                let trans = Mat([Fix::ONE, Fix::ZERO, Fix::ZERO, Fix::ONE, Fix::ZERO, Fix(-self.gs.leading.0)]);
                self.tlm = trans.concat(self.tlm);
                self.tm = self.tlm;
                if let Some(CsTok::Str(bytes)) = ops.last() {
                    self.emit_text_run(bytes)?;
                }
            }
            "\"" => {
                if ops.len() >= 3 {
                    self.gs.word_space = as_fix(&ops[ops.len() - 3]);
                    self.gs.char_space = as_fix(&ops[ops.len() - 2]);
                    let trans = Mat([Fix::ONE, Fix::ZERO, Fix::ZERO, Fix::ONE, Fix::ZERO, Fix(-self.gs.leading.0)]);
                    self.tlm = trans.concat(self.tlm);
                    self.tm = self.tlm;
                    if let CsTok::Str(bytes) = &ops[ops.len() - 1] {
                        self.emit_text_run(bytes)?;
                    }
                }
            }
            "TJ" => {
                if let Some(CsTok::Arr(items)) = ops.last() {
                    let mut combined = Vec::new();
                    for it in items {
                        match it {
                            CsTok::Str(b) => combined.extend_from_slice(b),
                            CsTok::Fixed(adj) => {
                                // Large negative kerning adjustment (>= 250/1000 em) separates words with space.
                                if adj.0 <= -250 * MICRO && !combined.is_empty() && combined.last() != Some(&b' ') {
                                    combined.push(b' ');
                                }
                            }
                            _ => {}
                        }
                    }
                    if !combined.is_empty() {
                        self.emit_text_run(&combined)?;
                    }
                }
            }
            "Do" => {
                if let Some(CsTok::Name(name)) = ops.last() {
                    if self.image_xobjects.contains(name) {
                        self.saw_image_do = true;
                    }
                }
            }
            // Path construction for callout frame boxes and display-tile segments.
            "m" => {
                if ops.len() >= 2 {
                    let p = (as_fix(&ops[ops.len() - 2]), as_fix(&ops[ops.len() - 1]));
                    self.path_cur = Some(p);
                    self.path_start = Some(p);
                    self.push_path_pt(p);
                }
            }
            "l" => {
                if ops.len() >= 2 {
                    let p = (as_fix(&ops[ops.len() - 2]), as_fix(&ops[ops.len() - 1]));
                    if let Some(prev) = self.path_cur {
                        self.push_segment(prev, p);
                    }
                    self.path_cur = Some(p);
                    self.push_path_pt(p);
                }
            }
            "re" => {
                if ops.len() >= 4 {
                    let x = as_fix(&ops[ops.len() - 4]);
                    let y = as_fix(&ops[ops.len() - 3]);
                    let w = as_fix(&ops[ops.len() - 2]);
                    let h = as_fix(&ops[ops.len() - 1]);
                    let p0 = (x, y);
                    let p1 = (x.add(w), y);
                    let p2 = (x.add(w), y.add(h));
                    let p3 = (x, y.add(h));
                    self.push_segment(p0, p1);
                    self.push_segment(p1, p2);
                    self.push_segment(p2, p3);
                    self.push_segment(p3, p0);
                    self.push_path_pt(p0);
                    self.push_path_pt(p2);
                    self.path_cur = Some(p0);
                    self.path_start = Some(p0);
                }
            }
            "h" => {
                if let (Some(cur), Some(st)) = (self.path_cur, self.path_start) {
                    self.push_segment(cur, st);
                    self.path_cur = Some(st);
                }
            }
            "S" | "s" | "f" | "F" | "f*" | "B" | "B*" | "b" | "b*" => {
                if matches!(op, "s" | "b" | "b*") {
                    if let (Some(cur), Some(st)) = (self.path_cur, self.path_start) {
                        self.push_segment(cur, st);
                    }
                }
                if self.include_frame_boxes && self.path_pts.len() >= 2 && self.frame_boxes.len() < MAX_FRAME_BOXES {
                    if let Some(rect) = self.current_path_bbox() {
                        // Record callout-sized rectangles (filter out full-sheet borders > 80% of page width).
                        if rect.width() >= 100 && rect.height() >= 100 && rect.width() * 5 < self.mapper.sheet_w_cmm * 4
                        {
                            self.frame_boxes.push(rect);
                        }
                    }
                }
                self.path_pts.clear();
                self.path_cur = None;
                self.path_start = None;
            }
            "n" => {
                self.path_pts.clear();
                self.path_cur = None;
                self.path_start = None;
            }
            _ => {}
        }
        Ok(())
    }

    fn push_path_pt(&mut self, p: (Fix, Fix)) {
        if self.path_pts.len() < 4096 {
            self.path_pts.push(self.gs.ctm.apply(p.0, p.1));
        }
    }

    fn push_segment(&mut self, a: (Fix, Fix), b: (Fix, Fix)) {
        if self.segments.len() >= MAX_SEGMENTS {
            return;
        }
        let pa = self.gs.ctm.apply(a.0, a.1);
        let pb = self.gs.ctm.apply(b.0, b.1);
        let (x0, y0) = self.mapper.point_to_sheet_cmm(pa.0, pa.1);
        let (x1, y1) = self.mapper.point_to_sheet_cmm(pb.0, pb.1);
        self.segments.push(SheetSegment { x0, y0, x1, y1 });
    }

    fn current_path_bbox(&self) -> Option<SheetRect> {
        let mut iter = self.path_pts.iter().map(|&(px, py)| self.mapper.point_to_sheet_cmm(px, py));
        let (fx, fy) = iter.next()?;
        let (mut min_x, mut max_x) = (fx, fx);
        let (mut min_y, mut max_y) = (fy, fy);
        for (x, y) in iter {
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
        }
        Some(SheetRect::new(min_x, min_y, max_x, max_y))
    }

    fn emit_text_run(&mut self, raw: &[u8]) -> Result<(), AutoTextError> {
        if !self.in_text || self.gs.render_mode == 3 {
            // Render mode 3 is invisible text (e.g. hidden OCR layer).
            return Ok(());
        }
        let decoded = match self.cmaps.get(&self.gs.font) {
            Some(cmap) => cmap.decode(raw),
            None => raw.iter().map(|&b| win_ansi_char(b)).collect(),
        };
        let cleaned: String = decoded.chars().filter(|&c| c != '\0').collect();
        let trimmed = cleaned.trim();
        if trimmed.is_empty() {
            return Ok(());
        }
        if self.runs.len() >= MAX_TEXT_RUNS_PER_SHEET {
            return Err(AutoTextError::LimitExceeded {
                limit: "text_runs",
                detail: format!("extracted text runs exceed {MAX_TEXT_RUNS_PER_SHEET}"),
            });
        }
        self.total_text_bytes = self.total_text_bytes.saturating_add(trimmed.len());
        if self.total_text_bytes > MAX_TEXT_BYTES_PER_SHEET {
            return Err(AutoTextError::LimitExceeded {
                limit: "text_bytes",
                detail: format!("extracted text exceeds {MAX_TEXT_BYTES_PER_SHEET} bytes"),
            });
        }

        // Combined text rendering matrix: TRM = [Tfs*Th, 0, 0, Tfs, 0, Trise] * Tm * CTM
        let tfs = self.gs.font_size;
        let h_ratio = Fix(self.gs.h_scale.0 / 100);
        let text_scale = Mat([tfs.mul(h_ratio), Fix::ZERO, Fix::ZERO, tfs, Fix::ZERO, self.gs.rise]);
        let trm = text_scale.concat(self.tm).concat(self.gs.ctm);

        let n_chars = trimmed.chars().count() as i64;
        // Standard Helvetica-like mean advance width: 0.60 em per glyph.
        let adv_em = Fix(i128::from(n_chars).saturating_mul(600_000));
        let ascent_em = Fix(800_000);
        let descent_em = Fix(-200_000);

        let corners = [
            trm.apply(Fix::ZERO, descent_em),
            trm.apply(adv_em, descent_em),
            trm.apply(adv_em, ascent_em),
            trm.apply(Fix::ZERO, ascent_em),
        ];
        let mut min_x = i64::MAX;
        let mut min_y = i64::MAX;
        let mut max_x = i64::MIN;
        let mut max_y = i64::MIN;
        for (px, py) in corners {
            let (sx, sy) = self.mapper.point_to_sheet_cmm(px, py);
            min_x = min_x.min(sx);
            max_x = max_x.max(sx);
            min_y = min_y.min(sy);
            max_y = max_y.max(sy);
        }

        // Font height in 0.01 mm from the transformed Y basis vector of TRM.
        let origin = trm.apply(Fix::ZERO, Fix::ZERO);
        let top = trm.apply(Fix::ZERO, Fix::ONE);
        let font_h_pt = fix_hypot(top.0.sub(origin.0), top.1.sub(origin.1));
        let font_h_cmm = self.mapper.length_to_cmm(font_h_pt);

        // Baseline direction angle in millidegrees from the transformed X basis vector of TRM.
        let right = trm.apply(Fix::ONE, Fix::ZERO);
        let angle_mdeg = angle_mdeg_from_vec(right.0.sub(origin.0), right.1.sub(origin.1), self.mapper.rotation);

        // Advance text matrix along baseline so consecutive Tj calls don't collapse to the same point.
        let adv_pt = adv_em.mul(tfs).mul(h_ratio);
        let adv_mat = Mat([Fix::ONE, Fix::ZERO, Fix::ZERO, Fix::ONE, adv_pt, Fix::ZERO]);
        self.tm = adv_mat.concat(self.tm);

        let text = Untrusted::text(trimmed).map_err(|_| AutoTextError::LimitExceeded {
            limit: "run_length",
            detail: "text run > 4096 bytes".into(),
        })?;

        let order = self.runs.len() as u32;
        self.runs.push(RawRun {
            order,
            text,
            bbox: SheetRect::new(min_x, min_y, max_x, max_y),
            font_h_cmm,
            angle_mdeg,
        });
        Ok(())
    }
}

fn as_fix(t: &CsTok) -> Fix {
    match t {
        CsTok::Fixed(f) => *f,
        _ => Fix::ZERO,
    }
}

fn next_cs_tok(data: &[u8], pos: &mut usize, depth: usize) -> Result<Option<CsTok>, AutoTextError> {
    if depth > MAX_NESTING {
        return Err(AutoTextError::LimitExceeded { limit: "nesting", detail: "content stream nesting > 64".into() });
    }
    skip_pdf_ws(data, pos);
    if *pos >= data.len() {
        return Ok(None);
    }
    let b = data[*pos];
    if b == b'(' {
        return Ok(Some(CsTok::Str(parse_lit_str(data, pos)?)));
    }
    if b == b'<' {
        if data.get(*pos + 1) == Some(&b'<') {
            // Inline dict (e.g. BDC properties): skip cleanly.
            let _ = parse_pdf_val(data, pos, depth + 1)?;
            return next_cs_tok(data, pos, depth + 1);
        }
        return Ok(Some(CsTok::Str(parse_hex_str(data, pos)?)));
    }
    if b == b'[' {
        *pos += 1;
        let mut items = Vec::new();
        loop {
            skip_pdf_ws(data, pos);
            if *pos >= data.len() {
                break;
            }
            if data[*pos] == b']' {
                *pos += 1;
                break;
            }
            if let Some(it) = next_cs_tok(data, pos, depth + 1)? {
                if items.len() < 4096 {
                    items.push(it);
                }
            } else {
                break;
            }
        }
        return Ok(Some(CsTok::Arr(items)));
    }
    if b == b'/' {
        return Ok(Some(CsTok::Name(parse_name(data, pos))));
    }
    let start = *pos;
    while *pos < data.len() && !is_delim(data[*pos]) && !is_ws(data[*pos]) {
        *pos += 1;
    }
    if *pos == start {
        *pos += 1;
        return next_cs_tok(data, pos, depth + 1);
    }
    let slice = &data[start..*pos];
    if let Some(f) = parse_fixed(slice) {
        return Ok(Some(CsTok::Fixed(f)));
    }
    let op = String::from_utf8_lossy(slice).into_owned();
    Ok(Some(CsTok::Op(op)))
}

// ------------------------------------------------------------------ minimal bounded PDF object loader

#[derive(Debug, Clone)]
#[allow(dead_code)]
enum PdfVal {
    Null,
    Bool(bool),
    Int(i64),
    Fixed(Fix),
    Name(String),
    Str(Vec<u8>),
    Arr(Vec<PdfVal>),
    Dict(BTreeMap<String, PdfVal>),
    Ref(u32, u16),
}

impl PdfVal {
    fn as_dict(&self) -> Option<&BTreeMap<String, PdfVal>> {
        match self {
            PdfVal::Dict(d) => Some(d),
            _ => None,
        }
    }
    fn as_arr(&self) -> Option<&[PdfVal]> {
        match self {
            PdfVal::Arr(a) => Some(a),
            _ => None,
        }
    }
    fn as_name(&self) -> Option<&str> {
        match self {
            PdfVal::Name(s) => Some(s),
            _ => None,
        }
    }
    fn as_ref(&self) -> Option<u32> {
        match self {
            PdfVal::Ref(n, _) => Some(*n),
            _ => None,
        }
    }
    fn as_int(&self) -> Option<i64> {
        match self {
            PdfVal::Int(i) => Some(*i),
            _ => None,
        }
    }
    fn as_fix(&self) -> Option<Fix> {
        match self {
            PdfVal::Int(i) => Some(Fix::from_int(*i)),
            PdfVal::Fixed(f) => Some(*f),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum XrefEntry {
    Offset(usize),
    Compressed { objstm: u32, index: usize },
}

struct PdfDoc<'a> {
    data: &'a [u8],
    xref: BTreeMap<u32, XrefEntry>,
    trailer_root: Option<u32>,
}

impl<'a> PdfDoc<'a> {
    fn parse(data: &'a [u8]) -> Result<Self, AutoTextError> {
        if data.len() < 8 || !data[..data.len().min(1024)].windows(5).any(|w| w == b"%PDF-") {
            return Err(AutoTextError::PdfMalformed { at: 0, detail: "missing %PDF- header".into() });
        }
        let startxref = find_startxref(data)?;
        let mut xref = BTreeMap::new();
        let mut visited_xrefs = BTreeSet::new();
        let mut trailer_root = None;
        let mut cur = Some(startxref);

        while let Some(off) = cur {
            if !visited_xrefs.insert(off) || visited_xrefs.len() > 64 {
                return Err(AutoTextError::PdfMalformed { at: off as u64, detail: "cyclic xref chain".into() });
            }
            if off >= data.len() {
                return Err(AutoTextError::PdfMalformed { at: off as u64, detail: "xref offset past EOF".into() });
            }
            let mut pos = off;
            skip_pdf_ws(data, &mut pos);
            if data.get(pos..pos + 4) == Some(b"xref") {
                pos += 4;
                let td = parse_xref_table(data, &mut pos, &mut xref)?;
                if td.contains_key("Encrypt") {
                    return Err(AutoTextError::PdfMalformed {
                        at: off as u64,
                        detail: "encrypted PDF unsupported".into(),
                    });
                }
                if trailer_root.is_none() {
                    trailer_root = td.get("Root").and_then(PdfVal::as_ref);
                }
                cur = td.get("Prev").and_then(PdfVal::as_int).and_then(|p| usize::try_from(p).ok());
            } else {
                let (val, stream) = parse_indirect_obj(data, off)?;
                let dict = val.as_dict().ok_or_else(|| AutoTextError::PdfMalformed {
                    at: off as u64,
                    detail: "xref stream not a dict".into(),
                })?;
                if dict.contains_key("Encrypt") {
                    return Err(AutoTextError::PdfMalformed {
                        at: off as u64,
                        detail: "encrypted PDF unsupported".into(),
                    });
                }
                if trailer_root.is_none() {
                    trailer_root = dict.get("Root").and_then(PdfVal::as_ref);
                }
                let raw_stream = stream.ok_or_else(|| AutoTextError::PdfMalformed {
                    at: off as u64,
                    detail: "xref stream missing payload".into(),
                })?;
                let decoded = decode_stream_bytes(dict, raw_stream)?;
                parse_xref_stream(dict, &decoded, &mut xref)?;
                cur = dict.get("Prev").and_then(PdfVal::as_int).and_then(|p| usize::try_from(p).ok());
            }
        }

        Ok(PdfDoc { data, xref, trailer_root })
    }

    fn resolve_val(&self, v: &PdfVal, depth: usize) -> Result<PdfVal, AutoTextError> {
        if depth > MAX_NESTING {
            return Err(AutoTextError::LimitExceeded {
                limit: "nesting",
                detail: "indirect reference nesting > 64".into(),
            });
        }
        match v {
            PdfVal::Ref(num, _) => Ok(self.resolve_obj(*num, depth + 1)?.0),
            other => Ok(other.clone()),
        }
    }

    fn resolve_obj(&self, obj_num: u32, depth: usize) -> Result<(PdfVal, Option<Vec<u8>>), AutoTextError> {
        if depth > MAX_NESTING {
            return Err(AutoTextError::LimitExceeded { limit: "nesting", detail: "object nesting > 64".into() });
        }
        let Some(&entry) = self.xref.get(&obj_num) else {
            return Ok((PdfVal::Null, None));
        };
        match entry {
            XrefEntry::Offset(off) => {
                let (v, s) = parse_indirect_obj(self.data, off)?;
                Ok((v, s.map(|b| b.to_vec())))
            }
            XrefEntry::Compressed { objstm, index } => {
                let (stm_val, raw) = self.resolve_obj(objstm, depth + 1)?;
                let dict = stm_val
                    .as_dict()
                    .ok_or_else(|| AutoTextError::PdfMalformed { at: 0, detail: "ObjStm is not a dict".into() })?;
                let raw_bytes =
                    raw.ok_or_else(|| AutoTextError::PdfMalformed { at: 0, detail: "ObjStm missing stream".into() })?;
                let decoded = decode_stream_bytes(dict, &raw_bytes)?;
                let n = dict.get("N").and_then(PdfVal::as_int).unwrap_or(0) as usize;
                let first = dict.get("First").and_then(PdfVal::as_int).unwrap_or(0) as usize;
                if index >= n || first > decoded.len() {
                    return Err(AutoTextError::PdfMalformed { at: 0, detail: "ObjStm index out of range".into() });
                }
                let mut pos = 0usize;
                let mut target_rel = None;
                for i in 0..n {
                    let _num = read_uint_tok(&decoded, &mut pos)?;
                    let rel = read_uint_tok(&decoded, &mut pos)?;
                    if i == index {
                        target_rel = Some(rel as usize);
                        break;
                    }
                }
                let obj_pos = first.saturating_add(target_rel.unwrap_or(0));
                if obj_pos >= decoded.len() {
                    return Err(AutoTextError::PdfMalformed { at: 0, detail: "ObjStm offset out of bounds".into() });
                }
                let mut p = obj_pos;
                let val = parse_pdf_val(&decoded, &mut p, depth + 1)?;
                Ok((val, None))
            }
        }
    }

    fn decode_stream(&self, obj_num: u32, depth: usize) -> Result<Vec<u8>, AutoTextError> {
        let (val, raw) = self.resolve_obj(obj_num, depth)?;
        let dict = val
            .as_dict()
            .ok_or_else(|| AutoTextError::PdfMalformed { at: 0, detail: "stream object is not a dict".into() })?;
        let raw_bytes =
            raw.ok_or_else(|| AutoTextError::PdfMalformed { at: 0, detail: "missing stream bytes".into() })?;
        decode_stream_bytes(dict, &raw_bytes)
    }
}

struct PageInfo {
    media_box: Option<[Fix; 4]>,
    crop_box: Option<[Fix; 4]>,
    rotation: u16,
    user_unit: Option<Fix>,
    resources: Option<BTreeMap<String, PdfVal>>,
    contents: Option<PdfVal>,
}

fn collect_pages(doc: &PdfDoc<'_>, root_pages_ref: u32) -> Result<Vec<PageInfo>, AutoTextError> {
    struct Frame {
        obj_num: u32,
        media: Option<[Fix; 4]>,
        crop: Option<[Fix; 4]>,
        rot: u16,
        res: Option<BTreeMap<String, PdfVal>>,
    }
    let mut pages = Vec::new();
    let mut visited = BTreeSet::new();
    let mut stack = vec![Frame { obj_num: root_pages_ref, media: None, crop: None, rot: 0, res: None }];

    while let Some(frame) = stack.pop() {
        if !visited.insert(frame.obj_num) {
            return Err(AutoTextError::PdfMalformed { at: 0, detail: "cyclic PDF page tree".into() });
        }
        let (val, _) = doc.resolve_obj(frame.obj_num, 0)?;
        let dict = val
            .as_dict()
            .ok_or_else(|| AutoTextError::PdfMalformed { at: 0, detail: "page tree node not a dict".into() })?;
        let media = extract_rect(doc, dict.get("MediaBox"))?.or(frame.media);
        let crop = extract_rect(doc, dict.get("CropBox"))?.or(frame.crop);
        let rot = match dict.get("Rotate").and_then(PdfVal::as_int) {
            Some(r) => (r.rem_euclid(360)) as u16,
            None => frame.rot,
        };
        let res = match dict.get("Resources") {
            Some(rv) => doc.resolve_val(rv, 0)?.as_dict().cloned().or(frame.res),
            None => frame.res,
        };
        let node_type = dict.get("Type").and_then(PdfVal::as_name);
        if node_type == Some("Pages") {
            let kids_val = dict
                .get("Kids")
                .ok_or_else(|| AutoTextError::PdfMalformed { at: 0, detail: "/Pages missing /Kids".into() })?;
            let kids_resolved = doc.resolve_val(kids_val, 0)?;
            let kids = kids_resolved
                .as_arr()
                .ok_or_else(|| AutoTextError::PdfMalformed { at: 0, detail: "/Kids is not an array".into() })?;
            for kid in kids.iter().rev() {
                if let Some(kref) = kid.as_ref() {
                    stack.push(Frame { obj_num: kref, media, crop, rot, res: res.clone() });
                }
            }
        } else {
            if pages.len() >= 200 {
                return Err(AutoTextError::LimitExceeded { limit: "pages", detail: "PDF exceeds 200 pages".into() });
            }
            let user_unit = dict.get("UserUnit").and_then(PdfVal::as_fix);
            let contents = match dict.get("Contents") {
                Some(c @ PdfVal::Ref(_, _)) => {
                    // Contents may be an indirect reference to either a stream or an array of streams.
                    let resolved = doc.resolve_val(c, 0)?;
                    if resolved.as_arr().is_some() {
                        Some(resolved)
                    } else {
                        Some(c.clone())
                    }
                }
                Some(other) => Some(other.clone()),
                None => None,
            };
            pages.push(PageInfo {
                media_box: media,
                crop_box: crop,
                rotation: rot,
                user_unit,
                resources: res,
                contents,
            });
        }
    }
    Ok(pages)
}

fn extract_rect(doc: &PdfDoc<'_>, v: Option<&PdfVal>) -> Result<Option<[Fix; 4]>, AutoTextError> {
    let Some(val) = v else { return Ok(None) };
    let resolved = doc.resolve_val(val, 0)?;
    let Some(arr) = resolved.as_arr() else { return Ok(None) };
    if arr.len() != 4 {
        return Ok(None);
    }
    let (Some(a), Some(b), Some(c), Some(d)) = (arr[0].as_fix(), arr[1].as_fix(), arr[2].as_fix(), arr[3].as_fix())
    else {
        return Ok(None);
    };
    Ok(Some([a, b, c, d]))
}

fn decode_stream_bytes(dict: &BTreeMap<String, PdfVal>, raw: &[u8]) -> Result<Vec<u8>, AutoTextError> {
    let filter_name = match dict.get("Filter") {
        None => None,
        Some(PdfVal::Name(n)) => Some(n.as_str()),
        Some(PdfVal::Arr(arr)) if arr.len() == 1 => arr[0].as_name(),
        _ => {
            return Err(AutoTextError::PdfMalformed { at: 0, detail: "unsupported multi-filter PDF stream".into() });
        }
    };
    let mut data = match filter_name {
        None => {
            if raw.len() > MAX_DECODED_STREAM_BYTES {
                return Err(AutoTextError::LimitExceeded {
                    limit: "stream_bytes",
                    detail: "raw PDF stream exceeds 16 MiB".into(),
                });
            }
            raw.to_vec()
        }
        Some("FlateDecode") => decompress_zlib(raw, MAX_DECODED_STREAM_BYTES)?,
        Some(other) => {
            return Err(AutoTextError::PdfMalformed { at: 0, detail: format!("unsupported stream filter /{other}") });
        }
    };

    // Handle PNG Up predictor for PDF 1.5 XRef streams if present.
    if let Some(dp) = dict.get("DecodeParms").and_then(PdfVal::as_dict) {
        let pred = dp.get("Predictor").and_then(PdfVal::as_int).unwrap_or(1);
        if pred >= 10 {
            let cols = dp.get("Columns").and_then(PdfVal::as_int).unwrap_or(1).max(1) as usize;
            let stride = cols + 1;
            if data.len() % stride == 0 {
                let rows = data.len() / stride;
                let mut out = vec![0u8; rows * cols];
                for r in 0..rows {
                    let f = data[r * stride];
                    for c in 0..cols {
                        let x = data[r * stride + 1 + c];
                        let left = if c > 0 { out[r * cols + c - 1] } else { 0 };
                        let up = if r > 0 { out[(r - 1) * cols + c] } else { 0 };
                        out[r * cols + c] = match f {
                            0 => x,
                            1 => x.wrapping_add(left),
                            2 => x.wrapping_add(up),
                            _ => x,
                        };
                    }
                }
                data = out;
            }
        }
    }
    Ok(data)
}

fn find_startxref(data: &[u8]) -> Result<usize, AutoTextError> {
    let tail_start = data.len().saturating_sub(4096);
    let tail = &data[tail_start..];
    let key = b"startxref";
    let mut found = None;
    for i in (0..=tail.len().saturating_sub(key.len())).rev() {
        if &tail[i..i + key.len()] == key {
            found = Some(tail_start + i + key.len());
            break;
        }
    }
    let mut pos = found
        .ok_or_else(|| AutoTextError::PdfMalformed { at: data.len() as u64, detail: "missing startxref".into() })?;
    let off = read_uint_tok(data, &mut pos)?;
    usize::try_from(off)
        .map_err(|_| AutoTextError::PdfMalformed { at: pos as u64, detail: "startxref overflow".into() })
}

fn parse_xref_table(
    data: &[u8],
    pos: &mut usize,
    xref: &mut BTreeMap<u32, XrefEntry>,
) -> Result<BTreeMap<String, PdfVal>, AutoTextError> {
    loop {
        skip_pdf_ws(data, pos);
        if data.get(*pos..*pos + 7) == Some(b"trailer") {
            *pos += 7;
            let v = parse_pdf_val(data, pos, 0)?;
            return match v {
                PdfVal::Dict(d) => Ok(d),
                _ => Err(AutoTextError::PdfMalformed { at: *pos as u64, detail: "trailer is not a dict".into() }),
            };
        }
        let first = read_uint_tok(data, pos)? as u32;
        let count = read_uint_tok(data, pos)? as u32;
        if xref.len().saturating_add(count as usize) > MAX_XREF_OBJECTS {
            return Err(AutoTextError::LimitExceeded { limit: "xref_objects", detail: "too many xref entries".into() });
        }
        for i in 0..count {
            let off = read_uint_tok(data, pos)? as usize;
            let _gen = read_uint_tok(data, pos)?;
            skip_pdf_ws(data, pos);
            let flag = *data.get(*pos).ok_or_else(|| AutoTextError::PdfMalformed {
                at: *pos as u64,
                detail: "truncated xref entry".into(),
            })?;
            *pos += 1;
            if flag == b'n' {
                xref.entry(first.saturating_add(i)).or_insert(XrefEntry::Offset(off));
            }
        }
    }
}

fn parse_xref_stream(
    dict: &BTreeMap<String, PdfVal>,
    decoded: &[u8],
    xref: &mut BTreeMap<u32, XrefEntry>,
) -> Result<(), AutoTextError> {
    let w_arr = dict
        .get("W")
        .and_then(PdfVal::as_arr)
        .ok_or_else(|| AutoTextError::PdfMalformed { at: 0, detail: "xref stream missing /W".into() })?;
    if w_arr.len() != 3 {
        return Err(AutoTextError::PdfMalformed { at: 0, detail: "xref /W length != 3".into() });
    }
    let w0 = w_arr[0].as_int().unwrap_or(0).clamp(0, 8) as usize;
    let w1 = w_arr[1].as_int().unwrap_or(0).clamp(0, 8) as usize;
    let w2 = w_arr[2].as_int().unwrap_or(0).clamp(0, 8) as usize;
    let entry_len = w0 + w1 + w2;
    if entry_len == 0 {
        return Err(AutoTextError::PdfMalformed { at: 0, detail: "xref /W sum is 0".into() });
    }
    let size = dict.get("Size").and_then(PdfVal::as_int).unwrap_or(0).max(0) as u32;
    let mut ranges = Vec::new();
    if let Some(idx) = dict.get("Index").and_then(PdfVal::as_arr) {
        for pair in idx.chunks_exact(2) {
            let s = pair[0].as_int().unwrap_or(0).max(0) as u32;
            let c = pair[1].as_int().unwrap_or(0).max(0) as u32;
            ranges.push((s, c));
        }
    } else {
        ranges.push((0, size));
    }
    let mut p = 0usize;
    for (first, count) in ranges {
        if xref.len().saturating_add(count as usize) > MAX_XREF_OBJECTS {
            return Err(AutoTextError::LimitExceeded {
                limit: "xref_objects",
                detail: "too many xref stream entries".into(),
            });
        }
        for i in 0..count {
            if p + entry_len > decoded.len() {
                break;
            }
            let ty = if w0 == 0 { 1 } else { read_be_uint(&decoded[p..p + w0]) };
            let f2 = read_be_uint(&decoded[p + w0..p + w0 + w1]);
            let f3 = read_be_uint(&decoded[p + w0 + w1..p + entry_len]);
            p += entry_len;
            match ty {
                1 => {
                    xref.entry(first.saturating_add(i)).or_insert(XrefEntry::Offset(f2 as usize));
                }
                2 => {
                    xref.entry(first.saturating_add(i))
                        .or_insert(XrefEntry::Compressed { objstm: f2 as u32, index: f3 as usize });
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn read_be_uint(slice: &[u8]) -> u64 {
    let mut v = 0u64;
    for &b in slice {
        v = (v << 8) | u64::from(b);
    }
    v
}

fn parse_indirect_obj(data: &[u8], offset: usize) -> Result<(PdfVal, Option<&[u8]>), AutoTextError> {
    let mut pos = offset;
    let _obj = read_uint_tok(data, &mut pos)?;
    let _gen = read_uint_tok(data, &mut pos)?;
    skip_pdf_ws(data, &mut pos);
    if data.get(pos..pos + 3) != Some(b"obj") {
        return Err(AutoTextError::PdfMalformed { at: pos as u64, detail: "expected 'obj'".into() });
    }
    pos += 3;
    let val = parse_pdf_val(data, &mut pos, 0)?;
    skip_pdf_ws(data, &mut pos);
    let stream = if data.get(pos..pos + 6) == Some(b"stream") {
        pos += 6;
        if data.get(pos) == Some(&b'\r') && data.get(pos + 1) == Some(&b'\n') {
            pos += 2;
        } else if data.get(pos) == Some(&b'\n') || data.get(pos) == Some(&b'\r') {
            pos += 1;
        }
        let declared_len = val.as_dict().and_then(|d| d.get("Length")).and_then(PdfVal::as_int);
        let end = if let Some(len) = declared_len.and_then(|l| usize::try_from(l).ok()) {
            let cand = pos.saturating_add(len);
            if cand <= data.len() {
                cand
            } else {
                find_endstream(data, pos)?
            }
        } else {
            find_endstream(data, pos)?
        };
        Some(&data[pos..end])
    } else {
        None
    };
    Ok((val, stream))
}

fn find_endstream(data: &[u8], start: usize) -> Result<usize, AutoTextError> {
    let key = b"endstream";
    if let Some(rel) = data[start..].windows(key.len()).position(|w| w == key) {
        let mut end = start + rel;
        while end > start && (data[end - 1] == b'\n' || data[end - 1] == b'\r') {
            end -= 1;
        }
        Ok(end)
    } else {
        Err(AutoTextError::PdfMalformed { at: start as u64, detail: "missing endstream".into() })
    }
}

fn parse_pdf_val(data: &[u8], pos: &mut usize, depth: usize) -> Result<PdfVal, AutoTextError> {
    if depth > MAX_NESTING {
        return Err(AutoTextError::LimitExceeded { limit: "nesting", detail: "PDF object nesting > 64".into() });
    }
    skip_pdf_ws(data, pos);
    if *pos >= data.len() {
        return Err(AutoTextError::PdfMalformed { at: *pos as u64, detail: "unexpected EOF".into() });
    }
    match data[*pos] {
        b'<' if data.get(*pos + 1) == Some(&b'<') => {
            *pos += 2;
            let mut map = BTreeMap::new();
            loop {
                skip_pdf_ws(data, pos);
                if data.get(*pos..*pos + 2) == Some(b">>") {
                    *pos += 2;
                    break;
                }
                if *pos >= data.len() {
                    return Err(AutoTextError::PdfMalformed { at: *pos as u64, detail: "unterminated dict".into() });
                }
                let k = parse_name(data, pos);
                let v = parse_pdf_val(data, pos, depth + 1)?;
                map.insert(k, v);
            }
            Ok(PdfVal::Dict(map))
        }
        b'<' => Ok(PdfVal::Str(parse_hex_str(data, pos)?)),
        b'(' => Ok(PdfVal::Str(parse_lit_str(data, pos)?)),
        b'[' => {
            *pos += 1;
            let mut arr = Vec::new();
            loop {
                skip_pdf_ws(data, pos);
                if data.get(*pos) == Some(&b']') {
                    *pos += 1;
                    break;
                }
                if *pos >= data.len() {
                    return Err(AutoTextError::PdfMalformed { at: *pos as u64, detail: "unterminated array".into() });
                }
                arr.push(parse_pdf_val(data, pos, depth + 1)?);
            }
            Ok(PdfVal::Arr(arr))
        }
        b'/' => Ok(PdfVal::Name(parse_name(data, pos))),
        _ => {
            let start = *pos;
            while *pos < data.len() && !is_ws(data[*pos]) && !is_delim(data[*pos]) {
                *pos += 1;
            }
            let tok = &data[start..*pos];
            match tok {
                b"null" => Ok(PdfVal::Null),
                b"true" => Ok(PdfVal::Bool(true)),
                b"false" => Ok(PdfVal::Bool(false)),
                _ => {
                    if let Ok(s) = std::str::from_utf8(tok) {
                        if let Ok(i) = s.parse::<i64>() {
                            // Look ahead for indirect reference `<obj> <gen> R`.
                            if i >= 0 {
                                let saved = *pos;
                                skip_pdf_ws(data, pos);
                                let gs = *pos;
                                while *pos < data.len() && data[*pos].is_ascii_digit() {
                                    *pos += 1;
                                }
                                if *pos > gs {
                                    if let Ok(gen_num) =
                                        std::str::from_utf8(&data[gs..*pos]).unwrap_or("").parse::<u16>()
                                    {
                                        skip_pdf_ws(data, pos);
                                        if data.get(*pos) == Some(&b'R')
                                            && data.get(*pos + 1).is_none_or(|&c| is_ws(c) || is_delim(c))
                                        {
                                            *pos += 1;
                                            return Ok(PdfVal::Ref(i as u32, gen_num));
                                        }
                                    }
                                }
                                *pos = saved;
                            }
                            return Ok(PdfVal::Int(i));
                        }
                    }
                    if let Some(f) = parse_fixed(tok) {
                        return Ok(PdfVal::Fixed(f));
                    }
                    Err(AutoTextError::PdfMalformed { at: start as u64, detail: "invalid PDF token".into() })
                }
            }
        }
    }
}

fn parse_name(data: &[u8], pos: &mut usize) -> String {
    if data.get(*pos) == Some(&b'/') {
        *pos += 1;
    }
    let mut out = Vec::new();
    while *pos < data.len() && !is_ws(data[*pos]) && !is_delim(data[*pos]) {
        if data[*pos] == b'#' && *pos + 2 < data.len() {
            if let (Some(h), Some(l)) = (hex_nibble(data[*pos + 1]), hex_nibble(data[*pos + 2])) {
                out.push((h << 4) | l);
                *pos += 3;
                continue;
            }
        }
        out.push(data[*pos]);
        *pos += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn parse_lit_str(data: &[u8], pos: &mut usize) -> Result<Vec<u8>, AutoTextError> {
    if data.get(*pos) != Some(&b'(') {
        return Err(AutoTextError::PdfMalformed { at: *pos as u64, detail: "expected '('".into() });
    }
    *pos += 1;
    let mut out = Vec::new();
    let mut depth = 1usize;
    while *pos < data.len() && depth > 0 {
        let b = data[*pos];
        *pos += 1;
        match b {
            b'\\' if *pos < data.len() => {
                let esc = data[*pos];
                *pos += 1;
                match esc {
                    b'n' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b't' => out.push(b'\t'),
                    b'b' => out.push(0x08),
                    b'f' => out.push(0x0C),
                    b'0'..=b'7' => {
                        let mut val = esc - b'0';
                        for _ in 0..2 {
                            if let Some(&d) = data.get(*pos) {
                                if (b'0'..=b'7').contains(&d) {
                                    val = val.wrapping_mul(8).wrapping_add(d - b'0');
                                    *pos += 1;
                                } else {
                                    break;
                                }
                            }
                        }
                        out.push(val);
                    }
                    b'\r' => {
                        if data.get(*pos) == Some(&b'\n') {
                            *pos += 1;
                        }
                    }
                    b'\n' => {}
                    other => out.push(other),
                }
            }
            b'(' => {
                depth += 1;
                out.push(b'(');
            }
            b')' => {
                depth -= 1;
                if depth > 0 {
                    out.push(b')');
                }
            }
            other => out.push(other),
        }
    }
    Ok(out)
}

fn parse_hex_str(data: &[u8], pos: &mut usize) -> Result<Vec<u8>, AutoTextError> {
    if data.get(*pos) != Some(&b'<') {
        return Err(AutoTextError::PdfMalformed { at: *pos as u64, detail: "expected '<'".into() });
    }
    *pos += 1;
    let mut out = Vec::new();
    let mut hi: Option<u8> = None;
    while *pos < data.len() {
        let b = data[*pos];
        *pos += 1;
        if b == b'>' {
            if let Some(h) = hi {
                out.push(h << 4);
            }
            return Ok(out);
        }
        if is_ws(b) {
            continue;
        }
        if let Some(nib) = hex_nibble(b) {
            if let Some(h) = hi.take() {
                out.push((h << 4) | nib);
            } else {
                hi = Some(nib);
            }
        }
    }
    Err(AutoTextError::PdfMalformed { at: *pos as u64, detail: "unterminated hex string".into() })
}

fn parse_fixed(slice: &[u8]) -> Option<Fix> {
    if slice.is_empty() {
        return None;
    }
    let mut i = 0usize;
    let neg = match slice[0] {
        b'-' => {
            i = 1;
            true
        }
        b'+' => {
            i = 1;
            false
        }
        _ => false,
    };
    if i >= slice.len() {
        return None;
    }
    let mut int_part: i128 = 0;
    let mut frac_part: i128 = 0;
    let mut frac_scale: i128 = MICRO;
    let mut saw_digit = false;
    let mut in_frac = false;
    while i < slice.len() {
        let b = slice[i];
        i += 1;
        if b == b'.' {
            if in_frac {
                return None;
            }
            in_frac = true;
        } else if b.is_ascii_digit() {
            saw_digit = true;
            let d = i128::from(b - b'0');
            if !in_frac {
                int_part = int_part.saturating_mul(10).saturating_add(d);
            } else if frac_scale > 1 {
                frac_scale /= 10;
                frac_part = frac_part.saturating_add(d.saturating_mul(frac_scale));
            }
        } else {
            return None;
        }
    }
    if !saw_digit {
        return None;
    }
    let mag = int_part.saturating_mul(MICRO).saturating_add(frac_part);
    Some(Fix(if neg { -mag } else { mag }))
}

fn read_uint_tok(data: &[u8], pos: &mut usize) -> Result<u64, AutoTextError> {
    skip_pdf_ws(data, pos);
    let start = *pos;
    while *pos < data.len() && data[*pos].is_ascii_digit() {
        *pos += 1;
    }
    if *pos == start {
        return Err(AutoTextError::PdfMalformed { at: start as u64, detail: "expected unsigned integer".into() });
    }
    std::str::from_utf8(&data[start..*pos])
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .ok_or_else(|| AutoTextError::PdfMalformed { at: start as u64, detail: "integer overflow".into() })
}

fn skip_pdf_ws(data: &[u8], pos: &mut usize) {
    while *pos < data.len() {
        let b = data[*pos];
        if is_ws(b) {
            *pos += 1;
        } else if b == b'%' {
            while *pos < data.len() && data[*pos] != b'\n' && data[*pos] != b'\r' {
                *pos += 1;
            }
        } else {
            break;
        }
    }
}

fn is_ws(b: u8) -> bool {
    matches!(b, 0x00 | 0x09 | 0x0A | 0x0C | 0x0D | 0x20)
}

fn is_delim(b: u8) -> bool {
    matches!(b, b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%')
}
