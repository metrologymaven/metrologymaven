//! Bounded pure-Rust multi-page TIFF drawing decoder (`IO.REG.PLUGIN.DRAW`, REQ-003, REQ-006, AC-5).
//!
//! Supports Little-Endian (`II*\0`) and Big-Endian (`MM\0*`), multi-page IFD chains with cycle
//! detection (`<= 200` pages, `<= 400 Mpx` per page), resolution tags (`XResolution`,
//! `YResolution`, `ResolutionUnit`), horizontal differencing (`Predictor = 2`), and
//! compressions:
//! - `1`: Uncompressed
//! - `2` / `3`: CCITT Modified Huffman / Group 3 (1D)
//! - `4`: CCITT Group 4 / ITU-T T.6 (2D MMR)
//! - `5`: TIFF LZW (MSB-first 9..12-bit)
//! - `8` / `32946`: Deflate (zlib)
//! - `32773`: PackBits

use crate::flate::zlib_decompress;
use crate::{round_half_even, DrawError, PixelFormat, RasterPage, MAX_PAGES, MAX_RASTER_PIXELS};
use std::collections::{BTreeMap, BTreeSet};

const MAX_IFD_ENTRIES: usize = 4_096;
const MAX_STRIP_RAW_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Endian {
    Le,
    Be,
}

impl Endian {
    fn u16(self, b: &[u8]) -> u16 {
        match self {
            Endian::Le => u16::from_le_bytes([b[0], b[1]]),
            Endian::Be => u16::from_be_bytes([b[0], b[1]]),
        }
    }

    fn u32(self, b: &[u8]) -> u32 {
        match self {
            Endian::Le => u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            Endian::Be => u32::from_be_bytes([b[0], b[1], b[2], b[3]]),
        }
    }
}

#[derive(Debug, Clone)]
enum TagVal {
    U32s(Vec<u32>),
    Rationals(Vec<(u32, u32)>),
}

impl TagVal {
    fn first_u32(&self) -> Option<u32> {
        match self {
            TagVal::U32s(v) => v.first().copied(),
            TagVal::Rationals(_) => None,
        }
    }

    fn as_u32s(&self) -> &[u32] {
        match self {
            TagVal::U32s(v) => v.as_slice(),
            TagVal::Rationals(_) => &[],
        }
    }

    fn first_rational(&self) -> Option<(u32, u32)> {
        match self {
            TagVal::Rationals(v) => v.first().copied(),
            TagVal::U32s(_) => None,
        }
    }
}

/// Decodes a single- or multi-page TIFF file into [`RasterPage`]s (`Gray8` or `Rgba8`).
pub fn decode_tiff(bytes: &[u8]) -> Result<Vec<RasterPage>, DrawError> {
    if bytes.len() < 8 {
        return Err(DrawError::truncated("TIFF shorter than 8-byte header", 0));
    }
    let endian = match &bytes[..4] {
        b"II\x2A\x00" => Endian::Le,
        b"MM\x00\x2A" => Endian::Be,
        _ => return Err(DrawError::malformed("invalid TIFF magic bytes", 0)),
    };

    let mut next_ifd = endian.u32(&bytes[4..8]) as usize;
    let mut visited: BTreeSet<usize> = BTreeSet::new();
    let mut pages = Vec::new();

    while next_ifd != 0 {
        if pages.len() >= MAX_PAGES {
            return Err(DrawError::limit_exceeded("pages", format!("TIFF exceeds {MAX_PAGES} pages")));
        }
        if !visited.insert(next_ifd) {
            return Err(DrawError::malformed("cycle detected in TIFF IFD chain", next_ifd as u64));
        }
        let (tags, after_ifd) = parse_ifd(bytes, next_ifd, endian)?;
        let page = decode_ifd_page(bytes, &tags, next_ifd as u64)?;
        pages.push(page);
        next_ifd = after_ifd;
    }

    if pages.is_empty() {
        return Err(DrawError::malformed("TIFF contains zero IFDs", 4));
    }
    Ok(pages)
}

fn parse_ifd(bytes: &[u8], off: usize, endian: Endian) -> Result<(BTreeMap<u16, TagVal>, usize), DrawError> {
    if off + 2 > bytes.len() {
        return Err(DrawError::truncated("TIFF IFD count truncated", off as u64));
    }
    let count = usize::from(endian.u16(&bytes[off..off + 2]));
    if count > MAX_IFD_ENTRIES {
        return Err(DrawError::malformed("TIFF IFD entry count exceeds limit", off as u64));
    }
    let table_end = off
        .checked_add(2)
        .and_then(|v| v.checked_add(count * 12))
        .and_then(|v| v.checked_add(4))
        .ok_or_else(|| DrawError::malformed("TIFF IFD offset overflow", off as u64))?;
    if table_end > bytes.len() {
        return Err(DrawError::truncated("TIFF IFD entries truncated", off as u64));
    }

    let mut tags = BTreeMap::new();
    for i in 0..count {
        let eoff = off + 2 + i * 12;
        let tag = endian.u16(&bytes[eoff..eoff + 2]);
        let typ = endian.u16(&bytes[eoff + 2..eoff + 4]);
        let cnt = endian.u32(&bytes[eoff + 4..eoff + 8]) as usize;
        let val_field = &bytes[eoff + 8..eoff + 12];

        let type_size: usize = match typ {
            1 | 2 | 6 | 7 => 1,
            3 | 8 => 2,
            4 | 9 | 11 => 4,
            5 | 10 | 12 => 8,
            _ => continue,
        };
        let Some(total_bytes) = cnt.checked_mul(type_size) else {
            return Err(DrawError::malformed("TIFF tag byte count overflow", eoff as u64));
        };
        let slice = if total_bytes <= 4 {
            &val_field[..total_bytes]
        } else {
            let data_off = endian.u32(val_field) as usize;
            let data_end = data_off
                .checked_add(total_bytes)
                .ok_or_else(|| DrawError::malformed("TIFF tag offset overflow", eoff as u64))?;
            if data_end > bytes.len() {
                return Err(DrawError::truncated("TIFF tag value extends past EOF", data_off as u64));
            }
            &bytes[data_off..data_end]
        };

        if cnt > 65_536 {
            continue;
        }
        match typ {
            1 => {
                tags.insert(tag, TagVal::U32s(slice.iter().map(|&b| u32::from(b)).collect()));
            }
            3 => {
                let vals = slice.chunks_exact(2).map(|c| u32::from(endian.u16(c))).collect();
                tags.insert(tag, TagVal::U32s(vals));
            }
            4 => {
                let vals = slice.chunks_exact(4).map(|c| endian.u32(c)).collect();
                tags.insert(tag, TagVal::U32s(vals));
            }
            5 => {
                let vals = slice.chunks_exact(8).map(|c| (endian.u32(&c[0..4]), endian.u32(&c[4..8]))).collect();
                tags.insert(tag, TagVal::Rationals(vals));
            }
            _ => {}
        }
    }

    let next_ifd = endian.u32(&bytes[table_end - 4..table_end]) as usize;
    Ok((tags, next_ifd))
}

fn decode_ifd_page(bytes: &[u8], tags: &BTreeMap<u16, TagVal>, ifd_off: u64) -> Result<RasterPage, DrawError> {
    let width = tags
        .get(&256)
        .and_then(TagVal::first_u32)
        .ok_or_else(|| DrawError::malformed("TIFF missing ImageWidth (256)", ifd_off))?;
    let height = tags
        .get(&257)
        .and_then(TagVal::first_u32)
        .ok_or_else(|| DrawError::malformed("TIFF missing ImageLength (257)", ifd_off))?;
    if width == 0 || height == 0 {
        return Err(DrawError::malformed("TIFF zero dimension", ifd_off));
    }
    let total_pixels = u64::from(width).saturating_mul(u64::from(height));
    if total_pixels > MAX_RASTER_PIXELS {
        return Err(DrawError::limit_exceeded("pixels", format!("{total_pixels} exceeds {MAX_RASTER_PIXELS}")));
    }

    let compression = tags.get(&259).and_then(TagVal::first_u32).unwrap_or(1);
    let photometric = tags.get(&262).and_then(TagVal::first_u32).unwrap_or(1);
    let fill_order = tags.get(&266).and_then(TagVal::first_u32).unwrap_or(1);
    let samples_per_pixel = tags.get(&277).and_then(TagVal::first_u32).unwrap_or(1) as usize;
    let bps_slice = tags.get(&258).map_or(&[][..], TagVal::as_u32s);
    let bits_per_sample = bps_slice.first().copied().unwrap_or(1) as u8;
    let rows_per_strip = tags.get(&278).and_then(TagVal::first_u32).unwrap_or(height).max(1);
    let planar_config = tags.get(&284).and_then(TagVal::first_u32).unwrap_or(1);
    let predictor = tags.get(&317).and_then(TagVal::first_u32).unwrap_or(1);
    if planar_config != 1 {
        return Err(DrawError::malformed("TIFF planar configuration != 1 unsupported", ifd_off));
    }
    if !(1..=4).contains(&samples_per_pixel) {
        return Err(DrawError::malformed("TIFF unsupported SamplesPerPixel", ifd_off));
    }
    if !matches!(bits_per_sample, 1 | 4 | 8 | 16) {
        return Err(DrawError::malformed("TIFF unsupported BitsPerSample", ifd_off));
    }

    let strip_offsets = tags
        .get(&273)
        .map(TagVal::as_u32s)
        .ok_or_else(|| DrawError::malformed("TIFF missing StripOffsets (273)", ifd_off))?;
    let strip_counts = tags
        .get(&279)
        .map(TagVal::as_u32s)
        .ok_or_else(|| DrawError::malformed("TIFF missing StripByteCounts (279)", ifd_off))?;
    if strip_offsets.is_empty() || strip_offsets.len() != strip_counts.len() {
        return Err(DrawError::malformed("TIFF StripOffsets / StripByteCounts mismatch", ifd_off));
    }

    let dpi = parse_tiff_dpi(tags);
    let palette = tags.get(&320).map(TagVal::as_u32s);

    let row_bits = (width as usize)
        .checked_mul(samples_per_pixel)
        .and_then(|v| v.checked_mul(usize::from(bits_per_sample)))
        .ok_or_else(|| DrawError::malformed("TIFF row size overflow", ifd_off))?;
    let row_bytes = row_bits.div_ceil(8);

    let format = match (photometric, samples_per_pixel) {
        (0 | 1, 1) => PixelFormat::Gray8,
        _ => PixelFormat::Rgba8,
    };
    let pixel_count = (width as usize)
        .checked_mul(height as usize)
        .ok_or_else(|| DrawError::malformed("TIFF pixel count overflow", ifd_off))?;
    let mut pixels = Vec::with_capacity(match format {
        PixelFormat::Gray8 => pixel_count,
        PixelFormat::Rgba8 => pixel_count.saturating_mul(4),
    });

    let mut rows_done = 0u32;
    for (&so_u32, &sc_u32) in strip_offsets.iter().zip(strip_counts.iter()) {
        if rows_done >= height {
            break;
        }
        let strip_rows = rows_per_strip.min(height - rows_done) as usize;
        let start = so_u32 as usize;
        let len = sc_u32 as usize;
        let end = start.checked_add(len).ok_or_else(|| DrawError::malformed("TIFF strip offset overflow", ifd_off))?;
        let raw_strip =
            bytes.get(start..end).ok_or_else(|| DrawError::truncated("TIFF strip extends past EOF", start as u64))?;

        if matches!(compression, 2..=4) {
            if bits_per_sample != 1 || samples_per_pixel != 1 {
                return Err(DrawError::malformed("CCITT compression requires 1-bit grayscale", start as u64));
            }
            let strip_data = if fill_order == 2 {
                raw_strip.iter().map(|&b| b.reverse_bits()).collect::<Vec<u8>>()
            } else {
                raw_strip.to_vec()
            };
            let white_is_zero = photometric == 0;
            let decoded_rows = match compression {
                4 => decode_ccitt_g4(&strip_data, width as usize, strip_rows, white_is_zero, start as u64)?,
                2 => decode_ccitt_mh(&strip_data, width as usize, strip_rows, white_is_zero, false, start as u64)?,
                3 => decode_ccitt_mh(&strip_data, width as usize, strip_rows, white_is_zero, true, start as u64)?,
                _ => Vec::new(),
            };
            pixels.extend_from_slice(&decoded_rows);
            rows_done += strip_rows as u32;
            continue;
        }

        let expected_bytes = strip_rows
            .checked_mul(row_bytes)
            .ok_or_else(|| DrawError::malformed("TIFF strip byte count overflow", start as u64))?;
        if expected_bytes > MAX_STRIP_RAW_BYTES {
            return Err(DrawError::limit_exceeded("pixels", "TIFF strip exceeds memory bound"));
        }

        let mut decoded = match compression {
            1 => {
                if raw_strip.len() < expected_bytes {
                    return Err(DrawError::truncated("TIFF uncompressed strip truncated", start as u64));
                }
                raw_strip[..expected_bytes].to_vec()
            }
            5 => decode_tiff_lzw(raw_strip, expected_bytes, start as u64)?,
            8 | 32946 => zlib_decompress(raw_strip, expected_bytes)
                .map_err(|_| DrawError::malformed("TIFF corrupt Deflate strip", start as u64))?,
            32773 => decode_packbits(raw_strip, expected_bytes, start as u64)?,
            _ => return Err(DrawError::malformed("TIFF unsupported compression scheme", start as u64)),
        };

        if decoded.len() < expected_bytes {
            return Err(DrawError::truncated("TIFF decoded strip shorter than expected", start as u64));
        }

        if predictor == 2 && bits_per_sample == 8 {
            for r in 0..strip_rows {
                let row = &mut decoded[r * row_bytes..(r + 1) * row_bytes];
                for i in samples_per_pixel..row.len() {
                    row[i] = row[i].wrapping_add(row[i - samples_per_pixel]);
                }
            }
        }

        for r in 0..strip_rows {
            let row = &decoded[r * row_bytes..(r + 1) * row_bytes];
            unpack_tiff_row(
                row,
                width as usize,
                bits_per_sample,
                samples_per_pixel,
                photometric,
                palette,
                &mut pixels,
            )?;
        }
        rows_done += strip_rows as u32;
    }

    Ok(RasterPage { width, height, format, dpi, pixels })
}

fn parse_tiff_dpi(tags: &BTreeMap<u16, TagVal>) -> Option<(u32, u32)> {
    let unit = tags.get(&296).and_then(TagVal::first_u32).unwrap_or(2);
    if unit == 1 {
        return None;
    }
    let (xn, xd) = tags.get(&282).and_then(TagVal::first_rational)?;
    let (yn, yd) = tags.get(&283).and_then(TagVal::first_rational)?;
    if xd == 0 || yd == 0 || xn == 0 || yn == 0 {
        return None;
    }
    match unit {
        2 => {
            let dx = round_half_even(i128::from(xn), i128::from(xd)).max(1) as u32;
            let dy = round_half_even(i128::from(yn), i128::from(yd)).max(1) as u32;
            Some((dx, dy))
        }
        3 => {
            let dx = round_half_even(i128::from(xn) * 254, i128::from(xd) * 100).max(1) as u32;
            let dy = round_half_even(i128::from(yn) * 254, i128::from(yd) * 100).max(1) as u32;
            Some((dx, dy))
        }
        _ => None,
    }
}

fn unpack_tiff_row(
    row: &[u8],
    width: usize,
    bps: u8,
    spp: usize,
    photometric: u32,
    palette: Option<&[u32]>,
    out: &mut Vec<u8>,
) -> Result<(), DrawError> {
    match (photometric, spp, bps) {
        (0 | 1, 1, 1) => {
            let white_is_zero = photometric == 0;
            for x in 0..width {
                let bit = (row[x / 8] >> (7 - (x % 8))) & 1;
                let g = if white_is_zero {
                    if bit == 0 {
                        255
                    } else {
                        0
                    }
                } else if bit == 0 {
                    0
                } else {
                    255
                };
                out.push(g);
            }
        }
        (0 | 1, 1, 8) => {
            for &b in &row[..width] {
                out.push(if photometric == 0 { 255 - b } else { b });
            }
        }
        (0 | 1, 1, 16) => {
            for px in row[..width * 2].chunks_exact(2) {
                let g = px[0];
                out.push(if photometric == 0 { 255 - g } else { g });
            }
        }
        (2, 3, 8) => {
            for px in row[..width * 3].chunks_exact(3) {
                out.extend_from_slice(&[px[0], px[1], px[2], 255]);
            }
        }
        (2, 4, 8) => {
            out.extend_from_slice(&row[..width * 4]);
        }
        (3, 1, 8) => {
            let pal = palette.ok_or_else(|| DrawError::malformed("TIFF palette image missing ColorMap", 0))?;
            let n = pal.len() / 3;
            for &idx_b in &row[..width] {
                let idx = usize::from(idx_b);
                if idx >= n {
                    return Err(DrawError::malformed("TIFF palette index OOB", 0));
                }
                let r = (pal[idx] >> 8) as u8;
                let g = (pal[n + idx] >> 8) as u8;
                let b = (pal[2 * n + idx] >> 8) as u8;
                out.extend_from_slice(&[r, g, b, 255]);
            }
        }
        _ => return Err(DrawError::malformed("TIFF unsupported photometric/sample layout", 0)),
    }
    Ok(())
}

fn decode_packbits(src: &[u8], expected: usize, offset: u64) -> Result<Vec<u8>, DrawError> {
    let mut out = Vec::with_capacity(expected);
    let mut pos = 0usize;
    while pos < src.len() && out.len() < expected {
        let n = src[pos] as i8;
        pos += 1;
        if n >= 0 {
            let count = usize::from(n as u8) + 1;
            let slice = src
                .get(pos..pos + count)
                .ok_or_else(|| DrawError::truncated("TIFF PackBits literal truncated", offset))?;
            if out.len().saturating_add(count) > expected {
                out.extend_from_slice(&slice[..expected - out.len()]);
            } else {
                out.extend_from_slice(slice);
            }
            pos += count;
        } else if n != -128 {
            let count = usize::from(n.unsigned_abs()) + 1;
            let val = *src.get(pos).ok_or_else(|| DrawError::truncated("TIFF PackBits repeat truncated", offset))?;
            pos += 1;
            let take = count.min(expected - out.len());
            out.resize(out.len() + take, val);
        }
    }
    Ok(out)
}

fn decode_tiff_lzw(src: &[u8], max_out: usize, offset: u64) -> Result<Vec<u8>, DrawError> {
    const CLEAR: u16 = 256;
    const EOI: u16 = 257;

    let mut bit_pos = 0usize;
    let total_bits = src.len().saturating_mul(8);
    let read_code = |bit_pos: &mut usize, width: usize| -> Option<u16> {
        if *bit_pos + width > total_bits {
            return None;
        }
        let mut val: u32 = 0;
        for _ in 0..width {
            let byte = src[*bit_pos >> 3];
            let bit = (byte >> (7 - (*bit_pos & 7))) & 1;
            val = (val << 1) | u32::from(bit);
            *bit_pos += 1;
        }
        Some(val as u16)
    };

    let mut out = Vec::with_capacity(max_out.min(65_536));
    let mut table: Vec<Vec<u8>> = Vec::with_capacity(4096);
    let mut code_width = 9usize;
    let mut prev: Option<Vec<u8>> = None;

    let reset = |table: &mut Vec<Vec<u8>>, code_width: &mut usize, prev: &mut Option<Vec<u8>>| {
        table.clear();
        for b in 0..=255u8 {
            table.push(vec![b]);
        }
        table.push(Vec::new());
        table.push(Vec::new());
        *code_width = 9;
        *prev = None;
    };
    reset(&mut table, &mut code_width, &mut prev);

    while let Some(code) = read_code(&mut bit_pos, code_width) {
        if code == EOI {
            break;
        }
        if code == CLEAR {
            reset(&mut table, &mut code_width, &mut prev);
            continue;
        }
        let entry = if (code as usize) < table.len() {
            table[code as usize].clone()
        } else if (code as usize) == table.len() {
            let Some(p) = prev.as_ref() else {
                return Err(DrawError::malformed("TIFF LZW bad first code", offset));
            };
            let mut e = p.clone();
            if let Some(&first) = p.first() {
                e.push(first);
            }
            e
        } else {
            return Err(DrawError::malformed("TIFF LZW out-of-range code", offset));
        };

        if out.len().saturating_add(entry.len()) > max_out {
            let rem = max_out - out.len();
            out.extend_from_slice(&entry[..rem]);
            break;
        }
        out.extend_from_slice(&entry);

        if let Some(p) = prev.as_ref() {
            if table.len() < 4096 {
                let mut new_e = p.clone();
                if let Some(&first) = entry.first() {
                    new_e.push(first);
                }
                table.push(new_e);
                if table.len() == 511 && code_width == 9 {
                    code_width = 10;
                } else if table.len() == 1023 && code_width == 10 {
                    code_width = 11;
                } else if table.len() == 2047 && code_width == 11 {
                    code_width = 12;
                }
            }
        }
        prev = Some(entry);
    }

    Ok(out)
}

// ------------------------------------------------------------------ ITU-T T.4 / T.6 (CCITT G3 & G4)

struct MsbBits<'a> {
    data: &'a [u8],
    bit_pos: usize,
    total_bits: usize,
}

impl<'a> MsbBits<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, bit_pos: 0, total_bits: data.len().saturating_mul(8) }
    }

    fn read_bit(&mut self) -> Option<u8> {
        if self.bit_pos >= self.total_bits {
            return None;
        }
        let b = (self.data[self.bit_pos >> 3] >> (7 - (self.bit_pos & 7))) & 1;
        self.bit_pos += 1;
        Some(b)
    }

    fn align_byte(&mut self) {
        let rem = self.bit_pos & 7;
        if rem != 0 {
            self.bit_pos = (self.bit_pos + (8 - rem)).min(self.total_bits);
        }
    }
}

#[derive(Clone, Copy)]
struct TrieNode {
    child: [u16; 2],
    run: i16,
}

struct CcittTables {
    white: Vec<TrieNode>,
    black: Vec<TrieNode>,
}

fn new_trie() -> Vec<TrieNode> {
    vec![TrieNode { child: [0, 0], run: -1 }]
}

fn trie_insert(nodes: &mut Vec<TrieNode>, bits: &str, run: i16) {
    let mut cur = 0usize;
    for ch in bits.bytes() {
        let b = usize::from(ch - b'0');
        let next = nodes[cur].child[b] as usize;
        if next == 0 {
            let idx = nodes.len() as u16;
            nodes.push(TrieNode { child: [0, 0], run: -1 });
            nodes[cur].child[b] = idx;
            cur = idx as usize;
        } else {
            cur = next;
        }
    }
    nodes[cur].run = run;
}

fn trie_decode(nodes: &[TrieNode], bits: &mut MsbBits<'_>) -> Option<i16> {
    let mut cur = 0usize;
    while let Some(b) = bits.read_bit() {
        let next = nodes[cur].child[usize::from(b)] as usize;
        if next == 0 {
            return None;
        }
        cur = next;
        if nodes[cur].run != -1 {
            return Some(nodes[cur].run);
        }
    }
    None
}

fn build_ccitt_tables() -> CcittTables {
    const WHITE_TERM: [&str; 64] = [
        "00110101", "000111", "0111", "1000", "1011", "1100", "1110", "1111", "10011", "10100", "00111", "01000",
        "001000", "000011", "110100", "110101", "101010", "101011", "0100111", "0001100", "0001000", "0010111",
        "0000011", "0000100", "0101000", "0101011", "0010011", "0100100", "0011000", "00000010", "00000011",
        "00011010", "00011011", "00010010", "00010011", "00010100", "00010101", "00010110", "00010111", "00101000",
        "00101001", "00101010", "00101011", "00101100", "00101101", "00000100", "00000101", "00001010", "00001011",
        "01010010", "01010011", "01010100", "01010101", "00100100", "00100101", "01011000", "01011001", "01011010",
        "01011011", "01001010", "01001011", "00110010", "00110011", "00110100",
    ];
    const BLACK_TERM: [&str; 64] = [
        "0000110111",
        "010",
        "11",
        "10",
        "011",
        "0011",
        "0010",
        "00011",
        "000101",
        "000100",
        "0000100",
        "0000101",
        "0000111",
        "00000100",
        "00000111",
        "000011000",
        "0000010111",
        "0000011000",
        "0000001000",
        "00001100111",
        "00001101000",
        "00001101100",
        "00000110111",
        "00000101000",
        "00000010111",
        "00000011000",
        "000011001010",
        "000011001011",
        "000011001100",
        "000011001101",
        "000001101000",
        "000001101001",
        "000001101010",
        "000001101011",
        "000011010010",
        "000011010011",
        "000011010100",
        "000011010101",
        "000011010110",
        "000011010111",
        "000001101100",
        "000001101101",
        "000011011010",
        "000011011011",
        "000001010100",
        "000001010101",
        "000001010110",
        "000001010111",
        "000001100100",
        "000001100101",
        "000001010010",
        "000001010011",
        "000000100100",
        "000000110111",
        "000000111000",
        "000000100111",
        "000000101000",
        "000001011000",
        "000001011001",
        "000000101011",
        "000000101100",
        "000001011010",
        "000001100110",
        "000001100111",
    ];
    const WHITE_MAKEUP: [(i16, &str); 27] = [
        (64, "11011"),
        (128, "10010"),
        (192, "010111"),
        (256, "0110111"),
        (320, "00110110"),
        (384, "00110111"),
        (448, "01100100"),
        (512, "01100101"),
        (576, "01101000"),
        (640, "01100111"),
        (704, "011001100"),
        (768, "011001101"),
        (832, "011010010"),
        (896, "011010011"),
        (960, "011010100"),
        (1024, "011010101"),
        (1088, "011010110"),
        (1152, "011010111"),
        (1216, "011011000"),
        (1280, "011011001"),
        (1344, "011011010"),
        (1408, "011011011"),
        (1472, "010011000"),
        (1536, "010011001"),
        (1600, "010011010"),
        (1664, "011000"),
        (1728, "010011011"),
    ];
    const BLACK_MAKEUP: [(i16, &str); 27] = [
        (64, "0000001111"),
        (128, "000011001000"),
        (192, "000011001001"),
        (256, "000001011011"),
        (320, "000000110011"),
        (384, "000000110100"),
        (448, "000000110101"),
        (512, "0000001101100"),
        (576, "0000001101101"),
        (640, "0000001001010"),
        (704, "0000001001011"),
        (768, "0000001001100"),
        (832, "0000001001101"),
        (896, "0000001110010"),
        (960, "0000001110011"),
        (1024, "0000001110100"),
        (1088, "0000001110101"),
        (1152, "0000001110110"),
        (1216, "0000001110111"),
        (1280, "0000001010010"),
        (1344, "0000001010011"),
        (1408, "0000001010100"),
        (1472, "0000001010101"),
        (1536, "0000001011010"),
        (1600, "0000001011011"),
        (1664, "0000001100100"),
        (1728, "0000001100101"),
    ];
    const EXT_MAKEUP: [(i16, &str); 13] = [
        (1792, "00000001000"),
        (1856, "00000001100"),
        (1920, "00000001101"),
        (1984, "000000010010"),
        (2048, "000000010011"),
        (2112, "000000010100"),
        (2176, "000000010101"),
        (2240, "000000010110"),
        (2304, "000000010111"),
        (2368, "000000011100"),
        (2432, "000000011101"),
        (2496, "000000011110"),
        (2560, "000000011111"),
    ];

    let mut white = new_trie();
    let mut black = new_trie();
    for (run, code) in WHITE_TERM.iter().enumerate() {
        trie_insert(&mut white, code, run as i16);
    }
    for (run, code) in BLACK_TERM.iter().enumerate() {
        trie_insert(&mut black, code, run as i16);
    }
    for &(run, code) in &WHITE_MAKEUP {
        trie_insert(&mut white, code, run);
    }
    for &(run, code) in &BLACK_MAKEUP {
        trie_insert(&mut black, code, run);
    }
    for &(run, code) in &EXT_MAKEUP {
        trie_insert(&mut white, code, run);
        trie_insert(&mut black, code, run);
    }
    trie_insert(&mut white, "000000000001", -2);
    trie_insert(&mut black, "000000000001", -2);
    CcittTables { white, black }
}

fn read_run(tables: &CcittTables, bits: &mut MsbBits<'_>, is_black: bool, max_run: usize) -> Result<usize, DrawError> {
    let trie = if is_black { &tables.black } else { &tables.white };
    let mut total = 0usize;
    loop {
        let r = trie_decode(trie, bits).ok_or_else(|| DrawError::malformed("invalid CCITT Huffman run code", 0))?;
        if r < 0 {
            return Err(DrawError::malformed("unexpected EOL in CCITT run", 0));
        }
        let ru = r as usize;
        total = total.saturating_add(ru);
        if total > max_run {
            return Err(DrawError::malformed("CCITT run exceeds scanline width", 0));
        }
        if ru < 64 {
            return Ok(total);
        }
    }
}

fn decode_ccitt_mh(
    data: &[u8],
    width: usize,
    rows: usize,
    white_is_zero: bool,
    has_eol: bool,
    offset: u64,
) -> Result<Vec<u8>, DrawError> {
    let tables = build_ccitt_tables();
    let mut bits = MsbBits::new(data);
    let mut out = Vec::with_capacity(width.saturating_mul(rows));
    let white_px: u8 = if white_is_zero { 255 } else { 0 };
    let black_px: u8 = if white_is_zero { 0 } else { 255 };

    for _ in 0..rows {
        if has_eol {
            let mut zeros = 0usize;
            while let Some(b) = bits.read_bit() {
                if b == 0 {
                    zeros += 1;
                } else if zeros >= 11 {
                    break;
                } else {
                    zeros = 0;
                }
            }
        }
        let mut col = 0usize;
        let mut is_black = false;
        while col < width {
            let run = read_run(&tables, &mut bits, is_black, width - col)
                .map_err(|_| DrawError::malformed("CCITT 1D decode error", offset))?;
            let px = if is_black { black_px } else { white_px };
            out.resize(out.len() + run, px);
            col += run;
            is_black = !is_black;
        }
        if !has_eol {
            bits.align_byte();
        }
    }
    Ok(out)
}

#[derive(Clone, Copy)]
enum Mode2D {
    Pass,
    Horizontal,
    Vertical(i32),
    Eofb,
}

fn read_mode_2d(bits: &mut MsbBits<'_>) -> Option<Mode2D> {
    if bits.read_bit()? == 1 {
        return Some(Mode2D::Vertical(0));
    }
    if bits.read_bit()? == 1 {
        return Some(if bits.read_bit()? == 1 { Mode2D::Vertical(1) } else { Mode2D::Vertical(-1) });
    }
    if bits.read_bit()? == 1 {
        return Some(Mode2D::Horizontal);
    }
    if bits.read_bit()? == 1 {
        return Some(Mode2D::Pass);
    }
    if bits.read_bit()? == 1 {
        return Some(if bits.read_bit()? == 1 { Mode2D::Vertical(2) } else { Mode2D::Vertical(-2) });
    }
    if bits.read_bit()? == 1 {
        return Some(if bits.read_bit()? == 1 { Mode2D::Vertical(3) } else { Mode2D::Vertical(-3) });
    }
    let mut zeros = 6usize;
    while let Some(b) = bits.read_bit() {
        if b == 0 {
            zeros += 1;
            if zeros > 32 {
                return None;
            }
        } else {
            return if zeros >= 11 { Some(Mode2D::Eofb) } else { None };
        }
    }
    None
}

fn find_b1_b2(ref_line: &[u8], a0: i32, cur_color: u8) -> (usize, usize) {
    let w = ref_line.len();
    let target = 1 - cur_color;
    let start = if a0 < 0 { 0 } else { (a0 as usize) + 1 };

    let mut b1 = w;
    for pos in start..w {
        let prev = if pos == 0 { 0u8 } else { ref_line[pos - 1] };
        if ref_line[pos] == target && (pos == 0 || ref_line[pos] != prev) && (a0 < 0 || pos > (a0 as usize)) {
            b1 = pos;
            break;
        }
    }

    let mut b2 = w;
    if b1 < w {
        let b1_color = ref_line[b1];
        for (pos, &c) in ref_line.iter().enumerate().skip(b1 + 1) {
            if c != b1_color {
                b2 = pos;
                break;
            }
        }
    }
    (b1, b2)
}

fn decode_ccitt_g4(
    data: &[u8],
    width: usize,
    rows: usize,
    white_is_zero: bool,
    offset: u64,
) -> Result<Vec<u8>, DrawError> {
    let tables = build_ccitt_tables();
    let mut bits = MsbBits::new(data);
    let mut ref_line = vec![0u8; width];
    let mut cur_line = vec![0u8; width];
    let mut out = Vec::with_capacity(width.saturating_mul(rows));
    let white_px: u8 = if white_is_zero { 255 } else { 0 };
    let black_px: u8 = if white_is_zero { 0 } else { 255 };

    for _ in 0..rows {
        cur_line.fill(0);
        let mut a0: i32 = -1;
        let mut color: u8 = 0;

        while (a0.max(0) as usize) < width {
            let mode =
                read_mode_2d(&mut bits).ok_or_else(|| DrawError::truncated("truncated CCITT G4 bitstream", offset))?;
            match mode {
                Mode2D::Eofb => break,
                Mode2D::Pass => {
                    let (_, b2) = find_b1_b2(&ref_line, a0, color);
                    let start = a0.max(0) as usize;
                    let end = b2.min(width);
                    cur_line[start..end].fill(color);
                    a0 = end as i32;
                }
                Mode2D::Horizontal => {
                    let start = a0.max(0) as usize;
                    let r1 = read_run(&tables, &mut bits, color == 1, width - start)
                        .map_err(|_| DrawError::malformed("CCITT G4 horizontal run 1 error", offset))?;
                    let mid = start + r1;
                    cur_line[start..mid].fill(color);
                    let other = 1 - color;
                    let r2 = read_run(&tables, &mut bits, other == 1, width - mid)
                        .map_err(|_| DrawError::malformed("CCITT G4 horizontal run 2 error", offset))?;
                    let end = mid + r2;
                    cur_line[mid..end].fill(other);
                    a0 = end as i32;
                }
                Mode2D::Vertical(delta) => {
                    let (b1, _) = find_b1_b2(&ref_line, a0, color);
                    let a1_i32 = (b1 as i32) + delta;
                    if a1_i32 < a0.max(0) || (a1_i32 as usize) > width {
                        return Err(DrawError::malformed("CCITT G4 vertical offset out of bounds", offset));
                    }
                    let start = a0.max(0) as usize;
                    let end = a1_i32 as usize;
                    cur_line[start..end].fill(color);
                    a0 = end as i32;
                    color = 1 - color;
                }
            }
        }

        for &bit in &cur_line {
            out.push(if bit == 0 { white_px } else { black_px });
        }
        std::mem::swap(&mut ref_line, &mut cur_line);
    }

    Ok(out)
}
