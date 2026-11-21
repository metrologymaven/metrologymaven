//! MIR-layer fixture (REQ-RPT.MODEL.FMT-002): floats that reach code outside `fmt` only by
//! type inference, which neither clippy `disallowed_types` nor the token scan sees. Every
//! function annotated `// mir` must be reported by `rpt_float_lint.py --mir-selftest`, and
//! no other. No line names a float type or holds a float literal.

pub mod fmt;

use dep::Measure;

pub fn field_of_dependency_struct() -> u64 { // mir
    let e = dep::make();
    let v = e.u;
    v.to_bits()
}

pub fn vec_of_floats() -> usize { // mir
    dep::many().len()
}

pub fn closure_over_floats() -> usize { // mir
    dep::many().iter().filter(|x| x.is_sign_positive()).count()
}

pub fn trait_method() -> bool { // mir
    dep::Probe.measure().is_finite()
}

pub fn foreign_constant() -> bool { // mir
    dep::LIMIT.is_finite()
}

pub fn generic_wrapper() -> bool { // mir
    let w = dep::wrapped();
    w.0.is_finite()
}

pub fn through_allowed_module(e: &dep::Expanded) -> u64 {
    fmt::bits(e)
}

pub fn clean(a: u32) -> u32 {
    a.wrapping_add(1)
}

pub struct Count(pub u32);

impl Count {
    pub fn next(&self) -> u32 {
        self.0.wrapping_add(1)
    }
}
