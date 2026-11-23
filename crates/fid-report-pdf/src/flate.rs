//! Pure-Rust, zero-float, deterministic RFC 1950 (zlib) + RFC 1951 (DEFLATE) codec.
//!
//! - [`zlib_compress`] uses fixed-parameter LZ77 + RFC 1951 fixed Huffman coding (falling back
//!   to stored blocks when smaller) with an Adler-32 trailer. Output is 100% byte-identical
//!   across x86-64, ARM64, and wasm32 forever.
//! - [`zlib_decompress`] decodes all RFC 1951 block types (stored, fixed Huffman, dynamic
//!   Huffman) with a strict output byte cap (`max_out`) and Adler-32 check so hostile PNG
//!   payloads cannot OOM or panic (CONSTITUTION §8).

use std::fmt;

/// Errors returned by [`zlib_decompress`].
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
        let msg = match self {
            FlateError::Truncated => "truncated zlib/deflate stream",
            FlateError::BadHeader => "invalid zlib header",
            FlateError::BadBlock => "invalid deflate block header or stored length",
            FlateError::BadHuffman => "invalid deflate Huffman table or code",
            FlateError::BadDistance => "invalid deflate back-reference distance",
            FlateError::OutputLimitExceeded => "decompressed stream exceeds maximum byte bound",
            FlateError::ChecksumMismatch => "zlib Adler-32 checksum mismatch",
        };
        f.write_str(msg)
    }
}

impl std::error::Error for FlateError {}

/// Computes RFC 1950 Adler-32 checksum using pure integer arithmetic.
pub fn adler32(data: &[u8]) -> u32 {
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

// ------------------------------------------------------------------ deterministic compressor

const HASH_BITS: usize = 12;
const HASH_SIZE: usize = 1 << HASH_BITS;
const WINDOW_SIZE: usize = 32_768;
const WINDOW_MASK: usize = WINDOW_SIZE - 1;
const MAX_MATCH: usize = 258;
const MIN_MATCH: usize = 3;
const MAX_CHAIN: usize = 16;

const LEN_BASE: [u16; 29] =
    [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEN_EXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];

const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145,
    8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] =
    [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

struct BitWriter {
    out: Vec<u8>,
    buf: u32,
    nbits: u8,
}

impl BitWriter {
    fn new(cap: usize) -> BitWriter {
        BitWriter { out: Vec::with_capacity(cap), buf: 0, nbits: 0 }
    }

    fn write_bits(&mut self, val: u32, count: u8) {
        self.buf |= val << self.nbits;
        self.nbits += count;
        while self.nbits >= 8 {
            self.out.push((self.buf & 0xFF) as u8);
            self.buf >>= 8;
            self.nbits -= 8;
        }
    }

    fn write_huff_msb(&mut self, code: u16, bits: u8) {
        // RFC 1951 Huffman codes are packed MSB-first into the LSB-first bitstream.
        let mut rev: u32 = 0;
        for i in 0..bits {
            if ((code >> (bits - 1 - i)) & 1) != 0 {
                rev |= 1u32 << i;
            }
        }
        self.write_bits(rev, bits);
    }

    fn write_fixed_litlen(&mut self, sym: u16) {
        match sym {
            0..=143 => self.write_huff_msb(0b0011_0000 + sym, 8),
            144..=255 => self.write_huff_msb(0b1_1001_0000 + (sym - 144), 9),
            256..=279 => self.write_huff_msb(sym - 256, 7),
            280..=287 => self.write_huff_msb(0b1100_0000 + (sym - 280), 8),
            _ => {}
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.nbits > 0 {
            self.out.push((self.buf & 0xFF) as u8);
        }
        self.out
    }
}

fn hash3(data: &[u8], pos: usize) -> usize {
    let v = (usize::from(data[pos]) << 16) | (usize::from(data[pos + 1]) << 8) | usize::from(data[pos + 2]);
    ((v.wrapping_mul(0x1E35_A7BD)) >> (32 - HASH_BITS)) & (HASH_SIZE - 1)
}

fn encode_length(len: usize) -> (u16, u8, u32) {
    for i in (0..LEN_BASE.len()).rev() {
        let base = usize::from(LEN_BASE[i]);
        if len >= base {
            return (257 + i as u16, LEN_EXTRA[i], (len - base) as u32);
        }
    }
    (257, 0, 0)
}

fn encode_distance(dist: usize) -> (u16, u8, u32) {
    for i in (0..DIST_BASE.len()).rev() {
        let base = usize::from(DIST_BASE[i]);
        if dist >= base {
            return (i as u16, DIST_EXTRA[i], (dist - base) as u32);
        }
    }
    (0, 0, 0)
}

fn deflate_fixed_lz77(data: &[u8]) -> Vec<u8> {
    let mut bw = BitWriter::new(data.len() / 2 + 16);
    // BFINAL=1, BTYPE=01 (fixed Huffman).
    bw.write_bits(1, 1);
    bw.write_bits(1, 2);

    let mut head = vec![u32::MAX; HASH_SIZE];
    let mut prev = vec![u32::MAX; WINDOW_SIZE];

    let n = data.len();
    let mut pos = 0usize;

    while pos < n {
        let mut best_len = 0usize;
        let mut best_dist = 0usize;

        if pos + MIN_MATCH <= n {
            let h = hash3(data, pos);
            let mut cand = head[h];
            prev[pos & WINDOW_MASK] = cand;
            head[h] = pos as u32;

            let max_len = MAX_MATCH.min(n - pos);
            let mut steps = 0usize;
            while cand != u32::MAX && steps < MAX_CHAIN {
                let cpos = cand as usize;
                let dist = pos.saturating_sub(cpos);
                if dist == 0 || dist > WINDOW_SIZE {
                    break;
                }
                if data[cpos] == data[pos] && data[cpos + 1] == data[pos + 1] && data[cpos + 2] == data[pos + 2] {
                    let mut mlen = 3usize;
                    while mlen < max_len && data[cpos + mlen] == data[pos + mlen] {
                        mlen += 1;
                    }
                    if mlen > best_len {
                        best_len = mlen;
                        best_dist = dist;
                        if best_len == max_len {
                            break;
                        }
                    }
                }
                let next_cand = prev[cpos & WINDOW_MASK];
                if next_cand >= cand {
                    break;
                }
                cand = next_cand;
                steps += 1;
            }
        }

        if best_len >= MIN_MATCH {
            let (lsym, lextra, lval) = encode_length(best_len);
            bw.write_fixed_litlen(lsym);
            if lextra > 0 {
                bw.write_bits(lval, lextra);
            }
            let (dsym, dextra, dval) = encode_distance(best_dist);
            bw.write_huff_msb(dsym, 5);
            if dextra > 0 {
                bw.write_bits(dval, dextra);
            }
            // Insert skipped positions into the hash chain deterministically.
            let end_skip = (pos + best_len).min(n.saturating_sub(MIN_MATCH - 1));
            for p in (pos + 1)..end_skip {
                let h = hash3(data, p);
                prev[p & WINDOW_MASK] = head[h];
                head[h] = p as u32;
            }
            pos += best_len;
        } else {
            bw.write_fixed_litlen(u16::from(data[pos]));
            pos += 1;
        }
    }

    // End-of-block symbol 256.
    bw.write_fixed_litlen(256);
    bw.finish()
}

fn deflate_stored(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + (data.len() / 65535 + 1) * 5);
    if data.is_empty() {
        out.extend_from_slice(&[0x01, 0x00, 0x00, 0xFF, 0xFF]);
        return out;
    }
    let chunks: Vec<&[u8]> = data.chunks(65535).collect();
    for (i, chunk) in chunks.iter().enumerate() {
        let is_final = i + 1 == chunks.len();
        out.push(u8::from(is_final));
        let len = chunk.len() as u16;
        let nlen = !len;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&nlen.to_le_bytes());
        out.extend_from_slice(chunk);
    }
    out
}

/// Deterministically compresses `data` into an RFC 1950 zlib stream (`/FlateDecode`).
pub fn zlib_compress(data: &[u8]) -> Vec<u8> {
    let compressed = deflate_fixed_lz77(data);
    let stored = if compressed.len() > data.len() + 5 { Some(deflate_stored(data)) } else { None };
    let payload = match stored.as_ref() {
        Some(s) if s.len() < compressed.len() => s.as_slice(),
        _ => compressed.as_slice(),
    };
    let mut out = Vec::with_capacity(2 + payload.len() + 4);
    // CMF=0x78 (DEFLATE, 32K window), FLG=0x9C (default level, FCHECK valid: 0x789C % 31 == 0).
    out.push(0x78);
    out.push(0x9C);
    out.extend_from_slice(payload);
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

// ------------------------------------------------------------------ bounded decompressor

struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    buf: u32,
    nbits: u8,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> BitReader<'a> {
        BitReader { data, pos: 0, buf: 0, nbits: 0 }
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

const MAX_HUFF_BITS: usize = 15;

struct HuffTable {
    counts: [u16; MAX_HUFF_BITS + 1],
    symbols: Vec<u16>,
    max_bits: u8,
}

impl HuffTable {
    fn from_lengths(lengths: &[u8]) -> Result<HuffTable, FlateError> {
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
            return Ok(HuffTable { counts, symbols: Vec::new(), max_bits: 0 });
        }
        // Check over-subscribed tree.
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
        Ok(HuffTable { counts, symbols, max_bits })
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

/// Decompresses an RFC 1950 zlib stream with a strict `max_out` byte bound and Adler-32 check.
pub fn zlib_decompress(data: &[u8], max_out: usize) -> Result<Vec<u8>, FlateError> {
    if data.len() < 6 {
        return Err(FlateError::Truncated);
    }
    let cmf = data[0];
    let flg = data[1];
    if (cmf & 0x0F) != 8 || (cmf >> 4) > 7 {
        return Err(FlateError::BadHeader);
    }
    if ((u16::from(cmf) << 8) | u16::from(flg)) % 31 != 0 {
        return Err(FlateError::BadHeader);
    }
    if (flg & 0x20) != 0 {
        // Preset dictionary (FDICT) is not permitted in PNG or PDF Flate streams.
        return Err(FlateError::BadHeader);
    }

    let mut br = BitReader::new(&data[2..]);
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
                let (lit_table, dist_table) = if btype == 1 { fixed_tables()? } else { read_dynamic_tables(&mut br)? };
                loop {
                    let sym = lit_table.decode(&mut br)?;
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
                            let dsym = dist_table.decode(&mut br)? as usize;
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{zlib_compress, zlib_decompress, FlateError};

    #[test]
    fn round_trip_and_determinism() {
        let mut input = Vec::new();
        for i in 0..2000u32 {
            input.extend_from_slice(format!("Metrology Maven row {i:04} 11.001 mm\n").as_bytes());
        }
        let c1 = zlib_compress(&input);
        let c2 = zlib_compress(&input);
        assert_eq!(c1, c2);
        assert!(c1.len() < input.len(), "expected compression ratio < 1");
        let dec = zlib_decompress(&c1, input.len()).unwrap();
        assert_eq!(dec, input);
        assert_eq!(zlib_decompress(&c1, 100).unwrap_err(), FlateError::OutputLimitExceeded);
    }
}
