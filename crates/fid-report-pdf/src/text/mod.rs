//! Bundled fonts, text shaping, metrics and font subsetting. Spec node
//! `RPT.OUTPUT.PDF.TEXT`.
//!
//! - Manifest types: [`FontManifest`], [`FontEntry`], [`FontId`]; the bundled
//!   set ([`bundled_manifest`]) is compiled into the crate and every font file is checked
//!   against its SHA-256 before it is parsed. No system font is ever read (REQ-001).
//! - [`FontSet::shape`]: one glyph per character through the font's character map, with
//!   the manifest's fallback order and `.notdef` plus `report.glyph_missing` when no font
//!   has the character (REQ-003). Advances are integers in micrometres, computed with
//!   integer arithmetic only, so x86-64, ARM64 and wasm32 agree (REQ-002, D0).
//! - [`SubsetBuilder`] / [`Subset`]: exactly the used glyphs plus `.notdef`, a stable tag,
//!   byte-identical output for the same glyph set, and a ToUnicode [`CMap`] (REQ-004, -006).
//! - [`pdf`]: deterministic text objects and font objects, and a minimal one-page document.
//! - [`break_opportunities`]: a conservative subset of UAX #14.
//!
//! Measured values are shaped from the kernel formatter's output string as is
//! ([`FontSet::shape_reported`]); nothing here formats, rounds or localises a number.

mod cmap;
mod error;
mod linebreak;
mod manifest;
pub mod pdf;
mod sfnt;
mod shape;
mod subset;

pub use cmap::CMap;
pub use error::{TextError, TextWarning};
pub use linebreak::break_opportunities;
pub use manifest::{
    bundled_font_bytes, bundled_font_file, bundled_manifest, check_licences, spdx_ids, FontEntry, FontId, FontManifest, Slant, BUNDLED_SPDX, MAX_FONTS,
};
pub use shape::{DefaultStyle, FontSet, LoadedFont, Shaped, ShapedGlyph, ShapedRun};
pub use subset::{Subset, SubsetBuilder};

/// Largest font file accepted (32 MiB), checked before hashing.
pub const MAX_FONT_BYTES: usize = 32 * 1024 * 1024;
/// Longest string one shaping call accepts (1 MiB of UTF-8).
pub const MAX_TEXT_BYTES: usize = 1024 * 1024;
/// Largest font size in micrometres (1 m).
pub const MAX_SIZE_UM: i64 = 1_000_000;

/// `n / d` rounded half to even, for `d > 0` (returns 0 for `d <= 0`).
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

#[cfg(test)]
mod tests {
    use super::round_half_even;

    #[test]
    fn half_even() {
        assert_eq!(round_half_even(5, 2), 2);
        assert_eq!(round_half_even(7, 2), 4);
        assert_eq!(round_half_even(-5, 2), -2);
        assert_eq!(round_half_even(-7, 2), -4);
        assert_eq!(round_half_even(10, 3), 3);
        assert_eq!(round_half_even(11, 3), 4);
        assert_eq!(round_half_even(-11, 3), -4);
        assert_eq!(round_half_even(1, 0), 0);
    }
}
