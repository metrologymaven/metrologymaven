//! PDF report renderer of the Report app. Spec node `RPT.OUTPUT.PDF`.
//!
//! This crate holds the renderer's first leaf, `RPT.OUTPUT.PDF.TEXT` (module [`text`]):
//! the bundled font set, deterministic shaping to glyph IDs with integer advances in
//! micrometres, line-break opportunities, glyph fallback, deterministic font subsetting with
//! a ToUnicode map, and deterministic PDF text objects. Page layout, the other block painters
//! and the full PDF writer are later leaves (`RPT.OUTPUT.PDF.LAYOUT`, `.DRAW`, `.WRITE`).

pub mod text;
