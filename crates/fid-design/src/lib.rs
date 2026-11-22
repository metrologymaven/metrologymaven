//! `fid-design`: the design-system tooling crate (spec domain `UX`). Tooling layer; not a
//! registry command. Each UX leaf adds its check as a module and a subcommand.
//!
//! - [`pkg`]: `pkg-check` and `pkg-diff` (`UX.SYSTEM.PKG`).
//! - [`lint`]: `lint-literals` (`UX.SYSTEM.LINT`).
//! - [`princ`]: `princ-check` (`UX.SYSTEM.PRINC`).
//! - [`words`]: `words-check` (`UX.SYSTEM.PRINC`).
//! - [`review`]: `review-check` (`UX.SYSTEM.REVIEW`).
//! - [`screens`]: `screens-check` and `screens-gate` (`UX.SCREENS.INV`).
//! - [`sources`]: `sources-check` (`UX.SYSTEM.TOOL`).
//! - [`research`]: `research-check` (`UX.RESEARCH.PROTO`).
//! - [`tokens`]: `tokens-check` (`UX.TOKENS.COLOR`).
//! - [`fonts`]: `fonts-check` (`UX.TOKENS.TYPE`).
//! - [`status`]: `status-check` (`UX.TOKENS.STATUS`).
//! - [`palettes`]: `palettes-check` (`UX.TOKENS.DATAPAL`).
//! - [`color`]: `color-check`, the release gate over all of them (`UX.TOKENS.CHECK`).
//! - [`space`]: `space-check` (`UX.TOKENS.SPACE`).
//! - [`icons_lint`]: `icons-lint` (`UX.ICONS.GRID`).
//! - [`icons_prov`]: `icons-prov` and `icons-registry` (`UX.ICONS.PROV`).
//! - [`page`]: `page-check` (`UX.REPORT.PAGE`).

pub mod color;
pub mod finding;
pub mod fonts;
pub mod icons_lint;
pub mod icons_prov;
pub mod lint;
pub mod page;
pub mod palettes;
pub mod pkg;
pub mod princ;
pub mod research;
pub mod review;
pub mod screens;
pub mod sources;
pub mod space;
pub mod status;
pub mod tokens;
pub mod words;
