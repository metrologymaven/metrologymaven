//! Bounded pure-Rust RFC 1950 (zlib) and RFC 1951 (DEFLATE) decoder for PDF structure
//! streams, PNG IDAT streams, and TIFF Deflate strips (`IO.REG.PLUGIN.DRAW`, CONSTITUTION §8).

use std::fmt;

/// Errors returned while decoding a DEFLATE/zlib stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlateError {
    Truncated,
    BadHeader,
    BadBlock,
    BadHuffman,
    BadDistance,
    OutputLimitExceeded,
    ChecksumMismatch,
}

impl fmt::Display for FlateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            FlateError::Truncated => "truncated deflate/zlib stream",
            FlateError::BadHeader => "invalid zlib header",
            FlateError::BadBlock => "invalid deflate block header",
            FlateError::BadHuffman => "invalid deflate Huffman table or code",
            FlateError::BadDistance => "invalid deflate back-reference distance",
            FlateError::OutputLimitExceeded => "decompressed stream exceeds maximum bound",
            FlateError::ChecksumMismatch => "zlib Adler-32 checksum mismatch",
        };
        f.write_str(s)
    }
}

impl std::error::Error for FlateError {}

const LEN_BASE: [u16; 29] =
    [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEN_EXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145,
    8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] =
    [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];
const MAX_HUFF_BITS: usize = 15;

fn adler32(data: &[u8]) -> u32 {
    const MOD: u32 = 65521;
    let mut s1: u32 = 1;
    let mut s2: u32 = 0;
    for chunk in data.chunks(5552) {
        for &b in chunk {
            s1 += u32::from(b);
            s2 += s1;
        }
        s1 %= MOD;
        s2 %= MOD;
    }
    (s2 << 16) | s1
}

struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    buf: u32,
    nbits: u8,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0, buf: 0, nbits: 0 }
    }

    fn ensure(&mut self, n: u8) -> Result<(), FlateError> {
        while self.nbits < n {
            let b = *self.data.get(self.pos).ok_or(FlateError::Truncated)?;
            self.pos += 1;
            self.buf |= u32::from(b) << self.nbits;
            self.nbits += 8;
        }
        Ok(())
    }

    fn read_bits(&mut self, n: u8) -> Result<u32, FlateError> {
        if n == 0 {
            return Ok(0);
        }
        self.ensure(n)?;
        let val = self.buf & ((1u32 << n) - 1);
        self.buf >>= n;
        self.nbits -= n;
        Ok(val)
    }

    fn align_byte(&mut self) {
        let drop = self.nbits & 7;
        self.buf >>= drop;
        self.nbits -= drop;
    }

    fn read_u16_le(&mut self) -> Result<u16, FlateError> {
        let lo = self.read_bits(8)? as u16;
        let hi = self.read_bits(8)? as u16;
        Ok(lo | (hi << 8))
    }

    fn read_exact_bytes(&mut self, len: usize) -> Result<Vec<u8>, FlateError> {
        self.align_byte();
        let mut out = Vec::with_capacity(len);
        while out.len() < len && self.nbits >= 8 {
            out.push((self.buf & 0xFF) as u8);
            self.buf >>= 8;
            self.nbits -= 8;
        }
        let rem = len - out.len();
        let slice = self.data.get(self.pos..self.pos + rem).ok_or(FlateError::Truncated)?;
        self.pos += rem;
        out.extend_from_slice(slice);
        Ok(out)
    }

    fn remaining_bytes(&mut self) -> &'a [u8] {
        self.align_byte();
        let rewind = usize::from(self.nbits / 8);
        self.pos = self.pos.saturating_sub(rewind);
        self.buf = 0;
        self.nbits = 0;
        &self.data[self.pos..]
    }
}

struct HuffTable {
    counts: [u16; MAX_HUFF_BITS + 1],
    symbols: Vec<u16>,
    max_bits: u8,
}

impl HuffTable {
    fn from_lengths(lengths: &[u8]) -> Result<Self, FlateError> {
        let mut counts = [0u16; MAX_HUFF_BITS + 1];
        let mut max_bits = 0u8;
        for &len in lengths {
            if usize::from(len) > MAX_HUFF_BITS {
                return Err(FlateError::BadHuffman);
            }
            if len > 0 {
                counts[usize::from(len)] = counts[usize::from(len)].checked_add(1).ok_or(FlateError::BadHuffman)?;
                if len > max_bits {
                    max_bits = len;
                }
            }
        }
        if max_bits == 0 {
            return Ok(Self { counts, symbols: Vec::new(), max_bits: 0 });
        }
        let mut left: i32 = 1;
        for &c in &counts[1..=usize::from(max_bits)] {
            left = (left << 1) - i32::from(c);
            if left < 0 {
                return Err(FlateError::BadHuffman);
            }
        }
        let mut offsets = [0usize; MAX_HUFF_BITS + 1];
        let mut total = 0usize;
        for b in 1..=usize::from(max_bits) {
            offsets[b] = total;
            total += usize::from(counts[b]);
        }
        let mut symbols = vec![0u16; total];
        for (sym, &len) in lengths.iter().enumerate() {
            if len > 0 {
                let idx = offsets[usize::from(len)];
                symbols[idx] = sym as u16;
                offsets[usize::from(len)] += 1;
            }
        }
        Ok(Self { counts, symbols, max_bits })
    }

    fn decode(&self, br: &mut BitReader<'_>) -> Result<u16, FlateError> {
        let mut code: i32 = 0;
        let mut first: i32 = 0;
        let mut index: usize = 0;
        for len in 1..=usize::from(self.max_bits) {
            code |= br.read_bits(1)? as i32;
            let count = i32::from(self.counts[len]);
            if code - count < first {
                return self.symbols.get(index + (code - first) as usize).copied().ok_or(FlateError::BadHuffman);
            }
            index += count as usize;
            first = (first + count) << 1;
            code <<= 1;
        }
        Err(FlateError::BadHuffman)
    }
}

fn fixed_tables() -> Result<(HuffTable, HuffTable), FlateError> {
    let mut ll = [0u8; 288];
    ll[..144].fill(8);
    ll[144..256].fill(9);
    ll[256..280].fill(7);
    ll[280..288].fill(8);
    let dist = [5u8; 32];
    Ok((HuffTable::from_lengths(&ll)?, HuffTable::from_lengths(&dist)?))
}

fn read_dynamic_tables(br: &mut BitReader<'_>) -> Result<(HuffTable, HuffTable), FlateError> {
    const CL_ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
    let hlit = br.read_bits(5)? as usize + 257;
    let hdist = br.read_bits(5)? as usize + 1;
    let hclen = br.read_bits(4)? as usize + 4;
    if hlit > 286 || hdist > 30 {
        return Err(FlateError::BadHuffman);
    }
    let mut cl_lens = [0u8; 19];
    for &idx in CL_ORDER.iter().take(hclen) {
        cl_lens[idx] = br.read_bits(3)? as u8;
    }
    let cl_table = HuffTable::from_lengths(&cl_lens)?;
    let total = hlit + hdist;
    let mut lens = Vec::with_capacity(total);
    while lens.len() < total {
        let sym = cl_table.decode(br)?;
        match sym {
            0..=15 => lens.push(sym as u8),
            16 => {
                let prev = *lens.last().ok_or(FlateError::BadHuffman)?;
                let repeat = br.read_bits(2)? as usize + 3;
                if lens.len() + repeat > total {
                    return Err(FlateError::BadHuffman);
                }
                lens.resize(lens.len() + repeat, prev);
            }
            17 => {
                let repeat = br.read_bits(3)? as usize + 3;
                if lens.len() + repeat > total {
                    return Err(FlateError::BadHuffman);
                }
                lens.resize(lens.len() + repeat, 0);
            }
            18 => {
                let repeat = br.read_bits(7)? as usize + 11;
                if lens.len() + repeat > total {
                    return Err(FlateError::BadHuffman);
                }
                lens.resize(lens.len() + repeat, 0);
            }
            _ => return Err(FlateError::BadHuffman),
        }
    }
    Ok((HuffTable::from_lengths(&lens[..hlit])?, HuffTable::from_lengths(&lens[hlit..])?))
}

fn inflate_raw(br: &mut BitReader<'_>, max_out: usize) -> Result<Vec<u8>, FlateError> {
    let mut out = Vec::new();
    loop {
        let bfinal = br.read_bits(1)?;
        let btype = br.read_bits(2)?;
        match btype {
            0 => {
                br.align_byte();
                let len = br.read_u16_le()?;
                let nlen = br.read_u16_le()?;
                if len != !nlen {
                    return Err(FlateError::BadBlock);
                }
                let len_usize = usize::from(len);
                if out.len().saturating_add(len_usize) > max_out {
                    return Err(FlateError::OutputLimitExceeded);
                }
                let bytes = br.read_exact_bytes(len_usize)?;
                out.extend_from_slice(&bytes);
            }
            1 | 2 => {
                let (lit_table, dist_table) = if btype == 1 { fixed_tables()? } else { read_dynamic_tables(br)? };
                loop {
                    let sym = lit_table.decode(br)?;
                    match sym {
                        0..=255 => {
                            if out.len() >= max_out {
                                return Err(FlateError::OutputLimitExceeded);
                            }
                            out.push(sym as u8);
                        }
                        256 => break,
                        257..=285 => {
                            let li = usize::from(sym - 257);
                            let length = usize::from(LEN_BASE[li]) + br.read_bits(LEN_EXTRA[li])? as usize;
                            let dsym = dist_table.decode(br)? as usize;
                            if dsym >= DIST_BASE.len() {
                                return Err(FlateError::BadDistance);
                            }
                            let dist = usize::from(DIST_BASE[dsym]) + br.read_bits(DIST_EXTRA[dsym])? as usize;
                            if dist == 0 || dist > out.len() {
                                return Err(FlateError::BadDistance);
                            }
                            if out.len().saturating_add(length) > max_out {
                                return Err(FlateError::OutputLimitExceeded);
                            }
                            for _ in 0..length {
                                let b = out[out.len() - dist];
                                out.push(b);
                            }
                        }
                        _ => return Err(FlateError::BadHuffman),
                    }
                }
            }
            _ => return Err(FlateError::BadBlock),
        }
        if bfinal != 0 {
            break;
        }
    }
    Ok(out)
}

/// Decompresses an RFC 1950 zlib stream with strict `max_out` cap and Adler-32 verification.
pub fn zlib_decompress(data: &[u8], max_out: usize) -> Result<Vec<u8>, FlateError> {
    if data.len() < 6 {
        return Err(FlateError::Truncated);
    }
    let cmf = data[0];
    let flg = data[1];
    if (cmf & 0x0F) != 8 || (cmf >> 4) > 7 {
        return Err(FlateError::BadHeader);
    }
    if ((u16::from(cmf) << 8) | u16::from(flg)) % 31 != 0 || (flg & 0x20) != 0 {
        return Err(FlateError::BadHeader);
    }
    let mut br = BitReader::new(&data[2..]);
    let out = inflate_raw(&mut br, max_out)?;
    let rem = br.remaining_bytes();
    if rem.len() < 4 {
        return Err(FlateError::Truncated);
    }
    let expected = u32::from_be_bytes([rem[0], rem[1], rem[2], rem[3]]);
    if adler32(&out) != expected {
        return Err(FlateError::ChecksumMismatch);
    }
    Ok(out)
}
