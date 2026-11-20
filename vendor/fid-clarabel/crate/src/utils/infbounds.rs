// Modified from Clarabel 0.11.1 by Metrology Maven;
// see vendor/fid-clarabel/patches/
/// Constant indicating that an inequality bound is to be treated as infinite.
///
/// If the setting [`presolve_enable`](crate::solver::DefaultSettings::presolve_enable)
/// is `true`, any such constraints are removed.   Bounds for all other cones with
/// values greater than this are capped at this value.
///
/// KNP-1 (patch 0007): upstream kept this bound in a mutable global (`lazy_static` and an
/// atomic, with `set_infinity`/`default_infinity`); this fork fixes it at the default so
/// that a solve depends on its arguments only.
pub const INFINITY_DEFAULT: f64 = crate::_INFINITY_DEFAULT;

/// The internal infinity bound: always [`INFINITY_DEFAULT`] in this fork.
pub fn get_infinity() -> f64 {
    INFINITY_DEFAULT
}
