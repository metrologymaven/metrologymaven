//! Canonical bytes and output digest of a [`TanPlaneResult`].
//!
//! Compact JSON, ASCII, keys in this fixed order (shown wrapped):
//!
//! ```text
//! {"algorithm":"<id>","variant":"<slug>","feature":<plane>,"objective":<f>,"side_tol":<f>,
//!  "max_violation":<f>,"contacts":[<i>...],"residuals":[<f>...],
//!  "stats":{"n":<i>,"rms":<f>,"max":<f>,"min":<f>,"mean":<f>,"std_dev":<f>},
//!  "optimality":"<closed_form|strict_local>","certificate":null|"sha256:<64 hex>",
//!  "rock":{"rock_tol":<f>,"selected":<i>,"rocking":true|false,"tie_break":"<t>",
//!          "merged":<i>,"excluded":<i>,
//!          "candidates":[{"contacts":[<i>...],"objective":<f>,"feature":<plane>}...]},
//!  "seed":null|<i>}
//! <plane> = {"kind":"plane","p":[<f>,<f>,<f>],"n":[<f>,<f>,<f>]}
//! ```
//!
//! `<f>` is the `F64Bits` form: a JSON string `0x` followed by the 16 lowercase hexadecimal
//! digits of the IEEE-754 binary64 bits; `<i>` a JSON string of decimal digits. Only values
//! derived from certified parameters and the caller's inputs appear: no iterate, iteration
//! count, start count or per-start history.

use fid_kci::kci::sha256::sha256_digest;
use fid_num::NumError;
use fid_types::Digest;

use super::TanPlaneResult;
use crate::contract::{FitError, FittedFeature};

const OP: &str = "fit.tan_plane.encode";

struct Out(Vec<u8>);

impl Out {
    fn b(&mut self, b: &[u8]) -> Result<(), FitError> {
        self.0
            .try_reserve(b.len())
            .map_err(|_| FitError::Num(NumError::Domain { op: OP, detail: "allocation_failed" }))?;
        self.0.extend_from_slice(b);
        Ok(())
    }

    fn s(&mut self, s: &str) -> Result<(), FitError> {
        self.b(s.as_bytes())
    }

    fn f(&mut self, x: f64) -> Result<(), FitError> {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let bits = x.to_bits();
        let mut b = [b'"'; 20];
        b[1] = b'0';
        b[2] = b'x';
        for (k, slot) in b[3..19].iter_mut().enumerate() {
            *slot = HEX[((bits >> (60 - 4 * k)) & 0xF) as usize];
        }
        self.b(&b)
    }

    fn i(&mut self, v: u64) -> Result<(), FitError> {
        let mut d = [b'"'; 22];
        let mut pos = d.len() - 1;
        let mut x = v;
        loop {
            pos -= 1;
            d[pos] = b'0' + (x % 10) as u8;
            x /= 10;
            if x == 0 {
                break;
            }
        }
        self.b(&d[pos - 1..])
    }

    fn fs(&mut self, xs: &[f64]) -> Result<(), FitError> {
        self.s("[")?;
        for (k, &x) in xs.iter().enumerate() {
            if k > 0 {
                self.s(",")?;
            }
            self.f(x)?;
        }
        self.s("]")
    }

    fn is(&mut self, xs: &[u32]) -> Result<(), FitError> {
        self.s("[")?;
        for (k, &x) in xs.iter().enumerate() {
            if k > 0 {
                self.s(",")?;
            }
            self.i(u64::from(x))?;
        }
        self.s("]")
    }

    fn plane(&mut self, f: &FittedFeature) -> Result<(), FitError> {
        let FittedFeature::Plane { p, n } = f else {
            return Err(FitError::InvalidParams { field: "feature" });
        };
        self.s("{\"kind\":\"plane\",\"p\":")?;
        self.fs(p)?;
        self.s(",\"n\":")?;
        self.fs(n)?;
        self.s("}")
    }
}

fn usize_u64(x: usize) -> u64 {
    // usize is at most 64 bits on every supported target.
    x as u64
}

/// The canonical bytes of `r` (module documentation).
pub fn encode(r: &TanPlaneResult) -> Result<Vec<u8>, FitError> {
    let mut o = Out(Vec::new());
    o.s("{\"algorithm\":\"")?;
    o.s(&r.algorithm.to_string())?;
    o.s("\",\"variant\":\"")?;
    o.s(r.variant.slug())?;
    o.s("\",\"feature\":")?;
    o.plane(&r.feature)?;
    o.s(",\"objective\":")?;
    o.f(r.objective)?;
    o.s(",\"side_tol\":")?;
    o.f(r.side_tol)?;
    o.s(",\"max_violation\":")?;
    o.f(r.max_violation)?;
    o.s(",\"contacts\":")?;
    o.is(&r.contacts)?;
    o.s(",\"residuals\":")?;
    o.fs(&r.residuals)?;
    let st = &r.stats;
    o.s(",\"stats\":{\"n\":")?;
    o.i(usize_u64(st.n))?;
    for (k, x) in [("rms", st.rms), ("max", st.max), ("min", st.min), ("mean", st.mean), ("std_dev", st.std_dev)] {
        o.s(",\"")?;
        o.s(k)?;
        o.s("\":")?;
        o.f(x)?;
    }
    o.s("},\"optimality\":\"")?;
    o.s(r.optimality.as_str())?;
    o.s("\",\"certificate\":")?;
    match &r.certificate {
        None => o.s("null")?,
        Some(c) => {
            o.s("\"")?;
            o.s(c.digest().as_str())?;
            o.s("\"")?;
        }
    }
    let rk = &r.rock;
    o.s(",\"rock\":{\"rock_tol\":")?;
    o.f(rk.rock_tol)?;
    o.s(",\"selected\":")?;
    o.i(u64::from(rk.selected))?;
    o.s(if rk.rocking { ",\"rocking\":true" } else { ",\"rocking\":false" })?;
    o.s(",\"tie_break\":\"")?;
    o.s(rk.tie_break.as_str())?;
    o.s("\",\"merged\":")?;
    o.i(u64::from(rk.merged))?;
    o.s(",\"excluded\":")?;
    o.i(u64::from(rk.excluded))?;
    o.s(",\"candidates\":[")?;
    for (k, c) in rk.candidates.iter().enumerate() {
        o.s(if k == 0 { "{\"contacts\":" } else { ",{\"contacts\":" })?;
        o.is(&c.contacts)?;
        o.s(",\"objective\":")?;
        o.f(c.objective)?;
        o.s(",\"feature\":")?;
        o.plane(&c.feature)?;
        o.s("}")?;
    }
    o.s("]},\"seed\":")?;
    match r.seed {
        None => o.s("null")?,
        Some(s) => o.i(s)?,
    }
    o.s("}")?;
    Ok(o.0)
}

/// The output digest: SHA-256 of [`encode`], as `sha256:<64 hex>` (REQ-005's output digest).
pub fn output_digest(r: &TanPlaneResult) -> Result<Digest, FitError> {
    Ok(sha256_digest(&encode(r)?))
}
