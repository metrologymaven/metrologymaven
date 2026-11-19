// Regression cases for the bypasses found in the first review of PR #215.
use std as s2; //~ KNP1-SIMD=std KNP1-IO-FS=std KNP1-IO-NET=std KNP1-CLOCK=std KNP1-ENV=std KNP1-THREAD=std
use core::include_str as t; //~ KNP1-IO-FS=include_str
use core::option_env as oe; //~ KNP1-ENV=option_env
use std::thread_local as tl; //~ KNP1-THREAD=thread_local
use libm::pow; //~ KNP1-POW=pow
type F = f64;

pub fn a() -> bool {
    super::s2::fs::metadata("x").is_ok() //~ KNP1-IO-FS=super
}

pub fn b() -> &'static str {
    t!("table.txt") //~ KNP1-IO-FS=t
}

pub fn c() -> Option<&'static str> {
    oe!("HOME") //~ KNP1-ENV=oe
}

tl! { //~ KNP1-THREAD=tl
    static K: u8 = 0;
}

pub fn d(x: f64) -> f64 {
    pow(x, 2.0) + i32::pow(2, 3) as f64 //~ KNP1-POW=pow#1
}

pub fn e(x: f64) -> f64 {
    <f64>::sin(x) + <f64 as Float>::cos(x) + libm::Libm::<f64>::exp(x) //~ KNP1-LIBM=sin KNP1-LIBM=cos KNP1-LIBM=exp
}

pub fn f(a: f64, b: f64) -> f64 {
    F::max(a, b) + f64::from(a).max(b) + { 1.0_f64 }.max(b) + [1.0][0].max(b) //~ KNP1-MINMAX=max#1 KNP1-MINMAX=max#2 KNP1-MINMAX=max#3 KNP1-MINMAX=max#4
}

pub fn f2(v: &[u32]) -> Option<u32> {
    v.iter().copied().map(|x| x + 1).max()
}

#[no_mangle] //~ KNP1-UNSAFE=no_mangle
pub fn g() {}

#[export_name = "h2"] //~ KNP1-UNSAFE=export_name
pub fn h() {}

#[link_section = ".data"] //~ KNP1-UNSAFE=link_section
pub static S: u8 = 0;

pub fn i(x: f64) -> f64 {
    core::intrinsics::simd_fma(x, x, x) + x.exp10() + libm::j0(x) + libm::sincos(x).0 //~ KNP1-FMA=simd_fma KNP1-LIBM=exp10 KNP1-LIBM=j0 KNP1-LIBM=sincos
}

macro_rules! hide {
    ($x:expr, $f:ident) => {
        $x.$f() //~ KNP1-PARSE=$f
    };
    ($($p:ident)::*) => { //~ KNP1-PARSE=$
        $($p)::*::read("x") //~ KNP1-PARSE=$
    };
    ($g:ident) => {
        f64::$g(1.0) //~ KNP1-PARSE=$g
    };
}
