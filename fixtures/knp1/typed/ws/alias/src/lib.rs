//! REQ-004: an aliased `HashMap`.

use std::collections::HashMap as M;

pub fn f() -> usize {
    let m: M<u8, u8> = M::new();
    m.len()
}
