//! The allowed module: floats here are not findings.

pub fn bits(e: &dep::Expanded) -> u64 {
    let v = e.u;
    v.to_bits()
}

pub struct Held(pub f64);

impl Held {
    pub fn get(&self) -> u64 {
        self.0.to_bits()
    }
}
