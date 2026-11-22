//! `fid-design`: the design-system tooling crate (spec domain `UX`). Tooling layer; not a
//! registry command. Each UX leaf adds its check as a module and a subcommand.
//!
//! - [`pkg`]: `pkg-check` and `pkg-diff` (`UX.SYSTEM.PKG`).
//! - [`princ`]: `princ-check` (`UX.SYSTEM.PRINC`).
//! - [`words`]: `words-check` (`UX.SYSTEM.PRINC`).
//! - [`review`]: `review-check` (`UX.SYSTEM.REVIEW`).
//! - [`screens`]: `screens-check` and `screens-gate` (`UX.SCREENS.INV`).
//! - [`tokens`]: `tokens-check` (`UX.TOKENS.COLOR`).
//! - [`status`]: `status-check` (`UX.TOKENS.STATUS`).
//! - [`palettes`]: `palettes-check` (`UX.TOKENS.DATAPAL`).
//! - [`color`]: `color-check`, the release gate over all of them (`UX.TOKENS.CHECK`).
//! - [`icons_prov`]: `icons-prov` and `icons-registry` (`UX.ICONS.PROV`).
//! - [`page`]: `page-check` (`UX.REPORT.PAGE`).

pub mod color;
pub mod finding;
pub mod icons_prov;
pub mod page;
pub mod palettes;
pub mod pkg;
pub mod princ;
pub mod review;
pub mod screens;
pub mod status;
pub mod tokens;
pub mod words;
