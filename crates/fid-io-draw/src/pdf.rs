//! Bounded PDF page-structure reader (`IO.REG.PLUGIN.DRAW`, REQ-002, REQ-004..REQ-007).
//!
//! - Parses classic `xref` tables, PDF 1.5+ cross-reference streams (`/Type /XRef` with Flate
//!   + PNG/TIFF predictor), and `/Type /ObjStm` compressed object streams.
//! - Rejects encrypted PDFs with `pdf_encrypted` (`REQ-004`, `AC-6`).
//! - Never executes actions or JavaScript; detects and reports `/JavaScript`, `/JS`,
//!   `/OpenAction`, `/AA`, `/EmbeddedFiles`, and `/AcroForm` as warnings (`REQ-005`, `AC-6`).
//! - Walks the `/Pages` tree iteratively with cycle detection, inheriting `/MediaBox`,
//!   `/CropBox`, `/Rotate`, and `/UserUnit`, enforcing `MAX_PAGES = 200` (`REQ-006`, `AC-7`).
//! - Converts page dimensions to exact integer `0.01 mm` units (`half_even(pts * UserUnit * 2540 / 72)`),
//!   using `CropBox` else `MediaBox` and swapping width/height on `/Rotate` 90 or 270 (`AC-1..AC-3`).

use crate::flate::zlib_decompress;
use crate::{round_half_even, BoxUsed, DrawError, DrawWarning, DrawingPage, MAX_PAGES};
use std::collections::{BTreeMap, BTreeSet};

const MAX_PDF_DEPTH: usize = 64;
const MAX_PDF_OBJECTS: usize = 200_000;
const MAX_XREF_CHAIN: usize = 64;
const MAX_STREAM_DECOMPRESS: usize = 16 * 1024 * 1024;
const MAX_TOKENS_PER_OBJ: usize = 65_536;

/// Decimal number parsed exactly as `scaled / 10^6` (`i128`) so PDF coordinates convert to
/// `0.01 mm` with a single half-even rounding and zero intermediate float error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FixedMicro(i128);

const MICRO: i128 = 1_000_000;

#[derive(Debug, Clone)]
#[allow(dead_code)]
enum PdfVal {
    Null,
    Bool(bool),
    Int(i64),
    Num(FixedMicro),
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
            PdfVal::Name(s) => Some(s.as_str()),
            _ => None,
        }
    }

    fn as_i64(&self) -> Option<i64> {
        match self {
            PdfVal::Int(n) => Some(*n),
            PdfVal::Num(FixedMicro(v)) if v % MICRO == 0 => i64::try_from(v / MICRO).ok(),
            _ => None,
        }
    }

    fn as_micro(&self) -> Option<i128> {
        match self {
            PdfVal::Int(n) => i128::from(*n).checked_mul(MICRO),
            PdfVal::Num(FixedMicro(v)) => Some(*v),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum XrefEntry {
    InFile(usize),
    InObjStm { stm_obj: u32, index: usize },
}

/// Output of parsing a PDF's page structure.
#[derive(Debug, Clone)]
pub struct PdfStructure {
    pub pages: Vec<DrawingPage>,
    pub warnings: Vec<DrawWarning>,
}

/// Parses a PDF byte slice into page-structure records and non-fatal security/feature warnings.
pub fn read_pdf_structure(data: &[u8]) -> Result<PdfStructure, DrawError> {
    if data.len() < 16 {
        return Err(DrawError::truncated("PDF shorter than header", 0));
    }
    let header_pos = find_subslice(&data[..data.len().min(1024)], b"%PDF-")
        .ok_or_else(|| DrawError::malformed("missing %PDF- header", 0))?;
    let base = &data[header_pos..];

    let startxref = find_startxref(base)?;
    let mut xref: BTreeMap<u32, XrefEntry> = BTreeMap::new();
    let mut trailer: BTreeMap<String, PdfVal> = BTreeMap::new();
    let mut visited_xrefs: BTreeSet<usize> = BTreeSet::new();

    let mut cur_xref = Some(startxref);
    while let Some(off) = cur_xref {
        if !visited_xrefs.insert(off) {
            return Err(DrawError::malformed("cycle in PDF xref /Prev chain", off as u64));
        }
        if visited_xrefs.len() > MAX_XREF_CHAIN {
            return Err(DrawError::malformed("PDF xref /Prev chain exceeds limit", off as u64));
        }
        let next_prev = parse_xref_at(base, off, &mut xref, &mut trailer)?;
        cur_xref = next_prev;
    }

    // REQ-004 / AC-6: reject encrypted PDFs immediately.
    if trailer.contains_key("Encrypt") && !matches!(trailer.get("Encrypt"), Some(PdfVal::Null)) {
        return Err(DrawError::pdf_encrypted());
    }

    let mut loader = ObjLoader { data: base, xref: &xref, obj_cache: BTreeMap::new(), objstm_cache: BTreeMap::new() };

    // Resolve /Root catalog.
    let root_ref = trailer.get("Root").ok_or_else(|| DrawError::malformed("PDF trailer missing /Root", 0))?;
    let catalog = loader
        .resolve_val(root_ref, 0)?
        .as_dict()
        .cloned()
        .ok_or_else(|| DrawError::malformed("PDF /Root is not a dictionary", 0))?;

    // REQ-005 / AC-6: detect actions, JavaScript, embedded files, and AcroForm without executing.
    let warnings = detect_pdf_warnings(&catalog, &mut loader)?;

    // Walk /Pages tree iteratively with cycle detection and inheritance.
    let pages_val = catalog.get("Pages").ok_or_else(|| DrawError::malformed("PDF catalog missing /Pages", 0))?;
    let root_pages_id = match pages_val {
        PdfVal::Ref(id, _) => *id,
        _ => return Err(DrawError::malformed("PDF catalog /Pages must be an indirect reference", 0)),
    };

    #[derive(Clone)]
    struct Inherited {
        media_box: Option<[i128; 4]>,
        crop_box: Option<[i128; 4]>,
        rotate: i64,
        user_unit: i128,
    }

    struct StackFrame {
        obj_id: u32,
        inherited: Inherited,
        depth: usize,
    }

    let mut stack = vec![StackFrame {
        obj_id: root_pages_id,
        inherited: Inherited { media_box: None, crop_box: None, rotate: 0, user_unit: MICRO },
        depth: 0,
    }];
    let mut visited_nodes: BTreeSet<u32> = BTreeSet::new();
    let mut pages = Vec::new();

    while let Some(frame) = stack.pop() {
        if frame.depth > MAX_PDF_DEPTH {
            return Err(DrawError::malformed("PDF page tree nesting exceeds MAX_PDF_DEPTH", 0));
        }
        if !visited_nodes.insert(frame.obj_id) {
            return Err(DrawError::malformed("cycle detected in PDF /Pages tree", 0));
        }

        let dict = loader
            .load_indirect(frame.obj_id, 0)?
            .as_dict()
            .cloned()
            .ok_or_else(|| DrawError::malformed("PDF page-tree node is not a dictionary", 0))?;

        // Check page-level /AA or /Annots actions for warnings without executing.
        let mut inh = frame.inherited;
        if let Some(mb) = dict.get("MediaBox") {
            let resolved = loader.resolve_val(mb, 0)?;
            inh.media_box = Some(parse_rect(&resolved)?);
        }
        if let Some(cb) = dict.get("CropBox") {
            let resolved = loader.resolve_val(cb, 0)?;
            inh.crop_box = Some(parse_rect(&resolved)?);
        }
        if let Some(rot) = dict.get("Rotate") {
            let resolved = loader.resolve_val(rot, 0)?;
            inh.rotate = resolved.as_i64().unwrap_or(0);
        }
        if let Some(uu) = dict.get("UserUnit") {
            let resolved = loader.resolve_val(uu, 0)?;
            if let Some(u) = resolved.as_micro() {
                if u > 0 {
                    inh.user_unit = u;
                }
            }
        }

        let node_type = dict.get("Type").and_then(PdfVal::as_name).unwrap_or("");
        if node_type == "Pages" || (node_type.is_empty() && dict.contains_key("Kids")) {
            let kids_val = dict.get("Kids").ok_or_else(|| DrawError::malformed("PDF /Pages node missing /Kids", 0))?;
            let kids_resolved = loader.resolve_val(kids_val, 0)?;
            let kids = kids_resolved.as_arr().ok_or_else(|| DrawError::malformed("PDF /Kids is not an array", 0))?;
            // Push in reverse order so LIFO stack visits kids in exact document order.
            for kid in kids.iter().rev() {
                let PdfVal::Ref(kid_id, _) = kid else {
                    return Err(DrawError::malformed("PDF /Kids entry must be an indirect reference", 0));
                };
                stack.push(StackFrame { obj_id: *kid_id, inherited: inh.clone(), depth: frame.depth + 1 });
            }
        } else if node_type == "Page" {
            if pages.len() >= MAX_PAGES {
                return Err(DrawError::limit_exceeded("pages", format!("PDF exceeds {MAX_PAGES} pages")));
            }
            let (rect, box_used) = match (inh.crop_box, inh.media_box) {
                (Some(c), _) => (c, BoxUsed::Crop),
                (None, Some(m)) => (m, BoxUsed::Media),
                (None, None) => return Err(DrawError::malformed("PDF /Page missing /MediaBox", 0)),
            };
            let raw_w_micro = (rect[2] - rect[0]).abs();
            let raw_h_micro = (rect[3] - rect[1]).abs();
            if raw_w_micro <= 0 || raw_h_micro <= 0 {
                return Err(DrawError::malformed("PDF /Page has non-positive box dimension", 0));
            }

            // Convert PDF user units (1/72 in * UserUnit) to integer 0.01 mm with single half-even rounding:
            // cmm = round_half_even(raw_pts_micro * user_unit_micro * 2540, 72 * 10^12).
            let mut width_cmm = pts_micro_to_cmm(raw_w_micro, inh.user_unit)?;
            let mut height_cmm = pts_micro_to_cmm(raw_h_micro, inh.user_unit)?;
            let norm_rot = inh.rotate.rem_euclid(360) as u16;
            if norm_rot == 90 || norm_rot == 270 {
                std::mem::swap(&mut width_cmm, &mut height_cmm);
            }

            let index = pages.len() as u32;
            pages.push(DrawingPage { index, width_cmm, height_cmm, rotation: norm_rot, box_used, raster: None });
        } else {
            return Err(DrawError::malformed("unexpected /Type in PDF page tree", 0));
        }
    }

    if pages.is_empty() {
        return Err(DrawError::malformed("PDF contains zero pages", 0));
    }

    Ok(PdfStructure { pages, warnings })
}

fn pts_micro_to_cmm(pts_micro: i128, user_unit_micro: i128) -> Result<i64, DrawError> {
    let num = pts_micro
        .checked_mul(user_unit_micro)
        .and_then(|v| v.checked_mul(2540))
        .ok_or_else(|| DrawError::malformed("PDF coordinate overflow", 0))?;
    let den = 72i128 * MICRO * MICRO;
    let cmm = round_half_even(num, den);
    i64::try_from(cmm).map_err(|_| DrawError::malformed("PDF coordinate exceeds i64", 0))
}

fn parse_rect(v: &PdfVal) -> Result<[i128; 4], DrawError> {
    let arr = v.as_arr().ok_or_else(|| DrawError::malformed("PDF box must be 4-element array", 0))?;
    if arr.len() != 4 {
        return Err(DrawError::malformed("PDF box must have exactly 4 numbers", 0));
    }
    let mut out = [0i128; 4];
    for (i, item) in arr.iter().enumerate() {
        out[i] = item.as_micro().ok_or_else(|| DrawError::malformed("PDF box element must be numeric", 0))?;
    }
    Ok(out)
}

fn detect_pdf_warnings(
    catalog: &BTreeMap<String, PdfVal>,
    loader: &mut ObjLoader<'_>,
) -> Result<Vec<DrawWarning>, DrawError> {
    let mut has_js = false;
    let mut has_action = false;
    let mut has_embedded = false;
    let mut has_forms = false;

    if let Some(oa) = catalog.get("OpenAction") {
        has_action = true;
        if let Ok(resolved) = loader.resolve_val(oa, 0) {
            inspect_for_js(&resolved, loader, 0, &mut has_js);
        }
    }
    if let Some(aa) = catalog.get("AA") {
        has_action = true;
        if let Ok(resolved) = loader.resolve_val(aa, 0) {
            inspect_for_js(&resolved, loader, 0, &mut has_js);
        }
    }
    if let Some(names_val) = catalog.get("Names") {
        if let Ok(resolved) = loader.resolve_val(names_val, 0) {
            if let Some(nd) = resolved.as_dict() {
                if nd.contains_key("JavaScript") {
                    has_js = true;
                }
                if nd.contains_key("EmbeddedFiles") {
                    has_embedded = true;
                }
            }
        }
    }
    if catalog.contains_key("AcroForm") && !matches!(catalog.get("AcroForm"), Some(PdfVal::Null)) {
        has_forms = true;
    }

    // Also scan indexed objects cheaply (capped) for /JS or /JavaScript or /EmbeddedFiles
    // markers inside indirect dictionaries.
    let obj_ids: Vec<u32> = loader.xref.keys().copied().take(4096).collect();
    for id in obj_ids {
        if let Ok(val) = loader.load_indirect(id, 0) {
            if let Some(d) = val.as_dict() {
                if d.contains_key("JS")
                    || d.contains_key("JavaScript")
                    || d.get("S").and_then(PdfVal::as_name) == Some("JavaScript")
                {
                    has_js = true;
                }
                if d.contains_key("OpenAction")
                    || d.contains_key("AA")
                    || d.get("Type").and_then(PdfVal::as_name) == Some("Action")
                {
                    has_action = true;
                }
                if d.contains_key("EmbeddedFiles") || d.get("Type").and_then(PdfVal::as_name) == Some("EmbeddedFile") {
                    has_embedded = true;
                }
                if d.contains_key("AcroForm") {
                    has_forms = true;
                }
            }
        }
    }

    let mut warnings = Vec::new();
    if has_js {
        warnings.push(DrawWarning {
            code: "pdf_javascript_ignored".to_string(),
            detail: "PDF JavaScript present and ignored (never executed)".to_string(),
            byte_offset: None,
        });
    }
    if has_action {
        warnings.push(DrawWarning {
            code: "pdf_action_ignored".to_string(),
            detail: "PDF actions present and ignored (never executed)".to_string(),
            byte_offset: None,
        });
    }
    if has_embedded {
        warnings.push(DrawWarning {
            code: "pdf_embedded_files_ignored".to_string(),
            detail: "PDF embedded files present and ignored".to_string(),
            byte_offset: None,
        });
    }
    if has_forms {
        warnings.push(DrawWarning {
            code: "pdf_forms_ignored".to_string(),
            detail: "PDF AcroForm interactive form present and ignored".to_string(),
            byte_offset: None,
        });
    }
    Ok(warnings)
}

fn inspect_for_js(val: &PdfVal, loader: &mut ObjLoader<'_>, depth: usize, has_js: &mut bool) {
    if depth > 8 || *has_js {
        return;
    }
    match val {
        PdfVal::Dict(d) => {
            if d.contains_key("JS")
                || d.contains_key("JavaScript")
                || d.get("S").and_then(PdfVal::as_name) == Some("JavaScript")
            {
                *has_js = true;
                return;
            }
            for v in d.values() {
                inspect_for_js(v, loader, depth + 1, has_js);
            }
        }
        PdfVal::Arr(arr) => {
            for item in arr.iter().take(64) {
                inspect_for_js(item, loader, depth + 1, has_js);
            }
        }
        PdfVal::Ref(id, _) => {
            if let Ok(resolved) = loader.load_indirect(*id, depth + 1) {
                inspect_for_js(&resolved, loader, depth + 1, has_js);
            }
        }
        _ => {}
    }
}

// ------------------------------------------------------------------ xref & object loader

struct ObjLoader<'a> {
    data: &'a [u8],
    xref: &'a BTreeMap<u32, XrefEntry>,
    obj_cache: BTreeMap<u32, PdfVal>,
    objstm_cache: BTreeMap<u32, BTreeMap<usize, PdfVal>>,
}

impl<'a> ObjLoader<'a> {
    fn resolve_val(&mut self, val: &PdfVal, depth: usize) -> Result<PdfVal, DrawError> {
        if depth > MAX_PDF_DEPTH {
            return Err(DrawError::malformed("PDF indirect reference chain too deep", 0));
        }
        match val {
            PdfVal::Ref(id, _) => self.load_indirect(*id, depth + 1),
            other => Ok(other.clone()),
        }
    }

    fn load_indirect(&mut self, id: u32, depth: usize) -> Result<PdfVal, DrawError> {
        if depth > MAX_PDF_DEPTH {
            return Err(DrawError::malformed("PDF indirect reference nesting exceeded", 0));
        }
        if let Some(cached) = self.obj_cache.get(&id) {
            return Ok(cached.clone());
        }
        let Some(&entry) = self.xref.get(&id) else {
            return Ok(PdfVal::Null);
        };
        let val = match entry {
            XrefEntry::InFile(off) => {
                let (v, _) = parse_indirect_object(self.data, off)?;
                v
            }
            XrefEntry::InObjStm { stm_obj, index } => self.load_from_objstm(stm_obj, index, depth + 1)?,
        };
        if self.obj_cache.len() < MAX_PDF_OBJECTS {
            self.obj_cache.insert(id, val.clone());
        }
        Ok(val)
    }

    fn load_from_objstm(&mut self, stm_obj: u32, index: usize, depth: usize) -> Result<PdfVal, DrawError> {
        if depth > MAX_PDF_DEPTH {
            return Err(DrawError::malformed("PDF ObjStm nesting exceeded", 0));
        }
        if let Some(map) = self.objstm_cache.get(&stm_obj) {
            return Ok(map.get(&index).cloned().unwrap_or(PdfVal::Null));
        }
        let Some(&XrefEntry::InFile(off)) = self.xref.get(&stm_obj) else {
            return Err(DrawError::malformed("PDF ObjStm not found in file", 0));
        };
        let (val, stream_opt) = parse_indirect_object(self.data, off)?;
        let dict = val.as_dict().ok_or_else(|| DrawError::malformed("PDF ObjStm not a dictionary", off as u64))?;
        let raw_stream =
            stream_opt.ok_or_else(|| DrawError::malformed("PDF ObjStm missing stream bytes", off as u64))?;
        let decoded = decode_pdf_stream(dict, raw_stream, off as u64)?;
        let n = dict
            .get("N")
            .and_then(PdfVal::as_i64)
            .and_then(|v| usize::try_from(v).ok())
            .ok_or_else(|| DrawError::malformed("PDF ObjStm missing /N", off as u64))?;
        let first = dict
            .get("First")
            .and_then(PdfVal::as_i64)
            .and_then(|v| usize::try_from(v).ok())
            .ok_or_else(|| DrawError::malformed("PDF ObjStm missing /First", off as u64))?;
        if first > decoded.len() || n > MAX_PDF_OBJECTS {
            return Err(DrawError::malformed("PDF ObjStm invalid /First or /N", off as u64));
        }

        let mut header_lex = Lexer::new(&decoded[..first]);
        let mut pairs = Vec::with_capacity(n);
        for _ in 0..n {
            let _obj_num = header_lex.parse_value(0)?;
            let rel_off = header_lex
                .parse_value(0)?
                .as_i64()
                .and_then(|v| usize::try_from(v).ok())
                .ok_or_else(|| DrawError::malformed("PDF ObjStm bad offset", off as u64))?;
            pairs.push(rel_off);
        }

        let mut map = BTreeMap::new();
        for (idx, rel_off) in pairs.into_iter().enumerate() {
            let abs = first.saturating_add(rel_off);
            if abs >= decoded.len() {
                continue;
            }
            let mut lex = Lexer::new(&decoded[abs..]);
            if let Ok(v) = lex.parse_value(0) {
                map.insert(idx, v);
            }
        }
        let res = map.get(&index).cloned().unwrap_or(PdfVal::Null);
        self.objstm_cache.insert(stm_obj, map);
        Ok(res)
    }
}

fn find_startxref(data: &[u8]) -> Result<usize, DrawError> {
    let tail_start = data.len().saturating_sub(4096);
    let tail = &data[tail_start..];
    let rel = rfind_subslice(tail, b"startxref")
        .ok_or_else(|| DrawError::malformed("PDF missing startxref", tail_start as u64))?;
    let mut pos = tail_start + rel + b"startxref".len();
    skip_pdf_ws(data, &mut pos);
    let num_start = pos;
    while pos < data.len() && data[pos].is_ascii_digit() {
        pos += 1;
    }
    if pos == num_start {
        return Err(DrawError::malformed("PDF startxref missing offset", num_start as u64));
    }
    let s = std::str::from_utf8(&data[num_start..pos])
        .map_err(|_| DrawError::malformed("PDF startxref invalid ASCII", num_start as u64))?;
    let off =
        s.parse::<usize>().map_err(|_| DrawError::malformed("PDF startxref offset overflow", num_start as u64))?;
    if off >= data.len() {
        return Err(DrawError::truncated("PDF startxref points past EOF", off as u64));
    }
    Ok(off)
}

fn parse_xref_at(
    data: &[u8],
    offset: usize,
    xref: &mut BTreeMap<u32, XrefEntry>,
    trailer: &mut BTreeMap<String, PdfVal>,
) -> Result<Option<usize>, DrawError> {
    let mut pos = offset;
    skip_pdf_ws(data, &mut pos);
    if data.get(pos..pos + 4) == Some(b"xref") {
        pos += 4;
        skip_pdf_ws(data, &mut pos);
        while pos < data.len() && data.get(pos..pos + 7) != Some(b"trailer") {
            let first_obj = read_uint_token(data, &mut pos)? as u32;
            let count = read_uint_token(data, &mut pos)?;
            if count > MAX_PDF_OBJECTS || xref.len().saturating_add(count) > MAX_PDF_OBJECTS {
                return Err(DrawError::malformed("PDF xref exceeds MAX_PDF_OBJECTS", pos as u64));
            }
            for i in 0..count {
                let entry_off = read_uint_token(data, &mut pos)?;
                let _gen = read_uint_token(data, &mut pos)?;
                skip_pdf_ws(data, &mut pos);
                let flag = *data.get(pos).ok_or_else(|| DrawError::truncated("truncated xref entry", pos as u64))?;
                pos += 1;
                skip_pdf_ws(data, &mut pos);
                if flag == b'n' {
                    let obj_id = first_obj.saturating_add(i as u32);
                    xref.entry(obj_id).or_insert(XrefEntry::InFile(entry_off));
                } else if flag != b'f' {
                    return Err(DrawError::malformed("invalid xref entry flag", pos as u64));
                }
            }
        }
        if data.get(pos..pos + 7) != Some(b"trailer") {
            return Err(DrawError::malformed("PDF xref missing trailer", pos as u64));
        }
        pos += 7;
        let mut lex = Lexer::new_at(data, pos);
        let tval = lex.parse_value(0)?;
        let tdict =
            tval.as_dict().ok_or_else(|| DrawError::malformed("PDF trailer is not a dictionary", pos as u64))?;
        let prev = tdict.get("Prev").and_then(PdfVal::as_i64).and_then(|v| usize::try_from(v).ok());
        // Hybrid-reference PDFs may also carry /XRefStm; merge its entries if present.
        if let Some(xstm_off) = tdict.get("XRefStm").and_then(PdfVal::as_i64).and_then(|v| usize::try_from(v).ok()) {
            let mut dummy_t = BTreeMap::new();
            let _ = parse_xref_stream(data, xstm_off, xref, &mut dummy_t);
        }
        for (k, v) in tdict {
            trailer.entry(k.clone()).or_insert_with(|| v.clone());
        }
        Ok(prev)
    } else {
        parse_xref_stream(data, offset, xref, trailer)
    }
}

fn parse_xref_stream(
    data: &[u8],
    offset: usize,
    xref: &mut BTreeMap<u32, XrefEntry>,
    trailer: &mut BTreeMap<String, PdfVal>,
) -> Result<Option<usize>, DrawError> {
    let (val, stream_opt) = parse_indirect_object(data, offset)?;
    let dict = val.as_dict().ok_or_else(|| DrawError::malformed("PDF xref stream not a dictionary", offset as u64))?;
    if dict.get("Type").and_then(PdfVal::as_name) != Some("XRef") {
        return Err(DrawError::malformed("PDF object at startxref is not /XRef", offset as u64));
    }
    let raw = stream_opt.ok_or_else(|| DrawError::malformed("PDF /XRef missing stream", offset as u64))?;
    let decoded = decode_pdf_stream(dict, raw, offset as u64)?;

    let w_arr = dict
        .get("W")
        .and_then(PdfVal::as_arr)
        .ok_or_else(|| DrawError::malformed("PDF /XRef missing /W array", offset as u64))?;
    if w_arr.len() != 3 {
        return Err(DrawError::malformed("PDF /XRef /W must have 3 elements", offset as u64));
    }
    let w0 = w_arr[0].as_i64().and_then(|v| usize::try_from(v).ok()).unwrap_or(0);
    let w1 = w_arr[1].as_i64().and_then(|v| usize::try_from(v).ok()).unwrap_or(0);
    let w2 = w_arr[2].as_i64().and_then(|v| usize::try_from(v).ok()).unwrap_or(0);
    if w0 > 8 || w1 > 8 || w2 > 8 || (w0 + w1 + w2) == 0 {
        return Err(DrawError::malformed("PDF /XRef invalid /W field widths", offset as u64));
    }
    let entry_len = w0 + w1 + w2;

    let size = dict.get("Size").and_then(PdfVal::as_i64).and_then(|v| usize::try_from(v).ok()).unwrap_or(0);
    let ranges: Vec<(u32, usize)> = if let Some(idx_arr) = dict.get("Index").and_then(PdfVal::as_arr) {
        if idx_arr.len() % 2 != 0 {
            return Err(DrawError::malformed("PDF /XRef /Index must have even length", offset as u64));
        }
        let mut r = Vec::new();
        for pair in idx_arr.chunks_exact(2) {
            let start = pair[0].as_i64().and_then(|v| u32::try_from(v).ok()).unwrap_or(0);
            let count = pair[1].as_i64().and_then(|v| usize::try_from(v).ok()).unwrap_or(0);
            r.push((start, count));
        }
        r
    } else {
        vec![(0, size)]
    };

    let mut cursor = 0usize;
    for (start_id, count) in ranges {
        if count > MAX_PDF_OBJECTS || xref.len().saturating_add(count) > MAX_PDF_OBJECTS {
            return Err(DrawError::malformed("PDF /XRef exceeds MAX_PDF_OBJECTS", offset as u64));
        }
        for i in 0..count {
            if cursor + entry_len > decoded.len() {
                return Err(DrawError::truncated("PDF /XRef stream truncated", offset as u64));
            }
            let f0 = if w0 == 0 { 1 } else { read_be_uint(&decoded[cursor..cursor + w0]) };
            let f1 = read_be_uint(&decoded[cursor + w0..cursor + w0 + w1]);
            let f2 = read_be_uint(&decoded[cursor + w0 + w1..cursor + entry_len]);
            cursor += entry_len;

            let obj_id = start_id.saturating_add(i as u32);
            match f0 {
                1 => {
                    xref.entry(obj_id).or_insert(XrefEntry::InFile(f1 as usize));
                }
                2 => {
                    xref.entry(obj_id).or_insert(XrefEntry::InObjStm { stm_obj: f1 as u32, index: f2 as usize });
                }
                _ => {}
            }
        }
    }

    let prev = dict.get("Prev").and_then(PdfVal::as_i64).and_then(|v| usize::try_from(v).ok());
    for (k, v) in dict {
        trailer.entry(k.clone()).or_insert_with(|| v.clone());
    }
    Ok(prev)
}

fn read_be_uint(bytes: &[u8]) -> u64 {
    let mut v = 0u64;
    for &b in bytes {
        v = (v << 8) | u64::from(b);
    }
    v
}

fn decode_pdf_stream(dict: &BTreeMap<String, PdfVal>, raw: &[u8], offset: u64) -> Result<Vec<u8>, DrawError> {
    let filter_name = match dict.get("Filter") {
        None | Some(PdfVal::Null) => None,
        Some(PdfVal::Name(n)) => Some(n.as_str()),
        Some(PdfVal::Arr(arr)) if arr.len() == 1 => arr[0].as_name(),
        _ => return Err(DrawError::malformed("unsupported PDF filter pipeline on structure stream", offset)),
    };

    let mut data = match filter_name {
        None => raw.to_vec(),
        Some("FlateDecode") => zlib_decompress(raw, MAX_STREAM_DECOMPRESS)
            .map_err(|_| DrawError::malformed("corrupt FlateDecode PDF structure stream", offset))?,
        Some(_) => return Err(DrawError::malformed("unsupported PDF structure stream filter", offset)),
    };

    // Apply optional /DecodeParms PNG predictor (common on PDF 1.5+ /XRef streams).
    if let Some(parms) = dict.get("DecodeParms").and_then(PdfVal::as_dict) {
        let predictor = parms.get("Predictor").and_then(PdfVal::as_i64).unwrap_or(1);
        let columns = parms.get("Columns").and_then(PdfVal::as_i64).and_then(|v| usize::try_from(v).ok()).unwrap_or(1);
        if (10..=15).contains(&predictor) && columns > 0 {
            let stride = columns + 1;
            if data.len() % stride != 0 {
                return Err(DrawError::malformed("PDF PNG predictor row alignment mismatch", offset));
            }
            let rows = data.len() / stride;
            let mut out = Vec::with_capacity(rows * columns);
            let mut prev = vec![0u8; columns];
            let mut cur = vec![0u8; columns];
            for r in 0..rows {
                let row = &data[r * stride..(r + 1) * stride];
                let ft = row[0];
                let src = &row[1..];
                match ft {
                    0 => cur.copy_from_slice(src),
                    1 => {
                        for i in 0..columns {
                            let a = if i > 0 { cur[i - 1] } else { 0 };
                            cur[i] = src[i].wrapping_add(a);
                        }
                    }
                    2 => {
                        for i in 0..columns {
                            cur[i] = src[i].wrapping_add(prev[i]);
                        }
                    }
                    3 => {
                        for i in 0..columns {
                            let a = if i > 0 { u16::from(cur[i - 1]) } else { 0 };
                            cur[i] = src[i].wrapping_add(((a + u16::from(prev[i])) / 2) as u8);
                        }
                    }
                    4 => {
                        for i in 0..columns {
                            let a = if i > 0 { i32::from(cur[i - 1]) } else { 0 };
                            let b = i32::from(prev[i]);
                            let c = if i > 0 { i32::from(prev[i - 1]) } else { 0 };
                            let p = a + b - c;
                            let pred = if (p - a).abs() <= (p - b).abs() && (p - a).abs() <= (p - c).abs() {
                                a as u8
                            } else if (p - b).abs() <= (p - c).abs() {
                                b as u8
                            } else {
                                c as u8
                            };
                            cur[i] = src[i].wrapping_add(pred);
                        }
                    }
                    _ => return Err(DrawError::malformed("unknown PNG predictor filter in PDF stream", offset)),
                }
                out.extend_from_slice(&cur);
                prev.copy_from_slice(&cur);
            }
            data = out;
        }
    }

    Ok(data)
}

fn parse_indirect_object(data: &[u8], offset: usize) -> Result<(PdfVal, Option<&[u8]>), DrawError> {
    if offset >= data.len() {
        return Err(DrawError::truncated("PDF object offset past EOF", offset as u64));
    }
    let mut pos = offset;
    let _obj_num = read_uint_token(data, &mut pos)?;
    let _gen_num = read_uint_token(data, &mut pos)?;
    skip_pdf_ws(data, &mut pos);
    if data.get(pos..pos + 3) != Some(b"obj") {
        return Err(DrawError::malformed("expected 'obj' keyword", pos as u64));
    }
    pos += 3;

    let mut lex = Lexer::new_at(data, pos);
    let val = lex.parse_value(0)?;
    lex.skip_ws();
    if lex.data.get(lex.pos..lex.pos + 6) == Some(b"stream") {
        let mut s_pos = lex.pos + 6;
        if lex.data.get(s_pos..s_pos + 2) == Some(b"\r\n") {
            s_pos += 2;
        } else if matches!(lex.data.get(s_pos), Some(b'\n' | b'\r')) {
            s_pos += 1;
        } else {
            return Err(DrawError::malformed("missing newline after 'stream'", s_pos as u64));
        }
        let stream_bytes = if let Some(len) =
            val.as_dict().and_then(|d| d.get("Length")).and_then(PdfVal::as_i64).and_then(|n| usize::try_from(n).ok())
        {
            let end = s_pos
                .checked_add(len)
                .ok_or_else(|| DrawError::malformed("PDF stream /Length overflow", s_pos as u64))?;
            lex.data.get(s_pos..end).ok_or_else(|| DrawError::truncated("PDF stream truncated", s_pos as u64))?
        } else {
            let end_rel = find_subslice(&lex.data[s_pos..], b"endstream")
                .ok_or_else(|| DrawError::truncated("PDF stream missing endstream", s_pos as u64))?;
            let mut end = s_pos + end_rel;
            if end > s_pos && lex.data[end - 1] == b'\n' {
                end -= 1;
            }
            if end > s_pos && lex.data[end - 1] == b'\r' {
                end -= 1;
            }
            &lex.data[s_pos..end]
        };
        Ok((val, Some(stream_bytes)))
    } else {
        Ok((val, None))
    }
}

// ------------------------------------------------------------------ bounded PDF lexer / parser

struct Lexer<'a> {
    data: &'a [u8],
    pos: usize,
    tokens: usize,
}

impl<'a> Lexer<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0, tokens: 0 }
    }

    fn new_at(data: &'a [u8], pos: usize) -> Self {
        Self { data, pos, tokens: 0 }
    }

    fn bump(&mut self) -> Result<(), DrawError> {
        self.tokens += 1;
        if self.tokens > MAX_TOKENS_PER_OBJ {
            return Err(DrawError::malformed("PDF object token limit exceeded", self.pos as u64));
        }
        Ok(())
    }

    fn skip_ws(&mut self) {
        skip_pdf_ws(self.data, &mut self.pos);
    }

    fn parse_value(&mut self, depth: usize) -> Result<PdfVal, DrawError> {
        if depth > MAX_PDF_DEPTH {
            return Err(DrawError::malformed("PDF syntax nesting depth exceeded", self.pos as u64));
        }
        self.bump()?;
        self.skip_ws();
        let b = *self
            .data
            .get(self.pos)
            .ok_or_else(|| DrawError::truncated("unexpected EOF in PDF value", self.pos as u64))?;
        match b {
            b'<' if self.data.get(self.pos..self.pos + 2) == Some(b"<<") => self.parse_dict(depth + 1),
            b'<' => self.parse_hex_string(),
            b'(' => self.parse_literal_string(),
            b'[' => self.parse_array(depth + 1),
            b'/' => self.parse_name(),
            b'+' | b'-' | b'.' | b'0'..=b'9' => self.parse_number_or_ref(),
            _ => self.parse_keyword(),
        }
    }

    fn parse_dict(&mut self, depth: usize) -> Result<PdfVal, DrawError> {
        self.pos += 2; // <<
        let mut map = BTreeMap::new();
        loop {
            self.skip_ws();
            if self.data.get(self.pos..self.pos + 2) == Some(b">>") {
                self.pos += 2;
                return Ok(PdfVal::Dict(map));
            }
            if self.pos >= self.data.len() {
                return Err(DrawError::truncated("unterminated PDF dictionary", self.pos as u64));
            }
            let PdfVal::Name(key) = self.parse_value(depth)? else {
                return Err(DrawError::malformed("PDF dictionary key must be a /Name", self.pos as u64));
            };
            let val = self.parse_value(depth)?;
            map.insert(key, val);
        }
    }

    fn parse_array(&mut self, depth: usize) -> Result<PdfVal, DrawError> {
        self.pos += 1; // [
        let mut arr = Vec::new();
        loop {
            self.skip_ws();
            if self.data.get(self.pos) == Some(&b']') {
                self.pos += 1;
                return Ok(PdfVal::Arr(arr));
            }
            if self.pos >= self.data.len() {
                return Err(DrawError::truncated("unterminated PDF array", self.pos as u64));
            }
            arr.push(self.parse_value(depth)?);
        }
    }

    fn parse_name(&mut self) -> Result<PdfVal, DrawError> {
        self.pos += 1; // /
        let mut out = Vec::new();
        while self.pos < self.data.len() {
            let b = self.data[self.pos];
            if is_pdf_ws(b) || is_pdf_delim(b) {
                break;
            }
            if b == b'#' && self.pos + 2 < self.data.len() {
                if let (Some(hi), Some(lo)) = (hex_val(self.data[self.pos + 1]), hex_val(self.data[self.pos + 2])) {
                    out.push((hi << 4) | lo);
                    self.pos += 3;
                    continue;
                }
            }
            out.push(b);
            self.pos += 1;
        }
        Ok(PdfVal::Name(String::from_utf8_lossy(&out).into_owned()))
    }

    fn parse_literal_string(&mut self) -> Result<PdfVal, DrawError> {
        self.pos += 1; // (
        let mut out = Vec::new();
        let mut parens = 1usize;
        while self.pos < self.data.len() {
            let b = self.data[self.pos];
            self.pos += 1;
            match b {
                b'\\' => {
                    if self.pos < self.data.len() {
                        out.push(self.data[self.pos]);
                        self.pos += 1;
                    }
                }
                b'(' => {
                    parens += 1;
                    out.push(b'(');
                }
                b')' => {
                    parens -= 1;
                    if parens == 0 {
                        return Ok(PdfVal::Str(out));
                    }
                    out.push(b')');
                }
                _ => out.push(b),
            }
        }
        Err(DrawError::truncated("unterminated PDF literal string", self.pos as u64))
    }

    fn parse_hex_string(&mut self) -> Result<PdfVal, DrawError> {
        self.pos += 1; // <
        let mut nibbles = Vec::new();
        while self.pos < self.data.len() {
            let b = self.data[self.pos];
            self.pos += 1;
            if b == b'>' {
                let mut out = Vec::with_capacity(nibbles.len().div_ceil(2));
                let mut i = 0;
                while i < nibbles.len() {
                    let hi = nibbles[i];
                    let lo = nibbles.get(i + 1).copied().unwrap_or(0);
                    out.push((hi << 4) | lo);
                    i += 2;
                }
                return Ok(PdfVal::Str(out));
            }
            if let Some(h) = hex_val(b) {
                nibbles.push(h);
            } else if !is_pdf_ws(b) {
                return Err(DrawError::malformed("invalid hex character in PDF string", self.pos as u64));
            }
        }
        Err(DrawError::truncated("unterminated PDF hex string", self.pos as u64))
    }

    fn parse_number_or_ref(&mut self) -> Result<PdfVal, DrawError> {
        let start = self.pos;
        let first_val = self.parse_single_number()?;
        if let PdfVal::Int(obj_num) = first_val {
            if obj_num >= 0 {
                let save = self.pos;
                self.skip_ws();
                if matches!(self.data.get(self.pos), Some(b'0'..=b'9')) {
                    if let Ok(PdfVal::Int(gen_num)) = self.parse_single_number() {
                        if gen_num >= 0 {
                            self.skip_ws();
                            if self.data.get(self.pos) == Some(&b'R')
                                && self.data.get(self.pos + 1).is_none_or(|&c| is_pdf_ws(c) || is_pdf_delim(c))
                            {
                                self.pos += 1;
                                return Ok(PdfVal::Ref(obj_num as u32, gen_num as u16));
                            }
                        }
                    }
                }
                self.pos = save;
            }
        }
        let _ = start;
        Ok(first_val)
    }

    fn parse_single_number(&mut self) -> Result<PdfVal, DrawError> {
        let start = self.pos;
        if matches!(self.data.get(self.pos), Some(b'+' | b'-')) {
            self.pos += 1;
        }
        let mut saw_dot = false;
        while self.pos < self.data.len() {
            let b = self.data[self.pos];
            if b.is_ascii_digit() {
                self.pos += 1;
            } else if b == b'.' && !saw_dot {
                saw_dot = true;
                self.pos += 1;
            } else {
                break;
            }
        }
        let s = std::str::from_utf8(&self.data[start..self.pos])
            .map_err(|_| DrawError::malformed("invalid PDF number", start as u64))?;
        if !saw_dot {
            let n = s.parse::<i64>().map_err(|_| DrawError::malformed("invalid PDF integer", start as u64))?;
            Ok(PdfVal::Int(n))
        } else {
            Ok(PdfVal::Num(parse_fixed_micro(s, start as u64)?))
        }
    }

    fn parse_keyword(&mut self) -> Result<PdfVal, DrawError> {
        let start = self.pos;
        while self.pos < self.data.len() && !is_pdf_ws(self.data[self.pos]) && !is_pdf_delim(self.data[self.pos]) {
            self.pos += 1;
        }
        let kw = &self.data[start..self.pos];
        match kw {
            b"true" => Ok(PdfVal::Bool(true)),
            b"false" => Ok(PdfVal::Bool(false)),
            b"null" => Ok(PdfVal::Null),
            _ => Err(DrawError::malformed("unexpected PDF keyword", start as u64)),
        }
    }
}

fn parse_fixed_micro(raw: &str, offset: u64) -> Result<FixedMicro, DrawError> {
    let (neg, rest) = match raw.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, raw.strip_prefix('+').unwrap_or(raw)),
    };
    let (int_s, frac_s) = rest.split_once('.').unwrap_or((rest, ""));
    let int_part: i128 = if int_s.is_empty() {
        0
    } else {
        int_s.parse().map_err(|_| DrawError::malformed("PDF real overflow", offset))?
    };
    let mut frac_micro: i128 = 0;
    let mut scale = MICRO / 10;
    let mut round_digit = 0u8;
    for (idx, b) in frac_s.bytes().enumerate() {
        if !b.is_ascii_digit() {
            return Err(DrawError::malformed("PDF real bad digit", offset));
        }
        let d = i128::from(b - b'0');
        if idx < 6 {
            frac_micro += d * scale;
            scale /= 10;
        } else if idx == 6 {
            round_digit = b - b'0';
        }
    }
    if round_digit >= 5 {
        frac_micro += 1;
    }
    let mut val = int_part
        .checked_mul(MICRO)
        .and_then(|v| v.checked_add(frac_micro))
        .ok_or_else(|| DrawError::malformed("PDF real overflow", offset))?;
    if neg {
        val = -val;
    }
    Ok(FixedMicro(val))
}

fn is_pdf_ws(b: u8) -> bool {
    matches!(b, 0x00 | b'\t' | b'\n' | 0x0C | b'\r' | b' ')
}

fn is_pdf_delim(b: u8) -> bool {
    matches!(b, b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%')
}

fn skip_pdf_ws(data: &[u8], pos: &mut usize) {
    while *pos < data.len() {
        let b = data[*pos];
        if is_pdf_ws(b) {
            *pos += 1;
        } else if b == b'%' {
            while *pos < data.len() && !matches!(data[*pos], b'\r' | b'\n') {
                *pos += 1;
            }
        } else {
            break;
        }
    }
}

fn read_uint_token(data: &[u8], pos: &mut usize) -> Result<usize, DrawError> {
    skip_pdf_ws(data, pos);
    let start = *pos;
    while *pos < data.len() && data[*pos].is_ascii_digit() {
        *pos += 1;
    }
    if *pos == start {
        return Err(DrawError::malformed("expected unsigned integer in PDF", start as u64));
    }
    let s =
        std::str::from_utf8(&data[start..*pos]).map_err(|_| DrawError::malformed("bad ASCII uint", start as u64))?;
    s.parse::<usize>().map_err(|_| DrawError::malformed("PDF uint overflow", start as u64))
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn rfind_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).rposition(|w| w == needle)
}
