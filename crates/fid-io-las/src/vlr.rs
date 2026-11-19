//! Variable-length records (VLRs) and extended VLRs (EVLRs), with bounded, untrusted
//! payloads (REQ-IO.CLOUD.LAS.HDR-005), and the extra-bytes descriptors.
//!
//! A VLR header is 54 bytes: two reserved bytes, a 16-byte user ID, a 16-bit record ID, a
//! 16-bit payload length and a 32-byte description. An EVLR header is 60 bytes: the same
//! fields with a 64-bit payload length. Text fields are NUL-padded and untrusted.

use crate::bytes::{u8_at, until_nul};
use crate::error::{LasError, LasField};
use fid_io_reg::{LimitKind, UntrustedText};

/// Size of a VLR header.
pub const VLR_HEADER_LEN: usize = 54;
/// Size of an EVLR header.
pub const EVLR_HEADER_LEN: usize = 60;
/// Default bound of one VLR or EVLR payload: 64 MiB (REQ-IO.CLOUD.LAS.HDR-005).
pub const DEFAULT_MAX_PAYLOAD: u64 = 64 * 1024 * 1024;
/// Size of one extra-bytes descriptor.
pub const EXTRA_BYTES_DESCRIPTOR_LEN: usize = 192;

/// User ID of the records the LAS specification itself defines.
pub const USER_LASF_SPEC: &[u8] = b"LASF_Spec";
/// User ID of the coordinate reference system records.
pub const USER_LASF_PROJECTION: &[u8] = b"LASF_Projection";
/// Record ID (under [`USER_LASF_SPEC`]) of the extra-bytes descriptors.
pub const RECORD_EXTRA_BYTES: u16 = 4;
/// Record ID (under [`USER_LASF_SPEC`]) of the waveform data packets EVLR.
pub const RECORD_WAVEFORM_DATA: u16 = 65535;

/// Bytes taken from a file whose length was checked against a bound before they were
/// read. They are untrusted data: never instructions.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BoundedBytes(Vec<u8>);

impl BoundedBytes {
    /// Wraps `bytes`, failing `limit_exceeded{alloc}` if longer than `max`.
    pub fn new(bytes: Vec<u8>, max: u64) -> Result<BoundedBytes, LasError> {
        if bytes.len() as u64 > max {
            return Err(LasError::LimitExceeded { limit: LimitKind::Alloc, value: max, at: 0 });
        }
        Ok(BoundedBytes(bytes))
    }

    /// The bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Length in bytes.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// True if empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The bytes up to the first NUL as untrusted text (invalid UTF-8 replaced).
    pub fn to_text(&self) -> UntrustedText {
        UntrustedText::from_utf8_lossy(until_nul(&self.0))
    }
}

/// A record payload: loaded and bounded, or (waveform data) counted but not imported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordPayload {
    /// The payload bytes.
    Loaded(BoundedBytes),
    /// Waveform data packets: only their length is kept.
    NotImported {
        /// Payload length in bytes.
        len: u64,
    },
}

impl RecordPayload {
    /// The payload length in bytes.
    pub fn len(&self) -> u64 {
        match self {
            RecordPayload::Loaded(b) => b.len() as u64,
            RecordPayload::NotImported { len } => *len,
        }
    }

    /// True if the payload is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The loaded bytes, if any.
    pub fn bytes(&self) -> Option<&BoundedBytes> {
        match self {
            RecordPayload::Loaded(b) => Some(b),
            RecordPayload::NotImported { .. } => None,
        }
    }
}

/// What a record is, by its user ID and record ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VlrKind {
    /// Coordinate system as WKT (untrusted text).
    WktCrs,
    /// GeoTIFF key directory.
    GeoKeyDirectory,
    /// GeoTIFF double parameters.
    GeoDoubleParams,
    /// GeoTIFF ASCII parameters.
    GeoAsciiParams,
    /// Extra-bytes descriptors.
    ExtraBytes,
    /// A waveform packet descriptor.
    WaveformPacketDescriptor,
    /// Waveform data packets (not imported).
    WaveformData,
    /// Anything else.
    Other,
}

/// A VLR or EVLR. Text fields are kept as their stored bytes so a written file is
/// byte-identical; read them as untrusted text through the accessors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vlr {
    /// The two reserved bytes.
    pub reserved: u16,
    /// User ID, 16 bytes as stored.
    pub user_id: [u8; 16],
    /// Record ID.
    pub record_id: u16,
    /// Description, 32 bytes as stored.
    pub description: [u8; 32],
    /// Payload.
    pub payload: RecordPayload,
}

fn padded<const N: usize>(src: &[u8]) -> [u8; N] {
    let mut out = [0u8; N];
    for (d, s) in out.iter_mut().zip(src) {
        *d = *s;
    }
    out
}

impl Vlr {
    /// A record with `user_id` and `description` NUL-padded (and cut) to 16 and 32 bytes;
    /// fails `limit_exceeded{alloc}` if `payload` is longer than [`DEFAULT_MAX_PAYLOAD`].
    pub fn new(user_id: &[u8], record_id: u16, description: &[u8], payload: Vec<u8>) -> Result<Vlr, LasError> {
        Ok(Vlr {
            reserved: 0,
            user_id: padded(user_id),
            record_id,
            description: padded(description),
            payload: RecordPayload::Loaded(BoundedBytes::new(payload, DEFAULT_MAX_PAYLOAD)?),
        })
    }

    /// The user ID as untrusted text.
    pub fn user_id_text(&self) -> UntrustedText {
        UntrustedText::from_utf8_lossy(until_nul(&self.user_id))
    }

    /// The description as untrusted text.
    pub fn description_text(&self) -> UntrustedText {
        UntrustedText::from_utf8_lossy(until_nul(&self.description))
    }

    /// True if the user ID (up to its first NUL) is `user`.
    pub fn is_user(&self, user: &[u8]) -> bool {
        until_nul(&self.user_id) == user
    }

    /// What the record is.
    pub fn kind(&self) -> VlrKind {
        if self.is_user(USER_LASF_PROJECTION) {
            match self.record_id {
                2112 => VlrKind::WktCrs,
                34735 => VlrKind::GeoKeyDirectory,
                34736 => VlrKind::GeoDoubleParams,
                34737 => VlrKind::GeoAsciiParams,
                _ => VlrKind::Other,
            }
        } else if self.is_user(USER_LASF_SPEC) {
            match self.record_id {
                RECORD_EXTRA_BYTES => VlrKind::ExtraBytes,
                100..=354 => VlrKind::WaveformPacketDescriptor,
                RECORD_WAVEFORM_DATA => VlrKind::WaveformData,
                _ => VlrKind::Other,
            }
        } else {
            VlrKind::Other
        }
    }

    /// The 54-byte VLR header for a payload of `len` bytes.
    pub(crate) fn vlr_header(&self, len: u16) -> Vec<u8> {
        let mut b = Vec::with_capacity(VLR_HEADER_LEN);
        b.extend_from_slice(&self.reserved.to_le_bytes());
        b.extend_from_slice(&self.user_id);
        b.extend_from_slice(&self.record_id.to_le_bytes());
        b.extend_from_slice(&len.to_le_bytes());
        b.extend_from_slice(&self.description);
        b
    }

    /// The 60-byte EVLR header for a payload of `len` bytes.
    pub(crate) fn evlr_header(&self, len: u64) -> Vec<u8> {
        let mut b = Vec::with_capacity(EVLR_HEADER_LEN);
        b.extend_from_slice(&self.reserved.to_le_bytes());
        b.extend_from_slice(&self.user_id);
        b.extend_from_slice(&self.record_id.to_le_bytes());
        b.extend_from_slice(&len.to_le_bytes());
        b.extend_from_slice(&self.description);
        b
    }
}

/// The value type of an extra-bytes attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExtraKind {
    /// Undocumented bytes, kept raw.
    Raw,
    /// Unsigned 8-bit.
    U8,
    /// Signed 8-bit.
    I8,
    /// Unsigned 16-bit.
    U16,
    /// Signed 16-bit.
    I16,
    /// Unsigned 32-bit.
    U32,
    /// Signed 32-bit.
    I32,
    /// Unsigned 64-bit.
    U64,
    /// Signed 64-bit.
    I64,
    /// IEEE binary32.
    F32,
    /// IEEE binary64.
    F64,
}

impl ExtraKind {
    /// Size of one element in bytes (1 for [`ExtraKind::Raw`]).
    pub fn size(self) -> usize {
        match self {
            ExtraKind::Raw | ExtraKind::U8 | ExtraKind::I8 => 1,
            ExtraKind::U16 | ExtraKind::I16 => 2,
            ExtraKind::U32 | ExtraKind::I32 | ExtraKind::F32 => 4,
            ExtraKind::U64 | ExtraKind::I64 | ExtraKind::F64 => 8,
        }
    }

    /// The stable name.
    pub fn as_str(self) -> &'static str {
        match self {
            ExtraKind::Raw => "raw",
            ExtraKind::U8 => "u8",
            ExtraKind::I8 => "i8",
            ExtraKind::U16 => "u16",
            ExtraKind::I16 => "i16",
            ExtraKind::U32 => "u32",
            ExtraKind::I32 => "i32",
            ExtraKind::U64 => "u64",
            ExtraKind::I64 => "i64",
            ExtraKind::F32 => "f32",
            ExtraKind::F64 => "f64",
        }
    }

    /// The scalar kind of descriptor data type codes 1 to 10.
    fn from_scalar_code(code: u8) -> Option<ExtraKind> {
        Some(match code {
            1 => ExtraKind::U8,
            2 => ExtraKind::I8,
            3 => ExtraKind::U16,
            4 => ExtraKind::I16,
            5 => ExtraKind::U32,
            6 => ExtraKind::I32,
            7 => ExtraKind::U64,
            8 => ExtraKind::I64,
            9 => ExtraKind::F32,
            10 => ExtraKind::F64,
            _ => return None,
        })
    }
}

/// One extra-bytes attribute: where its values come from and how they are typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtraSpec {
    /// Name from the descriptor (empty for bytes no descriptor covers).
    pub name: UntrustedText,
    /// Index of its descriptor, or `None` for trailing bytes no descriptor covers.
    pub descriptor: Option<usize>,
    /// Element type.
    pub kind: ExtraKind,
    /// Elements per point: 1 to 3 for typed attributes (codes 11 to 30 are 2- and
    /// 3-element arrays), the byte width for [`ExtraKind::Raw`].
    pub count: usize,
}

impl ExtraSpec {
    /// Bytes per point.
    pub fn width(&self) -> usize {
        self.kind.size().saturating_mul(self.count)
    }
}

/// Splits `extra_len` extra bytes per record into typed attributes, using the first
/// extra-bytes descriptor record among `vlrs` and `evlrs`. Bytes no descriptor covers
/// become one trailing raw attribute. Descriptors that need more bytes than the record
/// has, an unknown data type, an undocumented attribute of width 0, or a payload that is
/// not a whole number of descriptors fail `malformed{extra_bytes_descriptor}`.
pub fn extra_layout(vlrs: &[Vlr], evlrs: &[Vlr], extra_len: usize) -> Result<Vec<ExtraSpec>, LasError> {
    let bad = || LasError::malformed(LasField::ExtraBytesDescriptor);
    let mut specs = Vec::new();
    let descriptor_record = vlrs.iter().chain(evlrs).find(|v| v.kind() == VlrKind::ExtraBytes);
    let mut used = 0usize;
    if let Some(record) = descriptor_record {
        let payload = record.payload.bytes().map(BoundedBytes::as_bytes).unwrap_or(&[]);
        if payload.len() % EXTRA_BYTES_DESCRIPTOR_LEN != 0 {
            return Err(bad());
        }
        for (index, d) in payload.chunks_exact(EXTRA_BYTES_DESCRIPTOR_LEN).enumerate() {
            let code = u8_at(d, 2);
            let options = u8_at(d, 3);
            let (kind, count) = match code {
                0 if options > 0 => (ExtraKind::Raw, usize::from(options)),
                1..=10 => (ExtraKind::from_scalar_code(code).ok_or_else(bad)?, 1),
                11..=20 => (ExtraKind::from_scalar_code(code - 10).ok_or_else(bad)?, 2),
                21..=30 => (ExtraKind::from_scalar_code(code - 20).ok_or_else(bad)?, 3),
                _ => return Err(bad()),
            };
            let name = UntrustedText::from_utf8_lossy(until_nul(d.get(4..36).unwrap_or(&[])));
            let spec = ExtraSpec { name, descriptor: Some(index), kind, count };
            used = used.checked_add(spec.width()).filter(|&u| u <= extra_len).ok_or_else(bad)?;
            specs.push(spec);
        }
    }
    if used < extra_len {
        specs.push(ExtraSpec {
            name: UntrustedText::new(""),
            descriptor: None,
            kind: ExtraKind::Raw,
            count: extra_len - used,
        });
    }
    Ok(specs)
}
