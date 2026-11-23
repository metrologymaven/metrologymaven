//! Canonical bytes and output digest of an [`MzFlatResult`].
//!
//! Compact JSON, ASCII, keys in fixed order:
//!
//! ```text
//! {"algorithm":"<id>","feature":<line2|plane>,"width":<f>,
//!  "critical_points":["<pt>:<side>"...],"residuals":[<f>...],
//!  "stats":{"n":<i>,"rms":<f>,"max":<f>,"min":<f>,"mean":<f>,"std_dev":<f>},
//!  "local_optimum_only":true|false,"certificate":null|"sha256:<64 hex>","seed":null}
//! ```
//!
//! `<f>` is the `F64Bits` form (`"0x<16 lowercase hex>"`) and `<i>` a decimal integer string.
//! Only certified result values enter the digest; solver iterates and iteration counts are
//! excluded (REQ-KRN.SOLV-006, REQ-KRN.MZ.LIN.FLAT-005).

use fid_kci::kci::sha256::sha256_digest;
use fid_num::NumError;
use fid_solv::cert::RowLabel;
use fid_types::Digest;

use super::MzFlatResult;
use crate::contract::{FitError, FittedFeature};

const OP: &str = "fit.mz_flat.encode";

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

    fn u64_raw(&mut self, v: u64) -> Result<(), FitError> {
        let mut d = [0u8; 20];
        let mut pos = d.len();
        let mut x = v;
        loop {
            pos -= 1;
            d[pos] = b'0' + (x % 10) as u8;
            x /= 10;
            if x == 0 {
                break;
            }
        }
        self.b(&d[pos..])
    }

    fn i(&mut self, v: u64) -> Result<(), FitError> {
        self.s("\"")?;
        self.u64_raw(v)?;
        self.s("\"")
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

    fn labels(&mut self, xs: &[RowLabel]) -> Result<(), FitError> {
        self.s("[")?;
        for (k, l) in xs.iter().enumerate() {
            if k > 0 {
                self.s(",")?;
            }
            self.s("\"")?;
            self.u64_raw(u64::from(l.point))?;
            self.s(":")?;
            self.u64_raw(u64::from(l.side))?;
            self.s("\"")?;
        }
        self.s("]")
    }

    fn feature(&mut self, f: &FittedFeature) -> Result<(), FitError> {
        match f {
            FittedFeature::Line2 { p, d } => {
                self.s("{\"kind\":\"line2\",\"p\":")?;
                self.fs(p)?;
                self.s(",\"d\":")?;
                self.fs(d)?;
                self.s("}")
            }
            FittedFeature::Plane { p, n } => {
                self.s("{\"kind\":\"plane\",\"p\":")?;
                self.fs(p)?;
                self.s(",\"n\":")?;
                self.fs(n)?;
                self.s("}")
            }
            _ => Err(FitError::InvalidParams { field: "feature" }),
        }
    }
}

fn usize_u64(x: usize) -> u64 {
    x as u64
}

/// Canonical bytes of `r` (module documentation).
pub fn encode(r: &MzFlatResult) -> Result<Vec<u8>, FitError> {
    let mut o = Out(Vec::new());
    o.s("{\"algorithm\":\"")?;
    o.s(&r.algorithm.to_string())?;
    o.s("\",\"feature\":")?;
    o.feature(&r.feature)?;
    o.s(",\"width\":")?;
    o.f(r.width)?;
    o.s(",\"critical_points\":")?;
    o.labels(&r.critical_points)?;
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

/// The output digest (`sha256:<64 hex>`) of [`encode`] (REQ-KRN.MZ.LIN.FLAT-005).
pub fn output_digest(r: &MzFlatResult) -> Result<Digest, FitError> {
    Ok(sha256_digest(&encode(r)?))
}
