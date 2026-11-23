//! Deterministic PDF 1.7 writer and `report.export_pdf@1` export command
//! (spec node `RPT.OUTPUT.PDF.WRITE`).
//!
//! - Byte-identical output on x86-64, ARM64 and wasm32 (`REQ-RPT.OUTPUT.PDF.WRITE-001`, `AC-6`).
//! - Fixed object numbering and dictionary key order:
//!   1. Catalog (`/Type /Catalog /Pages 2 0 R /Metadata 3 0 R`)
//!   2. Page tree (`/Type /Pages /Kids [...] /Count N`)
//!   3. XMP metadata stream carrying canonical `ExportMeta` JSON (no timestamps/host/user)
//!   4. Document `/Info` dictionary carrying `/ExportMeta` and `/Producer` (no dates/host/user)
//!   5. Embedded font subsets in `FontId` order (`Type0`, `CIDFontType2`, `FontDescriptor`,
//!      Flate-compressed `FontFile2`, Flate-compressed `ToUnicode` CMap)
//!   6. Embedded image XObjects in CAS `Digest` order (plus optional `/SMask` stream)
//!   7. Page objects and Flate-compressed content streams in page order
//! - Trailer `/ID [<16B> <16B>]` derived strictly from the first 16 bytes of
//!   `resolved_report.digest` (`REQ-002`, `AC-3`).
//! - On host platforms (`#[cfg(not(target_arch = "wasm32"))]`), [`cmd`] registers
//!   `report.export_pdf@1` (W1, `fs_effect = write_root`, CLI `fid report export-pdf --json`,
//!   MCP `report_export_pdf`) writing exclusively through `RPT.OUTPUT.META`'s `ExportSink`.

#[cfg(not(target_arch = "wasm32"))]
pub mod cmd;

use crate::draw::{paint_document, ImageStore, PaintError, PaintedDocument, PainterRegistry};
use crate::flate::zlib_compress;
use crate::image_codec::PdfImageObject;
use crate::int_json::{obj, to_canonical_bytes, JsonVal};
use crate::layout::{layout_document, LayoutError, MeasureRegistry, ResolvedDoc};
use crate::text::pdf::{escape_literal, escape_name, fmt_milli, milli_points};
use crate::text::{FontId, FontSet, Subset, TextError};
use fid_types::Digest;
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use std::fmt;

/// Renderer identifier recorded in `ExportMeta` (`REQ-RPT.OUTPUT.PDF.WRITE-003`).
pub const PDF_RENDERER_ID: &str = "pdf";
/// Pinned PDF renderer version recorded in `ExportMeta` and `/Producer`.
pub const PDF_RENDERER_VERSION: &str = "1.0.0";
/// Schema tag matching `RPT.OUTPUT.META` (`export_meta@1`).
pub const EXPORT_META_SCHEMA: &str = "export_meta@1";

// ------------------------------------------------------------------ errors

/// Typed errors of deterministic PDF rendering and export (`Design: Errors`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteError {
    /// Error from painting or upfront unsupported-block check (`report.unsupported_block`, etc.).
    Paint(PaintError),
    /// Error from layout (`report.layout_invalid`, `report.resolved_invalid`).
    Layout(LayoutError),
    /// Error from font/text subsystem (`RPT.OUTPUT.PDF.TEXT`).
    Text(TextError),
    /// `report.resolved_not_found`: no resolved report found for the requested digest/ID.
    ResolvedNotFound(String),
    /// `report.bad_params`: malformed command or CLI arguments.
    BadParams(&'static str),
    /// Error from the `RPT.OUTPUT.META` export sink (`raw_path_forbidden`, etc.).
    #[cfg(not(target_arch = "wasm32"))]
    Export(fid_report_out::meta::ExportError),
}

impl WriteError {
    pub fn code(&self) -> &'static str {
        match self {
            WriteError::Paint(e) => e.code(),
            WriteError::Layout(e) => e.code(),
            WriteError::Text(e) => e.code(),
            WriteError::ResolvedNotFound(_) => "report.resolved_not_found",
            WriteError::BadParams(_) => "report.bad_params",
            #[cfg(not(target_arch = "wasm32"))]
            WriteError::Export(e) => e.code(),
        }
    }
}

impl fmt::Display for WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WriteError::Paint(e) => e.fmt(f),
            WriteError::Layout(e) => e.fmt(f),
            WriteError::Text(e) => e.fmt(f),
            WriteError::ResolvedNotFound(id) => write!(f, "report.resolved_not_found: {id}"),
            WriteError::BadParams(r) => write!(f, "report.bad_params: {r}"),
            #[cfg(not(target_arch = "wasm32"))]
            WriteError::Export(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for WriteError {}

impl From<PaintError> for WriteError {
    fn from(e: PaintError) -> WriteError {
        WriteError::Paint(e)
    }
}

impl From<LayoutError> for WriteError {
    fn from(e: LayoutError) -> WriteError {
        WriteError::Layout(e)
    }
}

impl From<TextError> for WriteError {
    fn from(e: TextError) -> WriteError {
        WriteError::Text(e)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl From<fid_report_out::meta::ExportError> for WriteError {
    fn from(e: fid_report_out::meta::ExportError) -> WriteError {
        WriteError::Export(e)
    }
}

// ------------------------------------------------------------------ ExportMeta JSON builder

/// Constructs the RFC 8785 canonical `export_meta@1` JSON bytes from a parsed [`ResolvedDoc`]
/// without touching floats (byte-identical to `fid_report_out::meta::ExportMeta`).
pub fn build_export_meta_json(doc: &ResolvedDoc, renderer_version: &str) -> Vec<u8> {
    let prov = doc.provenance.as_obj();
    let kernel_version =
        prov.and_then(|p| p.get("kernel_version")).and_then(JsonVal::as_str).unwrap_or("0.1.0").to_string();

    let template = prov
        .and_then(|p| p.get("template"))
        .and_then(JsonVal::as_obj)
        .and_then(|t| {
            let id = t.get("id").and_then(JsonVal::as_str)?;
            let version = match t.get("version")? {
                JsonVal::Str(s) => s.clone(),
                JsonVal::Int(n) => n.to_string(),
                _ => return None,
            };
            Some(obj([("id", JsonVal::Str(id.to_string())), ("version", JsonVal::Str(version))]))
        })
        .unwrap_or(JsonVal::Null);

    let mut fmts: Vec<(String, String)> = Vec::new();
    if let Some(arr) = prov.and_then(|p| p.get("formatters")).and_then(JsonVal::as_arr) {
        for item in arr {
            if let Some(fo) = item.as_obj() {
                if let (Some(id), Some(ver)) =
                    (fo.get("id").and_then(JsonVal::as_str), fo.get("version").and_then(JsonVal::as_str))
                {
                    fmts.push((id.to_string(), ver.to_string()));
                }
            }
        }
    }
    fmts.sort();
    fmts.dedup();
    let formatters_json: Vec<JsonVal> = fmts
        .into_iter()
        .map(|(id, version)| obj([("id", JsonVal::Str(id)), ("version", JsonVal::Str(version))]))
        .collect();

    let meta = obj([
        ("formatters", JsonVal::Arr(formatters_json)),
        ("kernel_version", JsonVal::Str(kernel_version)),
        (
            "renderer",
            obj([
                ("id", JsonVal::Str(PDF_RENDERER_ID.to_string())),
                ("version", JsonVal::Str(renderer_version.to_string())),
            ]),
        ),
        ("report_digest", JsonVal::Str(doc.digest.as_str().to_string())),
        ("schema", JsonVal::Str(EXPORT_META_SCHEMA.to_string())),
        ("template", template),
    ]);
    to_canonical_bytes(&meta)
}

// ------------------------------------------------------------------ deterministic PDF 1.7 writer

struct PdfDoc {
    out: Vec<u8>,
    offsets: Vec<usize>,
}

impl PdfDoc {
    fn new() -> PdfDoc {
        let mut out = Vec::new();
        out.extend_from_slice(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n");
        PdfDoc { out, offsets: Vec::new() }
    }

    fn obj(&mut self, body: &[u8]) -> usize {
        self.offsets.push(self.out.len());
        let n = self.offsets.len();
        self.out.extend_from_slice(format!("{n} 0 obj\n").as_bytes());
        self.out.extend_from_slice(body);
        self.out.extend_from_slice(b"\nendobj\n");
        n
    }

    fn stream(&mut self, dict_extra: &str, data: &[u8]) -> usize {
        let mut body = format!("<< /Length {}{dict_extra} >>\nstream\n", data.len()).into_bytes();
        body.extend_from_slice(data);
        body.extend_from_slice(b"\nendstream");
        self.obj(&body)
    }

    fn finish(mut self, root_obj: usize, info_obj: usize, id_hex16: &str) -> Vec<u8> {
        let xref_offset = self.out.len();
        let n = self.offsets.len() + 1;
        let mut tail = format!("xref\n0 {n}\n0000000000 65535 f \n");
        for off in &self.offsets {
            tail.push_str(&format!("{off:010} 00000 n \n"));
        }
        tail.push_str(&format!(
            "trailer\n<< /ID [<{id_hex16}> <{id_hex16}>] /Info {info_obj} 0 R /Root {root_obj} 0 R /Size {n} >>\nstartxref\n{xref_offset}\n%%EOF\n"
        ));
        self.out.extend_from_slice(tail.as_bytes());
        self.out
    }
}

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

fn build_xmp_metadata(export_meta_str: &str) -> Vec<u8> {
    let escaped = xml_escape(export_meta_str);
    format!(
        "<?xpacket begin=\"\u{FEFF}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n\
         <x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n\
         <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n\
         <rdf:Description rdf:about=\"\" xmlns:fid=\"https://metrologymaven.io/ns/export/1/\">\n\
         <fid:ExportMeta>{escaped}</fid:ExportMeta>\n\
         </rdf:Description>\n\
         </rdf:RDF>\n\
         </x:xmpmeta>\n\
         <?xpacket end=\"r\"?>"
    )
    .into_bytes()
}

/// First 16 bytes of `digest` formatted as 32 uppercase ASCII hex characters (`REQ-002`, `AC-3`).
pub fn pdf_id_hex16(digest: &Digest) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let bytes = digest.to_bytes();
    let mut s = String::with_capacity(32);
    for &b in &bytes[..16] {
        s.push(char::from(HEX[usize::from(b >> 4)]));
        s.push(char::from(HEX[usize::from(b & 0x0F)]));
    }
    s
}

/// Serializes `painted` and canonical `export_meta_json` into deterministic PDF 1.7 bytes
/// (`REQ-RPT.OUTPUT.PDF.WRITE-001`..`003`, `AC-1`..`AC-3`, `AC-6`).
pub fn write_pdf(painted: &PaintedDocument, export_meta_json: &[u8]) -> Result<Vec<u8>, WriteError> {
    let meta_str = std::str::from_utf8(export_meta_json).map_err(|_| WriteError::BadParams("non_utf8_export_meta"))?;

    // Compute exact deterministic object numbers up front so Catalog, Pages, Fonts, Images,
    // and Page objects cross-reference cleanly in a single forward pass.
    //
    // Object layout:
    //   1       : Catalog
    //   2       : Pages tree
    //   3       : XMP Metadata stream
    //   4       : Info dictionary
    //   5..     : Fonts (5 objects per font subset in FontId order)
    //   img..   : Images (1 object per image + 1 object if SMask present, in Digest order)
    //   pages.. : Page object + Content stream object per page (2 objects per page)
    let mut next_obj = 5usize;

    let mut font_obj_ids: BTreeMap<FontId, usize> = BTreeMap::new();
    for fid in painted.subsets.keys() {
        font_obj_ids.insert(fid.clone(), next_obj);
        next_obj += 5;
    }

    let mut image_obj_ids: BTreeMap<Digest, (usize, Option<usize>)> = BTreeMap::new();
    for (digest, img) in &painted.images {
        let img_id = next_obj;
        next_obj += 1;
        let smask_id = if img.smask.is_some() {
            let sid = next_obj;
            next_obj += 1;
            Some(sid)
        } else {
            None
        };
        image_obj_ids.insert(*digest, (img_id, smask_id));
    }

    let first_page_obj = next_obj;
    let page_kids: Vec<String> = (0..painted.pages.len()).map(|i| format!("{} 0 R", first_page_obj + 2 * i)).collect();

    let mut doc = PdfDoc::new();

    // 1: Catalog
    let root_obj = doc.obj(b"<< /Metadata 3 0 R /Pages 2 0 R /Type /Catalog >>");
    // 2: Pages tree
    let pages_dict = format!("<< /Count {} /Kids [{}] /Type /Pages >>", painted.pages.len(), page_kids.join(" "));
    doc.obj(pages_dict.as_bytes());
    // 3: XMP Metadata stream (uncompressed per PDF 32000-1 §14.3.2 recommendation for XMP inspectability)
    let xmp = build_xmp_metadata(meta_str);
    doc.stream(" /Subtype /XML /Type /Metadata", &xmp);
    // 4: Info dictionary (NO CreationDate, NO ModDate, NO Author/Host/User)
    let producer = format!("Metrology Maven PDF {PDF_RENDERER_VERSION}");
    let info_dict = format!("<< /ExportMeta {} /Producer {} >>", escape_literal(meta_str), escape_literal(&producer),);
    let info_obj = doc.obj(info_dict.as_bytes());

    // 5..: Embedded font subsets in deterministic FontId order.
    for (fid, subset) in &painted.subsets {
        let base_id = font_obj_ids[fid];
        write_font_subset(&mut doc, base_id, subset)?;
    }

    // Images in deterministic Digest order.
    for (digest, img) in &painted.images {
        let (_, smask_id) = image_obj_ids[digest];
        write_image_xobject(&mut doc, img, smask_id);
    }

    // Pages + Flate-compressed Content Streams in page order.
    for (idx, page) in painted.pages.iter().enumerate() {
        let content_obj = first_page_obj + 2 * idx + 1;
        let mut font_entries = Vec::new();
        for fid in &page.font_resources {
            if let (Some(res_name), Some(&f_obj)) = (painted.font_names.get(fid), font_obj_ids.get(fid)) {
                font_entries.push(format!("{} {} 0 R", escape_name(res_name)?, f_obj));
            }
        }
        let mut xobj_entries = Vec::new();
        for digest in &page.image_digests {
            if let (Some(res_name), Some(&(i_obj, _))) = (painted.image_names.get(digest), image_obj_ids.get(digest)) {
                xobj_entries.push(format!("{} {} 0 R", escape_name(res_name)?, i_obj));
            }
        }

        let mut resources = String::from("<< ");
        if !font_entries.is_empty() {
            resources.push_str("/Font << ");
            resources.push_str(&font_entries.join(" "));
            resources.push_str(" >> ");
        }
        if !xobj_entries.is_empty() {
            resources.push_str("/XObject << ");
            resources.push_str(&xobj_entries.join(" "));
            resources.push_str(" >> ");
        }
        resources.push_str(">>");

        let page_dict = format!(
            "<< /Contents {content_obj} 0 R /MediaBox [0 0 {} {}] /Parent 2 0 R /Resources {resources} /Type /Page >>",
            fmt_milli(milli_points(page.size_um.0)),
            fmt_milli(milli_points(page.size_um.1)),
        );
        doc.obj(page_dict.as_bytes());

        let compressed_content = zlib_compress(&page.content_stream);
        doc.stream(" /Filter /FlateDecode", &compressed_content);
    }

    let id_hex16 = pdf_id_hex16(&painted.resolved_digest);
    Ok(doc.finish(root_obj, info_obj, &id_hex16))
}

fn write_font_subset(doc: &mut PdfDoc, base_id: usize, s: &Subset) -> Result<(), WriteError> {
    let base = escape_name(&s.base_font())?;
    // base_id + 0: Type0 Font
    doc.obj(
        format!(
            "<< /BaseFont {base} /DescendantFonts [{} 0 R] /Encoding /Identity-H /Subtype /Type0 /ToUnicode {} 0 R /Type /Font >>",
            base_id + 1,
            base_id + 4
        )
        .as_bytes(),
    );
    // base_id + 1: CIDFontType2
    let widths: Vec<String> = s.widths().iter().map(|w| w.to_string()).collect();
    doc.obj(
        format!(
            "<< /BaseFont {base} /CIDSystemInfo << /Ordering (Identity) /Registry (Adobe) /Supplement 0 >> /CIDToGIDMap /Identity /DW 0 /FontDescriptor {} 0 R /Subtype /CIDFontType2 /Type /Font /W [0 [{}]] >>",
            base_id + 2,
            widths.join(" ")
        )
        .as_bytes(),
    );
    // base_id + 2: FontDescriptor
    let b = s.bbox();
    doc.obj(
        format!(
            "<< /Ascent {} /CapHeight {} /Descent {} /Flags 4 /FontBBox [{} {} {} {}] /FontFile2 {} 0 R /FontName {base} /ItalicAngle 0 /StemV 80 /Type /FontDescriptor >>",
            s.ascent(),
            s.ascent(),
            s.descent(),
            b[0],
            b[1],
            b[2],
            b[3],
            base_id + 3
        )
        .as_bytes(),
    );
    // base_id + 3: FontFile2 (Flate-compressed subset TrueType program with original Length1)
    let font_compressed = zlib_compress(&s.bytes);
    doc.stream(&format!(" /Filter /FlateDecode /Length1 {}", s.bytes.len()), &font_compressed);
    // base_id + 4: ToUnicode CMap (Flate-compressed)
    let cmap_compressed = zlib_compress(s.to_unicode.bytes());
    doc.stream(" /Filter /FlateDecode", &cmap_compressed);
    Ok(())
}

fn write_image_xobject(doc: &mut PdfDoc, img: &PdfImageObject, smask_id: Option<usize>) {
    let smask_entry = smask_id.map_or(String::new(), |sid| format!(" /SMask {sid} 0 R"));
    let extra = format!(
        " /BitsPerComponent {} /ColorSpace {} /Filter {} /Height {}{} /Subtype /Image /Type /XObject /Width {}",
        img.bits_per_component,
        img.color_space.pdf_name(),
        img.filter.pdf_name(),
        img.height_px,
        smask_entry,
        img.width_px,
    );
    doc.stream(&extra, &img.data);

    if let Some(smask) = &img.smask {
        let smask_extra = format!(
            " /BitsPerComponent 8 /ColorSpace /DeviceGray /Filter /FlateDecode /Height {} /Subtype /Image /Type /XObject /Width {}",
            smask.height_px, smask.width_px
        );
        doc.stream(&smask_extra, &smask.data);
    }
}

// ------------------------------------------------------------------ end-to-end pure renderer

/// Rendered deterministic PDF output with digest and canonical `ExportMeta` bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedPdf {
    pub bytes: Vec<u8>,
    pub sha256: Digest,
    pub size_bytes: u64,
    pub page_count: u32,
    pub resolved_digest: Digest,
    pub export_meta_json: Vec<u8>,
}

/// Pure, wasm32-safe end-to-end deterministic PDF renderer:
/// `resolved_report@1 JSON -> painter check -> layout -> paint -> PDF 1.7 bytes` (`AC-1`, `AC-6`).
pub fn render_pdf_from_json(
    resolved_json: &[u8],
    fonts: &FontSet,
    images: &dyn ImageStore,
    measurers: &MeasureRegistry<'_>,
    painters: &PainterRegistry<'_>,
) -> Result<RenderedPdf, WriteError> {
    let doc = ResolvedDoc::from_json_bytes(resolved_json)?;
    // REQ-005 / AC-4: upfront whole-document painter check before layout or any byte emission.
    painters.check_document(&doc)?;
    let layout = layout_document(&doc, fonts, measurers)?;
    let painted = paint_document(&doc, &layout, fonts, images, painters)?;
    let export_meta_json = build_export_meta_json(&doc, PDF_RENDERER_VERSION);
    let bytes = write_pdf(&painted, &export_meta_json)?;
    let sha256 = Digest::from_bytes(Sha256::digest(&bytes).into());
    let size_bytes = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    let page_count = u32::try_from(painted.pages.len()).unwrap_or(u32::MAX);
    Ok(RenderedPdf { bytes, sha256, size_bytes, page_count, resolved_digest: doc.digest, export_meta_json })
}
