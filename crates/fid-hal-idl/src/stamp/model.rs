//! Typed v0 records and their closed enums.

use super::desc::{self, f64_hex, i64_dec, u64_dec, Field, Ty};
use super::text::UntrustedText;
use super::{child, StampError, StampErrorKind as K};
use crate::units::{Quantity, SiUnit, TimeSource};
use fid_canon::CValue;
use fid_types::Digest;
use fid_uuid::Uuid;
use std::collections::BTreeMap;
use std::fmt;

macro_rules! closed_enum {
    ($(#[$m:meta])* $name:ident { $($(#[$vm:meta])* $var:ident = $wire:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $name {
            $($(#[$vm])* $var),+
        }

        impl $name {
            /// Every value, in declaration order.
            pub const ALL: &'static [$name] = &[$($name::$var),+];
            /// The wire strings, in declaration order.
            pub const WIRE: &'static [&'static str] = &[$($wire),+];

            /// The wire string.
            pub fn as_str(self) -> &'static str {
                match self {
                    $($name::$var => $wire),+
                }
            }

            /// Parses a wire string, case-sensitively.
            pub fn parse(s: &str) -> Option<$name> {
                match s {
                    $($wire => Some($name::$var),)+
                    _ => None,
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

closed_enum! {
    /// How far the identity of the serving source is established (grading is `HAL.PAIR.GRADE`).
    Trust { Attested = "attested", Declared = "declared", Unknown = "unknown" }
}

closed_enum! {
    /// Where an observation came from: a live device, a file, or a manual entry.
    SourceKind { Device = "device", File = "file", Entered = "entered" }
}

closed_enum! {
    /// Why sequence numbers are missing.
    GapReason {
        CreditExhausted = "CREDIT_EXHAUSTED",
        DeviceDrop = "DEVICE_DROP",
        CommLost = "COMM_LOST",
        DriverRestart = "DRIVER_RESTART",
        Rejected = "REJECTED",
    }
}

closed_enum! {
    /// Status of the calibration references.
    CalibrationStatus { Valid = "VALID", Expired = "EXPIRED", Unknown = "UNKNOWN" }
}

closed_enum! {
    /// Warm-up state, re-using the health states of the `Device` base.
    WarmupState { WarmingUp = "WARMING_UP", Ready = "READY" }
}

closed_enum! {
    /// Driver tier (`HAL.HOST`): in-process Rust, out-of-process host, sandboxed WASM.
    DriverTier { Tier1 = "TIER1", Tier2 = "TIER2", Tier3 = "TIER3" }
}

closed_enum! {
    /// Outcome of the driver's compatibility report; `NOT_RUN` exactly when there is no report.
    CompatStatus { Pass = "PASS", Partial = "PARTIAL", Fail = "FAIL", NotRun = "NOT_RUN" }
}

closed_enum! {
    /// Who computed an external claim. Always the device: a claim is never a platform result.
    AlgorithmSource { Device = "device" }
}

// ------------------------------------------------------------------ records

/// An inclusive sequence range; wire form `["first", "last"]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SeqRange {
    pub first: u64,
    pub last: u64,
}

/// Missing sequence numbers `from..=to`, inside the batch's range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Gap {
    pub from: u64,
    pub to: u64,
    pub reason: GapReason,
}

/// The capability that produced the payload: `name@version` and its negotiated features.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Capability {
    /// Package name, e.g. `LineScan`.
    pub name: String,
    /// `major.minor`.
    pub version: String,
    /// Feature flags, strictly ascending.
    pub features: Vec<String>,
}

/// The driver that served the batch.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DriverIdentity {
    pub id: String,
    pub version: String,
    pub tier: DriverTier,
    /// Digest of the driver binary or package.
    pub digest: Digest,
    /// Signing key reference; `None` for an unsigned driver.
    pub signer: Option<String>,
    /// Digest of the driver host binary; `None` for Tier 1.
    pub host_digest: Option<Digest>,
    pub compat_report_digest: Option<Digest>,
    pub compat_status: CompatStatus,
}

/// The device as it declared itself. All strings are device-supplied and untrusted.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceIdentity {
    pub make: UntrustedText,
    pub model: UntrustedText,
    pub serial: Option<UntrustedText>,
    pub firmware: Option<UntrustedText>,
    /// Whether the firmware is inside the driver's tested range; `None` when not known.
    pub firmware_in_tested_range: Option<bool>,
}

/// Calibration references (strictly ascending) and their status.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Calibration {
    pub status: CalibrationStatus,
    pub refs: Vec<String>,
}

/// Ties the host clock to UTC: host time `t_host` (ns) was UTC `utc_ns` (ns since the Unix
/// epoch).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UtcAnchor {
    pub t_host: i64,
    pub utc_ns: i64,
}

/// Time source of the batch's timestamps, with its sync uncertainty (one sigma, ns).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TimeInfo {
    pub source: TimeSource,
    pub sync_sigma_ns: u64,
    pub utc_anchor: Option<UtcAnchor>,
}

/// The provenance stamp, `stamp/0` (REQ-HAL.IDL.STAMP-001, -002).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProvenanceStamp {
    pub batch_id: Uuid,
    pub seq: SeqRange,
    pub gaps: Vec<Gap>,
    pub payload_digest: Digest,
    pub payload_schema: String,
    pub encoding_ref: String,
    pub capability: Capability,
    pub driver: DriverIdentity,
    pub device: DeviceIdentity,
    pub config_digest: Digest,
    pub calibration: Calibration,
    pub sensor_model: Option<String>,
    pub time: TimeInfo,
    pub environment: Vec<String>,
    pub warmup: WarmupState,
    pub transcript_digest: Option<Digest>,
    pub trust: Trust,
    pub source_kind: SourceKind,
    pub simulated: bool,
}

/// The acquisition-batch envelope, `acquisition-batch/0` (REQ-HAL.IDL.STAMP-005). Its
/// batch ID, payload schema and digest, sequence range and gaps must equal its stamp's.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AcquisitionBatchEnvelope {
    pub batch_id: Uuid,
    pub payload_schema: String,
    pub payload_digest: Digest,
    pub seq: SeqRange,
    pub gaps: Vec<Gap>,
    pub stamp: ProvenanceStamp,
}

/// A batch an external claim refers to: its ID and the digest of its envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BatchRef {
    pub batch_id: Uuid,
    pub batch_digest: Digest,
}

/// A quantity a device computed itself, `external-claim/0` (REQ-HAL.IDL.STAMP-006). It has
/// no value-of-record flag and none can be added: unknown fields are refused.
#[derive(Debug, Clone, PartialEq)]
pub struct ExternalClaim {
    /// Quantity name, e.g. `diameter`.
    pub quantity: String,
    pub value: Quantity,
    pub algorithm_source: AlgorithmSource,
    pub method: UntrustedText,
    pub batch: BatchRef,
}

/// Any v0 record.
#[derive(Debug, Clone, PartialEq)]
pub enum Record {
    Stamp(ProvenanceStamp),
    Batch(AcquisitionBatchEnvelope),
    Claim(ExternalClaim),
}

/// The three record kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RecordKind {
    Stamp,
    Batch,
    Claim,
}

impl RecordKind {
    pub const ALL: [RecordKind; 3] = [RecordKind::Stamp, RecordKind::Batch, RecordKind::Claim];

    /// The kind part of the `schema` tag.
    pub fn as_str(self) -> &'static str {
        match self {
            RecordKind::Stamp => "stamp",
            RecordKind::Batch => "acquisition-batch",
            RecordKind::Claim => "external-claim",
        }
    }

    /// File name of the committed JSON Schema under `schemas/hal/`.
    pub fn schema_file(self) -> &'static str {
        match self {
            RecordKind::Stamp => "stamp-0.schema.json",
            RecordKind::Batch => "acquisition-batch-0.schema.json",
            RecordKind::Claim => "external-claim-0.schema.json",
        }
    }

    pub(crate) fn description(self) -> &'static str {
        match self {
            RecordKind::Stamp => "Provenance stamp v0 (spec node HAL.IDL.STAMP).",
            RecordKind::Batch => "Acquisition-batch envelope v0 with exactly one stamp (spec node HAL.IDL.STAMP).",
            RecordKind::Claim => {
                "External claim v0: a quantity a device computed itself, never a value of record (spec node HAL.IDL.STAMP)."
            }
        }
    }

    pub(crate) fn descriptor_fields(self) -> &'static [Field] {
        match self {
            RecordKind::Stamp => desc::STAMP,
            RecordKind::Batch => desc::BATCH,
            RecordKind::Claim => desc::CLAIM,
        }
    }
}

// ------------------------------------------------------------------ reading

fn invalid(ptr: &str) -> StampError {
    StampError::new(K::ValueInvalid, ptr)
}

/// Reads fields out of an object that already passed the structural check. Its errors
/// cannot occur after that check; they keep the conversion total without panicking.
struct Rd {
    o: BTreeMap<String, CValue>,
    ptr: String,
}

impl Rd {
    fn new(v: CValue, ptr: &str) -> Result<Rd, StampError> {
        match v {
            CValue::Obj(o) => Ok(Rd { o, ptr: ptr.to_string() }),
            _ => Err(invalid(ptr)),
        }
    }

    fn p(&self, k: &str) -> String {
        child(&self.ptr, k)
    }

    fn take(&mut self, k: &str) -> Result<CValue, StampError> {
        self.o.remove(k).ok_or_else(|| StampError::new(K::FieldMissing, &self.p(k)))
    }

    fn opt(&mut self, k: &str) -> Result<Option<CValue>, StampError> {
        Ok(match self.take(k)? {
            CValue::Null => None,
            v => Some(v),
        })
    }

    fn obj(&mut self, k: &str) -> Result<Rd, StampError> {
        let v = self.take(k)?;
        Rd::new(v, &self.p(k))
    }

    fn arr(&mut self, k: &str) -> Result<Vec<CValue>, StampError> {
        match self.take(k)? {
            CValue::Arr(a) => Ok(a),
            _ => Err(invalid(&self.p(k))),
        }
    }

    fn string(&mut self, k: &str) -> Result<String, StampError> {
        let v = self.take(k)?;
        as_string(v, &self.p(k))
    }

    fn opt_string(&mut self, k: &str) -> Result<Option<String>, StampError> {
        let p = self.p(k);
        self.opt(k)?.map(|v| as_string(v, &p)).transpose()
    }

    fn strings(&mut self, k: &str) -> Result<Vec<String>, StampError> {
        let p = self.p(k);
        self.arr(k)?.into_iter().map(|v| as_string(v, &p)).collect()
    }

    fn text(&mut self, k: &str) -> Result<UntrustedText, StampError> {
        Ok(UntrustedText::new(&self.string(k)?))
    }

    fn opt_text(&mut self, k: &str) -> Result<Option<UntrustedText>, StampError> {
        Ok(self.opt_string(k)?.map(|s| UntrustedText::new(&s)))
    }

    fn boolean(&mut self, k: &str) -> Result<bool, StampError> {
        match self.take(k)? {
            CValue::Bool(b) => Ok(b),
            _ => Err(invalid(&self.p(k))),
        }
    }

    fn opt_boolean(&mut self, k: &str) -> Result<Option<bool>, StampError> {
        match self.opt(k)? {
            None => Ok(None),
            Some(CValue::Bool(b)) => Ok(Some(b)),
            Some(_) => Err(invalid(&self.p(k))),
        }
    }

    fn u64(&mut self, k: &str) -> Result<u64, StampError> {
        let s = self.string(k)?;
        u64_dec(&s).ok_or_else(|| invalid(&self.p(k)))
    }

    fn i64(&mut self, k: &str) -> Result<i64, StampError> {
        let s = self.string(k)?;
        i64_dec(&s).ok_or_else(|| invalid(&self.p(k)))
    }

    fn digest(&mut self, k: &str) -> Result<Digest, StampError> {
        let s = self.string(k)?;
        Digest::parse(&s).map_err(|_| invalid(&self.p(k)))
    }

    fn opt_digest(&mut self, k: &str) -> Result<Option<Digest>, StampError> {
        let p = self.p(k);
        self.opt_string(k)?.map(|s| Digest::parse(&s).map_err(|_| invalid(&p))).transpose()
    }

    fn uuid(&mut self, k: &str) -> Result<Uuid, StampError> {
        let s = self.string(k)?;
        Uuid::parse(&s).map_err(|_| invalid(&self.p(k)))
    }

    fn wire<T>(&mut self, k: &str, parse: fn(&str) -> Option<T>) -> Result<T, StampError> {
        let s = self.string(k)?;
        parse(&s).ok_or_else(|| StampError::new(K::EnumInvalid, &self.p(k)))
    }

    fn seq(&mut self, k: &str) -> Result<SeqRange, StampError> {
        let p = self.p(k);
        let a = self.arr(k)?;
        let mut n = a.into_iter().map(|v| as_string(v, &p).and_then(|s| u64_dec(&s).ok_or_else(|| invalid(&p))));
        match (n.next(), n.next(), n.next()) {
            (Some(first), Some(last), None) => Ok(SeqRange { first: first?, last: last? }),
            _ => Err(invalid(&p)),
        }
    }

    fn gaps(&mut self, k: &str) -> Result<Vec<Gap>, StampError> {
        let p = self.p(k);
        let a = self.arr(k)?;
        a.into_iter()
            .enumerate()
            .map(|(i, v)| -> Result<Gap, StampError> {
                let mut g = Rd::new(v, &child(&p, &i.to_string()))?;
                Ok(Gap { from: g.u64("from")?, to: g.u64("to")?, reason: g.wire("reason", GapReason::parse)? })
            })
            .collect()
    }
}

fn as_string(v: CValue, ptr: &str) -> Result<String, StampError> {
    match v {
        CValue::Str(s) => Ok(s),
        _ => Err(invalid(ptr)),
    }
}

// ------------------------------------------------------------------ writing

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn st(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

fn dec<T: fmt::Display>(x: T) -> CValue {
    CValue::Str(x.to_string())
}

fn opt<T>(x: &Option<T>, f: impl Fn(&T) -> CValue) -> CValue {
    x.as_ref().map_or(CValue::Null, f)
}

fn strs(v: &[String]) -> CValue {
    CValue::Arr(v.iter().map(|s| st(s)).collect())
}

fn seq_value(s: SeqRange) -> CValue {
    CValue::Arr(vec![dec(s.first), dec(s.last)])
}

fn gaps_value(gaps: &[Gap]) -> CValue {
    CValue::Arr(
        gaps.iter()
            .map(|g| obj([("from", dec(g.from)), ("reason", st(g.reason.as_str())), ("to", dec(g.to))]))
            .collect(),
    )
}

// ------------------------------------------------------------------ cross-field rules

/// Every gap lies inside the range, has `from <= to`, and starts after the previous one ends.
fn check_gaps(seq: SeqRange, gaps: &[Gap], ptr: &str) -> Result<(), StampError> {
    let mut prev_to: Option<u64> = None;
    for (i, g) in gaps.iter().enumerate() {
        let ok = g.from <= g.to
            && g.from >= seq.first
            && g.to <= seq.last
            && prev_to.map_or(true, |t| g.from > t);
        if !ok {
            return Err(StampError::new(K::SeqInvalid, &child(&child(ptr, "gaps"), &i.to_string())));
        }
        prev_to = Some(g.to);
    }
    Ok(())
}

fn canonical(fields: &'static [Field], value: &CValue) -> Result<Vec<u8>, StampError> {
    desc::check(&Ty::Obj(fields), value, "")?;
    fid_canon::to_canonical_bytes(value).map_err(|_| StampError::new(K::JsonInvalid, ""))
}

fn digest_of(bytes: &[u8]) -> Digest {
    Digest::from_bytes(fid_canon::digest_bytes(bytes).0)
}

impl ProvenanceStamp {
    /// Validates a value as `stamp/0`.
    pub fn from_value(value: CValue) -> Result<ProvenanceStamp, StampError> {
        match super::validate_value(value)? {
            Record::Stamp(s) => Ok(s),
            _ => Err(StampError::new(K::EnumInvalid, "/schema")),
        }
    }

    pub(crate) fn read(value: CValue, ptr: &str) -> Result<ProvenanceStamp, StampError> {
        let mut r = Rd::new(value, ptr)?;
        let mut cap = r.obj("capability")?;
        let mut drv = r.obj("driver")?;
        let mut dev = r.obj("device")?;
        let mut cal = r.obj("calibration")?;
        let mut time = r.obj("time")?;
        let utc_anchor = match time.opt("utc_anchor")? {
            None => None,
            Some(v) => {
                let mut a = Rd::new(v, &time.p("utc_anchor"))?;
                Some(UtcAnchor { t_host: a.i64("t_host")?, utc_ns: a.i64("utc_ns")? })
            }
        };
        Ok(ProvenanceStamp {
            batch_id: r.uuid("batch_id")?,
            seq: r.seq("seq")?,
            gaps: r.gaps("gaps")?,
            payload_digest: r.digest("payload_digest")?,
            payload_schema: r.string("payload_schema")?,
            encoding_ref: r.string("encoding_ref")?,
            capability: Capability {
                name: cap.string("name")?,
                version: cap.string("version")?,
                features: cap.strings("features")?,
            },
            driver: DriverIdentity {
                id: drv.string("id")?,
                version: drv.string("version")?,
                tier: drv.wire("tier", DriverTier::parse)?,
                digest: drv.digest("digest")?,
                signer: drv.opt_string("signer")?,
                host_digest: drv.opt_digest("host_digest")?,
                compat_report_digest: drv.opt_digest("compat_report_digest")?,
                compat_status: drv.wire("compat_status", CompatStatus::parse)?,
            },
            device: DeviceIdentity {
                make: dev.text("make")?,
                model: dev.text("model")?,
                serial: dev.opt_text("serial")?,
                firmware: dev.opt_text("firmware")?,
                firmware_in_tested_range: dev.opt_boolean("firmware_in_tested_range")?,
            },
            config_digest: r.digest("config_digest")?,
            calibration: Calibration {
                status: cal.wire("status", CalibrationStatus::parse)?,
                refs: cal.strings("refs")?,
            },
            sensor_model: r.opt_string("sensor_model")?,
            time: TimeInfo {
                source: time.wire("source", |s| TimeSource::parse(s).ok())?,
                sync_sigma_ns: time.u64("sync_sigma_ns")?,
                utc_anchor,
            },
            environment: r.strings("environment")?,
            warmup: r.wire("warmup", WarmupState::parse)?,
            transcript_digest: r.opt_digest("transcript_digest")?,
            trust: r.wire("trust", Trust::parse)?,
            source_kind: r.wire("source_kind", SourceKind::parse)?,
            simulated: r.boolean("simulated")?,
        })
    }

    /// The wire value (not validated; see [`ProvenanceStamp::canonical_bytes`]).
    pub fn to_value(&self) -> CValue {
        let d = &self.driver;
        let v = &self.device;
        let text = |t: &UntrustedText| st(t.raw());
        let dg = |x: &Digest| st(x.as_str());
        obj([
            ("batch_id", dec(self.batch_id)),
            ("calibration", obj([("refs", strs(&self.calibration.refs)), ("status", st(self.calibration.status.as_str()))])),
            (
                "capability",
                obj([
                    ("features", strs(&self.capability.features)),
                    ("name", st(&self.capability.name)),
                    ("version", st(&self.capability.version)),
                ]),
            ),
            ("config_digest", dg(&self.config_digest)),
            (
                "device",
                obj([
                    ("firmware", opt(&v.firmware, text)),
                    ("firmware_in_tested_range", opt(&v.firmware_in_tested_range, |b| CValue::Bool(*b))),
                    ("make", text(&v.make)),
                    ("model", text(&v.model)),
                    ("serial", opt(&v.serial, text)),
                ]),
            ),
            (
                "driver",
                obj([
                    ("compat_report_digest", opt(&d.compat_report_digest, dg)),
                    ("compat_status", st(d.compat_status.as_str())),
                    ("digest", dg(&d.digest)),
                    ("host_digest", opt(&d.host_digest, dg)),
                    ("id", st(&d.id)),
                    ("signer", opt(&d.signer, |s| st(s))),
                    ("tier", st(d.tier.as_str())),
                    ("version", st(&d.version)),
                ]),
            ),
            ("encoding_ref", st(&self.encoding_ref)),
            ("environment", strs(&self.environment)),
            ("gaps", gaps_value(&self.gaps)),
            ("payload_digest", dg(&self.payload_digest)),
            ("payload_schema", st(&self.payload_schema)),
            ("schema", st("stamp/0")),
            ("sensor_model", opt(&self.sensor_model, |s| st(s))),
            ("seq", seq_value(self.seq)),
            ("simulated", CValue::Bool(self.simulated)),
            ("source_kind", st(self.source_kind.as_str())),
            (
                "time",
                obj([
                    ("source", st(self.time.source.as_str())),
                    ("sync_sigma_ns", dec(self.time.sync_sigma_ns)),
                    (
                        "utc_anchor",
                        opt(&self.time.utc_anchor, |a| obj([("t_host", dec(a.t_host)), ("utc_ns", dec(a.utc_ns))])),
                    ),
                ]),
            ),
            ("transcript_digest", opt(&self.transcript_digest, dg)),
            ("trust", st(self.trust.as_str())),
            ("warmup", st(self.warmup.as_str())),
        ])
    }

    pub(crate) fn semantic(&self, ptr: &str) -> Result<(), StampError> {
        check_gaps(self.seq, &self.gaps, ptr)?;
        let d = &self.driver;
        if (d.compat_status == CompatStatus::NotRun) != d.compat_report_digest.is_none() {
            return Err(invalid(&child(&child(ptr, "driver"), "compat_report_digest")));
        }
        Ok(())
    }

    /// RFC 8785 canonical bytes, after validating the record (fails closed).
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, StampError> {
        let v = self.to_value();
        let bytes = canonical(desc::STAMP, &v)?;
        self.semantic("")?;
        Ok(bytes)
    }

    /// `sha256:` digest of the canonical bytes.
    pub fn digest(&self) -> Result<Digest, StampError> {
        self.canonical_bytes().map(|b| digest_of(&b))
    }
}

impl AcquisitionBatchEnvelope {
    /// Validates a value as `acquisition-batch/0`.
    pub fn from_value(value: CValue) -> Result<AcquisitionBatchEnvelope, StampError> {
        match super::validate_value(value)? {
            Record::Batch(b) => Ok(b),
            _ => Err(StampError::new(K::EnumInvalid, "/schema")),
        }
    }

    pub(crate) fn read(value: CValue, ptr: &str) -> Result<AcquisitionBatchEnvelope, StampError> {
        let mut r = Rd::new(value, ptr)?;
        let stamp_ptr = r.p("stamp");
        Ok(AcquisitionBatchEnvelope {
            batch_id: r.uuid("batch_id")?,
            payload_schema: r.string("payload_schema")?,
            payload_digest: r.digest("payload_digest")?,
            seq: r.seq("seq")?,
            gaps: r.gaps("gaps")?,
            stamp: ProvenanceStamp::read(r.take("stamp")?, &stamp_ptr)?,
        })
    }

    /// The wire value (not validated; see [`AcquisitionBatchEnvelope::canonical_bytes`]).
    pub fn to_value(&self) -> CValue {
        obj([
            ("batch_id", dec(self.batch_id)),
            ("gaps", gaps_value(&self.gaps)),
            ("payload_digest", st(self.payload_digest.as_str())),
            ("payload_schema", st(&self.payload_schema)),
            ("schema", st("acquisition-batch/0")),
            ("seq", seq_value(self.seq)),
            ("stamp", self.stamp.to_value()),
        ])
    }

    pub(crate) fn semantic(&self) -> Result<(), StampError> {
        self.stamp.semantic("/stamp")?;
        let s = &self.stamp;
        let mismatch = [
            ("batch_id", self.batch_id != s.batch_id),
            ("gaps", self.gaps != s.gaps),
            ("payload_digest", self.payload_digest != s.payload_digest),
            ("payload_schema", self.payload_schema != s.payload_schema),
            ("seq", self.seq != s.seq),
        ];
        match mismatch.into_iter().find(|(_, differs)| *differs) {
            Some((field, _)) => Err(StampError::new(K::BatchMismatch, &child("/stamp", field))),
            None => Ok(()),
        }
    }

    /// RFC 8785 canonical bytes, after validating the record (fails closed).
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, StampError> {
        let v = self.to_value();
        let bytes = canonical(desc::BATCH, &v)?;
        self.semantic()?;
        Ok(bytes)
    }

    /// `sha256:` digest of the canonical bytes.
    pub fn digest(&self) -> Result<Digest, StampError> {
        self.canonical_bytes().map(|b| digest_of(&b))
    }
}

impl ExternalClaim {
    /// Validates a value as `external-claim/0`.
    pub fn from_value(value: CValue) -> Result<ExternalClaim, StampError> {
        match super::validate_value(value)? {
            Record::Claim(c) => Ok(c),
            _ => Err(StampError::new(K::EnumInvalid, "/schema")),
        }
    }

    pub(crate) fn read(value: CValue, ptr: &str) -> Result<ExternalClaim, StampError> {
        let mut r = Rd::new(value, ptr)?;
        let mut q = r.obj("value")?;
        let unit = q.wire("unit", |s| SiUnit::parse(s).ok())?;
        let bits = q.string("value")?;
        let value = f64_hex(&bits)
            .and_then(|x| Quantity::new(x, unit).ok())
            .ok_or_else(|| invalid(&q.p("value")))?;
        let mut b = r.obj("batch")?;
        Ok(ExternalClaim {
            quantity: r.string("quantity")?,
            value,
            algorithm_source: r.wire("algorithm_source", AlgorithmSource::parse)?,
            method: r.text("method")?,
            batch: BatchRef { batch_id: b.uuid("batch_id")?, batch_digest: b.digest("batch_digest")? },
        })
    }

    /// The wire value (not validated; see [`ExternalClaim::canonical_bytes`]).
    pub fn to_value(&self) -> CValue {
        let bits = format!("0x{:016x}", self.value.value().to_bits());
        obj([
            ("algorithm_source", st(self.algorithm_source.as_str())),
            (
                "batch",
                obj([("batch_digest", st(self.batch.batch_digest.as_str())), ("batch_id", dec(self.batch.batch_id))]),
            ),
            ("method", st(self.method.raw())),
            ("quantity", st(&self.quantity)),
            ("schema", st("external-claim/0")),
            ("value", obj([("unit", st(self.value.unit().as_str())), ("value", CValue::Str(bits))])),
        ])
    }

    /// RFC 8785 canonical bytes, after validating the record (fails closed).
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, StampError> {
        canonical(desc::CLAIM, &self.to_value())
    }

    /// `sha256:` digest of the canonical bytes.
    pub fn digest(&self) -> Result<Digest, StampError> {
        self.canonical_bytes().map(|b| digest_of(&b))
    }
}

impl Record {
    pub fn kind(&self) -> RecordKind {
        match self {
            Record::Stamp(_) => RecordKind::Stamp,
            Record::Batch(_) => RecordKind::Batch,
            Record::Claim(_) => RecordKind::Claim,
        }
    }

    /// The wire value.
    pub fn to_value(&self) -> CValue {
        match self {
            Record::Stamp(s) => s.to_value(),
            Record::Batch(b) => b.to_value(),
            Record::Claim(c) => c.to_value(),
        }
    }

    /// RFC 8785 canonical bytes, after validating the record (fails closed).
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, StampError> {
        match self {
            Record::Stamp(s) => s.canonical_bytes(),
            Record::Batch(b) => b.canonical_bytes(),
            Record::Claim(c) => c.canonical_bytes(),
        }
    }

    /// `sha256:` digest of the canonical bytes (REQ-HAL.IDL.STAMP-003).
    pub fn digest(&self) -> Result<Digest, StampError> {
        self.canonical_bytes().map(|b| digest_of(&b))
    }

    pub(crate) fn semantic(&self) -> Result<(), StampError> {
        match self {
            Record::Stamp(s) => s.semantic(""),
            Record::Batch(b) => b.semantic(),
            Record::Claim(_) => Ok(()),
        }
    }
}
