//! PDF report renderer of the Report app. Spec node `RPT.OUTPUT.PDF`.
//!
//! - [`text`] (`RPT.OUTPUT.PDF.TEXT`): bundled fonts, deterministic integer-µm shaping,
//!   line-break opportunities, glyph fallback, deterministic TrueType subsetting with a
//!   ToUnicode map, and deterministic PDF text objects.
//! - [`layout`] (`RPT.OUTPUT.PDF.LAYOUT`): integer-µm page layout engine turning a
//!   `ResolvedReport` into a canonical `PageLayout` display list with page breaks, continuation
//!   headers, column groups, designer frames, overflow clipping and page-number fill-in.

pub(crate) mod int_json;
pub mod layout;
pub mod text;
