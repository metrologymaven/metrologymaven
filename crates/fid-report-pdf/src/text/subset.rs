//! Deterministic font subsetting (REQ-RPT.OUTPUT.PDF.TEXT-004).
//!
//! The subset holds exactly the used glyphs plus `.notdef` (plus, for composite glyphs, the
//! components they reference), renumbered in ascending original-ID order, so glyph 0 stays
//! `.notdef`. Tables, in tag order: `cvt `, `fpgm`, `glyf`, `head`, `hhea`, `hmtx`, `loca`,
//! `maxp`, `prep` (hinting tables only when the source has them) - the set PDF 32000-1
//! §9.9 needs for an embedded TrueType program. Changes against the source tables:
//! `head.checkSumAdjustment` recomputed and `head.indexToLocFormat` = 1 (long `loca`);
//! `hhea.numberOfHMetrics` and `maxp.numGlyphs` = the subset's glyph count; `hmtx` holds one
//! full metric per glyph; composite component IDs are renumbered. Each glyph's bytes are
//! copied and zero-padded to a multiple of 4. The subset tag is six letters `A`-`Z`: letter
//! `i` is `A + (d[i] mod 26)` where `d` is the SHA-256 of the sorted original glyph IDs of
//! the subset, each as a big-endian u16.

use std::collections::{BTreeMap, BTreeSet};

use sha2::{Digest as _, Sha256};

use super::cmap::CMap;
use super::error::TextError;
use super::manifest::FontId;
use super::round_half_even;
use super::sfnt::{components, rd16};
use super::shape::{FontSet, LoadedFont, ShapedRun};

/// Deepest composite nesting followed.
const MAX_COMPONENT_DEPTH: usize = 16;

/// Collects the glyphs a document uses in one font.
#[derive(Debug, Clone)]
pub struct SubsetBuilder<'a> {
    font: &'a LoadedFont,
    /// Original glyph ID -> the character it shows (lowest code point on a conflict).
    used: BTreeMap<u16, Option<char>>,
}

/// An embeddable font subset with its ToUnicode map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subset {
    /// The source font.
    pub font: FontId,
    /// Six upper-case letters derived from the glyph set.
    pub tag: [u8; 6],
    /// The subset TrueType font program.
    pub bytes: Vec<u8>,
    /// The ToUnicode map, keyed by subset glyph ID.
    pub to_unicode: CMap,
    postscript_name: String,
    /// Original glyph IDs in subset order (index = subset glyph ID).
    originals: Vec<u16>,
    /// Advance widths in PDF glyph space (1/1000 em), by subset glyph ID.
    widths: Vec<i64>,
    /// Font bounding box, ascent and descent in PDF glyph space.
    bbox: [i64; 4],
    ascent: i64,
    descent: i64,
}

impl<'a> SubsetBuilder<'a> {
    /// A builder for `font` of `set`, holding `.notdef` already.
    pub fn new(set: &'a FontSet, font: &FontId) -> Result<SubsetBuilder<'a>, TextError> {
        let font = set.font(font).ok_or_else(|| TextError::FontUnknown { font: font.as_str().to_string() })?;
        let mut used = BTreeMap::new();
        used.insert(0u16, None);
        Ok(SubsetBuilder { font, used })
    }

    /// Adds the glyphs of a run of this builder's font, remembering each glyph's character
    /// for the ToUnicode map (`.notdef` never gets one).
    pub fn add_run(&mut self, run: &ShapedRun) -> Result<(), TextError> {
        if run.font != self.font.entry().id {
            return Err(TextError::FontUnknown { font: run.font.as_str().to_string() });
        }
        for g in &run.glyphs {
            self.add_glyph(g.glyph, run.char_of(g))?;
        }
        Ok(())
    }

    /// Adds one glyph, optionally with the character it shows. When two characters share a
    /// glyph, the lower code point is kept.
    pub fn add_glyph(&mut self, glyph: u16, c: Option<char>) -> Result<(), TextError> {
        if glyph >= self.font.glyph_count() {
            return Err(TextError::GlyphNotInSubset { font: self.font.entry().id.as_str().to_string(), glyph });
        }
        let c = if glyph == 0 { None } else { c };
        let slot = self.used.entry(glyph).or_insert(None);
        *slot = match (*slot, c) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        Ok(())
    }

    /// Number of distinct glyphs added so far, `.notdef` included.
    pub fn len(&self) -> usize {
        self.used.len()
    }

    /// Always false: `.notdef` is always present.
    pub fn is_empty(&self) -> bool {
        self.used.is_empty()
    }

    /// Builds the subset. The same glyph set always gives byte-identical output.
    pub fn finish(&self) -> Result<Subset, TextError> {
        let font = self.font;
        let id = &font.entry().id;
        let d = font.data();
        let p = font.parsed();

        // Composite closure.
        let mut set: BTreeSet<u16> = self.used.keys().copied().collect();
        let mut frontier: Vec<u16> = set.iter().copied().collect();
        let mut depth = 0;
        while !frontier.is_empty() {
            if depth > MAX_COMPONENT_DEPTH {
                return Err(TextError::invalid(id, "composite_depth"));
            }
            let mut next = Vec::new();
            for g in frontier {
                for (_, c) in components(p.glyph_data(d, g)).map_err(|w| TextError::invalid(id, w))? {
                    if c >= p.num_glyphs {
                        return Err(TextError::invalid(id, "bad_component"));
                    }
                    if set.insert(c) {
                        next.push(c);
                    }
                }
            }
            frontier = next;
            depth += 1;
        }
        let originals: Vec<u16> = set.iter().copied().collect();
        let new_id: BTreeMap<u16, u16> = originals.iter().enumerate().map(|(i, &g)| (g, i as u16)).collect();
        let n = originals.len();

        // glyf and loca
        let mut glyf = Vec::new();
        let mut loca = Vec::with_capacity(n + 1);
        for &g in &originals {
            loca.push(glyf.len() as u32);
            let src = p.glyph_data(d, g);
            let mut bytes = src.to_vec();
            for (pos, c) in components(src).map_err(|w| TextError::invalid(id, w))? {
                let nc = new_id.get(&c).copied().ok_or_else(|| TextError::invalid(id, "bad_component"))?;
                if let Some(slot) = bytes.get_mut(pos..pos + 2) {
                    slot.copy_from_slice(&nc.to_be_bytes());
                }
            }
            glyf.extend_from_slice(&bytes);
            while glyf.len() % 4 != 0 {
                glyf.push(0);
            }
        }
        loca.push(glyf.len() as u32);
        let loca_b: Vec<u8> = loca.iter().flat_map(|o| o.to_be_bytes()).collect();

        let mut hmtx = Vec::with_capacity(4 * n);
        for &g in &originals {
            let (adv, lsb) = p.metrics.get(usize::from(g)).copied().unwrap_or((0, 0));
            hmtx.extend_from_slice(&adv.to_be_bytes());
            hmtx.extend_from_slice(&lsb.to_be_bytes());
        }

        let mut head = p.head.of(d).to_vec();
        let mut hhea = p.hhea.of(d).to_vec();
        let mut maxp = p.maxp.of(d).to_vec();
        if head.len() < 54 || hhea.len() < 36 || maxp.len() < 6 || rd16(&maxp, 4).is_none() {
            return Err(TextError::invalid(id, "bad_tables"));
        }
        head[8..12].copy_from_slice(&[0, 0, 0, 0]);
        head[50..52].copy_from_slice(&1u16.to_be_bytes());
        hhea[34..36].copy_from_slice(&(n as u16).to_be_bytes());
        maxp[4..6].copy_from_slice(&(n as u16).to_be_bytes());

        let mut tables: Vec<([u8; 4], Vec<u8>)> = vec![
            (*b"glyf", glyf),
            (*b"head", head),
            (*b"hhea", hhea),
            (*b"hmtx", hmtx),
            (*b"loca", loca_b),
            (*b"maxp", maxp),
        ];
        for (tag, span) in &p.hinting {
            tables.push((*tag, span.of(d).to_vec()));
        }
        tables.sort_by_key(|t| t.0);
        let bytes = assemble(&tables);

        // Tag
        let mut h = Sha256::new();
        for g in &originals {
            h.update(g.to_be_bytes());
        }
        let digest: [u8; 32] = h.finalize().into();
        let mut tag = [0u8; 6];
        for (t, b) in tag.iter_mut().zip(digest.iter()) {
            *t = b'A' + (b % 26);
        }

        // ToUnicode, keyed by subset glyph ID
        let mut entries = Vec::new();
        for (g, c) in &self.used {
            if let (Some(c), Some(&ng)) = (c, new_id.get(g)) {
                entries.push((ng, *c));
            }
        }
        let to_unicode = CMap::build(entries);

        let upem = i128::from(p.upem);
        let scale = |v: i128| round_half_even(v * 1000, upem) as i64;
        let widths = originals.iter().map(|&g| scale(i128::from(font.advance_units(g)))).collect();
        let bbox = [scale(p.bbox[0].into()), scale(p.bbox[1].into()), scale(p.bbox[2].into()), scale(p.bbox[3].into())];
        Ok(Subset {
            font: id.clone(),
            tag,
            bytes,
            to_unicode,
            postscript_name: font.entry().postscript_name.clone(),
            originals,
            widths,
            bbox,
            ascent: scale(p.ascender.into()),
            descent: scale(p.descender.into()),
        })
    }
}

fn checksum(b: &[u8]) -> u32 {
    let mut s = 0u32;
    for chunk in b.chunks(4) {
        let mut w = [0u8; 4];
        w[..chunk.len()].copy_from_slice(chunk);
        s = s.wrapping_add(u32::from_be_bytes(w));
    }
    s
}

/// Writes an sfnt from tables already sorted by tag; `head` must have a zero
/// checkSumAdjustment, which is then set.
fn assemble(tables: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let n = tables.len();
    let mut es = 0u16;
    while (1usize << (es + 1)) <= n {
        es += 1;
    }
    let sr = (1u16 << es) * 16;
    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&(n as u16).to_be_bytes());
    out.extend_from_slice(&sr.to_be_bytes());
    out.extend_from_slice(&es.to_be_bytes());
    out.extend_from_slice(&((n as u16) * 16 - sr).to_be_bytes());
    let mut offset = 12 + 16 * n;
    let mut head_off = None;
    for (tag, data) in tables {
        out.extend_from_slice(tag);
        out.extend_from_slice(&checksum(data).to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        if tag == b"head" {
            head_off = Some(offset);
        }
        offset += data.len().div_ceil(4) * 4;
    }
    for (_, data) in tables {
        out.extend_from_slice(data);
        while out.len() % 4 != 0 {
            out.push(0);
        }
    }
    if let Some(h) = head_off {
        let adj = 0xB1B0_AFBAu32.wrapping_sub(checksum(&out));
        if let Some(slot) = out.get_mut(h + 8..h + 12) {
            slot.copy_from_slice(&adj.to_be_bytes());
        }
    }
    out
}

impl Subset {
    /// The subset tag as text.
    pub fn tag_str(&self) -> String {
        self.tag.iter().map(|&b| char::from(b)).collect()
    }

    /// `TAG+PostScriptName`, the PDF `/BaseFont` name.
    pub fn base_font(&self) -> String {
        format!("{}+{}", self.tag_str(), self.postscript_name)
    }

    /// Number of glyphs, `.notdef` included.
    pub fn glyph_count(&self) -> usize {
        self.originals.len()
    }

    /// The original glyph IDs in subset order.
    pub fn originals(&self) -> &[u16] {
        &self.originals
    }

    /// The subset glyph ID of an original glyph ID.
    pub fn new_id(&self, original: u16) -> Option<u16> {
        self.originals.binary_search(&original).ok().map(|i| i as u16)
    }

    /// Subset glyph IDs of a run, which must use this subset's font.
    pub fn encode_run(&self, run: &ShapedRun) -> Result<Vec<u16>, TextError> {
        if run.font != self.font {
            return Err(TextError::FontUnknown { font: run.font.as_str().to_string() });
        }
        run.glyphs
            .iter()
            .map(|g| {
                self.new_id(g.glyph)
                    .ok_or_else(|| TextError::GlyphNotInSubset { font: self.font.as_str().to_string(), glyph: g.glyph })
            })
            .collect()
    }

    /// Advance widths in PDF glyph space (1/1000 em), by subset glyph ID.
    pub fn widths(&self) -> &[i64] {
        &self.widths
    }

    /// Font bounding box in PDF glyph space.
    pub fn bbox(&self) -> [i64; 4] {
        self.bbox
    }

    /// Ascent in PDF glyph space.
    pub fn ascent(&self) -> i64 {
        self.ascent
    }

    /// Descent in PDF glyph space (negative below the baseline).
    pub fn descent(&self) -> i64 {
        self.descent
    }
}
