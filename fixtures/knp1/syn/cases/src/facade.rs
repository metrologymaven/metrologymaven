use fid_num::f::{cos, sqrt as root}; //~ note:KNP1-LIBM=cos note:KNP1-LIBM=sqrt
use fid_num::f as nf;
use fid_num::la::sin; //~ KNP1-LIBM=sin

pub fn a(x: f64) -> f64 {
    fid_num::f::sin(x) + nf::tan(x) + cos(x) + root(x) //~ note:KNP1-LIBM=sin note:KNP1-LIBM=tan note:KNP1-LIBM=cos note:KNP1-LIBM=root
}

pub fn b(x: f64) -> f64 {
    sin(x) + x.sin() //~ KNP1-LIBM=sin#1 KNP1-LIBM=sin#2
}

pub fn d(x: f64) -> f64 {
    crate::f::sqrt(x) + fid_num::f::cbrt(x) //~ KNP1-LIBM=sqrt note:KNP1-LIBM=cbrt
}
