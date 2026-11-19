//! Deterministic PDF text output: number formatting, escaping, text objects, the font
//! objects of a subset, and a minimal one-page document used to check text extraction.
//!
//! Glyphs are written as hex strings of subset glyph IDs (`/Identity-H` encoding with
//! `/CIDToGIDMap /Identity`), so no shaped string ever appears as a PDF literal and no text
//! needs escaping. [`escape_literal`] and [`escape_name`] are for the untrusted strings the
//! writer (`RPT.OUTPUT.PDF.WRITE`) places elsewhere, and `/BaseFont` names go through
//! [`escape_name`] too.
//!
//! [`single_page`] is the smallest complete document around the text objects. It is not the
//! report writer: page layout, metadata and the document ID belong to
//! `RPT.OUTPUT.PDF.LAYOUT` and `RPT.OUTPUT.PDF.WRITE`.

use super::error::TextError;
use super::shape::ShapedRun;
use super::subset::Subset;
use super::{round_half_even, MAX_SIZE_UM};

/// Most subsets (fonts) in one [`single_page`] document.
pub const MAX_PAGE_FONTS: usize = 64;
/// Most runs in one [`single_page`] document.
pub const MAX_PAGE_RUNS: usize = 100_000;
/// Largest coordinate or page size accepted, in micrometres (10 m).
pub const MAX_COORD_UM: i64 = 10_000_000;

/// Micrometres to thousandths of a PDF point: `round_half_even(um * 72000 / 25400)`.
pub fn milli_points(um: i64) -> i64 {
    round_half_even(i128::from(um) * 360, 127) as i64
}

/// Writes thousandths as a decimal with exactly three fraction digits: `-0.500`, `12.000`.
pub fn fmt_milli(v: i64) -> String {
    let sign = if v < 0 { "-" } else { "" };
    let a = v.unsigned_abs();
    format!("{sign}{}.{:03}", a / 1000, a % 1000)
}

/// A PDF name object for `s`, with the leading `/`. Bytes outside `!`..`~`, `#` and the PDF
/// delimiters are written as `#XX` (PDF 32000-1 §7.3.5). An empty name becomes `/`.
pub fn escape_name(s: &str) -> String {
    let mut out = String::from("/");
    for &b in s.as_bytes() {
        let plain = (0x21..=0x7E).contains(&b) && !b"#()<>[]{}/%".contains(&b);
        if plain {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("#{b:02X}"));
        }
    }
    out
}

/// A PDF literal string for `s` (UTF-8 bytes), with the parentheses: `\`, `(` and `)` are
/// backslash-escaped, and every byte outside printable ASCII is written as a three-digit
/// octal escape, so the result is plain ASCII with no line breaks.
pub fn escape_literal(s: &str) -> String {
    let mut out = String::from("(");
    for &b in s.as_bytes() {
        match b {
            b'\\' => out.push_str("\\\\"),
            b'(' => out.push_str("\\("),
            b')' => out.push_str("\\)"),
            0x20..=0x7E => out.push(char::from(b)),
            _ => out.push_str(&format!("\\{b:03o}")),
        }
    }
    out.push(')');
    out
}

fn check_coord(v: i64) -> Result<(), TextError> {
    if (-MAX_COORD_UM..=MAX_COORD_UM).contains(&v) {
        Ok(())
    } else {
        Err(TextError::Limit { what: "coordinate outside MAX_COORD_UM" })
    }
}

/// One text object showing `run` with font resource `resource` (for example `F1`) at
/// (`x_um`, `y_um`) from the page origin, byte for byte:
///
/// ```text
/// BT
/// /F1 <size pt> Tf
/// <x pt> <y pt> Td
/// <GGGG...> Tj
/// ET
/// ```
///
/// Sizes and coordinates are [`milli_points`] written with [`fmt_milli`]; `GGGG` are the
/// subset glyph IDs in upper-case hex.
pub fn text_object(
    resource: &str,
    subset: &Subset,
    run: &ShapedRun,
    x_um: i64,
    y_um: i64,
) -> Result<Vec<u8>, TextError> {
    check_coord(x_um)?;
    check_coord(y_um)?;
    if !(1..=MAX_SIZE_UM).contains(&run.size_um) {
        return Err(TextError::Limit { what: "font size outside 1..=MAX_SIZE_UM" });
    }
    let ids = subset.encode_run(run)?;
    let mut s = String::with_capacity(48 + 4 * ids.len());
    s.push_str("BT\n");
    s.push_str(&format!("{} {} Tf\n", escape_name(resource), fmt_milli(milli_points(run.size_um))));
    s.push_str(&format!("{} {} Td\n<", fmt_milli(milli_points(x_um)), fmt_milli(milli_points(y_um))));
    for g in ids {
        s.push_str(&format!("{g:04X}"));
    }
    s.push_str("> Tj\nET\n");
    Ok(s.into_bytes())
}

/// A run placed on the page.
#[derive(Debug, Clone, Copy)]
pub struct PlacedRun<'a> {
    /// The shaped run.
    pub run: &'a ShapedRun,
    /// Left of the run, from the page's left edge, in micrometres.
    pub x_um: i64,
    /// Baseline, from the page's bottom edge, in micrometres.
    pub y_um: i64,
}

struct Doc {
    out: Vec<u8>,
    offsets: Vec<usize>,
}

impl Doc {
    fn obj(&mut self, body: &[u8]) {
        self.offsets.push(self.out.len());
        let n = self.offsets.len();
        self.out.extend_from_slice(format!("{n} 0 obj\n").as_bytes());
        self.out.extend_from_slice(body);
        self.out.extend_from_slice(b"\nendobj\n");
    }

    fn stream(&mut self, dict_extra: &str, data: &[u8]) {
        let mut body = format!("<< /Length {}{dict_extra} >>\nstream\n", data.len()).into_bytes();
        body.extend_from_slice(data);
        body.extend_from_slice(b"\nendstream");
        self.obj(&body);
    }
}

/// A complete one-page PDF 1.7 document showing `runs` with the fonts of `subsets`
/// (resource `/F1` is `subsets[0]`, and so on; a run uses the first subset of its font).
/// Deterministic: no dates, no `/Info`, no `/ID`, uncompressed streams, fixed object order:
///
/// 1 catalog, 2 page tree, 3 page, 4 content stream, then per subset k (from 0) the objects
/// `5+5k` Type0 font, `6+5k` CIDFontType2, `7+5k` font descriptor, `8+5k` FontFile2 and
/// `9+5k` ToUnicode CMap; then the cross-reference table and trailer.
pub fn single_page(
    width_um: i64,
    height_um: i64,
    subsets: &[&Subset],
    runs: &[PlacedRun<'_>],
) -> Result<Vec<u8>, TextError> {
    if subsets.is_empty() || subsets.len() > MAX_PAGE_FONTS {
        return Err(TextError::Limit { what: "1 to MAX_PAGE_FONTS subsets" });
    }
    if runs.len() > MAX_PAGE_RUNS {
        return Err(TextError::Limit { what: "more than MAX_PAGE_RUNS runs" });
    }
    if !(1..=MAX_COORD_UM).contains(&width_um) || !(1..=MAX_COORD_UM).contains(&height_um) {
        return Err(TextError::Limit { what: "page size outside 1..=MAX_COORD_UM" });
    }
    let mut content = Vec::new();
    for pr in runs {
        let k = subsets
            .iter()
            .position(|s| s.font == pr.run.font)
            .ok_or_else(|| TextError::FontUnknown { font: pr.run.font.as_str().to_string() })?;
        content.extend_from_slice(&text_object(&format!("F{}", k + 1), subsets[k], pr.run, pr.x_um, pr.y_um)?);
    }

    let mut d = Doc { out: Vec::new(), offsets: Vec::new() };
    d.out.extend_from_slice(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n");
    d.obj(b"<< /Type /Catalog /Pages 2 0 R >>");
    d.obj(b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
    let fonts: Vec<String> = (0..subsets.len()).map(|k| format!("/F{} {} 0 R", k + 1, 5 + 5 * k)).collect();
    let page = format!(
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources << /Font << {} >> >> /Contents 4 0 R >>",
        fmt_milli(milli_points(width_um)),
        fmt_milli(milli_points(height_um)),
        fonts.join(" ")
    );
    d.obj(page.as_bytes());
    d.stream("", &content);
    for (k, s) in subsets.iter().enumerate() {
        let i = 5 + 5 * k;
        let base = escape_name(&s.base_font());
        d.obj(
            format!("<< /Type /Font /Subtype /Type0 /BaseFont {base} /Encoding /Identity-H /DescendantFonts [{} 0 R] /ToUnicode {} 0 R >>", i + 1, i + 4)
                .as_bytes(),
        );
        let widths: Vec<String> = s.widths().iter().map(|w| w.to_string()).collect();
        d.obj(
            format!(
                "<< /Type /Font /Subtype /CIDFontType2 /BaseFont {base} /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /FontDescriptor {} 0 R /DW 0 /W [0 [{}]] /CIDToGIDMap /Identity >>",
                i + 2,
                widths.join(" ")
            )
            .as_bytes(),
        );
        let b = s.bbox();
        d.obj(
            format!(
                "<< /Type /FontDescriptor /FontName {base} /Flags 4 /FontBBox [{} {} {} {}] /ItalicAngle 0 /Ascent {} /Descent {} /CapHeight {} /StemV 80 /FontFile2 {} 0 R >>",
                b[0],
                b[1],
                b[2],
                b[3],
                s.ascent(),
                s.descent(),
                s.ascent(),
                i + 3
            )
            .as_bytes(),
        );
        d.stream(&format!(" /Length1 {}", s.bytes.len()), &s.bytes);
        d.stream("", s.to_unicode.bytes());
    }
    let xref = d.out.len();
    let n = d.offsets.len() + 1;
    let mut tail = format!("xref\n0 {n}\n0000000000 65535 f \n");
    for o in &d.offsets {
        tail.push_str(&format!("{o:010} 00000 n \n"));
    }
    tail.push_str(&format!("trailer\n<< /Size {n} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n"));
    d.out.extend_from_slice(tail.as_bytes());
    Ok(d.out)
}
