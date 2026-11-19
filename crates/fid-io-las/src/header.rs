//! The public header block, LAS 1.0 to 1.4 (ASPRS LAS specification, public header block
//! section of each version; cited by designator only).
//!
//! All fields are little-endian. The block grows with the version: versions 1.0 to 1.2
//! stop after the per-axis bounding box ([`HEADER_LEN_1_0`] bytes), 1.3 adds the offset of
//! the waveform data packet record ([`HEADER_LEN_1_3`]), 1.4 adds the EVLR offset and
//! count, a 64-bit point count and fifteen 64-bit per-return counts ([`HEADER_LEN_1_4`]).
//! A declared header size larger than the version's block holds user bytes, kept verbatim.

use crate::bytes::{arr, f64_at, u16_at, u32_at, u64_at, u8_at, until_nul};
use crate::error::{LasError, LasField};
use crate::format::PointFormat;
use fid_io_reg::UntrustedText;

/// The file signature.
pub const SIGNATURE: [u8; 4] = *b"LASF";
/// Size of the public header block of versions 1.0 to 1.2.
pub const HEADER_LEN_1_0: u16 = 227;
/// Size of the public header block of version 1.3.
pub const HEADER_LEN_1_3: u16 = 235;
/// Size of the public header block of version 1.4.
pub const HEADER_LEN_1_4: u16 = 375;

/// The public header block, field by field, as stored. Values the reader checks against
/// the file (sizes, offsets, counts) are kept as read; the writer recomputes the layout
/// fields (see [`crate::LasWriter`]).
#[derive(Debug, Clone, PartialEq)]
pub struct LasHeader {
    /// File source ID (bytes 4..6; reserved in 1.0).
    pub file_source_id: u16,
    /// Global encoding bits (bytes 6..8; reserved in 1.0 and 1.1).
    pub global_encoding: u16,
    /// Project ID, a GUID, as its 16 stored bytes.
    pub project_id: [u8; 16],
    /// Major version (1).
    pub version_major: u8,
    /// Minor version (0 to 4).
    pub version_minor: u8,
    /// System identifier, 32 bytes as stored (untrusted).
    pub system_identifier: [u8; 32],
    /// Generating software, 32 bytes as stored (untrusted).
    pub generating_software: [u8; 32],
    /// File creation day of year.
    pub creation_day: u16,
    /// File creation year.
    pub creation_year: u16,
    /// Declared header size.
    pub header_size: u16,
    /// Offset of the first point record.
    pub offset_to_point_data: u32,
    /// Number of VLRs.
    pub number_of_vlrs: u32,
    /// Point data record format ID, as stored.
    pub point_data_format: u8,
    /// Point data record length (format size plus extra bytes).
    pub point_data_record_length: u16,
    /// Legacy 32-bit point count.
    pub legacy_point_count: u32,
    /// Legacy 32-bit counts by return 1 to 5.
    pub legacy_points_by_return: [u32; 5],
    /// Scale per axis (x, y, z).
    pub scale: [f64; 3],
    /// Offset per axis (x, y, z).
    pub offset: [f64; 3],
    /// Maximum per axis (x, y, z).
    pub max: [f64; 3],
    /// Minimum per axis (x, y, z).
    pub min: [f64; 3],
    /// Offset of the waveform data packet record (1.3 and later; 0 otherwise).
    pub start_of_waveform_data: u64,
    /// Offset of the first EVLR (1.4; 0 otherwise).
    pub start_of_first_evlr: u64,
    /// Number of EVLRs (1.4; 0 otherwise).
    pub number_of_evlrs: u32,
    /// 64-bit point count (1.4; 0 otherwise).
    pub point_count: u64,
    /// 64-bit counts by return 1 to 15 (1.4; zeros otherwise).
    pub points_by_return: [u64; 15],
    /// User bytes between the version's header block and the declared header size.
    pub header_extra: Vec<u8>,
}

impl Default for LasHeader {
    /// A LAS 1.4 header for point format 6 with no extra bytes, unit scale and zero
    /// offset (the writer's default output; `IO.CLOUD.LAS.QUANT` chooses real scales).
    fn default() -> Self {
        LasHeader::new(4, PointFormat::FORMAT_6, 0)
    }
}

/// Size of the version's public header block, or `None` for an unsupported version.
pub fn standard_header_len(major: u8, minor: u8) -> Option<u16> {
    match (major, minor) {
        (1, 0..=2) => Some(HEADER_LEN_1_0),
        (1, 3) => Some(HEADER_LEN_1_3),
        (1, 4) => Some(HEADER_LEN_1_4),
        _ => None,
    }
}

impl LasHeader {
    /// A header for LAS 1.`minor` with `format` and `extra_bytes` extra bytes per record;
    /// counts zero, scale 1, offset 0. The layout fields are filled in by the writer.
    pub fn new(minor: u8, format: PointFormat, extra_bytes: u16) -> LasHeader {
        LasHeader {
            file_source_id: 0,
            global_encoding: 0,
            project_id: [0; 16],
            version_major: 1,
            version_minor: minor,
            system_identifier: [0; 32],
            generating_software: [0; 32],
            creation_day: 0,
            creation_year: 0,
            header_size: standard_header_len(1, minor).unwrap_or(HEADER_LEN_1_4),
            offset_to_point_data: 0,
            number_of_vlrs: 0,
            point_data_format: format.id(),
            point_data_record_length: format.base_len().saturating_add(extra_bytes),
            legacy_point_count: 0,
            legacy_points_by_return: [0; 5],
            scale: [1.0; 3],
            offset: [0.0; 3],
            max: [0.0; 3],
            min: [0.0; 3],
            start_of_waveform_data: 0,
            start_of_first_evlr: 0,
            number_of_evlrs: 0,
            point_count: 0,
            points_by_return: [0; 15],
            header_extra: Vec::new(),
        }
    }

    /// Parses the version's header block from `raw`, which must hold at least the
    /// version's [`standard_header_len`] bytes. Checks only the signature and the
    /// version; layout checks against the file are the reader's.
    pub fn parse(raw: &[u8]) -> Result<LasHeader, LasError> {
        if raw.get(..4) != Some(&SIGNATURE[..]) {
            return Err(LasError::malformed(LasField::Signature));
        }
        let major = u8_at(raw, 24);
        let minor = u8_at(raw, 25);
        let std_len = standard_header_len(major, minor).ok_or(LasError::LasVersionUnsupported { major, minor })?;
        if raw.len() < usize::from(std_len) {
            return Err(LasError::TruncatedFile {
                declared: u64::from(std_len),
                actual: raw.len() as u64,
                unit: crate::error::CountUnit::Bytes,
            });
        }
        let axes = |at: usize| [f64_at(raw, at), f64_at(raw, at + 8), f64_at(raw, at + 16)];
        let mut legacy_by_return = [0u32; 5];
        for (i, v) in legacy_by_return.iter_mut().enumerate() {
            *v = u32_at(raw, 111 + 4 * i);
        }
        let mut h = LasHeader {
            file_source_id: u16_at(raw, 4),
            global_encoding: u16_at(raw, 6),
            project_id: arr(raw, 8),
            version_major: major,
            version_minor: minor,
            system_identifier: arr(raw, 26),
            generating_software: arr(raw, 58),
            creation_day: u16_at(raw, 90),
            creation_year: u16_at(raw, 92),
            header_size: u16_at(raw, 94),
            offset_to_point_data: u32_at(raw, 96),
            number_of_vlrs: u32_at(raw, 100),
            point_data_format: u8_at(raw, 104),
            point_data_record_length: u16_at(raw, 105),
            legacy_point_count: u32_at(raw, 107),
            legacy_points_by_return: legacy_by_return,
            scale: axes(131),
            offset: axes(155),
            // Bounds are stored as max x, min x, max y, min y, max z, min z.
            max: [f64_at(raw, 179), f64_at(raw, 195), f64_at(raw, 211)],
            min: [f64_at(raw, 187), f64_at(raw, 203), f64_at(raw, 219)],
            start_of_waveform_data: 0,
            start_of_first_evlr: 0,
            number_of_evlrs: 0,
            point_count: 0,
            points_by_return: [0; 15],
            header_extra: Vec::new(),
        };
        if minor >= 3 {
            h.start_of_waveform_data = u64_at(raw, 227);
        }
        if minor >= 4 {
            h.start_of_first_evlr = u64_at(raw, 235);
            h.number_of_evlrs = u32_at(raw, 243);
            h.point_count = u64_at(raw, 247);
            for (i, v) in h.points_by_return.iter_mut().enumerate() {
                *v = u64_at(raw, 255 + 8 * i);
            }
        }
        Ok(h)
    }

    /// The system identifier as untrusted text (up to its first NUL).
    pub fn system_identifier_text(&self) -> UntrustedText {
        UntrustedText::from_utf8_lossy(until_nul(&self.system_identifier))
    }

    /// The generating software as untrusted text (up to its first NUL).
    pub fn generating_software_text(&self) -> UntrustedText {
        UntrustedText::from_utf8_lossy(until_nul(&self.generating_software))
    }

    /// The version's header block size, or `None` for an unsupported version.
    pub fn standard_len(&self) -> Option<u16> {
        standard_header_len(self.version_major, self.version_minor)
    }

    /// The point format, if the stored ID is 0 to 10.
    pub fn format(&self) -> Option<PointFormat> {
        PointFormat::new(self.point_data_format)
    }

    /// The number of point records the header declares. Before 1.4 it is the legacy
    /// count; in 1.4 the 64-bit count, and a non-zero legacy count that differs from it
    /// is `malformed{legacy_point_count}`.
    pub fn declared_points(&self) -> Result<u64, LasError> {
        if self.version_minor >= 4 {
            let legacy = u64::from(self.legacy_point_count);
            if legacy != 0 && legacy != self.point_count {
                return Err(LasError::malformed(LasField::LegacyPointCount));
            }
            Ok(self.point_count)
        } else {
            Ok(u64::from(self.legacy_point_count))
        }
    }

    /// The header block as stored: the version's fields followed by [`header_extra`].
    /// Fields of later versions are not written for earlier ones.
    ///
    /// [`header_extra`]: LasHeader::header_extra
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(usize::from(HEADER_LEN_1_4) + self.header_extra.len());
        b.extend_from_slice(&SIGNATURE);
        b.extend_from_slice(&self.file_source_id.to_le_bytes());
        b.extend_from_slice(&self.global_encoding.to_le_bytes());
        b.extend_from_slice(&self.project_id);
        b.push(self.version_major);
        b.push(self.version_minor);
        b.extend_from_slice(&self.system_identifier);
        b.extend_from_slice(&self.generating_software);
        b.extend_from_slice(&self.creation_day.to_le_bytes());
        b.extend_from_slice(&self.creation_year.to_le_bytes());
        b.extend_from_slice(&self.header_size.to_le_bytes());
        b.extend_from_slice(&self.offset_to_point_data.to_le_bytes());
        b.extend_from_slice(&self.number_of_vlrs.to_le_bytes());
        b.push(self.point_data_format);
        b.extend_from_slice(&self.point_data_record_length.to_le_bytes());
        b.extend_from_slice(&self.legacy_point_count.to_le_bytes());
        for v in self.legacy_points_by_return {
            b.extend_from_slice(&v.to_le_bytes());
        }
        for v in self.scale.iter().chain(self.offset.iter()) {
            b.extend_from_slice(&v.to_bits().to_le_bytes());
        }
        for axis in 0..3 {
            for v in [self.max.get(axis), self.min.get(axis)] {
                b.extend_from_slice(&v.copied().unwrap_or(0.0).to_bits().to_le_bytes());
            }
        }
        if self.version_minor >= 3 {
            b.extend_from_slice(&self.start_of_waveform_data.to_le_bytes());
        }
        if self.version_minor >= 4 {
            b.extend_from_slice(&self.start_of_first_evlr.to_le_bytes());
            b.extend_from_slice(&self.number_of_evlrs.to_le_bytes());
            b.extend_from_slice(&self.point_count.to_le_bytes());
            for v in self.points_by_return {
                b.extend_from_slice(&v.to_le_bytes());
            }
        }
        b.extend_from_slice(&self.header_extra);
        b
    }
}
