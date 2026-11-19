// Scan-only part of the bad fixture: needs other crates or unstable APIs, so it is not a
// module of lib.rs and is never compiled. Annotations as in lib.rs.
#![allow(warnings)] // expect: RPT-LINT-ALLOW

pub fn exp_trait(x: f64) -> String { // expect: RPT-FLOAT-TYPE
    let mut s = String::new();
    let _ = core::fmt::LowerExp::fmt(&x, &mut core::fmt::Formatter::new(&mut s)); // expect: RPT-FLOAT-TRAIT
    s
}

pub fn float_crate(x: f64) -> String { // expect: RPT-FLOAT-TYPE
    ryu::Buffer::new().format(x).to_owned() // expect: RPT-FLOAT-CRATE
}
