//! Bounded PNG decoder and JPEG pass-through into deterministic PDF image XObjects
//! (spec node `RPT.OUTPUT.PDF.DRAW`, CONSTITUTION §8).
//!
//! - JPEG images are validated for structure and dimensions and passed through verbatim as
//!   `/DCTDecode` streams.
//! - PNG images are decoded with strict dimension, chunk-CRC and byte bounds, unfiltered in
//!   pure integer arithmetic, split into RGB/Gray + optional `/SMask` alpha, and re-encoded
//!   deterministically via [`crate::flate::zlib_compress`] (`/FlateDecode`).

use crate::flate::zlib_compress;
use crate::flate::zlib_decompress;

/// Maximum encoded image file size accepted (32 MiB).
pub const MAX_IMAGE_FILE_BYTES: usize = 32 * 1024 * 1024;
/// Maximum PNG pixel dimension in either axis (8192 px).
pub const MAX_PNG_DIM: u32 = 8192;
/// Maximum JPEG pixel dimension in either axis (16384 px).
pub const MAX_JPEG_DIM: u32 = 16384;
/// Maximum decoded raw scanline buffer size for a PNG image (32 MiB).
pub const MAX_DECODED_BYTES: usize = 32 * 1024 * 1024;
/// Maximum number of PNG chunks processed in one file.
pub const MAX_PNG_CHUNKS: usize = 65_536;

/// PDF image stream filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PdfImageFilter {
    FlateDecode,
    DctDecode,
}

impl PdfImageFilter {
    pub const fn pdf_name(self) -> &'static str {
        match self {
            PdfImageFilter::FlateDecode => "/FlateDecode",
            PdfImageFilter::DctDecode => "/DCTDecode",
        }
    }
}

/// PDF image color space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PdfColorSpace {
    DeviceGray,
    DeviceRgb,
    DeviceCmyk,
}

impl PdfColorSpace {
    pub const fn pdf_name(self) -> &'static str {
        match self {
            PdfColorSpace::DeviceGray => "/DeviceGray",
            PdfColorSpace::DeviceRgb => "/DeviceRGB",
            PdfColorSpace::DeviceCmyk => "/DeviceCMYK",
        }
    }
}

/// Optional deterministic `/SMask` alpha stream for PNG images with alpha.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfSoftMask {
    pub width_px: u32,
    pub height_px: u32,
    pub data: Vec<u8>,
}

/// Deterministic PDF image XObject ready to embed into a PDF document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfImageObject {
    pub width_px: u32,
    pub height_px: u32,
    pub color_space: PdfColorSpace,
    pub bits_per_component: u8,
    pub filter: PdfImageFilter,
    pub data: Vec<u8>,
    pub smask: Option<PdfSoftMask>,
}

/// Decodes a PNG or JPEG image into a deterministic [`PdfImageObject`].
pub fn decode_image(bytes: &[u8]) -> Result<PdfImageObject, &'static str> {
    if bytes.is_empty() {
        return Err("empty_image");
    }
    if bytes.len() > MAX_IMAGE_FILE_BYTES {
        return Err("image_too_large");
    }
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        decode_png(bytes)
    } else if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xD8 {
        decode_jpeg(bytes)
    } else {
        Err("unsupported_image_format")
    }
}

// ------------------------------------------------------------------ JPEG pass-through

fn decode_jpeg(bytes: &[u8]) -> Result<PdfImageObject, &'static str> {
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return Err("jpeg_missing_soi");
    }
    let mut pos = 2usize;
    let mut dims: Option<(u32, u32, u8)> = None;
    let mut saw_eoi = false;

    while pos < bytes.len() {
        if bytes[pos] != 0xFF {
            // Inside entropy-coded scan after SOS, scan forward to next non-stuffed marker.
            pos += 1;
            continue;
        }
        while pos < bytes.len() && bytes[pos] == 0xFF {
            pos += 1;
        }
        if pos >= bytes.len() {
            break;
        }
        let marker = bytes[pos];
        pos += 1;

        match marker {
            0x00 | 0x01 | 0xD0..=0xD7 => {
                // Stuffed zero or standalone restart marker.
                continue;
            }
            0xD9 => {
                saw_eoi = true;
                break;
            }
            0xDA => {
                // SOS: skip header, then scan entropy data until EOI.
                if pos + 2 > bytes.len() {
                    return Err("jpeg_truncated_sos");
                }
                let seg_len = usize::from(u16::from_be_bytes([bytes[pos], bytes[pos + 1]]));
                if seg_len < 2 || pos + seg_len > bytes.len() {
                    return Err("jpeg_bad_sos_length");
                }
                pos += seg_len;
                while pos + 1 < bytes.len() {
                    if bytes[pos] == 0xFF && bytes[pos + 1] != 0x00 && !(0xD0..=0xD7).contains(&bytes[pos + 1]) {
                        break;
                    }
                    pos += 1;
                }
            }
            _ => {
                if pos + 2 > bytes.len() {
                    return Err("jpeg_truncated_segment");
                }
                let seg_len = usize::from(u16::from_be_bytes([bytes[pos], bytes[pos + 1]]));
                if seg_len < 2 || pos + seg_len > bytes.len() {
                    return Err("jpeg_bad_segment_length");
                }
                // SOF0 (baseline) / SOF1 (extended) / SOF2 (progressive)
                if matches!(marker, 0xC0..=0xC2) {
                    if seg_len < 8 {
                        return Err("jpeg_bad_sof");
                    }
                    let precision = bytes[pos + 2];
                    if precision != 8 {
                        return Err("jpeg_unsupported_precision");
                    }
                    let h = u32::from(u16::from_be_bytes([bytes[pos + 3], bytes[pos + 4]]));
                    let w = u32::from(u16::from_be_bytes([bytes[pos + 5], bytes[pos + 6]]));
                    let comps = bytes[pos + 7];
                    dims = Some((w, h, comps));
                }
                pos += seg_len;
            }
        }
    }

    if !saw_eoi {
        return Err("jpeg_missing_eoi");
    }
    let (width_px, height_px, comps) = dims.ok_or("jpeg_missing_sof")?;
    if width_px == 0 || height_px == 0 || width_px > MAX_JPEG_DIM || height_px > MAX_JPEG_DIM {
        return Err("jpeg_invalid_dimensions");
    }
    let color_space = match comps {
        1 => PdfColorSpace::DeviceGray,
        3 => PdfColorSpace::DeviceRgb,
        4 => PdfColorSpace::DeviceCmyk,
        _ => return Err("jpeg_unsupported_components"),
    };

    Ok(PdfImageObject {
        width_px,
        height_px,
        color_space,
        bits_per_component: 8,
        filter: PdfImageFilter::DctDecode,
        data: bytes.to_vec(),
        smask: None,
    })
}

// ------------------------------------------------------------------ bounded PNG decoder

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

fn decode_png(bytes: &[u8]) -> Result<PdfImageObject, &'static str> {
    if bytes.len() < 8 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" {
        return Err("png_bad_signature");
    }
    let mut pos = 8usize;
    let mut chunks = 0usize;

    let mut ihdr: Option<(u32, u32, u8, u8)> = None;
    let mut palette: Option<Vec<[u8; 3]>> = None;
    let mut idat = Vec::new();
    let mut saw_iend = false;

    while pos < bytes.len() {
        chunks += 1;
        if chunks > MAX_PNG_CHUNKS {
            return Err("png_too_many_chunks");
        }
        if pos + 12 > bytes.len() {
            return Err("png_truncated_chunk");
        }
        let len = u32::from_be_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]]) as usize;
        let ctype = &bytes[pos + 4..pos + 8];
        let data_start = pos + 8;
        let data_end = data_start.checked_add(len).ok_or("png_chunk_overflow")?;
        let crc_end = data_end.checked_add(4).ok_or("png_chunk_overflow")?;
        if crc_end > bytes.len() {
            return Err("png_truncated_chunk_data");
        }
        let stored_crc =
            u32::from_be_bytes([bytes[data_end], bytes[data_end + 1], bytes[data_end + 2], bytes[data_end + 3]]);
        let calc_crc = crc32(&bytes[pos + 4..data_end]);
        if stored_crc != calc_crc {
            return Err("png_crc_mismatch");
        }
        let chunk_data = &bytes[data_start..data_end];
        pos = crc_end;

        match ctype {
            b"IHDR" => {
                if ihdr.is_some() || chunk_data.len() != 13 {
                    return Err("png_bad_ihdr");
                }
                let w = u32::from_be_bytes([chunk_data[0], chunk_data[1], chunk_data[2], chunk_data[3]]);
                let h = u32::from_be_bytes([chunk_data[4], chunk_data[5], chunk_data[6], chunk_data[7]]);
                let bit_depth = chunk_data[8];
                let color_type = chunk_data[9];
                let comp = chunk_data[10];
                let filter = chunk_data[11];
                let interlace = chunk_data[12];
                if w == 0 || h == 0 || w > MAX_PNG_DIM || h > MAX_PNG_DIM {
                    return Err("png_invalid_dimensions");
                }
                if comp != 0 || filter != 0 {
                    return Err("png_unsupported_method");
                }
                if interlace != 0 {
                    return Err("png_interlaced_unsupported");
                }
                let valid_combo = match color_type {
                    0 => matches!(bit_depth, 1 | 2 | 4 | 8 | 16),
                    2 | 4 | 6 => matches!(bit_depth, 8 | 16),
                    3 => matches!(bit_depth, 1 | 2 | 4 | 8),
                    _ => false,
                };
                if !valid_combo {
                    return Err("png_invalid_color_depth");
                }
                ihdr = Some((w, h, bit_depth, color_type));
            }
            b"PLTE" => {
                if chunk_data.is_empty() || chunk_data.len() % 3 != 0 || chunk_data.len() > 256 * 3 {
                    return Err("png_bad_plte");
                }
                let mut pal = Vec::with_capacity(chunk_data.len() / 3);
                for rgb in chunk_data.chunks_exact(3) {
                    pal.push([rgb[0], rgb[1], rgb[2]]);
                }
                palette = Some(pal);
            }
            b"IDAT" => {
                if ihdr.is_none() {
                    return Err("png_idat_before_ihdr");
                }
                if idat.len().saturating_add(chunk_data.len()) > MAX_IMAGE_FILE_BYTES {
                    return Err("png_idat_too_large");
                }
                idat.extend_from_slice(chunk_data);
            }
            b"IEND" => {
                saw_iend = true;
                break;
            }
            _ => {}
        }
    }

    if !saw_iend {
        return Err("png_missing_iend");
    }
    let (width_px, height_px, bit_depth, color_type) = ihdr.ok_or("png_missing_ihdr")?;
    let channels: usize = match color_type {
        0 | 3 => 1,
        2 => 3,
        4 => 2,
        6 => 4,
        _ => return Err("png_invalid_color_depth"),
    };
    let bits_per_pixel = channels * usize::from(bit_depth);
    let row_bytes =
        (width_px as usize).checked_mul(bits_per_pixel).map(|bits| bits.div_ceil(8)).ok_or("png_size_overflow")?;
    let expected_raw = (height_px as usize)
        .checked_mul(row_bytes.checked_add(1).ok_or("png_size_overflow")?)
        .ok_or("png_size_overflow")?;
    if expected_raw > MAX_DECODED_BYTES {
        return Err("png_decoded_too_large");
    }

    let raw = zlib_decompress(&idat, expected_raw).map_err(|_| "png_bad_flate")?;
    if raw.len() != expected_raw {
        return Err("png_scanline_length_mismatch");
    }

    let bpp = bits_per_pixel.div_ceil(8).max(1);
    let mut prev_row = vec![0u8; row_bytes];
    let mut cur_row = vec![0u8; row_bytes];

    let pixel_count = (width_px as usize).checked_mul(height_px as usize).ok_or("png_size_overflow")?;
    let (color_space, mut color_buf, mut alpha_buf) = match color_type {
        0 => (PdfColorSpace::DeviceGray, Vec::with_capacity(pixel_count), None),
        4 => (PdfColorSpace::DeviceGray, Vec::with_capacity(pixel_count), Some(Vec::with_capacity(pixel_count))),
        2 | 3 => (PdfColorSpace::DeviceRgb, Vec::with_capacity(pixel_count * 3), None),
        6 => (PdfColorSpace::DeviceRgb, Vec::with_capacity(pixel_count * 3), Some(Vec::with_capacity(pixel_count))),
        _ => return Err("png_invalid_color_depth"),
    };

    if color_type == 3 && palette.is_none() {
        return Err("png_indexed_missing_plte");
    }

    for row_idx in 0..(height_px as usize) {
        let base = row_idx * (row_bytes + 1);
        let filter_type = raw[base];
        let src = &raw[base + 1..base + 1 + row_bytes];
        unfilter_row(filter_type, src, &prev_row, &mut cur_row, bpp)?;

        match (color_type, bit_depth) {
            (0, 8) => color_buf.extend_from_slice(&cur_row),
            (0, 16) => {
                for pair in cur_row.chunks_exact(2) {
                    color_buf.push(pair[0]);
                }
            }
            (0, 1 | 2 | 4) => {
                let max_val = (1u16 << bit_depth) - 1;
                for x in 0..(width_px as usize) {
                    let v = extract_subbyte(&cur_row, x, bit_depth);
                    let scaled = ((u16::from(v) * 255) / max_val) as u8;
                    color_buf.push(scaled);
                }
            }
            (2, 8) => color_buf.extend_from_slice(&cur_row),
            (2, 16) => {
                for ch in cur_row.chunks_exact(2) {
                    color_buf.push(ch[0]);
                }
            }
            (3, 1 | 2 | 4 | 8) => {
                let pal = palette.as_ref().ok_or("png_indexed_missing_plte")?;
                for x in 0..(width_px as usize) {
                    let idx = usize::from(extract_subbyte(&cur_row, x, bit_depth));
                    let rgb = *pal.get(idx).ok_or("png_palette_index_oob")?;
                    color_buf.extend_from_slice(&rgb);
                }
            }
            (4, 8) => {
                let alpha = alpha_buf.as_mut().ok_or("png_alpha_missing")?;
                for px in cur_row.chunks_exact(2) {
                    color_buf.push(px[0]);
                    alpha.push(px[1]);
                }
            }
            (4, 16) => {
                let alpha = alpha_buf.as_mut().ok_or("png_alpha_missing")?;
                for px in cur_row.chunks_exact(4) {
                    color_buf.push(px[0]);
                    alpha.push(px[2]);
                }
            }
            (6, 8) => {
                let alpha = alpha_buf.as_mut().ok_or("png_alpha_missing")?;
                for px in cur_row.chunks_exact(4) {
                    color_buf.extend_from_slice(&px[..3]);
                    alpha.push(px[3]);
                }
            }
            (6, 16) => {
                let alpha = alpha_buf.as_mut().ok_or("png_alpha_missing")?;
                for px in cur_row.chunks_exact(8) {
                    color_buf.push(px[0]);
                    color_buf.push(px[2]);
                    color_buf.push(px[4]);
                    alpha.push(px[6]);
                }
            }
            _ => return Err("png_invalid_color_depth"),
        }

        prev_row.copy_from_slice(&cur_row);
    }

    let smask = alpha_buf.map(|alpha| PdfSoftMask { width_px, height_px, data: zlib_compress(&alpha) });

    Ok(PdfImageObject {
        width_px,
        height_px,
        color_space,
        bits_per_component: 8,
        filter: PdfImageFilter::FlateDecode,
        data: zlib_compress(&color_buf),
        smask,
    })
}

fn extract_subbyte(row: &[u8], x: usize, bit_depth: u8) -> u8 {
    match bit_depth {
        8 => row[x],
        4 => {
            let byte = row[x / 2];
            if x % 2 == 0 {
                byte >> 4
            } else {
                byte & 0x0F
            }
        }
        2 => {
            let byte = row[x / 4];
            let shift = 6 - 2 * (x % 4);
            (byte >> shift) & 0x03
        }
        1 => {
            let byte = row[x / 8];
            let shift = 7 - (x % 8);
            (byte >> shift) & 0x01
        }
        _ => 0,
    }
}

fn unfilter_row(filter_type: u8, src: &[u8], prev: &[u8], dst: &mut [u8], bpp: usize) -> Result<(), &'static str> {
    match filter_type {
        0 => {
            dst.copy_from_slice(src);
        }
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
        _ => return Err("png_unknown_filter"),
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
