// Fixture of the RPT float rule (RPT.MODEL.FMT REQ-002, AC-7): code that must fail.
// Not a workspace member. `tools/rpt_float_lint.py --clippy-selftest` compiles it as a
// standalone crate with crates/fid-report/clippy.toml and requires a clippy
// `disallowed_types` error on every line annotated `clippy`; tools/tests/
// test_rpt_float_lint.py requires exactly the scan rules annotated on each line (and none
// elsewhere). Annotations are hand-labelled from the rules. `scan_only.rs` holds cases that
// do not compile without other crates; it is scanned, not compiled.
#![allow(dead_code)]

use std::fmt::{self, Write as _};

// AC-7: a float literal formatted directly.
pub fn ac7() -> String {
    format!("{}", 1.5_f64) // expect: RPT-FLOAT-ARG RPT-FLOAT-LITERAL
}

// Review bypass 1: `format!("{}", x)` on a float variable.
pub fn bypass1(x: f64) -> String { // expect: RPT-FLOAT-TYPE clippy
    format!("{}", x)
}

// Review bypass 2: inline capture `format!("{x}")`.
pub fn bypass2(x: f64) -> String { // expect: RPT-FLOAT-TYPE clippy
    format!("{x}")
}

// Review bypass 3: `x.to_string()`.
pub fn bypass3(x: f64) -> String { // expect: RPT-FLOAT-TYPE clippy
    x.to_string()
}

// Review bypass 4: `ToString::to_string(&x)`.
pub fn bypass4(x: f64) -> String { // expect: RPT-FLOAT-TYPE clippy
    ToString::to_string(&x)
}

// Review bypass 5: a type alias hides the float.
pub type Length = f64; // expect: RPT-FLOAT-TYPE clippy

pub fn bypass5(x: Length) -> String {
    format!("{x}")
}

// Review bypass 6: a newtype with its own Display.
pub struct Shown(f64); // expect: RPT-FLOAT-TYPE clippy

impl fmt::Display for Shown {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// Review bypass 7: `format_args!` through `write_fmt`.
pub fn bypass7(out: &mut String, x: f64) -> fmt::Result { // expect: RPT-FLOAT-TYPE clippy
    out.write_fmt(format_args!("{}", x))
}

// Review bypass 8: a macro_rules wrapper around format!.
macro_rules! show {
    ($e:expr) => {
        format!("{}", $e)
    };
}

pub fn bypass8(x: f64) -> String { // expect: RPT-FLOAT-TYPE clippy
    show!(x)
}

// A float created from a literal with no type named: the scan sees the literal.
pub fn literal_binding() -> String {
    let y = 2.5; // expect: RPT-FLOAT-LITERAL
    format!("{y}")
}

pub fn precision(x: f32) -> String { // expect: RPT-FLOAT-TYPE clippy
    format!("{:.3}", x) // expect: RPT-FLOAT-SPEC
}

pub fn exponent(out: &mut String, x: f64) { // expect: RPT-FLOAT-TYPE clippy
    let _ = write!(out, "{x:e}"); // expect: RPT-FLOAT-SPEC
}

pub fn cast(n: u32) -> String {
    format!("{}", n as f64) // expect: RPT-FLOAT-ARG RPT-FLOAT-TYPE
}

pub fn to_string_group(n: u32) -> String {
    (n as f64).to_string() // expect: RPT-FLOAT-TOSTRING RPT-FLOAT-TYPE
}

pub fn to_string_literal() -> String {
    2.5.to_string() // expect: RPT-FLOAT-LITERAL RPT-FLOAT-TOSTRING
}

pub fn to_string_path(x: f64) -> String { // expect: RPT-FLOAT-TYPE clippy
    f64::to_string(&x) // expect: RPT-FLOAT-TOSTRING RPT-FLOAT-TYPE
}

pub fn concat_literal() -> &'static str {
    concat!("v=", 1e3) // expect: RPT-FLOAT-ARG RPT-FLOAT-LITERAL
}

// Switching the type rule off outside fid_report::fmt is itself a finding.
#[allow(clippy::disallowed_types)] // expect: RPT-LINT-ALLOW
pub fn hidden(x: f64) -> String { // expect: RPT-FLOAT-TYPE
    format!("{x}")
}
