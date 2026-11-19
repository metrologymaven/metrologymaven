// Regression cases from the re-review of PR #235: `clippy` and `debug_assertions` cfg
// predicates are KNP1-CFG (never waivable) in `cfg`, `cfg_attr` predicates and `cfg!`.
#[cfg(not(clippy))] //~ KNP1-CFG=clippy
pub fn a() {}

#[cfg(all(unix, any(clippy, test)))] //~ KNP1-CFG=clippy
pub fn b() {}

#[cfg_attr(clippy, inline)] //~ KNP1-CFG=clippy
pub fn c() {}

#[cfg_attr(test, allow(clippy::all))]
pub fn d() {}

pub fn e() -> bool {
    cfg!(debug_assertions) || cfg!(any(unix, clippy)) //~ KNP1-CFG=debug_assertions KNP1-CFG=clippy
}

#[cfg(debug_assertions)] //~ KNP1-CFG=debug_assertions
pub fn f() {}

#[cfg_attr(not(debug_assertions), inline)] //~ KNP1-CFG=debug_assertions
pub fn g() {}

pub fn h(x: u32) {
    debug_assert!(x > 0);
}
