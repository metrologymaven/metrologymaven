//! The font manifest and the bundled font set (REQ-RPT.OUTPUT.PDF.TEXT-001, -005).

use fid_types::Digest;

use super::error::TextError;

/// The ID of a font in a manifest: 1 to 64 characters from `a-z`, `0-9` and `-`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FontId(String);

impl FontId {
    /// Checks and wraps an ID.
    pub fn new(s: &str) -> Result<FontId, TextError> {
        let ok = !s.is_empty()
            && s.len() <= 64
            && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if ok {
            Ok(FontId(s.to_string()))
        } else {
            Err(TextError::ManifestInvalid { reason: format!("bad font id {:?}", truncate(s)) })
        }
    }

    /// The ID as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn truncate(s: &str) -> String {
    s.chars().take(64).collect()
}

/// Upright or italic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Slant {
    /// Upright (roman).
    Upright,
    /// Italic or oblique.
    Italic,
}

/// One font of a manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontEntry {
    /// Manifest ID.
    pub id: FontId,
    /// Family name, for example `Maven Report Sans`.
    pub family: String,
    /// Weight class, 1 to 1000 (400 regular, 700 bold).
    pub weight: u16,
    /// Slant.
    pub style: Slant,
    /// SHA-256 of the font file; checked before the file is parsed.
    pub sha256: Digest,
    /// SPDX licence expression of the font file.
    pub spdx: String,
    /// PostScript name, used in the PDF `/BaseFont` (1 to 63 characters from `A-Z`, `a-z`,
    /// `0-9`, `-` and `_`).
    pub postscript_name: String,
}

/// A font manifest: the fonts and the fallback order used when the selected font has no
/// glyph for a character.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontManifest {
    /// The fonts.
    pub fonts: Vec<FontEntry>,
    /// Fallback order (font IDs).
    pub fallback: Vec<FontId>,
}

/// Most fonts a manifest (or a bundled set merged with a script-font manifest) may hold.
pub const MAX_FONTS: usize = 64;

impl FontManifest {
    /// Checks IDs (unique), fallback entries (known, unique), weights and PostScript names.
    pub fn validate(&self) -> Result<(), TextError> {
        if self.fonts.is_empty() || self.fonts.len() > MAX_FONTS {
            return Err(TextError::ManifestInvalid {
                reason: format!("{} fonts (1 to {MAX_FONTS} allowed)", self.fonts.len()),
            });
        }
        for (i, f) in self.fonts.iter().enumerate() {
            if self.fonts[..i].iter().any(|g| g.id == f.id) {
                return Err(TextError::ManifestInvalid { reason: format!("duplicate font id {}", f.id.as_str()) });
            }
            if f.weight == 0 || f.weight > 1000 {
                return Err(TextError::ManifestInvalid {
                    reason: format!("font {}: weight {}", f.id.as_str(), f.weight),
                });
            }
            let ps = f.postscript_name.as_bytes();
            let ps_ok = !ps.is_empty()
                && ps.len() <= 63
                && ps.iter().all(|b| b.is_ascii_alphanumeric() || *b == b'-' || *b == b'_');
            if !ps_ok {
                return Err(TextError::ManifestInvalid {
                    reason: format!("font {}: bad PostScript name", f.id.as_str()),
                });
            }
        }
        for (i, id) in self.fallback.iter().enumerate() {
            if !self.fonts.iter().any(|f| &f.id == id) {
                return Err(TextError::ManifestInvalid {
                    reason: format!("fallback names unknown font {}", id.as_str()),
                });
            }
            if self.fallback[..i].contains(id) {
                return Err(TextError::ManifestInvalid { reason: format!("fallback lists {} twice", id.as_str()) });
            }
        }
        Ok(())
    }
}

/// SPDX expression of the bundled fonts: they are original work of the project, under the
/// project licence.
pub const BUNDLED_SPDX: &str = "MIT OR Apache-2.0";

/// One bundled font: ID, family, weight, PostScript name, file name, digest, bytes.
struct Bundled {
    id: &'static str,
    family: &'static str,
    weight: u16,
    postscript_name: &'static str,
    file: &'static str,
    sha256: &'static str,
    bytes: &'static [u8],
}

const BUNDLED: [Bundled; 4] = [
    Bundled {
        id: "sans-regular",
        family: "Maven Report Sans",
        weight: 400,
        postscript_name: "MavenReportSans-Regular",
        file: "MavenReportSans-Regular.ttf",
        sha256: "sha256:00f2fe03ad5d918ad455ff3cc04c88759952a20c4804fb424afa3d75a9b1f719",
        bytes: include_bytes!("../../assets/fonts/MavenReportSans-Regular.ttf"),
    },
    Bundled {
        id: "sans-bold",
        family: "Maven Report Sans",
        weight: 700,
        postscript_name: "MavenReportSans-Bold",
        file: "MavenReportSans-Bold.ttf",
        sha256: "sha256:d6ca575f93d6b050e205908fa5c54e5ebdeb27516d4fbd700203ff6dc91fe843",
        bytes: include_bytes!("../../assets/fonts/MavenReportSans-Bold.ttf"),
    },
    Bundled {
        id: "mono-regular",
        family: "Maven Report Mono",
        weight: 400,
        postscript_name: "MavenReportMono-Regular",
        file: "MavenReportMono-Regular.ttf",
        sha256: "sha256:055e0b9797a2dc28f79ee9c56a94ccc9f28c50fd59bc60f9275430e3bea76c5f",
        bytes: include_bytes!("../../assets/fonts/MavenReportMono-Regular.ttf"),
    },
    Bundled {
        id: "mono-bold",
        family: "Maven Report Mono",
        weight: 700,
        postscript_name: "MavenReportMono-Bold",
        file: "MavenReportMono-Bold.ttf",
        sha256: "sha256:01e943e9a54a08ba8b1bc8487ef8cd0b0fde91cd571ad54738301326f4201fb0",
        bytes: include_bytes!("../../assets/fonts/MavenReportMono-Bold.ttf"),
    },
];

/// The bundled font manifest. Fallback order: sans regular, sans bold, mono regular, mono
/// bold. The fonts are compiled into the crate; no system font is ever read.
pub fn bundled_manifest() -> Result<FontManifest, TextError> {
    let mut fonts = Vec::with_capacity(BUNDLED.len());
    let mut fallback = Vec::with_capacity(BUNDLED.len());
    for b in &BUNDLED {
        let id = FontId::new(b.id)?;
        let sha256 = Digest::parse(b.sha256)
            .map_err(|_| TextError::ManifestInvalid { reason: format!("bad digest for {}", b.id) })?;
        fonts.push(FontEntry {
            id: id.clone(),
            family: b.family.to_string(),
            weight: b.weight,
            style: Slant::Upright,
            sha256,
            spdx: BUNDLED_SPDX.to_string(),
            postscript_name: b.postscript_name.to_string(),
        });
        fallback.push(id);
    }
    Ok(FontManifest { fonts, fallback })
}

/// The bytes of a bundled font file, by manifest ID.
pub fn bundled_font_bytes(id: &str) -> Option<&'static [u8]> {
    BUNDLED.iter().find(|b| b.id == id).map(|b| b.bytes)
}

/// The file name (under `crates/fid-report-pdf/assets/fonts/`) of a bundled font.
pub fn bundled_font_file(id: &str) -> Option<&'static str> {
    BUNDLED.iter().find(|b| b.id == id).map(|b| b.file)
}

pub(crate) fn bundled_files() -> Vec<(&'static str, &'static [u8])> {
    BUNDLED.iter().map(|b| (b.id, b.bytes)).collect()
}

/// The licence IDs of an SPDX expression: every token except `OR`, `AND` and parentheses.
/// A `WITH` exception is kept as the token `WITH` and the exception ID, so it is never
/// mistaken for an allowed licence.
pub fn spdx_ids(expr: &str) -> Vec<&str> {
    expr.split(|c: char| c.is_whitespace() || c == '(' || c == ')')
        .filter(|t| !t.is_empty() && *t != "OR" && *t != "AND")
        .collect()
}

/// AC-5: every licence ID of every font's SPDX expression must be in `allow` (all of them,
/// even for `OR`, so that no choice is needed). Returns the offending (font, ID) pairs.
pub fn check_licences(manifest: &FontManifest, allow: &[&str]) -> Result<(), Vec<(FontId, String)>> {
    let mut bad = Vec::new();
    for f in &manifest.fonts {
        let ids = spdx_ids(&f.spdx);
        if ids.is_empty() {
            bad.push((f.id.clone(), String::new()));
        }
        for id in ids {
            if !allow.contains(&id) {
                bad.push((f.id.clone(), id.to_string()));
            }
        }
    }
    if bad.is_empty() {
        Ok(())
    } else {
        Err(bad)
    }
}
