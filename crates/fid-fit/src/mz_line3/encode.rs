//! Canonical bytes and output digest of an [`MzLine3Result`].
//!
//! Compact JSON, ASCII, keys in fixed order:
//!
//! ```text
//! {"algorithm":"<id>","feature":{"kind":"line3","p":[<f>,<f>,<f>],"d":[<f>,<f>,<f>]},
//!  "diameter":<f>,"critical_points":[<i>...],"residuals":[<f>...],
//!  "stats":{"n":<i>,"rms":<f>,"max":<f>,"min":<f>,"mean":<f>,"std_dev":<f>},
//!  "local_optimum_only":true|false,"certificate":null|"sha256:<64 hex>","seed":null}
//! ```
//!
//! `<f>` is `F64Bits` (`"0x<16 lowercase hex>"`) and `<i>` a decimal integer string.
//! Only certified values enter the digest; solver iterates and iteration counts are excluded.

use fid_kci::kci::sha256::sha256_digest;
use fid_num::NumError;
use fid_types::Digest;

use super::MzLine3Result;
use crate::contract::{FitError, FittedFeature};

const OP: &str = "fit.mz_line3.encode";

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
}

fn usize_u64(x: usize) -> u64 {
    x as u64
}

/// Canonical bytes of `r`.
pub fn encode(r: &MzLine3Result) -> Result<Vec<u8>, FitError> {
    let FittedFeature::Line3 { p, d } = &r.feature else {
        return Err(FitError::InvalidParams { field: "feature" });
    };
    let mut o = Out(Vec::new());
    o.s("{\"algorithm\":\"")?;
    o.s(&r.algorithm.to_string())?;
    o.s("\",\"feature\":{\"kind\":\"line3\",\"p\":")?;
    o.fs(p)?;
    o.s(",\"d\":")?;
    o.fs(d)?;
    o.s("},\"diameter\":")?;
    o.f(r.diameter)?;
    o.s(",\"critical_points\":")?;
    o.is(&r.critical_points)?;
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
    o.s(if r.local_optimum_only {
        "},\"local_optimum_only\":true,\"certificate\":"
    } else {
        "},\"local_optimum_only\":false,\"certificate\":"
    })?;
    match &r.certificate {
        None => o.s("null")?,
        Some(c) => {
            o.s("\"")?;
            o.s(c.digest().as_str())?;
            o.s("\"")?;
        }
    }
    o.s(",\"seed\":null}")?;
    Ok(o.0)
}

/// SHA-256 output digest (`sha256:<64 hex>`) of [`encode`].
pub fn output_digest(r: &MzLine3Result) -> Result<Digest, FitError> {
    Ok(sha256_digest(&encode(r)?))
}
