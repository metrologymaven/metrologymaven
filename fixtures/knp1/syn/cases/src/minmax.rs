pub fn a(x: f64, y: f64) -> f64 {
    let p = f64::max(x, y); //~ KNP1-MINMAX=max
    let q = <f64>::min(x, y); //~ KNP1-MINMAX=min
    let r = 1.5f64.clamp(x, y); //~ KNP1-MINMAX=clamp
    let s = (x as f64).min(y); //~ KNP1-MINMAX=min
    let t = x.max(f64::EPSILON); //~ KNP1-MINMAX=max
    let u = core::f32::consts::PI.min(2.0); //~ KNP1-MINMAX=min
    let v = [f64::min][0](x, y); //~ KNP1-MINMAX=min
    let w = x.max(-0.0); //~ KNP1-MINMAX=max
    p + q + r + s + t + f64::from(u) + v + w
}

pub fn c(x: f64) -> f64 {
    fmax(x, 0.0) //~ KNP1-MINMAX=fmax
}
