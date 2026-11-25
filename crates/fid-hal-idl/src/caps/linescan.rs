//! The `LineScan@1` package for laser line and profile sensors (spec node `HAL.IDL.SENS.LINE`):
//! its settings, the columnar `ProfileBatch`, and the `LineScan-core@1` conformance profile.
//!
//! - **REQ-HAL.IDL.SENS.LINE-001:** [`ProfileBatch`] carries the sequence range, `t_host` and
//!   optional `t_dev` per profile, profile IDs, x and z [`EncodedArray`]s with one
//!   [`EncodingDescriptor`] each, an optional intensity array, a [`ValidityMask`] and an
//!   optional pose reference per profile ([`PoseColumn`]).
//! - **REQ-HAL.IDL.SENS.LINE-002:** [`ProfileBatch::validate`] and [`ProfileBatch::decode`] fail
//!   with `array_length_mismatch` unless every per-point array holds profiles x
//!   points-per-profile values and every per-profile array one value per profile.
//! - **REQ-HAL.IDL.SENS.LINE-003:** x and z must have target unit m (`encoding_invalid`), and the
//!   batch names its sensor frame ([`ProfileBatch::frame`], kind `sensor`).
//! - **REQ-HAL.IDL.SENS.LINE-004:** [`LINESCAN_CORE`] lists items `LS-01` .. `LS-05`; the built
//!   package adds the `Stream-common@1` items. [`check_batch`] evaluates `LS-01` .. `LS-04` on
//!   one batch and [`check_trigger_mode`] evaluates `LS-05`.
//! - **REQ-HAL.IDL.SENS.LINE-005:** the layout is columnar: 100 x 2 000 points with i32 x and z
//!   and no intensity is 1.6 MB of coordinates plus about 27 kB of mask, timestamps, IDs and
//!   header ([`SIZE_BUDGET_BYTES`]).
//!
//! **Wire form.** A batch is a header (RFC 8785 canonical JSON through `fid-canon`) and a
//! binary payload. Header, with closed key sets and `u64` values as canonical decimal strings:
//!
//! ```text
//! {"schema": "linescan-batch/1", "first_seq", "last_seq", "profiles", "points_per_profile",
//!  "frame": {"id", "kind": "sensor"}, "pose_stream"?: "<stream id>",
//!  "arrays": {"t_host": {"len"}, "t_dev"?: {"len"}, "profile_id": {"len"}, "pose_seq"?: {"len"},
//!             "mask": {"len"}, "x": {"encoding", "len"}, "z": {"encoding", "len"},
//!             "intensity"?: {"encoding", "len"}}}
//! ```
//!
//! `len` counts elements (mask: bits). One sequence number per profile, so
//! `last_seq = first_seq + profiles - 1`. The payload is the arrays in this order, packed
//! little-endian with no padding and nothing after them: `t_host` (i64 ns), `t_dev` (i64 ns),
//! `profile_id` (u64), `pose_seq` (u64), `mask` (one bit per sample, sample `i` in bit `i % 8`
//! of byte `i / 8`, 1 = valid, padding bits 0), then `x`, `z` and `intensity` in their declared
//! numeric types. Sample `i` is point `i % points_per_profile` of profile
//! `i / points_per_profile`.
//!
//! The header has no origin field: provenance is the acquisition-batch envelope's job
//! (`HAL.IDL.STAMP`), and a header cannot claim to come from a device (D-045). Stream records
//! built from batch summaries go through [`crate::caps::stream::BatchRecord::new`], which refuses
//! `device`.

use crate::base::config::{validate as validate_config, ConfigError, ConfigSchema, Configuration};
use crate::base::config::{SettingSchema, SettingType, SettingValue};
use crate::base::device::{ConformanceProfile, ProfileItem};
use crate::caps::stream::{BatchSummary, ItemFailure, StreamError, StreamingDescription, StreamingPackage};
use crate::package::{BatchColumnDesc, BatchSchemaDesc, CapabilityPackage, PackageDescription};
use crate::package::{PackageManifest, SemanticsNote};
use crate::units::{kept, EncodingDescriptor, Frame, FrameKind, NumericType, RawValue, SiUnit};
use fid_canon::CValue;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Package name.
pub const PACKAGE_NAME: &str = "LineScan";
/// Package version.
pub const PACKAGE_VERSION: &str = "1.0.0";
/// ID of the conformance profile.
pub const PROFILE_ID: &str = "LineScan-core@1";
/// ID of the package's batch schema.
pub const BATCH_SCHEMA_ID: &str = "linescan.profile_batch";
/// Wire tag of a batch header.
pub const HEADER_TAG: &str = "linescan-batch/1";

/// Item `LS-01`: encoding declaration.
pub const ITEM_ENCODING: &str = "LS-01";
/// Item `LS-02`: array lengths.
pub const ITEM_LENGTHS: &str = "LS-02";
/// Item `LS-03`: timestamp monotonicity.
pub const ITEM_TIMESTAMPS: &str = "LS-03";
/// Item `LS-04`: masked-sample handling.
pub const ITEM_MASK: &str = "LS-04";
/// Item `LS-05`: trigger-mode behaviour.
pub const ITEM_TRIGGER: &str = "LS-05";

/// REQ-005 budget for 100 x 2 000 points with i32 x and z and no intensity (1.7 MB, read as
/// 10^6 bytes as in `HAL.IDL.SENS.STREAM`).
pub const SIZE_BUDGET_BYTES: u64 = 1_700_000;
/// Largest accepted header, in bytes.
pub const MAX_HEADER_BYTES: usize = 64 * 1024;
/// Deepest accepted header nesting (root, `arrays`, one array, its encoding, one spare).
const MAX_HEADER_DEPTH: usize = 5;
/// Most JSON elements in one header (keys and values).
const MAX_HEADER_ELEMENTS: usize = 256;
/// Most profiles in one batch.
pub const MAX_PROFILES: u64 = 100_000;
/// Most points in one profile.
pub const MAX_POINTS_PER_PROFILE: u64 = 65_536;
/// Most samples (profiles x points) in one batch; bounds every allocation by the header.
pub const MAX_SAMPLES: u64 = 1 << 24;
/// Longest pose stream ID, in bytes.
pub const MAX_STREAM_ID_BYTES: usize = 64;
/// `LS-05`: largest deviation of a device-time profile interval from the trigger period, in
/// percent (proposal) [unverified].
pub const TRIGGER_TOLERANCE_PERCENT: i128 = 5;

// ------------------------------------------------------------------ errors

/// A failure of a profile batch, a setting or the package (stable codes from [`Self::code`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineScanError {
    /// `array_length_mismatch`: an array, the sequence range or the payload does not have the
    /// length the batch shape requires. Element counts (mask: bits; payload: bytes).
    ArrayLength { array: &'static str, expected: u64, found: u64 },
    /// `encoding_invalid`: a malformed encoding, or x or z without target unit m, or
    /// intensity without target unit `1`.
    Encoding { array: &'static str, reason: String },
    /// `timestamps_not_monotonic`: `t_host` or `t_dev` decreases at profile `index`.
    NotMonotonic { array: &'static str, index: usize },
    /// `sample_not_finite`: a sample whose mask bit is set does not decode to a finite value.
    SampleNotFinite { array: &'static str, index: usize },
    /// `mask_invalid`: the mask bytes do not match its bit count, or a padding bit is set.
    MaskInvalid,
    /// `batch_header_invalid`: a header that is not the closed `linescan-batch/1` shape, or a
    /// batch outside the size limits; `pointer` is an RFC 6901 JSON pointer.
    Header { pointer: String, reason: &'static str },
    /// `settings_invalid`: a core setting missing from the device schema, of the wrong type,
    /// not digested, or with a value the trigger mode or geometry forbids.
    Settings { setting: String, reason: &'static str },
    /// The configuration does not validate against its schema (`CONFIG_INVALID`,
    /// `OUT_OF_RANGE`).
    Config(ConfigError),
    /// The streaming template or the vendor-extension rule failed.
    Stream(StreamError),
}

impl LineScanError {
    /// Stable machine-readable error code.
    pub fn code(&self) -> &'static str {
        match self {
            LineScanError::ArrayLength { .. } => "array_length_mismatch",
            LineScanError::Encoding { .. } => "encoding_invalid",
            LineScanError::NotMonotonic { .. } => "timestamps_not_monotonic",
            LineScanError::SampleNotFinite { .. } => "sample_not_finite",
            LineScanError::MaskInvalid => "mask_invalid",
            LineScanError::Header { .. } => "batch_header_invalid",
            LineScanError::Settings { .. } => "settings_invalid",
            LineScanError::Config(e) => e.code(),
            LineScanError::Stream(e) => e.code(),
        }
    }

    /// The `LineScan-core@1` item a batch failure belongs to, if any.
    pub fn item(&self) -> Option<&'static str> {
        match self {
            LineScanError::Encoding { .. } => Some(ITEM_ENCODING),
            LineScanError::ArrayLength { .. } => Some(ITEM_LENGTHS),
            LineScanError::NotMonotonic { .. } => Some(ITEM_TIMESTAMPS),
            LineScanError::SampleNotFinite { .. } | LineScanError::MaskInvalid => Some(ITEM_MASK),
            _ => None,
        }
    }

    fn header(pointer: impl Into<String>, reason: &'static str) -> LineScanError {
        LineScanError::Header { pointer: pointer.into(), reason }
    }

    fn settings(setting: &str, reason: &'static str) -> LineScanError {
        LineScanError::Settings { setting: kept(setting), reason }
    }
}

impl fmt::Display for LineScanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LineScanError::ArrayLength { array, expected, found } => {
                write!(f, "array_length_mismatch: {array} expected {expected}, found {found}")
            }
            LineScanError::Encoding { array, reason } => write!(f, "encoding_invalid: {array}: {reason}"),
            LineScanError::NotMonotonic { array, index } => {
                write!(f, "timestamps_not_monotonic: {array} decreases at profile {index}")
            }
            LineScanError::SampleNotFinite { array, index } => write!(f, "sample_not_finite: {array} at {index}"),
            LineScanError::MaskInvalid => f.write_str("mask_invalid"),
            LineScanError::Header { pointer, reason } => write!(f, "batch_header_invalid{{{pointer}}}: {reason}"),
            LineScanError::Settings { setting, reason } => write!(f, "settings_invalid: {setting:?}: {reason}"),
            LineScanError::Config(e) => write!(f, "{e}"),
            LineScanError::Stream(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for LineScanError {}

// ------------------------------------------------------------------ arrays

/// Bytes per element of a numeric type.
pub const fn element_bytes(t: NumericType) -> usize {
    match t {
        NumericType::I16 | NumericType::U16 => 2,
        NumericType::I32 | NumericType::U32 | NumericType::F32 => 4,
        NumericType::I64 | NumericType::F64 => 8,
    }
}

/// One encoded per-point array: its declared encoding and its raw little-endian bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct EncodedArray {
    pub descriptor: EncodingDescriptor,
    pub bytes: Vec<u8>,
}

impl EncodedArray {
    /// An array from raw values, which must all have the descriptor's numeric type
    /// (`encoding_invalid`).
    pub fn from_values(descriptor: EncodingDescriptor, values: &[RawValue]) -> Result<EncodedArray, LineScanError> {
        let mut bytes = Vec::with_capacity(values.len().saturating_mul(element_bytes(descriptor.numeric())));
        for v in values {
            if v.numeric() != descriptor.numeric() {
                return Err(LineScanError::Encoding {
                    array: "values",
                    reason: format!(
                        "raw value is {} but the descriptor declares {}",
                        v.numeric(),
                        descriptor.numeric()
                    ),
                });
            }
            match *v {
                RawValue::I16(x) => bytes.extend_from_slice(&x.to_le_bytes()),
                RawValue::I32(x) => bytes.extend_from_slice(&x.to_le_bytes()),
                RawValue::I64(x) => bytes.extend_from_slice(&x.to_le_bytes()),
                RawValue::U16(x) => bytes.extend_from_slice(&x.to_le_bytes()),
                RawValue::U32(x) => bytes.extend_from_slice(&x.to_le_bytes()),
                RawValue::F32(x) => bytes.extend_from_slice(&x.to_le_bytes()),
                RawValue::F64(x) => bytes.extend_from_slice(&x.to_le_bytes()),
            }
        }
        Ok(EncodedArray { descriptor, bytes })
    }

    /// Number of whole elements in the bytes.
    pub fn len(&self) -> usize {
        self.bytes.len() / element_bytes(self.descriptor.numeric())
    }

    /// Whether the array has no elements.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The raw value of element `i`.
    pub fn raw(&self, i: usize) -> Option<RawValue> {
        let w = element_bytes(self.descriptor.numeric());
        let start = i.checked_mul(w)?;
        let chunk = self.bytes.get(start..start.checked_add(w)?)?;
        read_raw(self.descriptor.numeric(), chunk)
    }
}

fn read_raw(t: NumericType, c: &[u8]) -> Option<RawValue> {
    Some(match t {
        NumericType::I16 => RawValue::I16(i16::from_le_bytes(c.try_into().ok()?)),
        NumericType::U16 => RawValue::U16(u16::from_le_bytes(c.try_into().ok()?)),
        NumericType::I32 => RawValue::I32(i32::from_le_bytes(c.try_into().ok()?)),
        NumericType::U32 => RawValue::U32(u32::from_le_bytes(c.try_into().ok()?)),
        NumericType::F32 => RawValue::F32(f32::from_le_bytes(c.try_into().ok()?)),
        NumericType::I64 => RawValue::I64(i64::from_le_bytes(c.try_into().ok()?)),
        NumericType::F64 => RawValue::F64(f64::from_le_bytes(c.try_into().ok()?)),
    })
}

/// The validity mask: one bit per sample, 1 = valid, sample `i` in bit `i % 8` of byte `i / 8`.
/// The bytes always match the bit count and padding bits are always 0.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidityMask {
    bits: u64,
    bytes: Vec<u8>,
}

impl ValidityMask {
    /// A mask of `bits` samples, all valid.
    pub fn all_valid(bits: u64) -> ValidityMask {
        let n = mask_bytes(bits);
        let mut bytes = vec![0xff; n];
        if let Some(last) = bytes.last_mut() {
            let used = (bits % 8) as u32;
            if used != 0 {
                *last = (1u8 << used) - 1;
            }
        }
        ValidityMask { bits, bytes }
    }

    /// A mask read from its packed bytes; a wrong byte count or a set padding bit is
    /// `mask_invalid`.
    pub fn from_bytes(bits: u64, bytes: Vec<u8>) -> Result<ValidityMask, LineScanError> {
        if bytes.len() != mask_bytes(bits) {
            return Err(LineScanError::MaskInvalid);
        }
        let used = (bits % 8) as u32;
        if let (Some(last), true) = (bytes.last(), used != 0) {
            if last >> used != 0 {
                return Err(LineScanError::MaskInvalid);
            }
        }
        Ok(ValidityMask { bits, bytes })
    }

    /// Number of samples the mask covers.
    pub fn bits(&self) -> u64 {
        self.bits
    }

    /// The packed bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Whether sample `i` is valid; samples outside the mask are not.
    pub fn is_valid(&self, i: u64) -> bool {
        i < self.bits
            && usize::try_from(i / 8).ok().and_then(|b| self.bytes.get(b)).is_some_and(|b| (b >> (i % 8)) & 1 == 1)
    }

    /// Marks sample `i` valid or masked; samples outside the mask are ignored.
    pub fn set(&mut self, i: u64, valid: bool) {
        if i >= self.bits {
            return;
        }
        if let Some(b) = usize::try_from(i / 8).ok().and_then(|b| self.bytes.get_mut(b)) {
            let bit = 1u8 << (i % 8);
            if valid {
                *b |= bit;
            } else {
                *b &= !bit;
            }
        }
    }

    /// Number of valid samples.
    pub fn count_valid(&self) -> u64 {
        self.bytes.iter().map(|b| u64::from(b.count_ones())).sum()
    }
}

fn mask_bytes(bits: u64) -> usize {
    usize::try_from(bits.div_ceil(8)).unwrap_or(usize::MAX)
}

/// The pose reference of one profile: a sample of a `PortablePose@1` stream.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PoseRef {
    pub stream_id: String,
    pub seq: u64,
}

/// Pose references for every profile of a batch: one pose stream and one sequence number per
/// profile. A batch has a pose reference for all its profiles or for none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoseColumn {
    pub stream_id: String,
    pub seqs: Vec<u64>,
}

/// One batch of profiles (REQ-001). [`ProfileBatch::validate`] checks it; [`Self::encode`]
/// and [`Self::decode`] convert it to and from the wire form.
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileBatch {
    /// Sequence number of the first profile; profile `k` has `first_seq + k`.
    pub first_seq: u64,
    /// Number of profiles.
    pub profiles: u64,
    /// Points in every profile.
    pub points_per_profile: u64,
    /// The sensor frame x and z are expressed in (REQ-003).
    pub frame: Frame,
    /// Host time of each profile, in ns.
    pub t_host: Vec<i64>,
    /// Device time of each profile, in ns, when the device has a clock.
    pub t_dev: Option<Vec<i64>>,
    /// The device's ID of each profile.
    pub profile_ids: Vec<u64>,
    /// Lateral coordinate of each sample.
    pub x: EncodedArray,
    /// Range coordinate of each sample.
    pub z: EncodedArray,
    /// Intensity of each sample (dimensionless).
    pub intensity: Option<EncodedArray>,
    /// Which samples are valid.
    pub mask: ValidityMask,
    /// Pose reference of each profile.
    pub pose: Option<PoseColumn>,
}

/// A batch in wire form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedBatch {
    /// The RFC 8785 canonical header.
    pub header: Vec<u8>,
    /// The packed payload.
    pub payload: Vec<u8>,
}

impl EncodedBatch {
    /// Serialised size: header plus payload, in bytes.
    pub fn size(&self) -> u64 {
        (self.header.len() as u64).saturating_add(self.payload.len() as u64)
    }
}

impl ProfileBatch {
    /// Sequence number of the last profile.
    pub fn last_seq(&self) -> Option<u64> {
        self.first_seq.checked_add(self.profiles.checked_sub(1)?)
    }

    /// Number of samples (profiles x points per profile).
    pub fn samples(&self) -> Option<u64> {
        self.profiles.checked_mul(self.points_per_profile)
    }

    /// The pose reference of profile `k`.
    pub fn pose_ref(&self, k: usize) -> Option<PoseRef> {
        let p = self.pose.as_ref()?;
        Some(PoseRef { stream_id: p.stream_id.clone(), seq: *p.seqs.get(k)? })
    }

    /// Checks the batch, in order: shape and limits (`batch_header_invalid`); the x, z and
    /// intensity encodings (`encoding_invalid`); every array length (`array_length_mismatch`);
    /// `t_host`, then `t_dev`, never decreasing (`timestamps_not_monotonic`); every valid
    /// sample of x, z and intensity decoding to a finite value (`sample_not_finite`). Masked
    /// samples are not decoded, so they may hold any raw value.
    pub fn validate(&self) -> Result<(), LineScanError> {
        let samples = self.check_shape()?;
        check_encoding("x", &self.x.descriptor, SiUnit::Metre)?;
        check_encoding("z", &self.z.descriptor, SiUnit::Metre)?;
        if let Some(i) = &self.intensity {
            check_encoding("intensity", &i.descriptor, SiUnit::One)?;
        }
        let p = self.profiles;
        check_len("t_host", p, self.t_host.len() as u64)?;
        if let Some(t) = &self.t_dev {
            check_len("t_dev", p, t.len() as u64)?;
        }
        check_len("profile_id", p, self.profile_ids.len() as u64)?;
        if let Some(pose) = &self.pose {
            check_len("pose_seq", p, pose.seqs.len() as u64)?;
        }
        check_len("mask", samples, self.mask.bits())?;
        check_array("x", samples, &self.x)?;
        check_array("z", samples, &self.z)?;
        if let Some(i) = &self.intensity {
            check_array("intensity", samples, i)?;
        }
        check_monotonic("t_host", &self.t_host)?;
        if let Some(t) = &self.t_dev {
            check_monotonic("t_dev", t)?;
        }
        let mut arrays = vec![("x", &self.x), ("z", &self.z)];
        if let Some(i) = &self.intensity {
            arrays.push(("intensity", i));
        }
        for (name, a) in arrays {
            for i in 0..samples {
                if !self.mask.is_valid(i) {
                    continue;
                }
                let index = usize::try_from(i).unwrap_or(usize::MAX);
                let finite = a.raw(index).is_some_and(|raw| a.descriptor.decode(raw).is_ok());
                if !finite {
                    return Err(LineScanError::SampleNotFinite { array: name, index });
                }
            }
        }
        Ok(())
    }

    fn check_shape(&self) -> Result<u64, LineScanError> {
        check_limits(self.profiles, self.points_per_profile)?;
        if self.last_seq().is_none() {
            return Err(LineScanError::header("/last_seq", "seq_overflow"));
        }
        if self.frame.kind != FrameKind::Sensor {
            return Err(LineScanError::header("/frame/kind", "frame_not_sensor"));
        }
        if let Some(p) = &self.pose {
            if !is_stream_id(&p.stream_id) {
                return Err(LineScanError::header("/pose_stream", "stream_id_invalid"));
            }
        }
        self.samples().ok_or_else(|| LineScanError::header("/profiles", "too_large"))
    }

    /// The wire header (validates the batch first).
    pub fn header_bytes(&self) -> Result<Vec<u8>, LineScanError> {
        self.validate()?;
        let len = |n: u64| obj(vec![("len", dec(n))]);
        let enc = |a: &EncodedArray| obj(vec![("encoding", a.descriptor.to_value()), ("len", dec(a.len() as u64))]);
        let p = self.profiles;
        let mut arrays = vec![
            ("mask", len(self.mask.bits())),
            ("profile_id", len(p)),
            ("t_host", len(p)),
            ("x", enc(&self.x)),
            ("z", enc(&self.z)),
        ];
        if self.t_dev.is_some() {
            arrays.push(("t_dev", len(p)));
        }
        if self.pose.is_some() {
            arrays.push(("pose_seq", len(p)));
        }
        if let Some(i) = &self.intensity {
            arrays.push(("intensity", enc(i)));
        }
        let mut top = vec![
            ("arrays", obj(arrays)),
            ("first_seq", dec(self.first_seq)),
            ("frame", self.frame.to_value()),
            ("last_seq", dec(self.last_seq().unwrap_or(u64::MAX))),
            ("points_per_profile", dec(self.points_per_profile)),
            ("profiles", dec(p)),
            ("schema", CValue::Str(HEADER_TAG.to_string())),
        ];
        if let Some(pose) = &self.pose {
            top.push(("pose_stream", CValue::Str(pose.stream_id.clone())));
        }
        fid_canon::to_canonical_bytes(&obj(top)).map_err(|_| LineScanError::header("", "json"))
    }

    /// Payload size in bytes, from the array lengths.
    pub fn payload_len(&self) -> u64 {
        let p = self.profiles.saturating_mul(8);
        let mut n = p.saturating_mul(2); // t_host, profile_id
        if self.t_dev.is_some() {
            n = n.saturating_add(p);
        }
        if self.pose.is_some() {
            n = n.saturating_add(p);
        }
        n = n.saturating_add(self.mask.as_bytes().len() as u64);
        n = n.saturating_add(self.x.bytes.len() as u64).saturating_add(self.z.bytes.len() as u64);
        if let Some(i) = &self.intensity {
            n = n.saturating_add(i.bytes.len() as u64);
        }
        n
    }

    /// The wire form (validates the batch first).
    pub fn encode(&self) -> Result<EncodedBatch, LineScanError> {
        let header = self.header_bytes()?;
        let mut payload = Vec::with_capacity(usize::try_from(self.payload_len()).unwrap_or(0));
        for t in &self.t_host {
            payload.extend_from_slice(&t.to_le_bytes());
        }
        if let Some(td) = &self.t_dev {
            for t in td {
                payload.extend_from_slice(&t.to_le_bytes());
            }
        }
        for id in &self.profile_ids {
            payload.extend_from_slice(&id.to_le_bytes());
        }
        if let Some(pose) = &self.pose {
            for s in &pose.seqs {
                payload.extend_from_slice(&s.to_le_bytes());
            }
        }
        payload.extend_from_slice(self.mask.as_bytes());
        payload.extend_from_slice(&self.x.bytes);
        payload.extend_from_slice(&self.z.bytes);
        if let Some(i) = &self.intensity {
            payload.extend_from_slice(&i.bytes);
        }
        Ok(EncodedBatch { header, payload })
    }

    /// Reads and checks a batch from its wire form, in order: the header shape
    /// (`batch_header_invalid`); the sequence range against `profiles`; the limits; the
    /// encodings (`encoding_invalid`); every declared array length, then the payload size
    /// (`array_length_mismatch`); the mask padding (`mask_invalid`); then the rest of
    /// [`ProfileBatch::validate`].
    pub fn decode(header: &[u8], payload: &[u8]) -> Result<ProfileBatch, LineScanError> {
        let h = Header::parse(header)?;
        let span =
            h.last_seq.checked_sub(h.first_seq).ok_or_else(|| LineScanError::header("/last_seq", "range_reversed"))?;
        let span = span.checked_add(1).ok_or_else(|| LineScanError::header("/last_seq", "seq_overflow"))?;
        if span != h.profiles {
            return Err(LineScanError::ArrayLength { array: "seq_range", expected: h.profiles, found: span });
        }
        check_limits(h.profiles, h.points_per_profile)?;
        let samples = h.profiles.saturating_mul(h.points_per_profile);
        check_encoding("x", &h.x.1, SiUnit::Metre)?;
        check_encoding("z", &h.z.1, SiUnit::Metre)?;
        if let Some((_, d)) = &h.intensity {
            check_encoding("intensity", d, SiUnit::One)?;
        }
        let p = h.profiles;
        check_len("t_host", p, h.t_host)?;
        if let Some(n) = h.t_dev {
            check_len("t_dev", p, n)?;
        }
        check_len("profile_id", p, h.profile_id)?;
        if let Some(n) = h.pose_seq {
            check_len("pose_seq", p, n)?;
        }
        check_len("mask", samples, h.mask)?;
        check_len("x", samples, h.x.0)?;
        check_len("z", samples, h.z.0)?;
        if let Some((n, _)) = &h.intensity {
            check_len("intensity", samples, *n)?;
        }

        // Every length is now within MAX_SAMPLES, so the sums below cannot overflow.
        let per_profile = 2 + u64::from(h.t_dev.is_some()) + u64::from(h.pose_seq.is_some());
        let point_bytes = |d: &EncodingDescriptor| samples * element_bytes(d.numeric()) as u64;
        let mut want = p * 8 * per_profile + samples.div_ceil(8) + point_bytes(&h.x.1) + point_bytes(&h.z.1);
        if let Some((_, d)) = &h.intensity {
            want += point_bytes(d);
        }
        if want != payload.len() as u64 {
            return Err(LineScanError::ArrayLength { array: "payload", expected: want, found: payload.len() as u64 });
        }

        let mut r = Reader { rest: payload };
        let t_host = r.i64s(p);
        let t_dev = h.t_dev.map(|_| r.i64s(p));
        let profile_ids = r.u64s(p);
        let pose = match (h.pose_stream, h.pose_seq) {
            (Some(stream_id), Some(_)) => Some(PoseColumn { stream_id, seqs: r.u64s(p) }),
            _ => None,
        };
        let mask = ValidityMask::from_bytes(samples, r.take(samples.div_ceil(8)).to_vec())?;
        let x = EncodedArray { descriptor: h.x.1, bytes: r.take(point_bytes(&h.x.1)).to_vec() };
        let z = EncodedArray { descriptor: h.z.1, bytes: r.take(point_bytes(&h.z.1)).to_vec() };
        let intensity =
            h.intensity.map(|(_, d)| EncodedArray { descriptor: d, bytes: r.take(point_bytes(&d)).to_vec() });
        let batch = ProfileBatch {
            first_seq: h.first_seq,
            profiles: p,
            points_per_profile: h.points_per_profile,
            frame: h.frame,
            t_host,
            t_dev,
            profile_ids,
            x,
            z,
            intensity,
            mask,
            pose,
        };
        batch.validate()?;
        Ok(batch)
    }

    /// The `Stream-common@1` summary of this batch: its sequence range, its serialised size
    /// and, with device time, the span from its first to its last profile.
    pub fn summary(&self) -> Result<BatchSummary, LineScanError> {
        let header = self.header_bytes()?;
        Ok(BatchSummary {
            first_seq: self.first_seq,
            last_seq: self.last_seq().unwrap_or(u64::MAX),
            bytes: (header.len() as u64).saturating_add(self.payload_len()),
            span_ns: self.span_ns(),
        })
    }

    fn span_ns(&self) -> Option<u64> {
        let t = self.t_dev.as_ref()?;
        let d = i128::from(*t.last()?) - i128::from(*t.first()?);
        u64::try_from(d).ok()
    }
}

/// Items `LS-01` .. `LS-04` on one batch in wire form: decodes and validates it, and returns
/// its `Stream-common@1` summary with `bytes` the exact header plus payload size. The error's
/// [`LineScanError::item`] names the failing item.
pub fn check_batch(header: &[u8], payload: &[u8]) -> Result<BatchSummary, LineScanError> {
    let batch = ProfileBatch::decode(header, payload)?;
    Ok(BatchSummary {
        first_seq: batch.first_seq,
        last_seq: batch.last_seq().unwrap_or(u64::MAX),
        bytes: (header.len() as u64).saturating_add(payload.len() as u64),
        span_ns: batch.span_ns(),
    })
}

fn check_limits(profiles: u64, points: u64) -> Result<(), LineScanError> {
    if profiles == 0 || profiles > MAX_PROFILES {
        return Err(LineScanError::header("/profiles", "out_of_range"));
    }
    if points == 0 || points > MAX_POINTS_PER_PROFILE {
        return Err(LineScanError::header("/points_per_profile", "out_of_range"));
    }
    if profiles.saturating_mul(points) > MAX_SAMPLES {
        return Err(LineScanError::header("/profiles", "too_large"));
    }
    Ok(())
}

fn check_encoding(array: &'static str, d: &EncodingDescriptor, unit: SiUnit) -> Result<(), LineScanError> {
    if d.unit() != unit {
        return Err(LineScanError::Encoding {
            array,
            reason: format!("target unit is {:?}, not {:?}", d.unit().as_str(), unit.as_str()),
        });
    }
    Ok(())
}

fn check_len(array: &'static str, expected: u64, found: u64) -> Result<(), LineScanError> {
    if expected != found {
        return Err(LineScanError::ArrayLength { array, expected, found });
    }
    Ok(())
}

fn check_array(array: &'static str, samples: u64, a: &EncodedArray) -> Result<(), LineScanError> {
    let w = element_bytes(a.descriptor.numeric()) as u64;
    let bytes = a.bytes.len() as u64;
    if bytes != samples.saturating_mul(w) {
        return Err(LineScanError::ArrayLength { array, expected: samples, found: bytes / w });
    }
    Ok(())
}

fn check_monotonic(array: &'static str, t: &[i64]) -> Result<(), LineScanError> {
    match t.windows(2).position(|w| w[1] < w[0]) {
        Some(i) => Err(LineScanError::NotMonotonic { array, index: i + 1 }),
        None => Ok(()),
    }
}

/// A pose stream ID: `[a-z0-9][a-z0-9._-]*`, at most [`MAX_STREAM_ID_BYTES`] bytes.
fn is_stream_id(s: &str) -> bool {
    let mut b = s.bytes();
    s.len() <= MAX_STREAM_ID_BYTES
        && matches!(b.next(), Some(b'a'..=b'z' | b'0'..=b'9'))
        && b.all(|c| matches!(c, b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-'))
}

/// Takes bytes off the front of a payload whose total size was checked.
struct Reader<'a> {
    rest: &'a [u8],
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: u64) -> &'a [u8] {
        let n = usize::try_from(n).unwrap_or(usize::MAX).min(self.rest.len());
        let (head, tail) = self.rest.split_at(n);
        self.rest = tail;
        head
    }

    fn i64s(&mut self, n: u64) -> Vec<i64> {
        self.take(n.saturating_mul(8))
            .chunks_exact(8)
            .map(|c| i64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]))
            .collect()
    }

    fn u64s(&mut self, n: u64) -> Vec<u64> {
        self.take(n.saturating_mul(8))
            .chunks_exact(8)
            .map(|c| u64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]))
            .collect()
    }
}

// ------------------------------------------------------------------ the header

/// A parsed header, before the payload is read.
struct Header {
    first_seq: u64,
    last_seq: u64,
    profiles: u64,
    points_per_profile: u64,
    frame: Frame,
    pose_stream: Option<String>,
    t_host: u64,
    t_dev: Option<u64>,
    profile_id: u64,
    pose_seq: Option<u64>,
    mask: u64,
    x: (u64, EncodingDescriptor),
    z: (u64, EncodingDescriptor),
    intensity: Option<(u64, EncodingDescriptor)>,
}

const HEADER_FIELDS: [&str; 8] =
    ["arrays", "first_seq", "frame", "last_seq", "points_per_profile", "pose_stream", "profiles", "schema"];
const ARRAY_FIELDS: [&str; 8] = ["intensity", "mask", "pose_seq", "profile_id", "t_dev", "t_host", "x", "z"];

impl Header {
    fn parse(bytes: &[u8]) -> Result<Header, LineScanError> {
        let v = fid_canon::parse_with_budget(bytes, MAX_HEADER_BYTES, MAX_HEADER_DEPTH, MAX_HEADER_ELEMENTS)
            .map_err(|_| LineScanError::header("", "json"))?;
        let o = fields(&v, "", &HEADER_FIELDS)?;
        if req_str(o, "", "schema")? != HEADER_TAG {
            return Err(LineScanError::header("/schema", "schema_unknown"));
        }
        let first_seq = req_u64(o, "", "first_seq")?;
        let last_seq = req_u64(o, "", "last_seq")?;
        let profiles = req_u64(o, "", "profiles")?;
        let points_per_profile = req_u64(o, "", "points_per_profile")?;
        let frame_v = o.get("frame").ok_or_else(|| LineScanError::header("/frame", "field_missing"))?;
        let frame = Frame::from_value(frame_v.clone()).map_err(|_| LineScanError::header("/frame", "frame_invalid"))?;
        if frame.kind != FrameKind::Sensor {
            return Err(LineScanError::header("/frame/kind", "frame_not_sensor"));
        }
        let pose_stream = match o.get("pose_stream") {
            None => None,
            Some(CValue::Str(s)) if is_stream_id(s) => Some(s.clone()),
            Some(CValue::Str(_)) => return Err(LineScanError::header("/pose_stream", "stream_id_invalid")),
            Some(_) => return Err(LineScanError::header("/pose_stream", "wrong_type")),
        };
        let a = match o.get("arrays") {
            Some(a) => fields(a, "/arrays", &ARRAY_FIELDS)?,
            None => return Err(LineScanError::header("/arrays", "field_missing")),
        };
        let plain = |k: &str| -> Result<Option<u64>, LineScanError> {
            match a.get(k) {
                None => Ok(None),
                Some(v) => {
                    let ptr = format!("/arrays/{k}");
                    let f = fields(v, &ptr, &["len"])?;
                    req_u64(f, &ptr, "len").map(Some)
                }
            }
        };
        let encoded = |k: &'static str| -> Result<Option<(u64, EncodingDescriptor)>, LineScanError> {
            match a.get(k) {
                None => Ok(None),
                Some(v) => {
                    let ptr = format!("/arrays/{k}");
                    let f = fields(v, &ptr, &["encoding", "len"])?;
                    let len = req_u64(f, &ptr, "len")?;
                    let Some(e) = f.get("encoding") else {
                        return Err(LineScanError::Encoding { array: k, reason: "encoding missing".to_string() });
                    };
                    let d = EncodingDescriptor::from_value(e.clone())
                        .map_err(|err| LineScanError::Encoding { array: k, reason: err.to_string() })?;
                    Ok(Some((len, d)))
                }
            }
        };
        let required =
            |k: &str, v: Option<u64>| v.ok_or_else(|| LineScanError::header(format!("/arrays/{k}"), "field_missing"));
        let t_host = required("t_host", plain("t_host")?)?;
        let t_dev = plain("t_dev")?;
        let profile_id = required("profile_id", plain("profile_id")?)?;
        let pose_seq = plain("pose_seq")?;
        let mask = required("mask", plain("mask")?)?;
        let x = encoded("x")?.ok_or_else(|| LineScanError::header("/arrays/x", "field_missing"))?;
        let z = encoded("z")?.ok_or_else(|| LineScanError::header("/arrays/z", "field_missing"))?;
        let intensity = encoded("intensity")?;
        match (&pose_stream, pose_seq) {
            (Some(_), None) => return Err(LineScanError::header("/arrays/pose_seq", "field_missing")),
            (None, Some(_)) => return Err(LineScanError::header("/pose_stream", "field_missing")),
            _ => {}
        }
        Ok(Header {
            first_seq,
            last_seq,
            profiles,
            points_per_profile,
            frame,
            pose_stream,
            t_host,
            t_dev,
            profile_id,
            pose_seq,
            mask,
            x,
            z,
            intensity,
        })
    }
}

// ------------------------------------------------------------------ settings

/// Exposure time (s).
pub const SETTING_EXPOSURE: &str = "exposure";
/// Point spacing along the line (m).
pub const SETTING_POINT_SPACING: &str = "point_spacing";
/// Trigger mode: `time`, `encoder` or `external`.
pub const SETTING_TRIGGER_MODE: &str = "trigger_mode";
/// Trigger period in `time` mode (s).
pub const SETTING_TRIGGER_PERIOD: &str = "trigger_period";
/// Encoder divisor in `encoder` mode (counts per profile).
pub const SETTING_ENCODER_DIVISOR: &str = "encoder_divisor";
/// Region of interest, lateral minimum (m).
pub const SETTING_ROI_X_MIN: &str = "roi_x_min";
/// Region of interest, lateral maximum (m).
pub const SETTING_ROI_X_MAX: &str = "roi_x_max";
/// Region of interest, range minimum (m).
pub const SETTING_ROI_Z_MIN: &str = "roi_z_min";
/// Region of interest, range maximum (m).
pub const SETTING_ROI_Z_MAX: &str = "roi_z_max";

/// The package's core setting names, in name order. A device setting without a dot outside
/// this list is an undeclared extension (`HAL.IDL.SENS.STREAM`).
pub const CORE_SETTINGS: [&str; 9] = [
    SETTING_ENCODER_DIVISOR,
    SETTING_EXPOSURE,
    SETTING_POINT_SPACING,
    SETTING_ROI_X_MAX,
    SETTING_ROI_X_MIN,
    SETTING_ROI_Z_MAX,
    SETTING_ROI_Z_MIN,
    SETTING_TRIGGER_MODE,
    SETTING_TRIGGER_PERIOD,
];

/// How profiles are triggered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TriggerMode {
    /// At a fixed period of device time.
    Time,
    /// Every `encoder_divisor` encoder counts.
    Encoder,
    /// By an external trigger line.
    External,
}

impl TriggerMode {
    /// Every mode, in wire order.
    pub const ALL: [TriggerMode; 3] = [TriggerMode::Time, TriggerMode::Encoder, TriggerMode::External];

    /// The wire name.
    pub const fn as_str(self) -> &'static str {
        match self {
            TriggerMode::Time => "time",
            TriggerMode::Encoder => "encoder",
            TriggerMode::External => "external",
        }
    }

    /// Parses a wire name.
    pub fn parse(s: &str) -> Option<TriggerMode> {
        TriggerMode::ALL.into_iter().find(|m| m.as_str() == s)
    }
}

/// The trigger configuration: the mode and the one parameter it uses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Trigger {
    /// One profile every `period_s` seconds of device time.
    Time { period_s: f64 },
    /// One profile every `divisor` encoder counts.
    Encoder { divisor: u64 },
    /// One profile per external trigger.
    External,
}

impl Trigger {
    /// The mode.
    pub fn mode(&self) -> TriggerMode {
        match self {
            Trigger::Time { .. } => TriggerMode::Time,
            Trigger::Encoder { .. } => TriggerMode::Encoder,
            Trigger::External => TriggerMode::External,
        }
    }
}

/// The region of interest in the sensor frame, in m.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Roi {
    pub x_min: f64,
    pub x_max: f64,
    pub z_min: f64,
    pub z_max: f64,
}

/// The typed core settings of a `LineScan@1` device, read from a validated configuration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineScanSettings {
    pub exposure_s: f64,
    pub point_spacing_m: f64,
    pub trigger: Trigger,
    pub roi: Roi,
}

/// The reference declaration of the core settings: all acquisition-relevant, none live,
/// no ranges (a device's own `config-schema/0` adds its ranges).
pub fn core_setting_schemas() -> Vec<SettingSchema> {
    CORE_SETTINGS
        .iter()
        .filter_map(|name| {
            Some(SettingSchema {
                name: name.to_string(),
                ty: core_type(name)?,
                range: None,
                acquisition_relevant: true,
                live_changeable: false,
            })
        })
        .collect()
}

fn core_type(name: &str) -> Option<SettingType> {
    Some(match name {
        SETTING_EXPOSURE | SETTING_TRIGGER_PERIOD => SettingType::Quantity(SiUnit::Second),
        SETTING_POINT_SPACING | SETTING_ROI_X_MIN | SETTING_ROI_X_MAX | SETTING_ROI_Z_MIN | SETTING_ROI_Z_MAX => {
            SettingType::Quantity(SiUnit::Metre)
        }
        SETTING_ENCODER_DIVISOR => SettingType::Int,
        SETTING_TRIGGER_MODE => SettingType::Enum(TriggerMode::ALL.iter().map(|m| m.as_str().to_string()).collect()),
        _ => return None,
    })
}

/// Checks a device's `config-schema/0` against a `LineScan@1` package, failing closed: first
/// the vendor-extension rule (every setting a core setting or a declared extension,
/// `extension_undeclared`), then every core setting declared with its type and unit and
/// acquisition-relevant (`settings_invalid`). `trigger_mode` may list a non-empty subset of
/// the three modes. A description whose core settings are not [`CORE_SETTINGS`] is
/// `settings_invalid` (`not_linescan`).
pub fn check_device_schema(package: &StreamingDescription, schema: &ConfigSchema) -> Result<(), LineScanError> {
    let core: BTreeSet<String> = CORE_SETTINGS.iter().map(|s| s.to_string()).collect();
    if package.description().manifest.name != PACKAGE_NAME || *package.core_settings() != core {
        return Err(LineScanError::settings("", "not_linescan"));
    }
    package.check_config_schema(schema).map_err(LineScanError::Stream)?;
    for name in CORE_SETTINGS {
        let Some(s) = schema.get(name) else {
            return Err(LineScanError::settings(name, "missing"));
        };
        let type_ok = match (&s.ty, core_type(name)) {
            (SettingType::Enum(values), Some(SettingType::Enum(_))) => {
                !values.is_empty() && values.iter().all(|v| TriggerMode::parse(v).is_some())
            }
            (ty, Some(want)) => *ty == want,
            (_, None) => false,
        };
        if !type_ok {
            return Err(LineScanError::settings(name, "type_wrong"));
        }
        if !s.acquisition_relevant {
            return Err(LineScanError::settings(name, "not_digested"));
        }
    }
    Ok(())
}

impl LineScanSettings {
    /// Reads the core settings of a configuration, in order: [`check_device_schema`]; the
    /// configuration against the schema (`CONFIG_INVALID`, `OUT_OF_RANGE`); then the rules of
    /// the package (`settings_invalid`): exposure and point spacing positive, each ROI minimum
    /// below its maximum, a positive trigger period in `time` mode and an encoder divisor of at
    /// least 1 in `encoder` mode. The parameter of the other mode is not used.
    pub fn from_configuration(
        package: &StreamingDescription,
        schema: &ConfigSchema,
        cfg: &Configuration,
    ) -> Result<LineScanSettings, LineScanError> {
        check_device_schema(package, schema)?;
        validate_config(schema, cfg).map_err(LineScanError::Config)?;
        let q = |name: &str| match cfg.values.get(name) {
            Some(SettingValue::Quantity(v)) => Ok(v.value()),
            _ => Err(LineScanError::settings(name, "type_wrong")),
        };
        let positive = |name: &str| {
            let v = q(name)?;
            if v > 0.0 {
                Ok(v)
            } else {
                Err(LineScanError::settings(name, "not_positive"))
            }
        };
        let exposure_s = positive(SETTING_EXPOSURE)?;
        let point_spacing_m = positive(SETTING_POINT_SPACING)?;
        let roi = Roi {
            x_min: q(SETTING_ROI_X_MIN)?,
            x_max: q(SETTING_ROI_X_MAX)?,
            z_min: q(SETTING_ROI_Z_MIN)?,
            z_max: q(SETTING_ROI_Z_MAX)?,
        };
        if roi.x_min >= roi.x_max {
            return Err(LineScanError::settings(SETTING_ROI_X_MAX, "roi_empty"));
        }
        if roi.z_min >= roi.z_max {
            return Err(LineScanError::settings(SETTING_ROI_Z_MAX, "roi_empty"));
        }
        let mode = match cfg.values.get(SETTING_TRIGGER_MODE) {
            Some(SettingValue::Enum(s)) => TriggerMode::parse(s),
            _ => None,
        };
        let trigger = match mode {
            Some(TriggerMode::Time) => Trigger::Time { period_s: positive(SETTING_TRIGGER_PERIOD)? },
            Some(TriggerMode::Encoder) => match cfg.values.get(SETTING_ENCODER_DIVISOR) {
                Some(SettingValue::Int(d)) if *d >= 1 => Trigger::Encoder { divisor: d.unsigned_abs() },
                Some(SettingValue::Int(_)) => {
                    return Err(LineScanError::settings(SETTING_ENCODER_DIVISOR, "not_positive"))
                }
                _ => return Err(LineScanError::settings(SETTING_ENCODER_DIVISOR, "type_wrong")),
            },
            Some(TriggerMode::External) => Trigger::External,
            None => return Err(LineScanError::settings(SETTING_TRIGGER_MODE, "type_wrong")),
        };
        Ok(LineScanSettings { exposure_s, point_spacing_m, trigger, roi })
    }
}

/// Item `LS-05` over consecutive recorded batches of one stream.
///
/// The settings must come from [`LineScanSettings::from_configuration`], which already
/// enforces the parameter of each mode. In `time` mode every batch must carry device time
/// (`no_device_time`), the period must be at least 1 ns and at most about 292 years
/// (`period_invalid`), and every interval of device time between consecutive profiles, also
/// across two batches whose sequence ranges join, must be within
/// [`TRIGGER_TOLERANCE_PERCENT`] % of the period (`interval_off_period`, at the batch's index).
/// A `time` record with no such interval is `not_exercised`. `encoder` and `external` profiles
/// have no timing rule, so those modes pass on any non-empty record. An empty record is
/// `no_batches`.
pub fn check_trigger_mode(settings: &LineScanSettings, batches: &[ProfileBatch]) -> Result<(), ItemFailure> {
    let fail = |reason, index| Err(ItemFailure { item: ITEM_TRIGGER, reason, index });
    if batches.is_empty() {
        return fail("no_batches", None);
    }
    let Trigger::Time { period_s } = settings.trigger else {
        return Ok(());
    };
    let p = period_s * 1e9;
    if !(1.0..=9.0e18).contains(&p) {
        return fail("period_invalid", None);
    }
    // Exact for every period in range: the product is rounded once, then held as an integer.
    let period = i128::from(p.round() as i64);
    let ok = |dt: i128| (dt - period).abs() * 100 <= period * TRIGGER_TOLERANCE_PERCENT;
    let mut checked = 0usize;
    let mut prev: Option<(u64, i64)> = None; // last sequence number and device time
    for (k, b) in batches.iter().enumerate() {
        let Some(t) = &b.t_dev else {
            return fail("no_device_time", Some(k));
        };
        if let (Some((last_seq, last_t)), Some(first)) = (prev, t.first()) {
            if last_seq.checked_add(1) == Some(b.first_seq) {
                checked += 1;
                if !ok(i128::from(*first) - i128::from(last_t)) {
                    return fail("interval_off_period", Some(k));
                }
            }
        }
        for w in t.windows(2) {
            checked += 1;
            if !ok(i128::from(w[1]) - i128::from(w[0])) {
                return fail("interval_off_period", Some(k));
            }
        }
        prev = match (b.last_seq(), t.last()) {
            (Some(s), Some(l)) => Some((s, *l)),
            _ => None,
        };
    }
    if checked == 0 {
        return fail("not_exercised", None);
    }
    Ok(())
}

// ------------------------------------------------------------------ conformance profile

/// The `LineScan-core@1` items (REQ-004). Item IDs are stable: an item is never renumbered,
/// and a withdrawn ID is not reused. The built package appends the `Stream-common@1` items.
/// Editing the item text changes the package digest and needs a package version bump.
pub const LINESCAN_CORE: ConformanceProfile = ConformanceProfile {
    id: PROFILE_ID,
    items: &[
        ProfileItem {
            id: ITEM_ENCODING,
            title: "Encoding declaration",
            checks: &[
                "x and z each declare one exact encoding with target unit m",
                "an intensity array declares an exact encoding with target unit 1",
                "the batch names the sensor frame its x and z values are expressed in",
            ],
        },
        ProfileItem {
            id: ITEM_LENGTHS,
            title: "Array lengths",
            checks: &[
                "every per-point array holds profiles x points-per-profile values",
                "every per-profile array holds one value per profile and the sequence range one number per profile",
                "the payload holds exactly the declared arrays in layout order",
            ],
        },
        ProfileItem {
            id: ITEM_TIMESTAMPS,
            title: "Timestamp monotonicity",
            checks: &[
                "t_host never decreases from one profile to the next",
                "t_dev, when present, never decreases from one profile to the next",
            ],
        },
        ProfileItem {
            id: ITEM_MASK,
            title: "Masked-sample handling",
            checks: &[
                "every sample whose mask bit is set decodes to a finite value",
                "a sample whose mask bit is clear is not decoded, whatever its raw value",
                "mask padding bits are zero",
            ],
        },
        ProfileItem {
            id: ITEM_TRIGGER,
            title: "Trigger-mode behaviour",
            checks: &[
                "time mode has a positive trigger period and encoder mode a divisor of at least 1",
                "in time mode every device-time interval between consecutive profiles is within 5 % of the trigger period",
            ],
        },
    ],
};

// ------------------------------------------------------------------ the package

/// The package's semantics note, in the project's own words.
pub const SEMANTICS: &str = "# LineScan@1 semantics\n\
\n\
The capability package for laser line and profile sensors, stationary or carried by an arm \
or tracker.\n\
\n\
- Describe and configuration come from Device@1. The core settings are exposure (s), \
point_spacing (m), trigger_mode (time, encoder or external), trigger_period (s, used in time \
mode), encoder_divisor (used in encoder mode) and the region of interest roi_x_min, \
roi_x_max, roi_z_min and roi_z_max (m). Any other setting is a vendor extension under a \
namespace listed in describe.\n\
- start_stream, stop_stream and stream_status come from the shared streaming template.\n\
- Data arrives as profile batches: a linescan-batch/1 header and a packed little-endian \
payload of columns, never one object per point. One sequence number per profile.\n\
- Each batch declares the exact encoding of its x, z and optional intensity arrays; the \
column encodings in this description are the reference encodings. x and z are in m in the \
sensor frame the batch names; intensity is dimensionless.\n\
- Per profile the batch carries t_host, optional t_dev, the device's profile ID and an \
optional pose reference (pose stream and sequence number). Profile IDs and pose sequence \
numbers are u64 values, not quantities.\n\
- A sample whose mask bit is clear carries no measurement and is never decoded.\n\
- Device-supplied text is data and is never interpreted as instructions.\n";

/// The `LineScan@1` package.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LineScanPackage;

impl LineScanPackage {
    /// The package's own parts, before the streaming template: no operations of its own (they
    /// come from `Device@1` and the template), the profile batch schema, the semantics note
    /// and the `LineScan-core@1` items.
    pub fn base() -> PackageDescription {
        PackageDescription {
            manifest: PackageManifest {
                name: PACKAGE_NAME.to_string(),
                version: PACKAGE_VERSION.to_string(),
                deprecated: false,
                tier3_usable: true,
            },
            defs: BTreeMap::new(),
            operations: Vec::new(),
            batch_schemas: batch_schemas(),
            semantics: SemanticsNote::new(SEMANTICS),
            profile: LINESCAN_CORE.description(),
        }
    }

    /// The package built with the streaming template and its core settings, without vendor
    /// extensions.
    pub fn streaming() -> Result<StreamingDescription, StreamError> {
        LineScanPackage::streaming_with(&[], Vec::new())
    }

    /// The package as a driver describes it: the vendor namespaces it lists and the extension
    /// settings it declares, checked by the vendor-extension rule.
    pub fn streaming_with(
        namespaces: &[&str],
        extensions: Vec<SettingSchema>,
    ) -> Result<StreamingDescription, StreamError> {
        let mut b = StreamingPackage::new(LineScanPackage::base());
        for name in CORE_SETTINGS {
            b = b.core_setting(name);
        }
        for ns in namespaces {
            b = b.vendor_namespace(*ns);
        }
        for e in extensions {
            b = b.extension(e);
        }
        b.build()
    }
}

impl CapabilityPackage for LineScanPackage {
    /// The built package. Building cannot fail for these fixed parts (the tests check it); if
    /// it ever did, the description returned has an empty name, so it fails validation
    /// instead of passing without the template.
    fn description() -> PackageDescription {
        match LineScanPackage::streaming() {
            Ok(s) => s.description().clone(),
            Err(_) => {
                let mut d = LineScanPackage::base();
                d.manifest.name.clear();
                d
            }
        }
    }
}

/// The reference encodings: x and z i32 at 0.1 um, intensity u16 counts, times i64 ns.
fn batch_schemas() -> Vec<BatchSchemaDesc> {
    let col = |name: &str, numeric, scale, unit| {
        EncodingDescriptor::new(numeric, scale, 0.0, unit).ok().map(|encoding| BatchColumnDesc {
            name: name.to_string(),
            unit,
            encoding,
        })
    };
    let columns = [
        col("x", NumericType::I32, 1e-7, SiUnit::Metre),
        col("z", NumericType::I32, 1e-7, SiUnit::Metre),
        col("intensity", NumericType::U16, 1.0, SiUnit::One),
        col("t_host", NumericType::I64, 1e-9, SiUnit::Second),
        col("t_dev", NumericType::I64, 1e-9, SiUnit::Second),
    ];
    vec![BatchSchemaDesc {
        id: BATCH_SCHEMA_ID.to_string(),
        columns: columns.into_iter().flatten().collect(),
        validity_mask: true,
        seq_range: true,
    }]
}

// ------------------------------------------------------------------ wire helpers

fn dec(n: u64) -> CValue {
    CValue::Str(n.to_string())
}

fn obj(pairs: Vec<(&str, CValue)>) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn escape(seg: &str) -> String {
    kept(seg).replace('~', "~0").replace('/', "~1")
}

fn fields<'a>(v: &'a CValue, ptr: &str, allowed: &[&str]) -> Result<&'a BTreeMap<String, CValue>, LineScanError> {
    let CValue::Obj(o) = v else {
        return Err(LineScanError::header(ptr, "not_object"));
    };
    if let Some(k) = o.keys().find(|k| !allowed.contains(&k.as_str())) {
        return Err(LineScanError::header(format!("{ptr}/{}", escape(k)), "field_unknown"));
    }
    Ok(o)
}

fn req_str<'a>(o: &'a BTreeMap<String, CValue>, ptr: &str, k: &str) -> Result<&'a str, LineScanError> {
    match o.get(k) {
        Some(CValue::Str(s)) => Ok(s),
        Some(_) => Err(LineScanError::header(format!("{ptr}/{k}"), "wrong_type")),
        None => Err(LineScanError::header(format!("{ptr}/{k}"), "field_missing")),
    }
}

/// A `u64` as a canonical decimal string: digits only, no leading zero, at most 20 digits.
fn req_u64(o: &BTreeMap<String, CValue>, ptr: &str, k: &str) -> Result<u64, LineScanError> {
    let s = req_str(o, ptr, k)?;
    let canonical =
        !s.is_empty() && s.len() <= 20 && s.bytes().all(|c| c.is_ascii_digit()) && (s == "0" || !s.starts_with('0'));
    if !canonical {
        return Err(LineScanError::header(format!("{ptr}/{k}"), "not_u64"));
    }
    s.parse::<u64>().map_err(|_| LineScanError::header(format!("{ptr}/{k}"), "not_u64"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths_match_the_types() {
        let want = [2, 4, 8, 2, 4, 4, 8];
        for (t, w) in NumericType::ALL.into_iter().zip(want) {
            assert_eq!(element_bytes(t), w, "{t}");
        }
    }

    #[test]
    fn masks_keep_padding_clear() {
        for bits in [1u64, 7, 8, 9, 16, 17] {
            let m = ValidityMask::all_valid(bits);
            assert_eq!(m.count_valid(), bits);
            assert!(ValidityMask::from_bytes(bits, m.as_bytes().to_vec()).is_ok());
            assert!(!m.is_valid(bits));
        }
        assert_eq!(ValidityMask::from_bytes(3, vec![0b1000]), Err(LineScanError::MaskInvalid));
        assert_eq!(ValidityMask::from_bytes(8, vec![0, 0]), Err(LineScanError::MaskInvalid));
        let mut m = ValidityMask::all_valid(10);
        m.set(9, false);
        m.set(10, true);
        assert_eq!(m.count_valid(), 9);
        assert_eq!(m.as_bytes(), &[0xff, 0x01]);
    }

    #[test]
    fn stream_ids_are_checked() {
        for ok in ["pose", "0", "tracker-1.pose_a"] {
            assert!(is_stream_id(ok), "{ok}");
        }
        let long = "a".repeat(MAX_STREAM_ID_BYTES + 1);
        for bad in ["", "Pose", "-a", "a b", "a/b", long.as_str()] {
            assert!(!is_stream_id(bad), "{bad}");
        }
    }

    #[test]
    fn trigger_modes_round_trip() {
        for m in TriggerMode::ALL {
            assert_eq!(TriggerMode::parse(m.as_str()), Some(m));
        }
        assert_eq!(TriggerMode::parse("Time"), None);
    }
}
