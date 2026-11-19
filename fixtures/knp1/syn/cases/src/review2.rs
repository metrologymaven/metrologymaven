// Regression cases for the second review of PR #215: re-exports of built-in roots, any-position
// module paths, metavariables in callee position, path-building repetitions, banned names
// passed bare to macros, facade-root shadowing and integer `pow`.
pub use std; //~ KNP1-SIMD=std KNP1-IO-FS=std KNP1-IO-NET=std KNP1-CLOCK=std KNP1-ENV=std KNP1-THREAD=std
pub extern crate core; //~ KNP1-SIMD=core KNP1-ENV=core
extern crate alloc;
extern crate libm as fid_num; //~ KNP1-LIBM=fid_num

pub fn a() -> bool {
    crate::std::time::Instant::now().elapsed().as_secs() > 0 //~ KNP1-CLOCK=time
}

pub fn b() {
    let _ = super::std::thread::spawn(|| ()); //~ KNP1-THREAD=thread
}

pub fn c() {
    call!(env, option_env, thread_local); //~ KNP1-ENV=env KNP1-ENV=option_env KNP1-THREAD=thread_local
    other!(max, sqrt, sin); //~ KNP1-MINMAX=max KNP1-LIBM=sqrt KNP1-LIBM=sin
}

pub fn d() -> u32 {
    i32::pow(2, 3) as u32 + <u64>::pow(2, 3) as u32
}

macro_rules! m2 {
    ($f:ident) => {
        <f64 as Float>::$f(1.0) + F::$f(2.0) + num_traits::Float::$f(3.0) //~ KNP1-PARSE=$f#1 KNP1-PARSE=$f#2 KNP1-PARSE=$f#3
    };
    ($m:ident, $x:expr) => {
        $m!($x) + $f2($x) //~ KNP1-PARSE=$m KNP1-PARSE=$f2
    };
    ($($p:ident)*) => {
        $($p::)*read("x") //~ KNP1-PARSE=$
    };
}

mod fid_num_impl {}
mod fid_num {} //~ KNP1-LIBM=fid_num
