//! The valid-normal bitmask.

use super::error::RegError;

/// One bit per point of a cloud: set when the point's normal is valid (for example when
/// `SCAN.CLOUD.NORM` could estimate it). Bits beyond [`Bitmask::len`] are always clear, so two
/// masks with the same bits compare equal.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Bitmask {
    words: Vec<u64>,
    len: u64,
}

fn word_count(len: u64) -> Result<usize, RegError> {
    usize::try_from(len.div_ceil(64))
        .map_err(|_| RegError::invalid("valid", format!("a mask of {len} bits does not fit in memory")))
}

fn reserve(words: &mut Vec<u64>, n: usize) -> Result<(), RegError> {
    words
        .try_reserve_exact(n)
        .map_err(|_| RegError::AllocationFailed { bytes: u64::try_from(n).unwrap_or(u64::MAX).saturating_mul(8) })
}

impl Bitmask {
    /// A mask from one `bool` per point, in point order.
    pub fn from_bools(bits: &[bool]) -> Result<Bitmask, RegError> {
        let len = u64::try_from(bits.len()).unwrap_or(u64::MAX);
        let mut words = Vec::new();
        reserve(&mut words, word_count(len)?)?;
        for chunk in bits.chunks(64) {
            let mut w = 0u64;
            for (k, &b) in chunk.iter().enumerate() {
                if b {
                    w |= 1u64 << k;
                }
            }
            words.push(w);
        }
        Ok(Bitmask { words, len })
    }

    /// A mask of `len` bits, all set to `value`.
    pub fn filled(len: u64, value: bool) -> Result<Bitmask, RegError> {
        let n = word_count(len)?;
        let mut words = Vec::new();
        reserve(&mut words, n)?;
        words.resize(n, if value { u64::MAX } else { 0 });
        let tail = len % 64;
        if value && tail != 0 {
            if let Some(last) = words.last_mut() {
                *last = (1u64 << tail) - 1;
            }
        }
        Ok(Bitmask { words, len })
    }

    /// The number of bits (points).
    pub fn len(&self) -> u64 {
        self.len
    }

    /// Whether the mask has no bits.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Bit `i`; `false` beyond the end.
    pub fn get(&self, i: u64) -> bool {
        if i >= self.len {
            return false;
        }
        let Ok(w) = usize::try_from(i / 64) else { return false };
        self.words.get(w).is_some_and(|&word| word & (1u64 << (i % 64)) != 0)
    }

    /// The number of set bits.
    pub fn count_ones(&self) -> u64 {
        self.words.iter().map(|w| u64::from(w.count_ones())).sum()
    }
}
