//! MIR-layer fixture (REQ-RPT.MODEL.FMT-002): floats that reach code outside `fmt` only by
//! type inference, which neither clippy `disallowed_types` nor the token scan sees. Every
//! function annotated `// mir` must be reported by `rpt_float_lint.py --mir-selftest`, and
//! no other. No line names a float type or holds a float literal, except the signature of
//! `dep::Measure::measure`, which the trait dictates. The last two cases are trait impls,
//! one of a dependency's trait for a local type and one of a local trait for a foreign type
//! (`Vec<u8>`): rustc may print the latter as `<std::vec::Vec<u8> as LocalTrait>::...`,
//! which the MIR layer must attribute and report rather than skip as a `std::` shim.

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

impl Measure for Count {
    fn measure(&self) -> f64 { // mir
        dep::LIMIT
    }
}

pub trait LocalTrait {
    fn local_trait_on_foreign_type(&self) -> u64;
}

impl LocalTrait for Vec<u8> {
    fn local_trait_on_foreign_type(&self) -> u64 { // mir
        dep::LIMIT.to_bits() ^ self.len() as u64
    }
}
