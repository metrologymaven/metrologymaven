//! Loaded fonts, font selection and deterministic shaping (REQ-RPT.OUTPUT.PDF.TEXT-001 ..
//! -003, -006).

use fid_num::fmt::Reported;
use fid_types::Digest;
use sha2::{Digest as _, Sha256};

use super::error::{TextError, TextWarning};
use super::manifest::{bundled_files, bundled_manifest, FontEntry, FontId, FontManifest, Slant, MAX_FONTS};
use super::sfnt::{self, Parsed};
use super::{round_half_even, MAX_FONT_BYTES, MAX_SIZE_UM, MAX_TEXT_BYTES};

/// A font file that matched its manifest digest and parsed as a usable TrueType font.
#[derive(Debug, Clone)]
pub struct LoadedFont {
    entry: FontEntry,
    data: Vec<u8>,
    parsed: Parsed,
}

impl LoadedFont {
    /// Verifies `bytes` against the entry's SHA-256 (before any parsing), then parses them.
    /// Fails with `report.font_invalid` on a size or digest mismatch or a malformed font.
    pub fn load(entry: &FontEntry, bytes: &[u8]) -> Result<LoadedFont, TextError> {
        if bytes.len() > MAX_FONT_BYTES {
            return Err(TextError::invalid(&entry.id, "too_large"));
        }
        let digest = Digest::from_bytes(Sha256::digest(bytes).into());
        if digest != entry.sha256 {
            return Err(TextError::invalid(&entry.id, "digest_mismatch"));
        }
        let parsed = sfnt::parse(bytes).map_err(|why| TextError::invalid(&entry.id, why))?;
        Ok(LoadedFont { entry: entry.clone(), data: bytes.to_vec(), parsed })
    }

    /// The manifest entry.
    pub fn entry(&self) -> &FontEntry {
        &self.entry
    }

    /// Units per em.
    pub fn units_per_em(&self) -> u16 {
        self.parsed.upem
    }

    /// Number of glyphs, `.notdef` included.
    pub fn glyph_count(&self) -> u16 {
        self.parsed.num_glyphs
    }

    /// The glyph for a character, `None` when the font has none.
    pub fn glyph_for(&self, c: char) -> Option<u16> {
        match self.parsed.glyph_for(&self.data, u32::from(c)) {
            0 => None,
            g => Some(g),
        }
    }

    /// Advance width of a glyph in font units (0 for an unknown glyph).
    pub fn advance_units(&self, glyph: u16) -> u16 {
        self.parsed.metrics.get(usize::from(glyph)).map_or(0, |m| m.0)
    }

    pub(crate) fn data(&self) -> &[u8] {
        &self.data
    }

    pub(crate) fn parsed(&self) -> &Parsed {
        &self.parsed
    }
}

/// One shaped glyph: glyph ID, advance in micrometres, and the byte offset (in the shaped
/// string) of the character it shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShapedGlyph {
    /// Glyph ID in the run's font.
    pub glyph: u16,
    /// Advance in micrometres (font units scaled to the size, rounded half to even).
    pub advance_um: i64,
    /// Byte offset of the character in the shaped string.
    pub cluster: u32,
}

/// A maximal sequence of glyphs from one font.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShapedRun {
    /// The font.
    pub font: FontId,
    /// Font size (em height) in micrometres.
    pub size_um: i64,
    /// The glyphs, one per character, in string order.
    pub glyphs: Vec<ShapedGlyph>,
    /// Byte offset of the run's first character in the shaped string.
    pub start: u32,
    /// The characters the run shows, exactly as given.
    pub text: String,
}

impl ShapedRun {
    /// The sum of the advances, in micrometres.
    pub fn advance_um(&self) -> i64 {
        self.glyphs.iter().map(|g| g.advance_um).sum()
    }

    /// The character a glyph of this run shows.
    pub fn char_of(&self, g: &ShapedGlyph) -> Option<char> {
        let rel = g.cluster.checked_sub(self.start)? as usize;
        self.text.get(rel..)?.chars().next()
    }
}

/// The result of shaping a string: runs in string order and any warnings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shaped {
    /// The runs.
    pub runs: Vec<ShapedRun>,
    /// `report.glyph_missing` warnings, in string order.
    pub warnings: Vec<TextWarning>,
}

impl Shaped {
    /// Total advance in micrometres.
    pub fn advance_um(&self) -> i64 {
        self.runs.iter().map(ShapedRun::advance_um).sum()
    }

    /// The characters of all runs, concatenated: always the shaped string.
    pub fn text(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect()
    }
}

/// The default style names used when no `RPT.DESIGN.STYLE` style set is present.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultStyle {
    /// Body text: sans, weight 400.
    Body,
    /// Headings and emphasis: sans, weight 700.
    Strong,
    /// Code and fixed-width tables: mono, weight 400.
    Mono,
    /// Bold fixed-width: mono, weight 700.
    MonoStrong,
}

/// A set of loaded fonts with a fallback order.
#[derive(Debug, Clone)]
pub struct FontSet {
    fonts: Vec<LoadedFont>,
    fallback: Vec<usize>,
}

impl FontSet {
    /// Loads every font of the manifest from `files` (manifest ID, bytes). Each file is
    /// verified by SHA-256 before use; any mismatch fails the whole load with
    /// `report.font_invalid` (AC-3).
    pub fn load(manifest: &FontManifest, files: &[(&str, &[u8])]) -> Result<FontSet, TextError> {
        manifest.validate()?;
        for (i, (id, _)) in files.iter().enumerate() {
            if files[..i].iter().any(|(x, _)| x == id) {
                return Err(TextError::ManifestInvalid { reason: format!("file for {id} given twice") });
            }
            if !manifest.fonts.iter().any(|f| f.id.as_str() == *id) {
                return Err(TextError::ManifestInvalid { reason: format!("file for unknown font {id}") });
            }
        }
        let mut fonts = Vec::with_capacity(manifest.fonts.len());
        for e in &manifest.fonts {
            let bytes = files
                .iter()
                .find(|(id, _)| *id == e.id.as_str())
                .map(|(_, b)| *b)
                .ok_or_else(|| TextError::ManifestInvalid { reason: format!("no file for font {}", e.id.as_str()) })?;
            fonts.push(LoadedFont::load(e, bytes)?);
        }
        let mut fallback = Vec::with_capacity(manifest.fallback.len());
        for id in &manifest.fallback {
            if let Some(i) = fonts.iter().position(|f| &f.entry.id == id) {
                fallback.push(i);
            }
        }
        Ok(FontSet { fonts, fallback })
    }

    /// The bundled font set (sans and mono, regular and bold), compiled into the crate.
    pub fn bundled() -> Result<FontSet, TextError> {
        FontSet::load(&bundled_manifest()?, &bundled_files())
    }

    /// The bundled set plus script fonts published by `I18N.SCRIPT` (for example CJK). The
    /// script fonts come after the bundled ones in the fallback order.
    pub fn bundled_with_script_fonts(script: &FontManifest, files: &[(&str, &[u8])]) -> Result<FontSet, TextError> {
        let mut m = bundled_manifest()?;
        if m.fonts.len() + script.fonts.len() > MAX_FONTS {
            return Err(TextError::Limit { what: "too many fonts" });
        }
        m.fonts.extend(script.fonts.iter().cloned());
        m.fallback.extend(script.fallback.iter().cloned());
        let mut all: Vec<(&str, &[u8])> = bundled_files();
        all.extend(files.iter().copied());
        FontSet::load(&m, &all)
    }

    /// The fonts in manifest order.
    pub fn fonts(&self) -> &[LoadedFont] {
        &self.fonts
    }

    /// A font by ID.
    pub fn font(&self, id: &FontId) -> Option<&LoadedFont> {
        self.fonts.iter().find(|f| &f.entry.id == id)
    }

    fn index(&self, id: &FontId) -> Result<usize, TextError> {
        self.fonts.iter().position(|f| &f.entry.id == id).ok_or_else(|| TextError::FontUnknown { font: id.as_str().to_string() })
    }

    /// The first font (manifest order) with this family, weight and slant.
    pub fn select(&self, family: &str, weight: u16, style: Slant) -> Option<&FontId> {
        self.fonts.iter().find(|f| f.entry.family == family && f.entry.weight == weight && f.entry.style == style).map(|f| &f.entry.id)
    }

    /// The font of a default style (bundled families).
    pub fn default_style(&self, s: DefaultStyle) -> Option<&FontId> {
        let (family, weight) = match s {
            DefaultStyle::Body => ("Maven Report Sans", 400),
            DefaultStyle::Strong => ("Maven Report Sans", 700),
            DefaultStyle::Mono => ("Maven Report Mono", 400),
            DefaultStyle::MonoStrong => ("Maven Report Mono", 700),
        };
        self.select(family, weight, Slant::Upright)
    }

    /// Shapes `text` in `font` at `size_um` (em height in micrometres, 1 to 1 000 000).
    ///
    /// Every character becomes exactly one glyph: the selected font's glyph if it has one,
    /// else the first font of the fallback order that has one, else the selected font's
    /// `.notdef` with a `report.glyph_missing` warning. Advances are
    /// `round_half_even(advance_units * size_um / units_per_em)`, integer arithmetic only,
    /// so every platform gives the same result. Characters are never changed: the runs'
    /// texts concatenate to `text`.
    pub fn shape(&self, font: &FontId, text: &str, size_um: i64) -> Result<Shaped, TextError> {
        if text.len() > MAX_TEXT_BYTES {
            return Err(TextError::Limit { what: "text longer than MAX_TEXT_BYTES" });
        }
        if !(1..=MAX_SIZE_UM).contains(&size_um) {
            return Err(TextError::Limit { what: "font size outside 1..=MAX_SIZE_UM" });
        }
        let sel = self.index(font)?;
        let mut runs: Vec<ShapedRun> = Vec::new();
        let mut warnings = Vec::new();
        for (pos, c) in text.char_indices() {
            let cluster = pos as u32;
            let mut pick: Option<(usize, u16)> = self.fonts[sel].glyph_for(c).map(|g| (sel, g));
            if pick.is_none() {
                for &i in &self.fallback {
                    if i == sel {
                        continue;
                    }
                    if let Some(g) = self.fonts[i].glyph_for(c) {
                        pick = Some((i, g));
                        break;
                    }
                }
            }
            let (fi, glyph) = match pick {
                Some(p) => p,
                None => {
                    warnings.push(TextWarning::GlyphMissing { code_point: u32::from(c), cluster, font: font.clone() });
                    (sel, 0)
                }
            };
            let f = &self.fonts[fi];
            let adv = i128::from(f.advance_units(glyph)) * i128::from(size_um);
            let advance_um = round_half_even(adv, i128::from(f.units_per_em())) as i64;
            let g = ShapedGlyph { glyph, advance_um, cluster };
            match runs.last_mut() {
                Some(r) if r.font == f.entry.id => {
                    r.glyphs.push(g);
                    r.text.push(c);
                }
                _ => runs.push(ShapedRun { font: f.entry.id.clone(), size_um, glyphs: vec![g], start: cluster, text: c.to_string() }),
            }
        }
        Ok(Shaped { runs, warnings })
    }

    /// Shapes a measured value exactly as the kernel formatter (`KRN.NUM.FMT.LEN`) wrote it.
    /// The value string is never re-formatted, re-rounded or localised here
    /// (REQ-RPT.OUTPUT.PDF-004, REQ-RPT.OUTPUT.PDF.TEXT-006).
    pub fn shape_reported(&self, font: &FontId, value: &Reported, size_um: i64) -> Result<Shaped, TextError> {
        self.shape(font, &value.value, size_um)
    }
}
