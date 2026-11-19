use std::fs as files; //~ KNP1-IO-FS=fs
use std as s;
use std::{env, net::TcpStream}; //~ KNP1-ENV=env KNP1-IO-NET=net
use std::primitive::f64 as F;
use std::collections::{self as c, HashMap as Map}; //~ KNP1-HASH=HashMap
extern crate std as stdlib;
use libm::sin as sine; //~ KNP1-LIBM=sin
use std::*; //~ KNP1-SIMD=* KNP1-IO-FS=* KNP1-IO-NET=* KNP1-CLOCK=* KNP1-ENV=* KNP1-THREAD=*

pub fn a() -> bool {
    files::metadata("x").is_ok() //~ KNP1-IO-FS=files
}

pub fn b() -> bool {
    s::time::Instant::now().elapsed().as_secs() > 0 //~ KNP1-CLOCK=s
}

pub fn c2() -> usize {
    let m: Map<u8, u8> = Map::new(); //~ KNP1-HASH=Map#1 KNP1-HASH=Map#2
    m.len() + c::BTreeMap::<u8, u8>::new().len()
}

pub fn d(x: f64, y: f64) -> f64 {
    F::max(x, y) //~ KNP1-MINMAX=max
}

pub fn e(x: f64) -> f64 {
    sine(x) //~ KNP1-LIBM=sine
}

pub fn f() -> bool {
    stdlib::thread::panicking() //~ KNP1-THREAD=stdlib
}

pub fn g(t: TcpStream) -> TcpStream { //~ KNP1-IO-NET=TcpStream#1 KNP1-IO-NET=TcpStream#2
    t
}
