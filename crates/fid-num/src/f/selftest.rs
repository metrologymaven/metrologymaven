//! Cross-target self-test (REQ-KRN.NUM.FN-005). For every function: a fixed list of edge
//! inputs, then splitmix64 draws mapped into the function's domain, 10 000 inputs in all.
//! The generator definition is restated in `fixtures/num/fn-selftest-v1.txt`, whose golden
//! digests come from an independent Python model (`fixtures/num/make_fn_fixtures.py`).
//! `KRN.MAN` records the digest of [`selftest_outputs`].

use super::FnId;

/// Inputs per function.
pub const SELFTEST_INPUTS_PER_FN: usize = 10_000;

/// Output word emitted for an input that returned an error (never a valid result: it is a
/// NaN pattern, and no function returns NaN). Any appearance breaks the golden digest.
pub const SELFTEST_ERROR_BITS: u64 = 0x7FF8_DEAD_BEEF_0001;

const SEED_BASE: u64 = 0x5EED_F00D_0000_0000;
const B: u64 = 1023; // biased exponent of 1.0

/// Domain map per function: (lowest biased exponent, highest, signed), cycled by input index.
fn domains(f: FnId) -> &'static [(u64, u64, bool)] {
    const NARROW_TRIG: (u64, u64, bool) = (B - 30, B + 21, true);
    const ALL_TRIG: (u64, u64, bool) = (1, 2046, true);
    const TRIG: [(u64, u64, bool); 4] = [NARROW_TRIG, NARROW_TRIG, NARROW_TRIG, ALL_TRIG];
    match f {
        FnId::Sqrt | FnId::Ln => &[(1, 2046, false)],
        FnId::Sin | FnId::Cos | FnId::Tan => &TRIG,
        FnId::Asin | FnId::Acos => &[(B - 40, B - 1, true)],
        FnId::Atan | FnId::Atan2 => &[(B - 40, B + 40, true)],
        FnId::Exp => &[(B - 40, B + 9, true)],
        FnId::Hypot => &[(B - 500, B + 500, true)],
        FnId::Cbrt => &[(1, 2046, true)],
    }
}

const TRIG_EDGES: [u64; 29] = [
    0x0000_0000_0000_0000, // +0
    0x8000_0000_0000_0000, // -0
    0x0000_0000_0000_0001, // smallest subnormal
    0x0010_0000_0000_0000, // smallest normal
    0x3E40_0000_0000_0000, // 2^-27
    0x3E46_A09E_0000_0000, // cos small-argument threshold
    0x3E50_0000_0000_0000, // 2^-26
    0x3FE9_21FB_5444_2D18, // pi/4
    0xBFE9_21FB_5444_2D18, // -pi/4
    0x3FE9_21FC_0000_0000, // just above the pi/4 threshold
    0x3FF9_21FB_5444_2D18, // pi/2
    0xBFF9_21FB_5444_2D18, // -pi/2
    0x4009_21FB_5444_2D18, // pi
    0xC009_21FB_5444_2D18, // -pi
    0x4012_D97C_7F33_21D2, // 3pi/2
    0x4019_21FB_5444_2D18, // 2pi
    0x402F_6A7A_2955_385E, // 5pi
    0x4139_21FB_5444_2D18, // 2^20 pi/2 (medium/large boundary)
    0x4139_21FA_0000_0000, // just below it
    0x4139_21FC_0000_0000, // just above it
    0x4415_AF1D_78B5_8C40, // 1e20
    0x7FEF_FFFF_FFFF_FFFF, // largest finite
    0xFFEF_FFFF_FFFF_FFFF, // -largest finite
    0x7E37_E43C_8800_759C, // 1e300
    0x3FF0_0000_0000_0000, // 1
    0x4024_0000_0000_0000, // 10
    0x4480_F0CF_064D_D592, // 1e22
    0x7506_AC5B_262C_A1FF, // 6381956970095103 * 2^797 (closest approach to a multiple of pi/2)
    0xF506_AC5B_262C_A1FF, // its negative
];

fn unary_edges(f: FnId) -> &'static [u64] {
    match f {
        FnId::Sqrt => &[
            0x0,
            0x8000_0000_0000_0000,
            0x1,
            0x0010_0000_0000_0000,
            0x7FEF_FFFF_FFFF_FFFF,
            0x3FF0_0000_0000_0000,
            0x4000_0000_0000_0000,
            0x4010_0000_0000_0000,
            0x3FEF_FFFF_FFFF_FFFF,
        ],
        FnId::Sin | FnId::Cos | FnId::Tan => &TRIG_EDGES,
        FnId::Asin => &[
            0x0,
            0x8000_0000_0000_0000,
            0x1,
            0x3E50_0000_0000_0000,
            0x3FE0_0000_0000_0000,
            0xBFE0_0000_0000_0000,
            0x3FEF_3333_3333_3333,
            0x3FEF_3334_0000_0000,
            0x3FF0_0000_0000_0000,
            0xBFF0_0000_0000_0000,
            0x3FEF_FFFF_FFFF_FFFF,
            0xBFEF_FFFF_FFFF_FFFF,
        ],
        FnId::Acos => &[
            0x0,
            0x8000_0000_0000_0000,
            0x1,
            0x3C60_0000_0000_0000,
            0x3C70_0000_0000_0000,
            0x3FE0_0000_0000_0000,
            0xBFE0_0000_0000_0000,
            0x3FF0_0000_0000_0000,
            0xBFF0_0000_0000_0000,
            0x3FEF_FFFF_FFFF_FFFF,
            0xBFEF_FFFF_FFFF_FFFF,
        ],
        FnId::Atan => &[
            0x0,
            0x8000_0000_0000_0000,
            0x1,
            0x3E40_0000_0000_0000,
            0x3FDC_0000_0000_0000,
            0x3FE6_0000_0000_0000,
            0x3FF3_0000_0000_0000,
            0x4003_8000_0000_0000,
            0x3FF0_0000_0000_0000,
            0xBFF0_0000_0000_0000,
            0x4410_0000_0000_0000,
            0xC410_0000_0000_0000,
            0x7FEF_FFFF_FFFF_FFFF,
            0xFFEF_FFFF_FFFF_FFFF,
        ],
        FnId::Exp => &[
            0x0,
            0x8000_0000_0000_0000,
            0x1,
            0x3E30_0000_0000_0000,
            0x3FD6_2E42_FEFA_39EF,
            0x3FF0_A2B2_3F3B_AB73,
            0x3FF0_0000_0000_0000,
            0xBFF0_0000_0000_0000,
            0x4086_2000_0000_0000,
            0xC086_2000_0000_0000,
            0x4086_2E42_FEFA_39EF,
            0x4085_E000_0000_0000,
            0xC085_E000_0000_0000,
        ],
        FnId::Ln => &[
            0x3FF0_0000_0000_0000,
            0x4000_0000_0000_0000,
            0x3FE0_0000_0000_0000,
            0x1,
            0x0010_0000_0000_0000,
            0x7FEF_FFFF_FFFF_FFFF,
            0x4005_BF0A_8B14_5769,
            0x3FF0_0000_0000_0001,
            0x3FEF_FFFF_FFFF_FFFF,
            0x4024_0000_0000_0000,
            0x3FE6_A09E_667F_3BCD,
        ],
        FnId::Cbrt => &[
            0x0,
            0x8000_0000_0000_0000,
            0x1,
            0x8000_0000_0000_0001,
            0x0010_0000_0000_0000,
            0x7FEF_FFFF_FFFF_FFFF,
            0xFFEF_FFFF_FFFF_FFFF,
            0x403B_0000_0000_0000,
            0xC020_0000_0000_0000,
            0x01A5_6E1F_C2F8_F359,
            0x4000_0000_0000_0000,
        ],
        FnId::Atan2 | FnId::Hypot => &[],
    }
}

fn binary_edges(f: FnId) -> &'static [(u64, u64)] {
    match f {
        FnId::Atan2 => &[
            (0x0, 0x3FF0_0000_0000_0000),
            (0x8000_0000_0000_0000, 0x3FF0_0000_0000_0000),
            (0x0, 0xBFF0_0000_0000_0000),
            (0x8000_0000_0000_0000, 0xBFF0_0000_0000_0000),
            (0x3FF0_0000_0000_0000, 0x0),
            (0xBFF0_0000_0000_0000, 0x0),
            (0x3FF0_0000_0000_0000, 0x3FF0_0000_0000_0000),
            (0x3FF0_0000_0000_0000, 0xBFF0_0000_0000_0000),
            (0xBFF0_0000_0000_0000, 0xBFF0_0000_0000_0000),
            (0x7E37_E43C_8800_759C, 0x01A5_6E1F_C2F8_F359),
            (0x01A5_6E1F_C2F8_F359, 0xFE37_E43C_8800_759C),
            (0x7FEF_FFFF_FFFF_FFFF, 0x7FEF_FFFF_FFFF_FFFF),
            (0x0000_0000_0000_0001, 0x0000_0000_0000_0001),
            (0x3FF0_0000_0000_0000, 0x3B90_0000_0000_0000),
            (0x3B90_0000_0000_0000, 0xBFF0_0000_0000_0000),
            (0x4000_0000_0000_0000, 0x3FF0_0000_0000_0001),
        ],
        FnId::Hypot => &[
            (0x0, 0x0),
            (0x0, 0x8000_0000_0000_0000),
            (0x4008_0000_0000_0000, 0x4010_0000_0000_0000),
            (0xC008_0000_0000_0000, 0x4010_0000_0000_0000),
            (0x7FEF_FFFF_FFFF_FFFF, 0x0),
            (0x7FE1_CCF3_85EB_C8A0, 0x7FAC_7B1F_3CAC_7433),
            (0x1, 0x1),
            (0x3FF0_0000_0000_0000, 0x3B90_0000_0000_0000),
            (0x01A5_6E1F_C2F8_F359, 0x01A5_6E1F_C2F8_F359),
            (0x7FDF_FFFF_FFFF_FFFF, 0x7FDF_FFFF_FFFF_FFFF),
        ],
        _ => &[],
    }
}

fn edge(f: FnId, i: usize) -> Option<(f64, f64)> {
    if f.is_binary() {
        binary_edges(f).get(i).map(|&(a, b)| (f64::from_bits(a), f64::from_bits(b)))
    } else {
        unary_edges(f).get(i).map(|&a| (f64::from_bits(a), 0.0))
    }
}

/// splitmix64 (Steele, Lea, Flood 2014).
fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// sign (bit 63, if signed) | biased exponent lo + (bits 52..62 mod (hi - lo + 1)) | bits 0..51.
fn from_draw(r: u64, (lo, hi, signed): (u64, u64, bool)) -> f64 {
    let e = lo + ((r >> 52) & 0x7FF) % (hi - lo + 1);
    let s = if signed { r >> 63 } else { 0 };
    f64::from_bits((s << 63) | (e << 52) | (r & ((1u64 << 52) - 1)))
}

/// Iterator over the self-test inputs of one function.
#[derive(Debug, Clone)]
pub struct SelftestInputs {
    f: FnId,
    i: usize,
    state: u64,
}

impl Iterator for SelftestInputs {
    type Item = (f64, f64);

    fn next(&mut self) -> Option<(f64, f64)> {
        if self.i >= SELFTEST_INPUTS_PER_FN {
            return None;
        }
        let i = self.i;
        self.i += 1;
        if let Some(e) = edge(self.f, i) {
            return Some(e);
        }
        let doms = domains(self.f);
        let dom = doms[i % doms.len()];
        loop {
            let a = from_draw(splitmix64(&mut self.state), dom);
            let b = if self.f.is_binary() { from_draw(splitmix64(&mut self.state), dom) } else { 0.0 };
            // exp: rejection keeps every result normal and finite.
            if self.f == FnId::Exp && a.abs() > 708.0 {
                continue;
            }
            return Some((a, b));
        }
    }
}

/// The 10 000 self-test inputs of `f` as (a, b); `b` is 0.0 for unary functions and
/// `atan2` takes (y, x).
pub fn selftest_inputs(f: FnId) -> SelftestInputs {
    SelftestInputs { f, i: 0, state: SEED_BASE | f.index() as u64 }
}

/// Output bit patterns of `f` on its self-test inputs, in input order (REQ-KRN.NUM.FN-005).
/// An input that errs yields [`SELFTEST_ERROR_BITS`] (it never happens for a correct build).
pub fn selftest_outputs(f: FnId) -> impl Iterator<Item = u64> {
    selftest_inputs(f).map(move |(a, b)| match f.eval(a, b) {
        Ok(y) => y.to_bits(),
        Err(_) => SELFTEST_ERROR_BITS,
    })
}
