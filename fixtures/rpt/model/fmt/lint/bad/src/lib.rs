// Fixture for tools/rpt_float_lint.py (RPT.MODEL.FMT AC-7). Not a workspace member: never
// compiled. Every marked line must be a finding; the expected list is in
// tools/tests/test_rpt_float_lint.py, hand-labelled from the rules.
use core::fmt::Write as _;

pub fn ac7() -> String {
    format!("{}", 1.5_f64) // RPT-FLOAT-ARG (AC-7)
}

pub fn precision(x: f64) -> String {
    format!("{:.3}", x) // RPT-FLOAT-SPEC
}

pub fn exponent(out: &mut String, x: f64) {
    let _ = write!(out, "{x:e}"); // RPT-FLOAT-SPEC
}

pub fn cast(n: u32) -> String {
    format!("{}", n as f64) // RPT-FLOAT-ARG
}

pub fn to_string_group(n: u32) -> String {
    (n as f64).to_string() // RPT-FLOAT-TOSTRING
}

pub fn to_string_literal() -> String {
    2.5.to_string() // RPT-FLOAT-TOSTRING
}

pub fn to_string_path(x: f64) -> String {
    f64::to_string(&x) // RPT-FLOAT-TOSTRING
}

pub fn exp_trait(x: f64) -> String {
    let mut s = String::new();
    let _ = core::fmt::LowerExp::fmt(&x, &mut core::fmt::Formatter::new(&mut s)); // RPT-FLOAT-TRAIT
    s
}

pub fn float_crate(x: f64) -> String {
    ryu::Buffer::new().format(x).to_owned() // RPT-FLOAT-CRATE
}

pub fn concat_literal() -> &'static str {
    concat!("v=", 1e3) // RPT-FLOAT-ARG
}
