//! Canonical encoding of a [`FitResult`] for digests and journaling (REQ-KRN.LSQ.TRAIT-008).
//!
//! # Pinned format
//!
//! Compact JSON (no whitespace), ASCII only, keys in the fixed order below (not sorted):
//!
//! ```text
//! {"algorithm":"<id>","feature":{"kind":"<primitive>",<fields>},"residuals":[<f>...],
//!  "stats":{"n":<i>,"rms":<f>,"max":<f>,"min":<f>,"mean":<f>,"std_dev":<f>},
//!  "diagnostics":{"grad_norm":<f>,"rank":<i>,"cond":<f>,"iterations":<i>,"stop":"<reason>"},
//!  "certificate":null | {"support":[<i>...],"gap":<f>}}
//! ```
//!
//! (shown wrapped; the bytes have no line breaks). `<f>` is an `f64` as a JSON string of
//! exactly 16 lowercase hexadecimal digits, its IEEE-754 binary64 bit pattern (so `-0.0` is
//! `"8000000000000000"` and `+inf` is `"7ff0000000000000"`); `<i>` is an unsigned integer as a
//! JSON string of decimal digits without leading zeros. A 3D vector is `[<f>,<f>,<f>]`, a 2D
//! vector `[<f>,<f>]`. The feature fields follow the declaration order of
//! [`FittedFeature`]: `point: c`; `line2`/`line3: p, d`; `plane: p, n`; `circle2: c, r`;
//! `circle3: c, n, r`; `sphere: c, r`; `cylinder: p, d, r`; `cone: apex, d, half_angle`;
//! `torus: c, n, major_r, minor_r`; `slot`/`rectangle: c, n, d, length, width`;
//! `ellipse: c, n, d, a, b`; `paraboloid: apex, d, focal`; `polyline: vertices` (array of
//! 3D vectors); `spline: degree` (`<i>`)`, knots` (array of `<f>`)`, control` (array of 3D
//! vectors). The algorithm ID is `fid_kci::AlgorithmId`'s display form.

use super::error::{alloc_failed, FitError};
use super::primitive::{FittedFeature, V3};
use super::FitResult;

const OP: &str = "fit.encode";

struct Out {
    buf: Vec<u8>,
}

impl Out {
    fn put(&mut self, b: &[u8]) -> Result<(), FitError> {
        self.buf.try_reserve(b.len()).map_err(|_| alloc_failed(OP))?;
        self.buf.extend_from_slice(b);
        Ok(())
    }

    fn s(&mut self, s: &str) -> Result<(), FitError> {
        self.put(s.as_bytes())
    }

    fn f(&mut self, x: f64) -> Result<(), FitError> {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let bits = x.to_bits();
        let mut b = [0u8; 18];
        b[0] = b'"';
        b[17] = b'"';
        for (k, slot) in b[1..17].iter_mut().enumerate() {
            *slot = HEX[((bits >> (60 - 4 * k)) & 0xF) as usize];
        }
        self.put(&b)
    }

    fn i(&mut self, v: u64) -> Result<(), FitError> {
        let mut digits = [0u8; 22];
        let mut pos = digits.len() - 1;
        digits[pos] = b'"';
        let mut x = v;
        loop {
            pos -= 1;
            digits[pos] = b'0' + (x % 10) as u8;
            x /= 10;
            if x == 0 {
                break;
            }
        }
        pos -= 1;
        digits[pos] = b'"';
        self.put(&digits[pos..])
    }

    fn key(&mut self, first: bool, k: &str) -> Result<(), FitError> {
        if !first {
            self.s(",")?;
        }
        self.s("\"")?;
        self.s(k)?;
        self.s("\":")
    }

    fn fs(&mut self, xs: &[f64]) -> Result<(), FitError> {
        self.s("[")?;
        for (i, &x) in xs.iter().enumerate() {
            if i > 0 {
                self.s(",")?;
            }
            self.f(x)?;
        }
        self.s("]")
    }

    fn vs(&mut self, vs: &[V3]) -> Result<(), FitError> {
        self.s("[")?;
        for (i, v) in vs.iter().enumerate() {
            if i > 0 {
                self.s(",")?;
            }
            self.fs(v)?;
        }
        self.s("]")
    }

    fn fv(&mut self, k: &str, xs: &[f64]) -> Result<(), FitError> {
        self.key(false, k)?;
        self.fs(xs)
    }

    fn ff(&mut self, k: &str, x: f64) -> Result<(), FitError> {
        self.key(false, k)?;
        self.f(x)
    }
}

fn usize_u64(x: usize) -> u64 {
    // usize is at most 64 bits on every supported target.
    x as u64
}

fn feature(o: &mut Out, feat: &FittedFeature) -> Result<(), FitError> {
    use FittedFeature as F;
    o.s("{")?;
    o.key(true, "kind")?;
    o.s("\"")?;
    o.s(feat.primitive().slug())?;
    o.s("\"")?;
    match feat {
        F::Point { c } => o.fv("c", c)?,
        F::Line2 { p, d } => {
            o.fv("p", p)?;
            o.fv("d", d)?;
        }
        F::Line3 { p, d } => {
            o.fv("p", p)?;
            o.fv("d", d)?;
        }
        F::Plane { p, n } => {
            o.fv("p", p)?;
            o.fv("n", n)?;
        }
        F::Circle2 { c, r } => {
            o.fv("c", c)?;
            o.ff("r", *r)?;
        }
        F::Circle3 { c, n, r } => {
            o.fv("c", c)?;
            o.fv("n", n)?;
            o.ff("r", *r)?;
        }
        F::Sphere { c, r } => {
            o.fv("c", c)?;
            o.ff("r", *r)?;
        }
        F::Cylinder { p, d, r } => {
            o.fv("p", p)?;
            o.fv("d", d)?;
            o.ff("r", *r)?;
        }
        F::Cone { apex, d, half_angle } => {
            o.fv("apex", apex)?;
            o.fv("d", d)?;
            o.ff("half_angle", *half_angle)?;
        }
        F::Torus { c, n, major_r, minor_r } => {
            o.fv("c", c)?;
            o.fv("n", n)?;
            o.ff("major_r", *major_r)?;
            o.ff("minor_r", *minor_r)?;
        }
        F::Slot { c, n, d, length, width } | F::Rectangle { c, n, d, length, width } => {
            o.fv("c", c)?;
            o.fv("n", n)?;
            o.fv("d", d)?;
            o.ff("length", *length)?;
            o.ff("width", *width)?;
        }
        F::Ellipse { c, n, d, a, b } => {
            o.fv("c", c)?;
            o.fv("n", n)?;
            o.fv("d", d)?;
            o.ff("a", *a)?;
            o.ff("b", *b)?;
        }
        F::Paraboloid { apex, d, focal } => {
            o.fv("apex", apex)?;
            o.fv("d", d)?;
            o.ff("focal", *focal)?;
        }
        F::Polyline { vertices } => {
            o.key(false, "vertices")?;
            o.vs(vertices)?;
        }
        F::Spline { degree, knots, control } => {
            o.key(false, "degree")?;
            o.i(u64::from(*degree))?;
            o.fv("knots", knots)?;
            o.key(false, "control")?;
            o.vs(control)?;
        }
    }
    o.s("}")
}

/// The canonical bytes of one feature: the `"feature"` object of [`encode`] (module
/// documentation), on its own. Used as the feature key and in the rocking report
/// (`tan_rock`). Allocation failure is [`FitError::Num`]; nothing else fails.
pub fn encode_feature(feat: &FittedFeature) -> Result<Vec<u8>, FitError> {
    let mut o = Out { buf: Vec::new() };
    feature(&mut o, feat)?;
    Ok(o.buf)
}

/// The canonical bytes of `r` (module documentation). Deterministic: the same value gives the
/// same bytes on every target. Allocation failure is [`FitError::Num`]; nothing else fails.
pub fn encode(r: &FitResult) -> Result<Vec<u8>, FitError> {
    let mut o = Out { buf: Vec::new() };
    o.s("{")?;
    o.key(true, "algorithm")?;
    o.s("\"")?;
    o.s(&r.algorithm.to_string())?;
    o.s("\"")?;
    o.key(false, "feature")?;
    feature(&mut o, &r.feature)?;
    o.fv("residuals", &r.residuals)?;
    let st = &r.stats;
    o.key(false, "stats")?;
    o.s("{")?;
    o.key(true, "n")?;
    o.i(usize_u64(st.n))?;
    o.ff("rms", st.rms)?;
    o.ff("max", st.max)?;
    o.ff("min", st.min)?;
    o.ff("mean", st.mean)?;
    o.ff("std_dev", st.std_dev)?;
    o.s("}")?;
    let d = &r.diagnostics;
    o.key(false, "diagnostics")?;
    o.s("{")?;
    o.key(true, "grad_norm")?;
    o.f(d.grad_norm)?;
    o.key(false, "rank")?;
    o.i(usize_u64(d.rank))?;
    o.ff("cond", d.cond)?;
    o.key(false, "iterations")?;
    o.i(usize_u64(d.iterations))?;
    o.key(false, "stop")?;
    o.s("\"")?;
    o.s(d.stop.as_str())?;
    o.s("\"}")?;
    o.key(false, "certificate")?;
    match &r.certificate {
        None => o.s("null")?,
        Some(c) => {
            o.s("{")?;
            o.key(true, "support")?;
            o.s("[")?;
            for (i, &s) in c.support.iter().enumerate() {
                if i > 0 {
                    o.s(",")?;
                }
                o.i(usize_u64(s))?;
            }
            o.s("]")?;
            o.ff("gap", c.gap)?;
            o.s("}")?;
        }
    }
    o.s("}")?;
    Ok(o.buf)
}
