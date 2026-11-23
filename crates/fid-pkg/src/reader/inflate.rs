//! A bounded raw-deflate decoder (RFC 1951) written for hostile input.
//!
//! It is a pull decoder in the style of zlib's reference `puff.c`: the bit reader takes input
//! bytes from a reader on demand and counts them, so the caller knows exactly how much
//! compressed input stands behind the output at every flush. Output is appended to one flat
//! buffer that keeps at least the last 32 KiB as the window; matches and stored blocks are
//! copied in slices, not byte by byte. Every [`FLUSH`] bytes or more (a stored block chunk or
//! one match past the mark) the new output goes to a sink that enforces the size and ratio
//! caps and may stop decoding with an error. Memory is fixed (at most [`KEEP`] plus one flush
//! of output, the input buffer and the Huffman tables) whatever the input. Work is bounded too: every loop step consumes
//! input or produces output, and the sink stops output growth.
//!
//! Errors: any stream that breaks RFC 1951 is `pkg.malformed` `deflate`; running out of input
//! before the final block ends is `member_truncated`.

use super::error::ReadError;
use std::io::{ErrorKind, Read};

/// Largest chunk handed to the sink at once.
pub(crate) const FLUSH: usize = 32 * 1024;
const WINDOW: usize = 32 * 1024;
/// Output kept before the oldest bytes beyond the window are dropped.
pub(crate) const KEEP: usize = 8 * WINDOW;
const MAX_BITS: usize = 15;
const MAX_LCODES: usize = 286;
const MAX_DCODES: usize = 30;
const FIX_LCODES: usize = 288;

const LBASE: [u16; 29] =
    [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEXT: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DBASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145,
    8193, 12289, 16385, 24577,
];
const DEXT: [u8; 30] =
    [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];
const CL_ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

fn bad(what: &str) -> ReadError {
    ReadError::malformed("deflate", what)
}

/// The receiver of decoded bytes: `(chunk, compressed bytes consumed so far)`.
pub(crate) type Sink<'s> = dyn FnMut(&[u8], u64) -> Result<(), ReadError> + 's;

/// A canonical Huffman code: `count[len]` codes of each length and the symbols in code order.
struct Huffman {
    count: [u16; MAX_BITS + 1],
    symbol: Vec<u16>,
}

impl Huffman {
    /// Builds the code for `lengths`. Returns the code and how many codes are left unused
    /// (0: complete; > 0: incomplete). An over-subscribed set is an error.
    fn new(lengths: &[u8]) -> Result<(Huffman, i32), ReadError> {
        let mut count = [0u16; MAX_BITS + 1];
        for &l in lengths {
            let slot = count.get_mut(usize::from(l)).ok_or_else(|| bad("code length above 15"))?;
            *slot += 1;
        }
        let mut left: i32 = 1;
        for &c in count.iter().skip(1) {
            left <<= 1;
            left -= i32::from(c);
            if left < 0 {
                return Err(bad("over-subscribed code"));
            }
        }
        let mut offs = [0u16; MAX_BITS + 1];
        for len in 1..MAX_BITS {
            offs[len + 1] = offs[len] + count[len];
        }
        let mut symbol = vec![0u16; lengths.len()];
        for (sym, &l) in lengths.iter().enumerate() {
            if l != 0 {
                let o = &mut offs[usize::from(l)];
                let slot = symbol.get_mut(usize::from(*o)).ok_or_else(|| bad("code table"))?;
                *slot = u16::try_from(sym).map_err(|_| bad("code table"))?;
                *o += 1;
            }
        }
        Ok((Huffman { count, symbol }, left))
    }
}

/// Decodes one raw deflate stream from `input` into `sink`.
pub(crate) struct Inflater<R: Read> {
    input: R,
    inbuf: Vec<u8>,
    inpos: usize,
    inlen: usize,
    consumed: u64,
    bitbuf: u64,
    bitcnt: u32,
    /// Recent output: at least the last [`WINDOW`] bytes once that many were produced.
    out: Vec<u8>,
    /// Bytes of `out` already handed to the sink.
    flushed: usize,
    total: u64,
}

impl<R: Read> Inflater<R> {
    pub(crate) fn new(input: R) -> Inflater<R> {
        Inflater {
            input,
            inbuf: vec![0; 64 * 1024],
            inpos: 0,
            inlen: 0,
            consumed: 0,
            bitbuf: 0,
            bitcnt: 0,
            out: Vec::with_capacity(KEEP + FLUSH + 512),
            flushed: 0,
            total: 0,
        }
    }

    /// Decodes the whole stream, then checks that `input` holds nothing after it
    /// (`trailing_data`). Returns the number of bytes produced.
    pub(crate) fn run(mut self, sink: &mut Sink<'_>) -> Result<u64, ReadError> {
        loop {
            let last = self.bits(1)? == 1;
            match self.bits(2)? {
                0 => self.stored(sink)?,
                1 => {
                    let (lit, dist) = fixed_tables()?;
                    self.codes(&lit, &dist, sink)?;
                }
                2 => {
                    let (lit, dist) = self.dynamic_tables()?;
                    self.codes(&lit, &dist, sink)?;
                }
                _ => return Err(bad("reserved block type 3")),
            }
            if last {
                break;
            }
        }
        self.flush(sink)?;
        if self.inpos < self.inlen || self.refill()? {
            return Err(ReadError::malformed("trailing_data", "bytes after the final deflate block"));
        }
        Ok(self.total)
    }

    /// Fills the input buffer; `false` at the end of the input.
    fn refill(&mut self) -> Result<bool, ReadError> {
        loop {
            match self.input.read(&mut self.inbuf) {
                Ok(0) => return Ok(false),
                Ok(n) => {
                    self.inpos = 0;
                    self.inlen = n.min(self.inbuf.len());
                    return Ok(true);
                }
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(e) => return Err(ReadError::from_io(&e)),
            }
        }
    }

    fn byte(&mut self) -> Result<u8, ReadError> {
        if self.inpos >= self.inlen && !self.refill()? {
            return Err(ReadError::malformed("member_truncated", "the deflate stream ends early"));
        }
        let b = self.inbuf.get(self.inpos).copied().ok_or_else(|| bad("input buffer"))?;
        self.inpos += 1;
        self.consumed += 1;
        Ok(b)
    }

    fn bits(&mut self, need: u32) -> Result<u32, ReadError> {
        while self.bitcnt < need {
            self.bitbuf |= u64::from(self.byte()?) << self.bitcnt;
            self.bitcnt += 8;
        }
        let v = (self.bitbuf & ((1u64 << need) - 1)) as u32;
        self.bitbuf >>= need;
        self.bitcnt -= need;
        Ok(v)
    }

    /// Flushes once at least [`FLUSH`] bytes wait.
    fn maybe_flush(&mut self, sink: &mut Sink<'_>) -> Result<(), ReadError> {
        if self.out.len() - self.flushed >= FLUSH {
            self.flush(sink)?;
        }
        Ok(())
    }

    /// Hands the new output to the sink, then drops output older than the window once the
    /// buffer holds [`KEEP`] bytes.
    fn flush(&mut self, sink: &mut Sink<'_>) -> Result<(), ReadError> {
        if self.flushed < self.out.len() {
            sink(self.out.get(self.flushed..).unwrap_or_default(), self.consumed)?;
            self.flushed = self.out.len();
        }
        if self.out.len() >= KEEP {
            let cut = self.out.len() - WINDOW;
            self.out.drain(..cut);
            self.flushed -= cut;
        }
        Ok(())
    }

    fn stored(&mut self, sink: &mut Sink<'_>) -> Result<(), ReadError> {
        // Drop the rest of the current byte; the bit buffer never holds a whole unread byte.
        self.bitbuf = 0;
        self.bitcnt = 0;
        let len = u16::from_le_bytes([self.byte()?, self.byte()?]);
        let nlen = u16::from_le_bytes([self.byte()?, self.byte()?]);
        if len != !nlen {
            return Err(bad("stored block length does not match its complement"));
        }
        let mut left = usize::from(len);
        while left > 0 {
            if self.inpos >= self.inlen && !self.refill()? {
                return Err(ReadError::malformed("member_truncated", "the deflate stream ends early"));
            }
            let n = left.min(self.inlen - self.inpos);
            let chunk = self.inbuf.get(self.inpos..self.inpos + n).ok_or_else(|| bad("input buffer"))?;
            self.out.extend_from_slice(chunk);
            self.inpos += n;
            self.consumed += n as u64;
            self.total += n as u64;
            left -= n;
            self.maybe_flush(sink)?;
        }
        Ok(())
    }

    fn decode(&mut self, h: &Huffman) -> Result<u16, ReadError> {
        let (mut code, mut first, mut index): (i32, i32, i32) = (0, 0, 0);
        for len in 1..=MAX_BITS {
            code |= self.bits(1)? as i32;
            let count = i32::from(h.count[len]);
            if code - count < first {
                let at = usize::try_from(index + (code - first)).map_err(|_| bad("code"))?;
                return h.symbol.get(at).copied().ok_or_else(|| bad("code"));
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        Err(bad("code not in the table"))
    }

    fn codes(&mut self, lit: &Huffman, dist: &Huffman, sink: &mut Sink<'_>) -> Result<(), ReadError> {
        loop {
            let sym = self.decode(lit)?;
            if sym < 256 {
                self.out.push(sym as u8);
                self.total += 1;
                self.maybe_flush(sink)?;
                continue;
            }
            if sym == 256 {
                return Ok(());
            }
            let s = usize::from(sym - 257);
            let (base, ext) = match (LBASE.get(s), LEXT.get(s)) {
                (Some(&b), Some(&e)) => (b, e),
                _ => return Err(bad("invalid length symbol")),
            };
            let len = u32::from(base) + self.bits(u32::from(ext))?;
            let ds = usize::from(self.decode(dist)?);
            let (dbase, dext) = match (DBASE.get(ds), DEXT.get(ds)) {
                (Some(&b), Some(&e)) => (b, e),
                _ => return Err(bad("invalid distance symbol")),
            };
            let d = u64::from(dbase) + u64::from(self.bits(u32::from(dext))?);
            if d > self.total {
                return Err(bad("distance before the start of the output"));
            }
            // d <= 32768 <= the kept output whenever d <= total.
            let start = self.out.len().checked_sub(d as usize).ok_or_else(|| bad("window"))?;
            // The copy is periodic with period d, so copying from `start` in growing slices
            // equals copying byte by byte.
            let mut left = len as usize;
            while left > 0 {
                let n = left.min(self.out.len() - start);
                self.out.extend_from_within(start..start + n);
                left -= n;
            }
            self.total += u64::from(len);
            self.maybe_flush(sink)?;
        }
    }

    fn dynamic_tables(&mut self) -> Result<(Huffman, Huffman), ReadError> {
        let nlen = self.bits(5)? as usize + 257;
        let ndist = self.bits(5)? as usize + 1;
        let ncode = self.bits(4)? as usize + 4;
        if nlen > MAX_LCODES || ndist > MAX_DCODES {
            return Err(bad("too many length or distance codes"));
        }
        let mut lengths = [0u8; MAX_LCODES + MAX_DCODES];
        for &i in CL_ORDER.iter().take(ncode) {
            lengths[i] = self.bits(3)? as u8;
        }
        let (lencode, left) = Huffman::new(&lengths[..19])?;
        if left != 0 {
            return Err(bad("incomplete code-length code"));
        }
        let total = nlen + ndist;
        let mut index = 0;
        while index < total {
            let sym = self.decode(&lencode)?;
            if sym < 16 {
                lengths[index] = sym as u8;
                index += 1;
                continue;
            }
            let (value, repeat) = match sym {
                16 => {
                    let prev = index.checked_sub(1).and_then(|i| lengths.get(i)).copied();
                    let prev = prev.ok_or_else(|| bad("repeat with no previous length"))?;
                    (prev, 3 + self.bits(2)? as usize)
                }
                17 => (0, 3 + self.bits(3)? as usize),
                _ => (0, 11 + self.bits(7)? as usize),
            };
            if index + repeat > total {
                return Err(bad("too many code lengths"));
            }
            for slot in lengths.iter_mut().skip(index).take(repeat) {
                *slot = value;
            }
            index += repeat;
        }
        if lengths[256] == 0 {
            return Err(bad("no end-of-block code"));
        }
        let (lit, left) = Huffman::new(&lengths[..nlen])?;
        // As in puff.c: an incomplete code is allowed only when it is a single one-bit code.
        if left > 0 && nlen != usize::from(lit.count[0] + lit.count[1]) {
            return Err(bad("incomplete literal/length code"));
        }
        let (dist, left) = Huffman::new(&lengths[nlen..total])?;
        if left > 0 && ndist != usize::from(dist.count[0] + dist.count[1]) {
            return Err(bad("incomplete distance code"));
        }
        Ok((lit, dist))
    }
}

fn fixed_tables() -> Result<(Huffman, Huffman), ReadError> {
    let mut lengths = [0u8; FIX_LCODES];
    for (i, l) in lengths.iter_mut().enumerate() {
        *l = match i {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            _ => 8,
        };
    }
    let (lit, _) = Huffman::new(&lengths)?;
    let (dist, _) = Huffman::new(&[5u8; MAX_DCODES])?;
    Ok((lit, dist))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn inflate(data: &[u8]) -> Result<Vec<u8>, ReadError> {
        let mut out = Vec::new();
        let mut sink = |chunk: &[u8], _: u64| -> Result<(), ReadError> {
            out.extend_from_slice(chunk);
            Ok(())
        };
        Inflater::new(data).run(&mut sink)?;
        Ok(out)
    }

    fn reason(data: &[u8]) -> &'static str {
        inflate(data).unwrap_err().reason()
    }

    #[test]
    fn stored_and_fixed() {
        assert_eq!(inflate(&[1, 0, 0, 0xff, 0xff]).unwrap(), b"");
        assert_eq!(inflate(&[1, 2, 0, 0xfd, 0xff, b'a', b'b']).unwrap(), b"ab");
        // Fixed-Huffman empty block: BFINAL=1, BTYPE=01, end-of-block (7 zero bits).
        assert_eq!(inflate(&[0x03, 0x00]).unwrap(), b"");
    }

    #[test]
    fn errors() {
        assert_eq!(reason(&[0x07]), "deflate");
        assert_eq!(reason(&[1, 2, 0, 0, 0, b'a', b'b']), "deflate");
        assert_eq!(reason(&[1, 2, 0, 0xfd, 0xff, b'a']), "member_truncated");
        assert_eq!(reason(&[0, 0, 0, 0xff, 0xff]), "member_truncated");
        assert_eq!(reason(&[1, 0, 0, 0xff, 0xff, 0]), "trailing_data");
        assert_eq!(reason(&[]), "member_truncated");
    }

    #[test]
    fn sink_error_stops_decoding() {
        let mut calls = 0;
        let mut sink = |_: &[u8], _: u64| -> Result<(), ReadError> {
            calls += 1;
            Err(ReadError::limit("member_length", "stop"))
        };
        let mut data = vec![0u8];
        data.extend_from_slice(&[0xff, 0xff, 0, 0]);
        data.extend(std::iter::repeat_n(7u8, 0xffff));
        data.extend_from_slice(&[1, 0, 0, 0xff, 0xff]);
        let e = Inflater::new(&data[..]).run(&mut sink).unwrap_err();
        assert_eq!(e.reason(), "member_length");
        assert_eq!(calls, 1);
    }

    #[test]
    fn over_subscribed_code_is_refused() {
        assert!(Huffman::new(&[1, 1, 1]).is_err());
        let (_, left) = Huffman::new(&[1, 1]).unwrap();
        assert_eq!(left, 0);
        let (_, left) = Huffman::new(&[1, 0]).unwrap();
        assert!(left > 0, "an incomplete code leaves codes unused");
    }
}
