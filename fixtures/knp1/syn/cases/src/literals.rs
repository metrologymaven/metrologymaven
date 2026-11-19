//! Banned names in comments, strings, lifetimes and raw identifiers are not code: no findings.
// x.mul_add(y, z) and std::fs::read in a comment
/* nested /* block */ with unsafe { HashMap } */

/// Doc comment: `f64::max(a, b)` and `x.sqrt()`.
pub fn a() -> &'static str {
    "std::fs::read(x.mul_add(1.0, 2.0)) unsafe HashMap \" still a string"
}

pub fn b() -> &'static str {
    r#"powi "quoted" sqrt"#
}

pub fn c() -> &'static [u8] {
    br##"sin(x) "# cos"##
}

pub fn d() -> char {
    '"'
}

pub fn e<'unsafe_ish>(x: &'unsafe_ish str) -> &'unsafe_ish str {
    x
}

pub fn f(r#unsafe: u8) -> u8 {
    r#unsafe
}

pub fn g() -> f64 {
    let exp = 3.0;
    let gamma = exp * 2.0;
    gamma
}

pub fn h(n: u32) -> u32 {
    let m = n.max(7);
    m.clamp(1, 9).min(Ord::max(n, 2)) + 2u32.pow(3)
}
