//! Bounded pure-integer RFC 1950 (zlib) + RFC 1951 (DEFLATE) decompressor for PDF content and
//! CMap streams (`RPT.BALLOON.AUTO.TEXT`, `REQ-002`, `AC-3`).
//!
//! Enforces an explicit output byte cap to neutralize Flate stream bombs without panicking.

use super::model::AutoTextError;

/// Hard cap on any single decompressed PDF content/CMap/xref stream (16 MiB).
pub const MAX_DECODED_STREAM_BYTES: usize = 16 * 1024 * 1024;

/// Decompresses an RFC 1950 zlib stream with an explicit `max_out` byte cap.
pub(crate) fn decompress_zlib(input: &[u8], max_out: usize) -> Result<Vec<u8>, AutoTextError> {
    if input.len() < 6 {
        return Err(AutoTextError::PdfMalformed { at: 0, detail: "zlib stream truncated".into() });
    }
    let cmf = input[0];
    let flg = input[1];
    if (cmf & 0x0F) != 8 || (cmf >> 4) > 7 {
        return Err(AutoTextError::PdfMalformed { at: 0, detail: "invalid zlib CMF".into() });
    }
    if ((u16::from(cmf) << 8) | u16::from(flg)) % 31 != 0 {
        return Err(AutoTextError::PdfMalformed { at: 0, detail: "invalid zlib header checksum".into() });
    }
    let mut pos = 2usize;
    if (flg & 0x20) != 0 {
        if input.len() < 10 {
            return Err(AutoTextError::PdfMalformed { at: 2, detail: "truncated FDICT".into() });
        }
        pos = 6;
    }
    let mut r = BitReader::new(&input[pos..]);
    let out = inflate(&mut r, max_out)?;
    Ok(out)
}

struct BitReader<'a> {
    data: &'a [u8],
    byte_pos: usize,
    bit_buf: u32,
    bit_cnt: u32,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        BitReader { data, byte_pos: 0, bit_buf: 0, bit_cnt: 0 }
    }

    fn fill(&mut self, n: u32) {
        while self.bit_cnt < n && self.byte_pos < self.data.len() {
            self.bit_buf |= u32::from(self.data[self.byte_pos]) << self.bit_cnt;
            self.byte_pos += 1;
            self.bit_cnt += 8;
        }
    }

    fn read_bits(&mut self, n: u32) -> Result<u32, AutoTextError> {
        if n == 0 {
            return Ok(0);
        }
        self.fill(n);
        if self.bit_cnt < n {
            return Err(AutoTextError::PdfMalformed {
                at: self.byte_pos as u64,
                detail: "unexpected EOF in deflate stream".into(),
            });
        }
        let mask = (1u32 << n) - 1;
        let val = self.bit_buf & mask;
        self.bit_buf >>= n;
        self.bit_cnt -= n;
        Ok(val)
    }

    fn align_byte(&mut self) {
        let drop = self.bit_cnt & 7;
        self.bit_buf >>= drop;
        self.bit_cnt -= drop;
    }

    fn read_aligned_u16(&mut self) -> Result<u16, AutoTextError> {
        let lo = self.read_bits(8)?;
        let hi = self.read_bits(8)?;
        Ok((lo as u16) | ((hi as u16) << 8))
    }

    fn read_aligned_bytes(&mut self, len: usize) -> Result<Vec<u8>, AutoTextError> {
        let mut out = Vec::with_capacity(len.min(65_536));
        while self.bit_cnt >= 8 && out.len() < len {
            out.push((self.bit_buf & 0xFF) as u8);
            self.bit_buf >>= 8;
            self.bit_cnt -= 8;
        }
        let rem = len - out.len();
        if self.byte_pos + rem > self.data.len() {
            return Err(AutoTextError::PdfMalformed {
                at: self.byte_pos as u64,
                detail: "truncated stored deflate block".into(),
            });
        }
        out.extend_from_slice(&self.data[self.byte_pos..self.byte_pos + rem]);
        self.byte_pos += rem;
        Ok(out)
    }
}

struct HuffTable {
    counts: [u16; 16],
    symbols: Vec<u16>,
}

impl HuffTable {
    fn from_lengths(lengths: &[u8]) -> Result<Self, AutoTextError> {
        let mut counts = [0u16; 16];
        for &l in lengths {
            if usize::from(l) >= 16 {
                return Err(AutoTextError::PdfMalformed { at: 0, detail: "huffman code length > 15".into() });
            }
            if l > 0 {
                counts[usize::from(l)] = counts[usize::from(l)].saturating_add(1);
            }
        }
        let mut offsets = [0u16; 16];
        let mut total = 0u16;
        for bits in 1..16 {
            offsets[bits] = total;
            total = total.saturating_add(counts[bits]);
        }
        let mut symbols = vec![0u16; usize::from(total)];
        for (sym, &l) in lengths.iter().enumerate() {
            if l > 0 {
                let idx = usize::from(offsets[usize::from(l)]);
                if idx >= symbols.len() {
                    return Err(AutoTextError::PdfMalformed { at: 0, detail: "huffman table overflow".into() });
                }
                symbols[idx] = sym as u16;
                offsets[usize::from(l)] += 1;
            }
        }
        Ok(HuffTable { counts, symbols })
    }

    fn decode(&self, r: &mut BitReader<'_>) -> Result<u16, AutoTextError> {
        let mut code: i32 = 0;
        let mut first: i32 = 0;
        let mut index: i32 = 0;
        for len in 1..16 {
            let bit = r.read_bits(1)? as i32;
            code |= bit;
            let count = i32::from(self.counts[len]);
            if code - count < first {
                let pos = (index + (code - first)) as usize;
                return self.symbols.get(pos).copied().ok_or_else(|| AutoTextError::PdfMalformed {
                    at: r.byte_pos as u64,
                    detail: "invalid huffman symbol".into(),
                });
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
        }
        Err(AutoTextError::PdfMalformed { at: r.byte_pos as u64, detail: "invalid huffman code".into() })
    }
}

fn fixed_tables() -> Result<(HuffTable, HuffTable), AutoTextError> {
    let mut ll = [0u8; 288];
    for b in ll.iter_mut().take(144) {
        *b = 8;
    }
    for b in ll.iter_mut().skip(144).take(112) {
        *b = 9;
    }
    for b in ll.iter_mut().skip(256).take(24) {
        *b = 7;
    }
    for b in ll.iter_mut().skip(280).take(8) {
        *b = 8;
    }
    let dist = [5u8; 32];
    Ok((HuffTable::from_lengths(&ll)?, HuffTable::from_lengths(&dist)?))
}

fn inflate(r: &mut BitReader<'_>, max_out: usize) -> Result<Vec<u8>, AutoTextError> {
    const CL_ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
    const LEN_BASE: [u16; 29] = [
        3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227,
        258,
    ];
    const LEN_EXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
    const DIST_BASE: [u16; 30] = [
        1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097,
        6145, 8193, 12289, 16385, 24577,
    ];
    const DIST_EXTRA: [u8; 30] =
        [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

    let mut out = Vec::new();
    let mut blocks = 0usize;
    loop {
        blocks += 1;
        if blocks > 1_000_000 {
            return Err(AutoTextError::LimitExceeded {
                limit: "stream_blocks",
                detail: "too many deflate blocks".into(),
            });
        }
        let bfinal = r.read_bits(1)?;
        let btype = r.read_bits(2)?;
        match btype {
            0 => {
                r.align_byte();
                let len = usize::from(r.read_aligned_u16()?);
                let nlen = usize::from(r.read_aligned_u16()?);
                if (len ^ 0xFFFF) != nlen {
                    return Err(AutoTextError::PdfMalformed {
                        at: r.byte_pos as u64,
                        detail: "stored block LEN/NLEN mismatch".into(),
                    });
                }
                if out.len().saturating_add(len) > max_out {
                    return Err(AutoTextError::LimitExceeded {
                        limit: "stream_bytes",
                        detail: format!("decompressed stream exceeds {max_out} bytes"),
                    });
                }
                let chunk = r.read_aligned_bytes(len)?;
                out.extend_from_slice(&chunk);
            }
            1 | 2 => {
                let (ll_tab, dist_tab) = if btype == 1 {
                    fixed_tables()?
                } else {
                    let hlit = (r.read_bits(5)? as usize) + 257;
                    let hdist = (r.read_bits(5)? as usize) + 1;
                    let hclen = (r.read_bits(4)? as usize) + 4;
                    if hlit > 286 || hdist > 30 {
                        return Err(AutoTextError::PdfMalformed {
                            at: r.byte_pos as u64,
                            detail: "invalid dynamic huffman counts".into(),
                        });
                    }
                    let mut cl_lens = [0u8; 19];
                    for &slot in CL_ORDER.iter().take(hclen) {
                        cl_lens[slot] = r.read_bits(3)? as u8;
                    }
                    let cl_tab = HuffTable::from_lengths(&cl_lens)?;
                    let total = hlit + hdist;
                    let mut lens = Vec::with_capacity(total);
                    while lens.len() < total {
                        let sym = cl_tab.decode(r)?;
                        match sym {
                            0..=15 => lens.push(sym as u8),
                            16 => {
                                let prev = *lens.last().ok_or_else(|| AutoTextError::PdfMalformed {
                                    at: r.byte_pos as u64,
                                    detail: "repeat code 16 with no previous length".into(),
                                })?;
                                let rep = (r.read_bits(2)? as usize) + 3;
                                for _ in 0..rep {
                                    if lens.len() < total {
                                        lens.push(prev);
                                    }
                                }
                            }
                            17 => {
                                let rep = (r.read_bits(3)? as usize) + 3;
                                for _ in 0..rep {
                                    if lens.len() < total {
                                        lens.push(0);
                                    }
                                }
                            }
                            18 => {
                                let rep = (r.read_bits(7)? as usize) + 11;
                                for _ in 0..rep {
                                    if lens.len() < total {
                                        lens.push(0);
                                    }
                                }
                            }
                            _ => {
                                return Err(AutoTextError::PdfMalformed {
                                    at: r.byte_pos as u64,
                                    detail: "invalid code-length symbol".into(),
                                });
                            }
                        }
                    }
                    (HuffTable::from_lengths(&lens[..hlit])?, HuffTable::from_lengths(&lens[hlit..])?)
                };

                loop {
                    let sym = ll_tab.decode(r)?;
                    if sym < 256 {
                        if out.len() >= max_out {
                            return Err(AutoTextError::LimitExceeded {
                                limit: "stream_bytes",
                                detail: format!("decompressed stream exceeds {max_out} bytes"),
                            });
                        }
                        out.push(sym as u8);
                    } else if sym == 256 {
                        break;
                    } else {
                        let li = usize::from(sym - 257);
                        if li >= LEN_BASE.len() {
                            return Err(AutoTextError::PdfMalformed {
                                at: r.byte_pos as u64,
                                detail: "invalid deflate length symbol".into(),
                            });
                        }
                        let length = usize::from(LEN_BASE[li]) + (r.read_bits(u32::from(LEN_EXTRA[li]))? as usize);
                        let di = usize::from(dist_tab.decode(r)?);
                        if di >= DIST_BASE.len() {
                            return Err(AutoTextError::PdfMalformed {
                                at: r.byte_pos as u64,
                                detail: "invalid deflate distance symbol".into(),
                            });
                        }
                        let dist = usize::from(DIST_BASE[di]) + (r.read_bits(u32::from(DIST_EXTRA[di]))? as usize);
                        if dist == 0 || dist > out.len() {
                            return Err(AutoTextError::PdfMalformed {
                                at: r.byte_pos as u64,
                                detail: "deflate back-reference out of range".into(),
                            });
                        }
                        if out.len().saturating_add(length) > max_out {
                            return Err(AutoTextError::LimitExceeded {
                                limit: "stream_bytes",
                                detail: format!("decompressed stream exceeds {max_out} bytes"),
                            });
                        }
                        for _ in 0..length {
                            let b = out[out.len() - dist];
                            out.push(b);
                        }
                    }
                }
            }
            _ => {
                return Err(AutoTextError::PdfMalformed {
                    at: r.byte_pos as u64,
                    detail: "reserved deflate block type 3".into(),
                });
            }
        }
        if bfinal != 0 {
            break;
        }
    }
    Ok(out)
}
