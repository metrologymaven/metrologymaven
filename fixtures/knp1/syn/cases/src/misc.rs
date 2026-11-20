pub fn a(v: &mut [u8]) {
    let p = v.as_mut_ptr();
    unsafe { *p = 0 } //~ KNP1-UNSAFE=unsafe
}

extern "C" { //~ KNP1-UNSAFE=extern
    fn cbrt(x: f64) -> f64; //~ KNP1-LIBM=cbrt
}

pub fn b() -> &'static [u8] {
    include_bytes!("table.bin") //~ KNP1-IO-FS=include_bytes
}

pub fn c() -> &'static str {
    include_str!("misc.rs")
}

#[path = "../../outside.rs"] //~ KNP1-INCLUDE=path
pub mod outside;

#[path = "shadow.rs"]
pub mod shadow_again;

pub fn d() -> Option<&'static str> {
    option_env!("CARGO_PKG_NAME") //~ KNP1-ENV=option_env
}

#[target_feature(enable = "avx2")] //~ KNP1-SIMD=target_feature
pub fn e() {}

#[cfg(target_feature = "fma")] //~ KNP1-SIMD=target_feature
pub fn f() {}

pub fn g(x: f64) -> f64 {
    core::intrinsics::fmaf64(x, x, x) + x.powf(0.5) + std::f64::consts::E.ln() //~ KNP1-FMA=fmaf64 KNP1-POW=powf KNP1-LIBM=ln
}

thread_local! { //~ KNP1-THREAD=thread_local
    static K: u8 = 0;
}

pub fn h(x: f32) -> f32 {
    x.exp() + sinf32(x) + libm::exp(x) //~ KNP1-LIBM=exp#1 KNP1-LIBM=sinf32 KNP1-LIBM=exp#2
}

pub fn i(a: f64, b: f64) -> f64 {
    a.algebraic_add(b) //~ KNP1-FMA=algebraic_add
}

std::arch::global_asm!(""); //~ KNP1-SIMD=arch KNP1-UNSAFE=global_asm

pub fn j() -> bool {
    std::process::id() > 0 && core::time::Duration::ZERO.is_zero() //~ KNP1-THREAD=process
}
