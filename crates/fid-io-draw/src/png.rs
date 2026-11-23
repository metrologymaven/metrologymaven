//! Bounded pure-Rust PNG drawing decoder (`IO.REG.PLUGIN.DRAW`, REQ-003, REQ-006, AC-4).
//!
//! Decodes all standard PNG colour types (`0` Grayscale, `2` RGB, `3` Indexed, `4` Gray+Alpha,
//! `6` RGBA) and bit depths (`1, 2, 4, 8, 16`) into `Gray8` or `Rgba8`, extracts stated
//! resolution from the `pHYs` chunk (`unit == 1` pixels/metre -> exact half-to-even DPI;
//! otherwise `None`), and enforces the 400 Mpx per-page limit before allocating pixel buffers.

use crate::flate::zlib_decompress;
use crate::{round_half_even, DrawError, PixelFormat, RasterPage, MAX_RASTER_PIXELS};

const MAX_PNG_CHUNKS: usize = 65_536;
const MAX_IDAT_BYTES: usize = 500 * 1024 * 1024;
/// Maximum raw uncompressed scanline buffer (400 Mpx * 8 bytes/px + row filter bytes).
const MAX_PNG_RAW_BYTES: usize = 64 * 1024 * 1024;

fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// Decodes a PNG image into a [`RasterPage`] (`Gray8` or `Rgba8`) with optional stated DPI.
pub fn decode_png(bytes: &[u8]) -> Result<RasterPage, DrawError> {
    if bytes.len() < 8 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" {
        return Err(DrawError::malformed("invalid PNG signature", 0));
    }

    let mut pos = 8usize;
    let mut chunks = 0usize;
    let mut ihdr: Option<(u32, u32, u8, u8)> = None;
    let mut palette: Option<Vec<[u8; 3]>> = None;
    let mut trns_alpha: Option<Vec<u8>> = None;
    let mut dpi: Option<(u32, u32)> = None;
    let mut idat = Vec::new();
    let mut saw_iend = false;

    while pos < bytes.len() {
        chunks += 1;
        if chunks > MAX_PNG_CHUNKS {
            return Err(DrawError::malformed("PNG exceeds chunk limit", pos as u64));
        }
        if pos + 12 > bytes.len() {
            return Err(DrawError::truncated("truncated PNG chunk header", pos as u64));
        }
        let len = u32::from_be_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]]) as usize;
        let ctype = &bytes[pos + 4..pos + 8];
        let data_start = pos + 8;
        let data_end =
            data_start.checked_add(len).ok_or_else(|| DrawError::malformed("PNG chunk length overflow", pos as u64))?;
        let crc_end =
            data_end.checked_add(4).ok_or_else(|| DrawError::malformed("PNG chunk length overflow", pos as u64))?;
        if crc_end > bytes.len() {
            return Err(DrawError::truncated("truncated PNG chunk data", pos as u64));
        }

        let stored_crc =
            u32::from_be_bytes([bytes[data_end], bytes[data_end + 1], bytes[data_end + 2], bytes[data_end + 3]]);
        if stored_crc != crc32(&bytes[pos + 4..data_end]) {
            return Err(DrawError::malformed("PNG chunk CRC mismatch", pos as u64));
        }

        let cdata = &bytes[data_start..data_end];
        let chunk_off = pos as u64;
        pos = crc_end;

        match ctype {
            b"IHDR" => {
                if ihdr.is_some() || cdata.len() != 13 {
                    return Err(DrawError::malformed("invalid PNG IHDR chunk", chunk_off));
                }
                let w = u32::from_be_bytes([cdata[0], cdata[1], cdata[2], cdata[3]]);
                let h = u32::from_be_bytes([cdata[4], cdata[5], cdata[6], cdata[7]]);
                let bit_depth = cdata[8];
                let color_type = cdata[9];
                let comp = cdata[10];
                let filter = cdata[11];
                let interlace = cdata[12];

                if w == 0 || h == 0 {
                    return Err(DrawError::malformed("PNG zero dimension", chunk_off));
                }
                let pixels = u64::from(w).saturating_mul(u64::from(h));
                if pixels > MAX_RASTER_PIXELS {
                    return Err(DrawError::limit_exceeded("pixels", format!("{pixels} exceeds {MAX_RASTER_PIXELS}")));
                }
                if comp != 0 || filter != 0 {
                    return Err(DrawError::malformed("unsupported PNG compression/filter method", chunk_off));
                }
                if interlace != 0 {
                    return Err(DrawError::malformed("interlaced PNG unsupported", chunk_off));
                }
                let valid = match color_type {
                    0 => matches!(bit_depth, 1 | 2 | 4 | 8 | 16),
                    2 | 4 | 6 => matches!(bit_depth, 8 | 16),
                    3 => matches!(bit_depth, 1 | 2 | 4 | 8),
                    _ => false,
                };
                if !valid {
                    return Err(DrawError::malformed("invalid PNG color_type / bit_depth", chunk_off));
                }
                ihdr = Some((w, h, bit_depth, color_type));
            }
            b"PLTE" => {
                if cdata.is_empty() || cdata.len() % 3 != 0 || cdata.len() > 256 * 3 {
                    return Err(DrawError::malformed("invalid PNG PLTE chunk", chunk_off));
                }
                let mut pal = Vec::with_capacity(cdata.len() / 3);
                for rgb in cdata.chunks_exact(3) {
                    pal.push([rgb[0], rgb[1], rgb[2]]);
                }
                palette = Some(pal);
            }
            b"tRNS" => {
                trns_alpha = Some(cdata.to_vec());
            }
            b"pHYs" => {
                if cdata.len() != 9 {
                    return Err(DrawError::malformed("invalid PNG pHYs chunk", chunk_off));
                }
                let ppm_x = u32::from_be_bytes([cdata[0], cdata[1], cdata[2], cdata[3]]);
                let ppm_y = u32::from_be_bytes([cdata[4], cdata[5], cdata[6], cdata[7]]);
                let unit = cdata[8];
                if unit == 1 && ppm_x > 0 && ppm_y > 0 {
                    // 1 inch = 0.0254 m -> dpi = round_half_even(ppm * 254 / 10_000).
                    let dx = round_half_even(i128::from(ppm_x) * 254, 10_000).max(1) as u32;
                    let dy = round_half_even(i128::from(ppm_y) * 254, 10_000).max(1) as u32;
                    dpi = Some((dx, dy));
                }
            }
            b"IDAT" => {
                if ihdr.is_none() {
                    return Err(DrawError::malformed("PNG IDAT before IHDR", chunk_off));
                }
                if idat.len().saturating_add(cdata.len()) > MAX_IDAT_BYTES {
                    return Err(DrawError::limit_exceeded("bytes", "PNG IDAT exceeds 500 MiB"));
                }
                idat.extend_from_slice(cdata);
            }
            b"IEND" => {
                saw_iend = true;
                break;
            }
            _ => {}
        }
    }

    if !saw_iend {
        return Err(DrawError::truncated("PNG missing IEND", pos as u64));
    }
    let (width, height, bit_depth, color_type) = ihdr.ok_or_else(|| DrawError::malformed("PNG missing IHDR", 8))?;
    if color_type == 3 && palette.is_none() {
        return Err(DrawError::malformed("indexed PNG missing PLTE", 8));
    }

    let channels: usize = match color_type {
        0 | 3 => 1,
        2 => 3,
        4 => 2,
        6 => 4,
        _ => return Err(DrawError::malformed("invalid PNG color_type", 8)),
    };
    let bits_per_pixel = channels * usize::from(bit_depth);
    let row_bytes = (width as usize)
        .checked_mul(bits_per_pixel)
        .map(|bits| bits.div_ceil(8))
        .ok_or_else(|| DrawError::malformed("PNG row size overflow", 8))?;
    let expected_raw = (height as usize)
        .checked_mul(row_bytes.checked_add(1).ok_or_else(|| DrawError::malformed("PNG size overflow", 8))?)
        .ok_or_else(|| DrawError::malformed("PNG size overflow", 8))?;
    if expected_raw > MAX_PNG_RAW_BYTES {
        return Err(DrawError::limit_exceeded("pixels", "decoded PNG raw buffer exceeds memory cap"));
    }

    let raw = zlib_decompress(&idat, expected_raw).map_err(|_| DrawError::malformed("corrupt PNG zlib data", 8))?;
    if raw.len() != expected_raw {
        return Err(DrawError::truncated("PNG scanline buffer truncated", 8));
    }

    let bpp = bits_per_pixel.div_ceil(8).max(1);
    let mut prev_row = vec![0u8; row_bytes];
    let mut cur_row = vec![0u8; row_bytes];

    let pixel_count =
        (width as usize).checked_mul(height as usize).ok_or_else(|| DrawError::malformed("PNG pixel overflow", 8))?;
    let format = if color_type == 0 && trns_alpha.is_none() { PixelFormat::Gray8 } else { PixelFormat::Rgba8 };
    let out_bytes = match format {
        PixelFormat::Gray8 => pixel_count,
        PixelFormat::Rgba8 => {
            pixel_count.checked_mul(4).ok_or_else(|| DrawError::malformed("PNG output buffer overflow", 8))?
        }
    };
    let mut pixels = Vec::with_capacity(out_bytes);

    for row_idx in 0..(height as usize) {
        let base = row_idx * (row_bytes + 1);
        let filter_type = raw[base];
        let src = &raw[base + 1..base + 1 + row_bytes];
        unfilter_row(filter_type, src, &prev_row, &mut cur_row, bpp, base as u64)?;

        match (color_type, bit_depth, format) {
            (0, 8, PixelFormat::Gray8) => pixels.extend_from_slice(&cur_row),
            (0, 16, PixelFormat::Gray8) => {
                for pair in cur_row.chunks_exact(2) {
                    pixels.push(pair[0]);
                }
            }
            (0, 1 | 2 | 4, PixelFormat::Gray8) => {
                let max_val = (1u16 << bit_depth) - 1;
                for x in 0..(width as usize) {
                    let v = extract_subbyte(&cur_row, x, bit_depth);
                    pixels.push(((u16::from(v) * 255) / max_val) as u8);
                }
            }
            (0, _, PixelFormat::Rgba8) => {
                // Grayscale with tRNS transparency.
                let max_val = (1u16 << bit_depth.min(8)) - 1;
                for x in 0..(width as usize) {
                    let g = match bit_depth {
                        16 => cur_row[x * 2],
                        8 => cur_row[x],
                        _ => ((u16::from(extract_subbyte(&cur_row, x, bit_depth)) * 255) / max_val) as u8,
                    };
                    pixels.extend_from_slice(&[g, g, g, 255]);
                }
            }
            (2, 8, _) => {
                for px in cur_row.chunks_exact(3) {
                    pixels.extend_from_slice(&[px[0], px[1], px[2], 255]);
                }
            }
            (2, 16, _) => {
                for px in cur_row.chunks_exact(6) {
                    pixels.extend_from_slice(&[px[0], px[2], px[4], 255]);
                }
            }
            (3, 1 | 2 | 4 | 8, _) => {
                let pal = palette.as_ref().ok_or_else(|| DrawError::malformed("missing PLTE", 8))?;
                let trns = trns_alpha.as_deref().unwrap_or(&[]);
                for x in 0..(width as usize) {
                    let idx = usize::from(extract_subbyte(&cur_row, x, bit_depth));
                    let rgb = *pal.get(idx).ok_or_else(|| DrawError::malformed("PNG palette index OOB", 8))?;
                    let a = trns.get(idx).copied().unwrap_or(255);
                    pixels.extend_from_slice(&[rgb[0], rgb[1], rgb[2], a]);
                }
            }
            (4, 8, _) => {
                for px in cur_row.chunks_exact(2) {
                    pixels.extend_from_slice(&[px[0], px[0], px[0], px[1]]);
                }
            }
            (4, 16, _) => {
                for px in cur_row.chunks_exact(4) {
                    pixels.extend_from_slice(&[px[0], px[0], px[0], px[2]]);
                }
            }
            (6, 8, _) => pixels.extend_from_slice(&cur_row),
            (6, 16, _) => {
                for px in cur_row.chunks_exact(8) {
                    pixels.extend_from_slice(&[px[0], px[2], px[4], px[6]]);
                }
            }
            _ => return Err(DrawError::malformed("unsupported PNG format combination", 8)),
        }
        prev_row.copy_from_slice(&cur_row);
    }

    Ok(RasterPage { width, height, format, dpi, pixels })
}

fn extract_subbyte(row: &[u8], x: usize, bit_depth: u8) -> u8 {
    match bit_depth {
        8 => row[x],
        4 => {
            let b = row[x / 2];
            if x % 2 == 0 {
                b >> 4
            } else {
                b & 0x0F
            }
        }
        2 => (row[x / 4] >> (6 - 2 * (x % 4))) & 0x03,
        1 => (row[x / 8] >> (7 - (x % 8))) & 0x01,
        _ => 0,
    }
}

fn unfilter_row(
    filter_type: u8,
    src: &[u8],
    prev: &[u8],
    dst: &mut [u8],
    bpp: usize,
    offset: u64,
) -> Result<(), DrawError> {
    match filter_type {
        0 => dst.copy_from_slice(src),
        1 => {
            for i in 0..src.len() {
                let a = if i >= bpp { dst[i - bpp] } else { 0 };
                dst[i] = src[i].wrapping_add(a);
            }
        }
        2 => {
            for i in 0..src.len() {
                dst[i] = src[i].wrapping_add(prev[i]);
            }
        }
        3 => {
            for i in 0..src.len() {
                let a = if i >= bpp { u16::from(dst[i - bpp]) } else { 0 };
                let b = u16::from(prev[i]);
                dst[i] = src[i].wrapping_add(((a + b) / 2) as u8);
            }
        }
        4 => {
            for i in 0..src.len() {
                let a = if i >= bpp { dst[i - bpp] } else { 0 };
                let b = prev[i];
                let c = if i >= bpp { prev[i - bpp] } else { 0 };
                dst[i] = src[i].wrapping_add(paeth(a, b, c));
            }
        }
        _ => return Err(DrawError::malformed("unknown PNG scanline filter", offset)),
    }
    Ok(())
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let ai = i32::from(a);
    let bi = i32::from(b);
    let ci = i32::from(c);
    let p = ai + bi - ci;
    let pa = (p - ai).abs();
    let pb = (p - bi).abs();
    let pc = (p - ci).abs();
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}
