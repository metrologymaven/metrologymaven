//! Entity ID service. Spec node `CORE.PRJ.CONT.ID` (CONSTITUTION §5: stable IDs).
//!
//! - New entities get a UUIDv7 ([`IdService::new_v7`]). Its millisecond timestamp and its
//!   74 random bits come from an [`ObservationSource`]; the live source ([`live_source`])
//!   records both as observed inputs (class N), and a [`ReplaySource`] hands the recorded
//!   values back in order, so replay rebuilds the same IDs without drawing anything new.
//! - Derived entities get a UUIDv5 ([`IdService::derive_v5`]) of the project namespace UUID
//!   and the `fid-canon` (RFC 8785) canonical bytes of the derivation inputs.
//! - Imported entities keep their ID ([`IdService::import`]); an ID already in the project
//!   fails with `id_collision`.
//!
//! No operation changes an existing ID: [`Uuid`] is an immutable value and the service only
//! creates IDs (REQ-CORE.PRJ.CONT.ID-005).
//!
//! UUIDv7 bit convention: one 74-bit value `R` is drawn; `rand_a` is its top 12 bits
//! (`R >> 62`) and `rand_b` its low 62 bits (RFC 9562 §5.7 layout). No sub-millisecond
//! counter is used, so IDs created in the same millisecond, or after the clock steps back,
//! are not ordered by creation (RFC 9562 and the leaf allow this).
//!
//! A UUIDv7 draws its two observations as one unit ([`ObservationSource::draw_v7`]): if
//! either fails, nothing is recorded, so a retry leaves a clean `now_ms, random_bits` log.

use fid_canon::{to_canonical_bytes, CValue, CanonError};
use sha1::{Digest as _, Sha1};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

/// Random bits in a UUIDv7 (12 bits `rand_a` + 62 bits `rand_b`).
pub const V7_RANDOM_BITS: u32 = 74;
/// Largest 48-bit Unix millisecond timestamp.
const MAX_UNIX_MS: u64 = (1 << 48) - 1;

// ------------------------------------------------------------------ errors

/// Typed errors with stable codes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdError {
    /// The imported ID already exists in the project.
    IdCollision(Uuid),
    /// Not a well-formed UUID (or the nil or max UUID, which no entity may carry).
    BadId,
    /// The derivation inputs have no canonical form.
    BadInput(CanonError),
    /// An observation is out of range (timestamp over 48 bits, wrong bit width).
    BadObservation,
    /// The system clock or CSPRNG could not be read.
    ObservationUnavailable,
    /// Replay asked for more observations than were recorded.
    ReplayExhausted,
    /// The next recorded observation is of another kind or width.
    ReplayMismatch,
}

impl IdError {
    /// Stable machine-readable error code.
    pub fn code(&self) -> &'static str {
        match self {
            IdError::IdCollision(_) => "id_collision",
            IdError::BadId => "bad_id",
            IdError::BadInput(_) => "bad_input",
            IdError::BadObservation => "bad_observation",
            IdError::ObservationUnavailable => "observation_unavailable",
            IdError::ReplayExhausted => "replay_exhausted",
            IdError::ReplayMismatch => "replay_mismatch",
        }
    }
}

impl fmt::Display for IdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IdError::IdCollision(id) => write!(f, "id_collision: {id}"),
            IdError::BadInput(e) => write!(f, "bad_input: {e}"),
            other => f.write_str(other.code()),
        }
    }
}

impl std::error::Error for IdError {}

// ------------------------------------------------------------------ UUID value

/// A 128-bit UUID. Immutable: there is no way to change one in place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Uuid([u8; 16]);

impl Uuid {
    /// The nil UUID (all zero).
    pub const NIL: Uuid = Uuid([0; 16]);
    /// The max UUID (all one).
    pub const MAX: Uuid = Uuid([0xff; 16]);

    pub const fn from_bytes(bytes: [u8; 16]) -> Uuid {
        Uuid(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    pub const fn from_u128(v: u128) -> Uuid {
        Uuid(v.to_be_bytes())
    }

    pub const fn as_u128(&self) -> u128 {
        u128::from_be_bytes(self.0)
    }

    /// The version field (top nibble of octet 6).
    pub const fn version(&self) -> u8 {
        self.0[6] >> 4
    }

    /// The top two bits of octet 8; `0b10` for RFC 9562 UUIDs.
    pub const fn variant_bits(&self) -> u8 {
        self.0[8] >> 6
    }

    /// Parses the 36-character hyphenated form (`8-4-4-4-12` hex digits, either case).
    pub fn parse(text: &str) -> Result<Uuid, IdError> {
        let s = text.as_bytes();
        if s.len() != 36 {
            return Err(IdError::BadId);
        }
        let mut out = [0u8; 16];
        let mut nibble = 0usize;
        for (i, &c) in s.iter().enumerate() {
            if matches!(i, 8 | 13 | 18 | 23) {
                if c != b'-' {
                    return Err(IdError::BadId);
                }
                continue;
            }
            let v = hex_val(c).ok_or(IdError::BadId)?;
            let slot = out.get_mut(nibble / 2).ok_or(IdError::BadId)?;
            *slot |= if nibble % 2 == 0 { v << 4 } else { v };
            nibble += 1;
        }
        Ok(Uuid(out))
    }
}

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

impl fmt::Display for Uuid {
    /// Lowercase hyphenated form.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, b) in self.0.iter().enumerate() {
            if matches!(i, 4 | 6 | 8 | 10) {
                f.write_str("-")?;
            }
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

impl FromStr for Uuid {
    type Err = IdError;
    fn from_str(s: &str) -> Result<Uuid, IdError> {
        Uuid::parse(s)
    }
}

/// UUIDv7 (RFC 9562 §5.7) from a 48-bit Unix millisecond timestamp and 74 random bits.
pub fn uuid_v7(unix_ms: u64, random: &Bits) -> Result<Uuid, IdError> {
    if unix_ms > MAX_UNIX_MS || random.width() != V7_RANDOM_BITS {
        return Err(IdError::BadObservation);
    }
    let r = random.value();
    let rand_a = r >> 62;
    let rand_b = r & ((1u128 << 62) - 1);
    let n = (u128::from(unix_ms) << 80) | (0x7u128 << 76) | (rand_a << 64) | (0b10u128 << 62) | rand_b;
    Ok(Uuid::from_u128(n))
}

/// UUIDv5 (RFC 9562 §5.5): SHA-1 of namespace bytes and name, with version and variant set.
/// SHA-1 serves identity only, not security.
pub fn uuid_v5(namespace: &Uuid, name: &[u8]) -> Uuid {
    let mut h = Sha1::new();
    h.update(namespace.as_bytes());
    h.update(name);
    let digest = h.finalize();
    let mut b = [0u8; 16];
    for (o, d) in b.iter_mut().zip(digest.iter()) {
        *o = *d;
    }
    b[6] = (b[6] & 0x0f) | 0x50;
    b[8] = (b[8] & 0x3f) | 0x80;
    Uuid(b)
}

// ------------------------------------------------------------------ observations

/// Up to 128 random bits, right-aligned in `value`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bits {
    width: u32,
    value: u128,
}

impl Bits {
    /// Fails with `bad_observation` if `width > 128` or `value` has bits above `width`.
    pub fn new(width: u32, value: u128) -> Result<Bits, IdError> {
        if width > 128 || (width < 128 && value >> width != 0) {
            return Err(IdError::BadObservation);
        }
        Ok(Bits { width, value })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn value(&self) -> u128 {
        self.value
    }
}

/// One observed input, as recorded for replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Observation {
    NowMs(u64),
    RandomBits(Bits),
}

impl Observation {
    /// Journal form: `{"kind":"now_ms","value":"<ms>"}` or
    /// `{"kind":"random_bits","value":"0x<hex>","width":"<n>"}` (64-bit integers as strings, per `fid-canon`).
    pub fn to_cvalue(&self) -> CValue {
        let mut m = BTreeMap::new();
        match self {
            Observation::NowMs(ms) => {
                m.insert("kind".to_string(), CValue::Str("now_ms".to_string()));
                m.insert("value".to_string(), CValue::U64(*ms));
            }
            Observation::RandomBits(b) => {
                m.insert("kind".to_string(), CValue::Str("random_bits".to_string()));
                m.insert("value".to_string(), CValue::Str(format!("0x{:x}", b.value())));
                m.insert("width".to_string(), CValue::U64(u64::from(b.width())));
            }
        }
        CValue::Obj(m)
    }

    /// Inverse of [`Observation::to_cvalue`], for replay reading the journal. Accepts the
    /// 64-bit fields as [`CValue::U64`] or, as parsed from JSON text, as decimal strings.
    /// Fails closed with `bad_observation` on any other shape, extra key or non-canonical spelling.
    pub fn from_cvalue(v: &CValue) -> Result<Observation, IdError> {
        let m = match v {
            CValue::Obj(m) => m,
            _ => return Err(IdError::BadObservation),
        };
        match m.get("kind") {
            Some(CValue::Str(k)) if k == "now_ms" && m.len() == 2 => {
                Ok(Observation::NowMs(u64_field(m.get("value"))?))
            }
            Some(CValue::Str(k)) if k == "random_bits" && m.len() == 3 => {
                let width = u32::try_from(u64_field(m.get("width"))?).map_err(|_| IdError::BadObservation)?;
                let hex = match m.get("value") {
                    Some(CValue::Str(s)) => s.strip_prefix("0x").ok_or(IdError::BadObservation)?,
                    _ => return Err(IdError::BadObservation),
                };
                let canonical = !hex.is_empty()
                    && hex.len() <= 32
                    && hex.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'))
                    && (hex == "0" || !hex.starts_with('0'));
                if !canonical {
                    return Err(IdError::BadObservation);
                }
                let value = u128::from_str_radix(hex, 16).map_err(|_| IdError::BadObservation)?;
                Ok(Observation::RandomBits(Bits::new(width, value)?))
            }
            _ => Err(IdError::BadObservation),
        }
    }
}

/// A 64-bit field: `U64`, or a canonical decimal string (no sign, no leading zeros).
fn u64_field(v: Option<&CValue>) -> Result<u64, IdError> {
    match v {
        Some(CValue::U64(n)) => Ok(*n),
        Some(CValue::Str(s))
            if !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()) && (s == "0" || !s.starts_with('0')) =>
        {
            s.parse().map_err(|_| IdError::BadObservation)
        }
        _ => Err(IdError::BadObservation),
    }
}

/// Where the time and randomness of a UUIDv7 come from.
pub trait ObservationSource {
    /// Unix time in milliseconds.
    fn now_ms(&mut self) -> Result<u64, IdError>;
    /// `n` (at most 128) random bits.
    fn random_bits(&mut self, n: u32) -> Result<Bits, IdError>;

    /// The observations of one UUIDv7: `now_ms` then `random_bits(74)`, as one unit.
    /// [`Recording`] and [`ReplaySource`] override it so a failure records or consumes nothing.
    fn draw_v7(&mut self) -> Result<(u64, Bits), IdError> {
        let ms = self.now_ms()?;
        let bits = self.random_bits(V7_RANDOM_BITS)?;
        Ok((ms, bits))
    }
}

/// The system clock and the operating-system CSPRNG. Records nothing by itself;
/// wrap it in [`Recording`] (see [`live_source`]).
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemSource;

impl ObservationSource for SystemSource {
    fn now_ms(&mut self) -> Result<u64, IdError> {
        let d = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| IdError::ObservationUnavailable)?;
        u64::try_from(d.as_millis()).map_err(|_| IdError::ObservationUnavailable)
    }

    fn random_bits(&mut self, n: u32) -> Result<Bits, IdError> {
        if n > 128 {
            return Err(IdError::BadObservation);
        }
        let mut buf = [0u8; 16];
        getrandom::getrandom(&mut buf).map_err(|_| IdError::ObservationUnavailable)?;
        let raw = u128::from_be_bytes(buf);
        let value = if n == 128 { raw } else { raw & ((1u128 << n) - 1) };
        Bits::new(n, value)
    }
}

/// Wraps a source and records every value it returns, in order (the handler's observed inputs).
#[derive(Debug, Clone, Default)]
pub struct Recording<S> {
    inner: S,
    log: Vec<Observation>,
}

impl<S> Recording<S> {
    pub fn new(inner: S) -> Recording<S> {
        Recording { inner, log: Vec::new() }
    }

    /// The values drawn so far, in order.
    pub fn observations(&self) -> &[Observation] {
        &self.log
    }

    pub fn into_observations(self) -> Vec<Observation> {
        self.log
    }

    pub fn inner(&self) -> &S {
        &self.inner
    }
}

impl<S: ObservationSource> ObservationSource for Recording<S> {
    fn now_ms(&mut self) -> Result<u64, IdError> {
        let ms = self.inner.now_ms()?;
        self.log.push(Observation::NowMs(ms));
        Ok(ms)
    }

    fn random_bits(&mut self, n: u32) -> Result<Bits, IdError> {
        let b = self.inner.random_bits(n)?;
        if b.width() != n {
            return Err(IdError::BadObservation);
        }
        self.log.push(Observation::RandomBits(b));
        Ok(b)
    }

    /// Draws both values from the inner source first and records them only if both succeed.
    fn draw_v7(&mut self) -> Result<(u64, Bits), IdError> {
        let ms = self.inner.now_ms()?;
        let b = self.inner.random_bits(V7_RANDOM_BITS)?;
        if b.width() != V7_RANDOM_BITS {
            return Err(IdError::BadObservation);
        }
        self.log.push(Observation::NowMs(ms));
        self.log.push(Observation::RandomBits(b));
        Ok((ms, b))
    }
}

/// The live source: system clock and CSPRNG, each value recorded.
pub fn live_source() -> Recording<SystemSource> {
    Recording::new(SystemSource)
}

/// Replay mode: returns recorded observations in order and draws nothing new.
#[derive(Debug, Clone)]
pub struct ReplaySource {
    recorded: Vec<Observation>,
    next: usize,
}

impl ReplaySource {
    pub fn new(recorded: Vec<Observation>) -> ReplaySource {
        ReplaySource { recorded, next: 0 }
    }

    /// Recorded observations not yet consumed.
    pub fn remaining(&self) -> usize {
        self.recorded.len().saturating_sub(self.next)
    }
}

impl ObservationSource for ReplaySource {
    fn now_ms(&mut self) -> Result<u64, IdError> {
        match self.recorded.get(self.next) {
            None => Err(IdError::ReplayExhausted),
            Some(Observation::NowMs(ms)) => {
                self.next += 1;
                Ok(*ms)
            }
            Some(_) => Err(IdError::ReplayMismatch),
        }
    }

    fn random_bits(&mut self, n: u32) -> Result<Bits, IdError> {
        match self.recorded.get(self.next) {
            None => Err(IdError::ReplayExhausted),
            Some(Observation::RandomBits(b)) if b.width() == n => {
                self.next += 1;
                Ok(*b)
            }
            Some(_) => Err(IdError::ReplayMismatch),
        }
    }

    /// Consumes the pair only if both recorded values fit.
    fn draw_v7(&mut self) -> Result<(u64, Bits), IdError> {
        let start = self.next;
        let pair = self.now_ms().and_then(|ms| self.random_bits(V7_RANDOM_BITS).map(|b| (ms, b)));
        if pair.is_err() {
            self.next = start;
        }
        pair
    }
}

// ------------------------------------------------------------------ service

/// The IDs already present in a project.
pub trait IdIndex {
    fn contains_id(&self, id: &Uuid) -> bool;
}

impl IdIndex for BTreeSet<Uuid> {
    fn contains_id(&self, id: &Uuid) -> bool {
        self.contains(id)
    }
}

impl IdIndex for Vec<Uuid> {
    fn contains_id(&self, id: &Uuid) -> bool {
        self.contains(id)
    }
}

/// Assigns entity IDs. It only creates IDs; it offers no way to change one.
pub struct IdService<'a> {
    namespace: Uuid,
    source: &'a mut dyn ObservationSource,
    existing: &'a dyn IdIndex,
}

impl<'a> IdService<'a> {
    pub fn new(namespace: Uuid, source: &'a mut dyn ObservationSource, existing: &'a dyn IdIndex) -> IdService<'a> {
        IdService { namespace, source, existing }
    }

    /// The project namespace UUID used by [`IdService::derive_v5`].
    pub fn namespace(&self) -> Uuid {
        self.namespace
    }

    /// A fresh UUIDv7: draws `now_ms` then `random_bits(74)` from the source as one unit.
    pub fn new_v7(&mut self) -> Result<Uuid, IdError> {
        let (ms, r) = self.source.draw_v7()?;
        uuid_v7(ms, &r)
    }

    /// The UUIDv5 of the project namespace and the canonical bytes of `inputs`.
    pub fn derive_v5(&self, inputs: &CValue) -> Result<Uuid, IdError> {
        let name = to_canonical_bytes(inputs).map_err(IdError::BadInput)?;
        Ok(uuid_v5(&self.namespace, &name))
    }

    /// Imports an ID in hyphenated text form (as a QIF persistent ID carries it) unchanged.
    pub fn import(&self, text: &str) -> Result<Uuid, IdError> {
        self.import_uuid(Uuid::parse(text)?)
    }

    /// Imports an ID unchanged; `id_collision` if the project already has it.
    pub fn import_uuid(&self, id: Uuid) -> Result<Uuid, IdError> {
        if id == Uuid::NIL || id == Uuid::MAX {
            return Err(IdError::BadId);
        }
        if self.existing.contains_id(&id) {
            return Err(IdError::IdCollision(id));
        }
        Ok(id)
    }
}
