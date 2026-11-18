//! Counter-based random number generator. Spec node `KRN.NUM.RNG`.
//!
//! Philox4x32-10 (Salmon, Moraes, Dror, Shaw, "Parallel random numbers: as easy as
//! 1, 2, 3", SC 2011). Output `index` of stream `stream` under key `seed` is a pure
//! function of those three numbers: there is no hidden state, so any draw can be
//! reproduced on any machine (x86-64, ARM64, wasm32) and replayed exactly.
//!
//! Addressing: the 64-bit key is the seed, as (low word, high word). The 128-bit counter is
//! (index low, index high, stream low, stream high). One output index gives one u64:
//! `word1 << 32 | word0` of that block.

use crate::err::NumError;

const M0: u32 = 0xD251_1F53;
const M1: u32 = 0xCD9E_8D57;
const W0: u32 = 0x9E37_79B9;
const W1: u32 = 0xBB67_AE85;
/// 2^-53, exactly representable.
const TWO_POW_M53: f64 = 1.0 / 9_007_199_254_740_992.0;

fn mulhilo(a: u32, b: u32) -> (u32, u32) {
    let p = u64::from(a) * u64::from(b);
    ((p >> 32) as u32, p as u32)
}

/// The Philox4x32-10 block function (REQ-KRN.NUM.RNG-001).
pub fn philox4x32_10(ctr: [u32; 4], key: [u32; 2]) -> [u32; 4] {
    let [mut c0, mut c1, mut c2, mut c3] = ctr;
    let [mut k0, mut k1] = key;
    for round in 0..10 {
        if round > 0 {
            k0 = k0.wrapping_add(W0);
            k1 = k1.wrapping_add(W1);
        }
        let (hi0, lo0) = mulhilo(M0, c0);
        let (hi1, lo1) = mulhilo(M1, c2);
        let n0 = hi1 ^ c1 ^ k0;
        let n2 = hi0 ^ c3 ^ k1;
        c0 = n0;
        c1 = lo1;
        c2 = n2;
        c3 = lo0;
    }
    [c0, c1, c2, c3]
}

/// One block for the project addressing scheme.
pub fn block(seed: u64, stream: u64, index: u64) -> [u32; 4] {
    philox4x32_10(
        [index as u32, (index >> 32) as u32, stream as u32, (stream >> 32) as u32],
        [seed as u32, (seed >> 32) as u32],
    )
}

/// A random stream: a seed and a stream number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Address {
    pub seed: u64,
    pub stream: u64,
}

/// A position in a stream. A plain value: copying it forks the sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Cursor {
    pub addr: Address,
    pub index: u64,
}

impl Cursor {
    /// A cursor at `index` of `addr`.
    pub fn new(addr: Address, index: u64) -> Self {
        Cursor { addr, index }
    }

    /// The u64 at the current index; advances by one.
    pub fn next_u64(&mut self) -> u64 {
        let w = block(self.addr.seed, self.addr.stream, self.index);
        self.index = self.index.wrapping_add(1);
        (u64::from(w[1]) << 32) | u64::from(w[0])
    }

    /// A double in [0, 1): the top 53 bits of one draw times 2^-53, exactly (REQ-002).
    pub fn uniform_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * TWO_POW_M53
    }

    /// A uniform integer in [0, n). Rejection sampling; retries use the next indices of
    /// the same stream (REQ-003).
    pub fn below(&mut self, n: u64) -> Result<u64, NumError> {
        if n == 0 {
            return Err(NumError::Domain { op: "rng.below", detail: "n must be greater than zero" });
        }
        // Values below 2^64 mod n would bias the result; reject them.
        let threshold = n.wrapping_neg() % n;
        loop {
            let x = self.next_u64();
            if x >= threshold {
                return Ok(x % n);
            }
        }
    }
}

/// Fisher-Yates shuffle from the last element down, drawing with `below` (REQ-004).
pub fn shuffle<T>(items: &mut [T], cursor: &mut Cursor) -> Result<(), NumError> {
    for i in (1..items.len()).rev() {
        let j = cursor.below(i as u64 + 1)? as usize;
        items.swap(i, j);
    }
    Ok(())
}
