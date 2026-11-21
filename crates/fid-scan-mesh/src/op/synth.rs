//! The per-triangle `synthesized` attribute (REQ-SCAN.MESHOP.PROV-003, -004).

use super::MeshOpError;

/// One bit per triangle, `true` when the triangle is synthesized (not backed by measured
/// data). Bit-packed in `u64` words, triangle `t` at bit `t % 64` of word `t / 64`; the
/// bits past [`Synthesized::len`] are always zero, so equality is equality of the flags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Synthesized {
    words: Vec<u64>,
    len: u64,
}

impl Synthesized {
    /// No flags (zero triangles).
    pub fn new() -> Synthesized {
        Synthesized { words: Vec::new(), len: 0 }
    }

    /// `len` flags, all false (fully measured).
    pub fn all_false(len: u64) -> Result<Synthesized, MeshOpError> {
        let words = usize::try_from(len.div_ceil(64)).map_err(|_| MeshOpError::AllocationFailed)?;
        let mut v = Vec::new();
        v.try_reserve_exact(words).map_err(|_| MeshOpError::AllocationFailed)?;
        v.resize(words, 0);
        Ok(Synthesized { words: v, len })
    }

    /// The flags of `bits`, in order.
    pub fn from_bools(bits: &[bool]) -> Result<Synthesized, MeshOpError> {
        let mut s = Synthesized::new();
        s.reserve(bits.len() as u64)?;
        for &b in bits {
            s.push(b)?;
        }
        Ok(s)
    }

    /// The number of flags.
    pub fn len(&self) -> u64 {
        self.len
    }

    /// Whether there are no flags.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The flag of triangle `t`; `None` when `t >= len`.
    pub fn get(&self, t: u64) -> Option<bool> {
        if t >= self.len {
            return None;
        }
        let w = self.words.get(usize::try_from(t / 64).ok()?)?;
        Some((w >> (t % 64)) & 1 == 1)
    }

    /// The number of true flags (summed word by word, in order).
    pub fn count(&self) -> u64 {
        self.words.iter().map(|w| u64::from(w.count_ones())).sum()
    }

    /// The flags as bytes, triangle `t` at bit `t % 8` (least significant first) of byte
    /// `t / 8`, `ceil(len / 8)` bytes, unused high bits zero. This is the form hashed into
    /// the derived-mesh digest.
    pub fn to_bytes(&self) -> Result<Vec<u8>, MeshOpError> {
        let n = usize::try_from(self.len.div_ceil(8)).map_err(|_| MeshOpError::AllocationFailed)?;
        let mut out = Vec::new();
        out.try_reserve_exact(n).map_err(|_| MeshOpError::AllocationFailed)?;
        for w in &self.words {
            for b in w.to_le_bytes() {
                if out.len() < n {
                    out.push(b);
                }
            }
        }
        Ok(out)
    }

    pub(crate) fn reserve(&mut self, extra: u64) -> Result<(), MeshOpError> {
        let total = self.len.checked_add(extra).ok_or(MeshOpError::AllocationFailed)?;
        let words = usize::try_from(total.div_ceil(64)).map_err(|_| MeshOpError::AllocationFailed)?;
        let more = words.saturating_sub(self.words.len());
        self.words.try_reserve(more).map_err(|_| MeshOpError::AllocationFailed)
    }

    pub(crate) fn push(&mut self, b: bool) -> Result<(), MeshOpError> {
        if self.len % 64 == 0 {
            self.words.try_reserve(1).map_err(|_| MeshOpError::AllocationFailed)?;
            self.words.push(0);
        }
        if b {
            let i = usize::try_from(self.len / 64).map_err(|_| MeshOpError::AllocationFailed)?;
            if let Some(w) = self.words.get_mut(i) {
                *w |= 1u64 << (self.len % 64);
            }
        }
        self.len += 1;
        Ok(())
    }
}

impl Default for Synthesized {
    fn default() -> Self {
        Synthesized::new()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn packing() {
        let mut bits = vec![false; 70];
        bits[0] = true;
        bits[9] = true;
        bits[64] = true;
        bits[69] = true;
        let s = Synthesized::from_bools(&bits).unwrap();
        assert_eq!(s.len(), 70);
        assert_eq!(s.count(), 4);
        for (i, &b) in bits.iter().enumerate() {
            assert_eq!(s.get(i as u64), Some(b), "{i}");
        }
        assert_eq!(s.get(70), None);
        // Hand-labelled bytes: bit 0 -> byte0 0x01; bit 9 -> byte1 0x02; bit 64 -> byte8
        // 0x01; bit 69 -> byte8 0x20; 9 bytes in all.
        assert_eq!(s.to_bytes().unwrap(), vec![0x01, 0x02, 0, 0, 0, 0, 0, 0, 0x21]);
        assert_eq!(Synthesized::all_false(70).unwrap(), Synthesized::from_bools(&[false; 70]).unwrap());
        assert_eq!(Synthesized::all_false(0).unwrap(), Synthesized::default());
        assert!(Synthesized::all_false(0).unwrap().to_bytes().unwrap().is_empty());
    }
}
