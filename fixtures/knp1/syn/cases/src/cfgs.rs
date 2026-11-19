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

// Re-review 3 of PR #235: cfg predicates fed through macros, `panic`, and cfg names outside the
// kernel allowlist (custom names, which a build script could emit, included).
macro_rules! gate {
    ($p:meta) => {
        #[cfg($p)] //~ KNP1-CFG=$
        pub fn gated() {}
    };
}

macro_rules! check {
    ($p:tt) => {
        cfg!($p) //~ KNP1-CFG=$
    };
}

gate!(not(clippy)); //~ KNP1-CFG=clippy

pub fn i() -> bool {
    check!(panic) //~ KNP1-CFG=panic
}

#[cfg(panic = "abort")] //~ KNP1-CFG=panic
pub fn j() {}

#[cfg(knp1_custom)] //~ KNP1-CFG=knp1_custom
pub fn k() {}

#[cfg(any(miri, target_os = "linux"))] //~ KNP1-CFG=miri
pub fn l() {}

#[cfg_attr(all(unix, feature = "x"), inline)]
pub fn m() {}
