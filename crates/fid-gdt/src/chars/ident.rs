//! Stable characteristic identity (spec leaf `GDT.CHAR.IDENT`, REQ-GDT.CHAR-009).
//!
//! One identity per characteristic, kept from plan to result to export:
//!
//! - **Creation** ([`assign_identity`]): an imported QPId is kept as it was written
//!   (REQ-GDT.CHAR.IDENT-001); otherwise one UUIDv7 (RFC 9562 §5.7, CONSTITUTION §5) is
//!   drawn from an injected [`Clock`] and a seeded [`CounterRng`] (REQ-GDT.CHAR.IDENT-002).
//!   The library never reads the system clock or the operating-system RNG, so the same clock
//!   and seed give the same identity on every platform (D0). [`assign_identity_from`] draws
//!   from a project [`ObservationSource`] instead, for callers that record their observed
//!   inputs for replay.
//! - **Edits** ([`check_unchanged`], [`check_edit`]): any attempt to change the identity fails
//!   with `identity_immutable` (REQ-GDT.CHAR.IDENT-003, -005).
//! - **QIF mapping** ([`read_identity`], [`write_identity`]): where the element has a native
//!   QPId slot the identity is written there; otherwise it goes into the element's
//!   `Attributes` as a string attribute named `fx:qpid` and is read back from it on import
//!   (REQ-GDT.CHAR.IDENT-004).
//!
//! What this module does not do: it holds no characteristic types (`GDT.CHAR.MODEL`), parses
//! and writes no QIF XML (`IO.QIF`), and does not decide which QIF element types have a
//! native QPId slot; the caller names the [`QpidSlot`] of each element.
//!
//! Identity is compared by UUID value: a QPId written in upper case and the same UUID in lower
//! case are one identity. The text is still kept exactly as imported, so export writes the
//! imported bytes back unchanged.
//!
//! Nothing here panics on any input: every outcome is a returned `Result`.

use fid_project::{uuid_v7, Bits, ObservationSource, Uuid, V7_RANDOM_BITS};
use std::fmt;

use crate::chars::dprof::CharId;

/// Name of the QIF `Attributes` string attribute that carries the identity of an element
/// without a native QPId slot.
pub const FX_QPID: &str = "fx:qpid";
/// Largest 48-bit Unix millisecond timestamp a UUIDv7 can hold.
pub const MAX_UNIX_MS: u64 = (1 << 48) - 1;

// ------------------------------------------------------------------ errors

/// Typed errors with stable codes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentError {
    /// An edit tried to change the identity of a characteristic.
    IdentityImmutable {
        /// The identity the characteristic has.
        current: Uuid,
        /// The identity the edit carried.
        attempted: Uuid,
    },
    /// A QPId (native or `fx:qpid`) is not a usable UUID, or the identity fields of a QIF
    /// element contradict each other. `reason` is a stable sub-code.
    MalformedQpid {
        /// `not_uuid`, `reserved_uuid`, `duplicate_fx_qpid`, `conflicting_qpid` or
        /// `qpid_without_slot`.
        reason: &'static str,
    },
    /// The injected clock is past the 48-bit UUIDv7 range, or the observation source failed.
    BadObservation,
}

impl IdentError {
    /// Stable machine-readable error code.
    pub fn code(&self) -> &'static str {
        match self {
            IdentError::IdentityImmutable { .. } => "identity_immutable",
            IdentError::MalformedQpid { .. } => "malformed_qpid",
            IdentError::BadObservation => "bad_observation",
        }
    }
}

impl fmt::Display for IdentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IdentError::IdentityImmutable { current, attempted } => {
                write!(f, "identity_immutable: {current} cannot become {attempted}")
            }
            IdentError::MalformedQpid { reason } => write!(f, "malformed_qpid: {reason}"),
            IdentError::BadObservation => f.write_str("bad_observation"),
        }
    }
}

impl std::error::Error for IdentError {}

fn malformed(reason: &'static str) -> IdentError {
    IdentError::MalformedQpid { reason }
}

// ------------------------------------------------------------------ QPId value

/// A QPId: a UUID in the 36-character hyphenated form, with its text kept exactly as written
/// (apart from surrounding XML white space, which the QIF token type does not carry).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Qpid {
    uuid: Uuid,
    text: String,
}

impl Qpid {
    /// Parses QPId text. Leading and trailing XML white space (space, tab, CR, LF) is
    /// dropped; anything else that is not a hyphenated UUID fails with `malformed_qpid`
    /// (`not_uuid`), and the nil and max UUIDs, which no entity may carry, fail with
    /// `reserved_uuid`.
    pub fn parse(text: &str) -> Result<Qpid, IdentError> {
        let t = text.trim_matches(|c| matches!(c, ' ' | '\t' | '\r' | '\n'));
        let uuid = Uuid::parse(t).map_err(|_| malformed("not_uuid"))?;
        if uuid == Uuid::NIL || uuid == Uuid::MAX {
            return Err(malformed("reserved_uuid"));
        }
        Ok(Qpid { uuid, text: t.to_owned() })
    }

    /// The UUID value.
    pub fn uuid(&self) -> Uuid {
        self.uuid
    }

    /// The text as imported (or, for an assigned identity, the lowercase hyphenated form).
    pub fn as_str(&self) -> &str {
        &self.text
    }

    fn from_uuid(uuid: Uuid) -> Qpid {
        Qpid { uuid, text: uuid.to_string() }
    }
}

impl fmt::Display for Qpid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

// ------------------------------------------------------------------ identity

/// Where an identity came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IdSource {
    /// Read from an imported QPId (native slot or `fx:qpid`).
    Imported,
    /// Assigned as a fresh UUIDv7 at creation.
    Assigned,
}

/// The stable identity of one characteristic. There is no way to change it in place.
///
/// The derived equality also compares text and [`IdSource`]; use
/// [`CharIdentity::same_identity`] (or [`check_unchanged`]) to ask whether two values name
/// the same characteristic. An assigned identity that is exported and re-imported comes back
/// as [`IdSource::Imported`] with the same UUID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharIdentity {
    qpid: Qpid,
    source: IdSource,
}

impl CharIdentity {
    /// The UUID value.
    pub fn uuid(&self) -> Uuid {
        self.qpid.uuid
    }

    /// The QPId text written on export.
    pub fn as_str(&self) -> &str {
        &self.qpid.text
    }

    /// The QPId.
    pub fn qpid(&self) -> &Qpid {
        &self.qpid
    }

    /// Where the identity came from.
    pub fn source(&self) -> IdSource {
        self.source
    }

    /// True if both name the same characteristic (equal UUID values).
    pub fn same_identity(&self, other: &CharIdentity) -> bool {
        self.qpid.uuid == other.qpid.uuid
    }

    /// The identity as the characteristic ID the governing-spec, defaults-profile and
    /// ResultRecord modules key on. A hyphenated UUID always fits that grammar, so this is
    /// `None` only if the grammar were narrowed.
    pub fn to_char_id(&self) -> Option<CharId> {
        CharId::parse(&self.qpid.text)
    }
}

impl fmt::Display for CharIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.qpid.text)
    }
}

// ------------------------------------------------------------------ clock and RNG

/// The injected clock: Unix time in milliseconds. The library never reads the system time.
pub trait Clock {
    /// Unix time in milliseconds.
    fn now_ms(&self) -> u64;
}

/// A clock that always reads the same instant (tests, replay, batch import).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedClock(pub u64);

impl Clock for FixedClock {
    fn now_ms(&self) -> u64 {
        self.0
    }
}

const GAMMA: u64 = 0x9e37_79b9_7f4a_7c15;

/// A seeded counter-based generator: output `i` (1-based) is the SplitMix64 finaliser of
/// `seed + i * 0x9e3779b97f4a7c15` (wrapping). Pure integer arithmetic, so it gives the same
/// values on every platform. It is not a cryptographic generator: two sessions that use the
/// same seed and clock reading mint the same UUID, so the caller must choose distinct seeds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CounterRng {
    seed: u64,
    counter: u64,
}

impl CounterRng {
    /// A generator at counter 0.
    pub const fn new(seed: u64) -> CounterRng {
        CounterRng { seed, counter: 0 }
    }

    /// The seed.
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// How many 64-bit outputs have been drawn.
    pub fn counter(&self) -> u64 {
        self.counter
    }

    /// The next 64-bit output.
    pub fn next_u64(&mut self) -> u64 {
        self.counter = self.counter.wrapping_add(1);
        let mut z = self.seed.wrapping_add(self.counter.wrapping_mul(GAMMA));
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// The 74 random bits of one UUIDv7: two outputs `a` then `b`; the bits are the low 10
    /// bits of `a` followed by all 64 bits of `b`.
    pub fn next_v7_bits(&mut self) -> Result<Bits, IdentError> {
        let a = self.next_u64() & 0x3ff;
        let b = self.next_u64();
        Bits::new(V7_RANDOM_BITS, (u128::from(a) << 64) | u128::from(b)).map_err(|_| IdentError::BadObservation)
    }
}

// ------------------------------------------------------------------ assignment

/// The identity of a characteristic being created: the imported QPId unchanged if there is
/// one, else a fresh UUIDv7 from `clock` and `rng`. Call it exactly once per characteristic;
/// edits keep the result ([`check_unchanged`]).
///
/// A clock reading past [`MAX_UNIX_MS`] fails with `bad_observation` before anything is drawn
/// from `rng`. With an imported QPId neither the clock nor `rng` is used.
pub fn assign_identity(
    imported: Option<Qpid>,
    clock: &dyn Clock,
    rng: &mut CounterRng,
) -> Result<CharIdentity, IdentError> {
    if let Some(qpid) = imported {
        return Ok(CharIdentity { qpid, source: IdSource::Imported });
    }
    let ms = clock.now_ms();
    if ms > MAX_UNIX_MS {
        return Err(IdentError::BadObservation);
    }
    let bits = rng.next_v7_bits()?;
    assigned(ms, &bits)
}

/// As [`assign_identity`], but the UUIDv7 time and random bits come from a project
/// [`ObservationSource`] (which may record them for replay, CORE.PRJ.CONT.ID).
pub fn assign_identity_from(
    imported: Option<Qpid>,
    source: &mut dyn ObservationSource,
) -> Result<CharIdentity, IdentError> {
    if let Some(qpid) = imported {
        return Ok(CharIdentity { qpid, source: IdSource::Imported });
    }
    let (ms, bits) = source.draw_v7().map_err(|_| IdentError::BadObservation)?;
    assigned(ms, &bits)
}

fn assigned(ms: u64, bits: &Bits) -> Result<CharIdentity, IdentError> {
    let uuid = uuid_v7(ms, bits).map_err(|_| IdentError::BadObservation)?;
    Ok(CharIdentity { qpid: Qpid::from_uuid(uuid), source: IdSource::Assigned })
}

// ------------------------------------------------------------------ edits

/// `Ok` if `new` names the same characteristic as `old`, else `identity_immutable`.
pub fn check_unchanged(old: &CharIdentity, new: &CharIdentity) -> Result<(), IdentError> {
    if old.same_identity(new) {
        Ok(())
    } else {
        Err(IdentError::IdentityImmutable { current: old.uuid(), attempted: new.uuid() })
    }
}

/// Checks the identity field of an edit payload against the characteristic's identity: no
/// field passes; a field naming the same UUID passes; a malformed field fails with
/// `malformed_qpid`, and any other UUID with `identity_immutable`. Call it before applying
/// any part of the edit, so a rejected edit leaves the characteristic unchanged.
pub fn check_edit(current: &CharIdentity, payload_identity: Option<&str>) -> Result<(), IdentError> {
    match payload_identity {
        None => Ok(()),
        Some(text) => {
            let q = Qpid::parse(text)?;
            if q.uuid == current.uuid() {
                Ok(())
            } else {
                Err(IdentError::IdentityImmutable { current: current.uuid(), attempted: q.uuid })
            }
        }
    }
}

// ------------------------------------------------------------------ QIF mapping

/// Where a QIF element keeps its identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum QpidSlot {
    /// The element type has its own QPId slot.
    Native,
    /// The element type has none; the identity is the `fx:qpid` string attribute in the
    /// element's QIF `Attributes`.
    FxAttribute,
}

/// One string attribute of a QIF `Attributes` list (name and value, as text).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QifStrAttribute {
    /// The attribute name.
    pub name: String,
    /// The attribute value.
    pub value: String,
}

/// The identity-bearing fields of one QIF element, as the QIF reader hands them over and the
/// QIF writer takes them back: the native QPId text (if the element has one) and the string
/// attributes of its `Attributes` list, in document order. Other attribute kinds are not
/// this module's business and are not represented.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct QifIdentityView {
    /// Text of the native QPId slot, if present.
    pub native_qpid: Option<String>,
    /// String attributes of the element's `Attributes`, in order.
    pub attributes: Vec<QifStrAttribute>,
}

impl QifIdentityView {
    fn fx_values(&self) -> impl Iterator<Item = &str> {
        self.attributes.iter().filter(|a| a.name == FX_QPID).map(|a| a.value.as_str())
    }
}

/// Reads the identity of an imported element, or `None` if it carries none.
///
/// - `Native`: the native QPId. An `fx:qpid` attribute on the same element must name the same
///   UUID (`conflicting_qpid` otherwise), and may stand in for a missing native QPId.
/// - `FxAttribute`: the single `fx:qpid` attribute. A native QPId on such an element fails
///   with `qpid_without_slot`, and two or more `fx:qpid` attributes with `duplicate_fx_qpid`.
pub fn read_identity(slot: QpidSlot, view: &QifIdentityView) -> Result<Option<Qpid>, IdentError> {
    let mut fx = view.fx_values();
    let first_fx = fx.next();
    if fx.next().is_some() {
        return Err(malformed("duplicate_fx_qpid"));
    }
    let fx_qpid = first_fx.map(Qpid::parse).transpose()?;
    match slot {
        QpidSlot::FxAttribute => {
            if view.native_qpid.is_some() {
                return Err(malformed("qpid_without_slot"));
            }
            Ok(fx_qpid)
        }
        QpidSlot::Native => {
            let native = view.native_qpid.as_deref().map(Qpid::parse).transpose()?;
            match (native, fx_qpid) {
                (Some(n), Some(f)) if n.uuid != f.uuid => Err(malformed("conflicting_qpid")),
                (Some(n), _) => Ok(Some(n)),
                (None, f) => Ok(f),
            }
        }
    }
}

/// Writes `id` into the element's identity fields for export.
///
/// - `Native`: the native QPId text becomes `id`'s text, and any `fx:qpid` attribute is
///   removed (the native slot is the one place the identity lives).
/// - `FxAttribute`: the native QPId is cleared, the first `fx:qpid` attribute takes `id`'s
///   text in place (or one is appended at the end), and any further `fx:qpid` is removed.
///
/// Every other attribute is kept, in order.
pub fn write_identity(slot: QpidSlot, id: &CharIdentity, view: &mut QifIdentityView) {
    match slot {
        QpidSlot::Native => {
            view.native_qpid = Some(id.as_str().to_owned());
            view.attributes.retain(|a| a.name != FX_QPID);
        }
        QpidSlot::FxAttribute => {
            view.native_qpid = None;
            let mut seen = false;
            view.attributes.retain_mut(|a| {
                if a.name != FX_QPID {
                    return true;
                }
                if seen {
                    return false;
                }
                seen = true;
                a.value = id.as_str().to_owned();
                true
            });
            if !seen {
                view.attributes.push(QifStrAttribute { name: FX_QPID.to_owned(), value: id.as_str().to_owned() });
            }
        }
    }
}

/// Import-time identity of an element: [`read_identity`], then [`assign_identity`] (a fresh
/// UUIDv7 only if the element carried no identity).
pub fn identity_on_import(
    slot: QpidSlot,
    view: &QifIdentityView,
    clock: &dyn Clock,
    rng: &mut CounterRng,
) -> Result<CharIdentity, IdentError> {
    let imported = read_identity(slot, view)?;
    assign_identity(imported, clock, rng)
}
