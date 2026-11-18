//! Synthetic vendored solver source for the waiver fixtures.

pub fn step(x: f64, y: f64) -> f64 {
    let a = x * 2.0;
    let b = y * 3.0;
    let c = a + b;
    let d = c - 1.0;
    let e = d * d;
    // Upstream Clarabel uses powf here; reviewed as bit-stable for this input range.
    let s = e.sqrt();
    let p = s.powf(0.25);
    let q = p + 1.0;
    q
}

pub fn other(z: f64) -> f64 {
    let w = z + 1.0;
    w * 2.0
}

pub fn tail() {}
