//! Errors and warnings of the text module.

use std::fmt;

use super::manifest::FontId;

/// Why a font, shaping, subsetting or PDF text call failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextError {
    /// `report.font_invalid`: the font file does not match its manifest digest, is too large,
    /// or is not a well-formed TrueType font this module can use. The font is not used.
    FontInvalid {
        /// The manifest ID of the font.
        font: String,
        /// A short machine-readable reason, for example `digest_mismatch`.
        reason: &'static str,
    },
    /// `report.font_invalid`: the manifest itself is malformed (duplicate IDs, unknown
    /// fallback entries, a missing font file, a bad PostScript name).
    ManifestInvalid {
        /// What is wrong.
        reason: String,
    },
    /// `report.font_unknown`: no loaded font has this ID.
    FontUnknown {
        /// The ID asked for.
        font: String,
    },
    /// `report.text_limit`: an input or output bound was exceeded.
    Limit {
        /// Which bound.
        what: &'static str,
    },
    /// `report.pdf_name_invalid`: a PDF name would contain a NUL byte, which PDF forbids.
    PdfNameInvalid,
    /// `report.glyph_not_in_subset`: a run uses a glyph that the subset does not contain.
    GlyphNotInSubset {
        /// The font of the subset.
        font: String,
        /// The original glyph ID.
        glyph: u16,
    },
}

impl TextError {
    /// The stable error code.
    pub fn code(&self) -> &'static str {
        match self {
            TextError::FontInvalid { .. } | TextError::ManifestInvalid { .. } => "report.font_invalid",
            TextError::FontUnknown { .. } => "report.font_unknown",
            TextError::Limit { .. } => "report.text_limit",
            TextError::PdfNameInvalid => "report.pdf_name_invalid",
            TextError::GlyphNotInSubset { .. } => "report.glyph_not_in_subset",
        }
    }

    pub(crate) fn invalid(font: &FontId, reason: &'static str) -> TextError {
        TextError::FontInvalid { font: font.as_str().to_string(), reason }
    }
}

impl fmt::Display for TextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TextError::FontInvalid { font, reason } => write!(f, "{}: font {font}: {reason}", self.code()),
            TextError::ManifestInvalid { reason } => write!(f, "{}: manifest: {reason}", self.code()),
            TextError::FontUnknown { font } => write!(f, "{}: {font}", self.code()),
            TextError::Limit { what } => write!(f, "{}: {what}", self.code()),
            TextError::PdfNameInvalid => write!(f, "{}: NUL byte in a PDF name", self.code()),
            TextError::GlyphNotInSubset { font, glyph } => write!(f, "{}: font {font}: glyph {glyph}", self.code()),
        }
    }
}

impl std::error::Error for TextError {}

/// A warning reported while shaping; shaping still succeeds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextWarning {
    /// `report.glyph_missing`: no font of the selected font and the fallback order has a
    /// glyph for the code point, so the selected font's `.notdef` glyph was used.
    GlyphMissing {
        /// The Unicode scalar value.
        code_point: u32,
        /// Byte offset of the character in the shaped string.
        cluster: u32,
        /// The selected font, whose `.notdef` was used.
        font: FontId,
    },
    /// `report.glyph_shared`: two or more characters use the same glyph of a subset, and a
    /// ToUnicode map holds one character per glyph, so extracting text returns `kept` where
    /// `dropped` was shown.
    GlyphShared {
        /// The subset's font.
        font: FontId,
        /// The original glyph ID.
        glyph: u16,
        /// The character the ToUnicode map keeps (the lowest code point).
        kept: u32,
        /// A character that extracts as `kept`.
        dropped: u32,
    },
}

impl TextWarning {
    /// The stable warning code.
    pub fn code(&self) -> &'static str {
        match self {
            TextWarning::GlyphMissing { .. } => "report.glyph_missing",
            TextWarning::GlyphShared { .. } => "report.glyph_shared",
        }
    }
}

impl fmt::Display for TextWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TextWarning::GlyphMissing { code_point, cluster, font } => {
                write!(f, "{}: U+{code_point:04X} at byte {cluster} (font {})", self.code(), font.as_str())
            }
            TextWarning::GlyphShared { font, glyph, kept, dropped } => write!(
                f,
                "{}: U+{dropped:04X} extracts as U+{kept:04X} (glyph {glyph}, font {})",
                self.code(),
                font.as_str()
            ),
        }
    }
}
