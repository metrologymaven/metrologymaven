//! PDF report renderer of the Report app. Spec node `RPT.OUTPUT.PDF`.
//!
//! - [`text`] (`RPT.OUTPUT.PDF.TEXT`): bundled fonts, deterministic integer-µm shaping,
//!   line-break opportunities, glyph fallback, deterministic TrueType subsetting with a
//!   ToUnicode map, and deterministic PDF text objects.
//! - [`layout`] (`RPT.OUTPUT.PDF.LAYOUT`): integer-µm page layout engine turning a
//!   `ResolvedReport` into a canonical `PageLayout` display list with page breaks, continuation
//!   headers, column groups, designer frames, overflow clipping and page-number fill-in.

//! - [`draw`] (`RPT.OUTPUT.PDF.DRAW`): `Painter` trait, registry, upfront unsupported-block
//!   check, CAS image verification, bounded PNG/JPEG decoding, vector balloon overlays, and
//!   painters for `table`, `text`, `header`, `footer`, `image`, `view`, `drawing` and
//!   `form_field`.

pub mod draw;
pub(crate) mod flate;
pub mod image_codec;
pub(crate) mod int_json;
pub mod layout;
pub mod text;
