macro_rules! fused {
    ($a:expr, $b:expr) => {
        $a.mul_add($b, 1.0) //~ KNP1-FMA=mul_add
    };
}

macro_rules! via_meta {
    ($p:path) => {
        $p::fs::read("x") //~ KNP1-IO-FS=fs
    };
}

macro_rules! big {
    ($t:ident, $x:expr, $y:expr) => {
        $t::max($x, $y) //~ KNP1-MINMAX=max
    };
}

macro_rules! clampy {
    ($x:expr) => {
        $x.clamp(0, 1) //~ KNP1-MINMAX=clamp
    };
}

pub fn g(v: &[f64]) -> f64 {
    let n = v.len().min(3);
    fused!(v[0], v[1]) + n as f64
}

pub fn h() -> bool {
    some_crate::call!(std::time::Instant::now()) //~ KNP1-CLOCK=time
}

pub fn i() {
    let _ = core::arch::x86_64::_mm_fmadd_pd; //~ KNP1-SIMD=arch KNP1-FMA=_mm_fmadd_pd
}
