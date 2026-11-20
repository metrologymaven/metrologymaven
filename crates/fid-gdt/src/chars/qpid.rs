//! Stable characteristic identity (spec leaf `GDT.CHAR.IDENT`, REQ-GDT.CHAR-009).
//!
//! One identity per characteristic, kept from plan to result to export:
//!
//! - **Creation** ([`QpidAssigner::assign`]): every identity goes through the project's ID
//!   service (`fid_project::IdService`, CORE.PRJ.CONT.ID). An imported QPId is kept
//!   (`IdService::import_uuid`, REQ-GDT.CHAR.IDENT-001). Otherwise one UUIDv7 is drawn
//!   (`IdService::new_v7`, RFC 9562 §5.7, CONSTITUTION §5, REQ-GDT.CHAR.IDENT-002). Either
//!   way the ID is checked against the project's [`IdIndex`]. An ID the project already holds
//!   fails with `id_collision`, and nothing is redrawn. Live callers pass the recorded
//!   system source (`fid_project::live_source`), and replay passes `fid_project::ReplaySource`.
//!   [`CounterRng`] is a deterministic source for tests and fixtures only.
//! - **Edits** ([`check_unchanged`], [`check_edit`]): any attempt to change the identity fails
//!   with `identity_immutable` (REQ-GDT.CHAR.IDENT-003, -005).
//! - **QIF mapping** ([`read_identity`], [`write_identity`]): where the element has a native
//!   QPId slot, the identity goes there. Otherwise it goes into the element's `Attributes` as
//!   a string attribute named `fx:qpid` and is read back from it on import
//!   (REQ-GDT.CHAR.IDENT-004).
//!
//! **Equality and keys.** [`Qpid`] and [`CharIdentity`] compare, order and hash by UUID value
//! only, so an upper-case and a lower-case spelling are one identity. The characteristic key
//! ([`CharIdentity::to_char_id`]) is always the lowercase hyphenated UUID. The text exactly as
//! imported is kept only so that export into a native QPId slot writes the imported bytes
//! back. [`Qpid::same_text`] compares that text when it matters.
//!
//! **The `fx:qpid` attribute** is not yet reserved in a versioned `fx` catalogue. The Open
//! questions of `CORE.PRJ.QIF` ask for `qpid` to be reserved there. Until it is, an `fx:qpid`
//! value is accepted only in exactly the form this module writes: the 36-character lowercase
//! hyphenated UUID, with no surrounding white space. A third-party tool could still write a
//! well-formed UUID under that name, and this module would then adopt it as the identity.
//! That risk stays open until the catalogue entry exists.
//!
//! What this module does not do: it holds no characteristic types (`GDT.CHAR.MODEL`), parses
//! and writes no QIF XML (`IO.QIF`), and does not decide which QIF element types have a
//! native QPId slot. The caller names the [`QpidSlot`] of each element.
//!
//! Nothing here panics on any input: every outcome is a returned `Result`.

use fid_project::{Bits, IdError, IdIndex, IdService, ObservationSource, Uuid};
use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};

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
        /// `not_uuid`, `reserved_uuid`, `fx_qpid_form`, `duplicate_fx_qpid`,
        /// `conflicting_qpid` or `qpid_without_slot`.
        reason: &'static str,
    },
    /// An error of the project ID service, passed through with its own code:
    /// `id_collision` (the project already holds the ID), `bad_observation`,
    /// `observation_unavailable`, `replay_exhausted`, `replay_mismatch` and so on.
    Id(IdError),
}

impl IdentError {
    /// Stable machine-readable error code.
    pub fn code(&self) -> &'static str {
        match self {
            IdentError::IdentityImmutable { .. } => "identity_immutable",
            IdentError::MalformedQpid { .. } => "malformed_qpid",
            IdentError::Id(e) => e.code(),
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
            IdentError::Id(e) => fmt::Display::fmt(e, f),
        }
    }
}

impl std::error::Error for IdentError {}

impl From<IdError> for IdentError {
    fn from(e: IdError) -> IdentError {
        IdentError::Id(e)
    }
}

fn malformed(reason: &'static str) -> IdentError {
    IdentError::MalformedQpid { reason }
}

// ------------------------------------------------------------------ QPId value

/// A QPId: a UUID in the 36-character hyphenated form. The text is kept exactly as written,
/// apart from surrounding XML white space, which the QIF token type does not carry.
///
/// Equality, order and hash use the UUID value only; [`Qpid::same_text`] compares the text.
#[derive(Debug, Clone)]
pub struct Qpid {
    uuid: Uuid,
    text: String,
}

impl Qpid {
    /// Parses QPId text from a native slot. Leading and trailing XML white space (space, tab,
    /// CR, LF) is dropped. Anything else that is not a hyphenated UUID fails with
    /// `malformed_qpid` (`not_uuid`). The nil and max UUIDs, which no entity may carry, fail
    /// with `reserved_uuid`.
    pub fn parse(text: &str) -> Result<Qpid, IdentError> {
        let t = text.trim_matches(|c| matches!(c, ' ' | '\t' | '\r' | '\n'));
        let uuid = Uuid::parse(t).map_err(|_| malformed("not_uuid"))?;
        if uuid == Uuid::NIL || uuid == Uuid::MAX {
            return Err(malformed("reserved_uuid"));
        }
        Ok(Qpid { uuid, text: t.to_owned() })
    }

    /// Parses an `fx:qpid` value. Only the exact form this module writes is accepted: the
    /// 36-character lowercase hyphenated UUID with no white space (`fx_qpid_form` otherwise).
    pub fn parse_fx(text: &str) -> Result<Qpid, IdentError> {
        let q = Qpid::parse(text)?;
        if q.text != text || q.uuid.to_string() != text {
            return Err(malformed("fx_qpid_form"));
        }
        Ok(q)
    }

    /// The UUID value.
    pub fn uuid(&self) -> Uuid {
        self.uuid
    }

    /// The text as imported (or, for an assigned identity, the lowercase hyphenated form).
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// True if both have the same text byte for byte (not only the same UUID).
    pub fn same_text(&self, other: &Qpid) -> bool {
        self.text == other.text
    }

    fn from_uuid(uuid: Uuid) -> Qpid {
        Qpid { uuid, text: uuid.to_string() }
    }
}

impl PartialEq for Qpid {
    fn eq(&self, other: &Qpid) -> bool {
        self.uuid == other.uuid
    }
}

impl Eq for Qpid {}

impl PartialOrd for Qpid {
    fn partial_cmp(&self, other: &Qpid) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Qpid {
    fn cmp(&self, other: &Qpid) -> Ordering {
        self.uuid.cmp(&other.uuid)
    }
}

impl Hash for Qpid {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.uuid.hash(state);
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
/// Equality, order and hash use the UUID value only (not the text, not the [`IdSource`]). An
/// assigned identity that is exported and re-imported comes back as [`IdSource::Imported`]
/// and equal. [`CharIdentity::same_text`] compares the exported text.
#[derive(Debug, Clone)]
pub struct CharIdentity {
    qpid: Qpid,
    source: IdSource,
}

impl CharIdentity {
    /// The UUID value.
    pub fn uuid(&self) -> Uuid {
        self.qpid.uuid
    }

    /// The text written into a native QPId slot on export (the imported text, unchanged).
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

    /// True if both have the same QPId text byte for byte.
    pub fn same_text(&self, other: &CharIdentity) -> bool {
        self.qpid.same_text(&other.qpid)
    }

    /// The canonical characteristic key used by the governing-spec, defaults-profile and
    /// ResultRecord modules: the lowercase hyphenated UUID, whatever the imported spelling.
    /// A hyphenated UUID always fits that key grammar, so this is `None` only if the grammar
    /// were narrowed.
    pub fn to_char_id(&self) -> Option<CharId> {
        CharId::parse(&self.qpid.uuid.to_string())
    }
}

impl PartialEq for CharIdentity {
    fn eq(&self, other: &CharIdentity) -> bool {
        self.qpid == other.qpid
    }
}

impl Eq for CharIdentity {}

impl PartialOrd for CharIdentity {
    fn partial_cmp(&self, other: &CharIdentity) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for CharIdentity {
    fn cmp(&self, other: &CharIdentity) -> Ordering {
        self.qpid.cmp(&other.qpid)
    }
}

impl Hash for CharIdentity {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.qpid.hash(state);
    }
}

impl fmt::Display for CharIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.qpid.text)
    }
}

// ------------------------------------------------------------------ test source

const GAMMA: u64 = 0x9e37_79b9_7f4a_7c15;

/// A deterministic [`ObservationSource`] for tests, fixtures and reference vectors only. It is
/// never a live source: replay uses the recorded observations (`fid_project::ReplaySource`).
///
/// `now_ms` returns a set instant. Random output `i` (1-based) is the SplitMix64 finaliser of
/// `seed + i * 0x9e3779b97f4a7c15` (wrapping). `random_bits(n)` for `n <= 64` takes the low
/// `n` bits of one output. For `64 < n <= 128` it takes two outputs `a` then `b`: the low
/// `n - 64` bits of `a`, followed by all 64 bits of `b`. Two generators whose seeds differ by
/// a multiple of the step produce the same outputs shifted, which is one reason this is not
/// a live source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CounterRng {
    seed: u64,
    counter: u64,
    now_ms: u64,
}

impl CounterRng {
    /// A generator at counter 0 whose clock reads `now_ms`.
    pub const fn new(seed: u64, now_ms: u64) -> CounterRng {
        CounterRng { seed, counter: 0, now_ms }
    }

    /// Sets the instant `now_ms` returns.
    pub fn set_now_ms(&mut self, now_ms: u64) {
        self.now_ms = now_ms;
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
}

impl ObservationSource for CounterRng {
    /// The set instant; `bad_observation` past 48 bits, before anything is drawn.
    fn now_ms(&mut self) -> Result<u64, IdError> {
        if self.now_ms > MAX_UNIX_MS {
            return Err(IdError::BadObservation);
        }
        Ok(self.now_ms)
    }

    fn random_bits(&mut self, n: u32) -> Result<Bits, IdError> {
        if n > 128 {
            return Err(IdError::BadObservation);
        }
        let value = if n <= 64 {
            let a = self.next_u64();
            if n == 64 {
                u128::from(a)
            } else {
                u128::from(a & ((1u64 << n) - 1))
            }
        } else {
            let hi_bits = n - 64;
            let a = self.next_u64();
            let hi = if hi_bits == 64 { a } else { a & ((1u64 << hi_bits) - 1) };
            let b = self.next_u64();
            (u128::from(hi) << 64) | u128::from(b)
        };
        Bits::new(n, value)
    }
}

// ------------------------------------------------------------------ assignment

/// Assigns characteristic identities through the project ID service, checked against the
/// project's IDs. It only creates identities; it offers no way to change one.
pub struct QpidAssigner<'a> {
    ids: IdService<'a>,
    existing: &'a dyn IdIndex,
}

impl<'a> QpidAssigner<'a> {
    /// An assigner over the project's namespace, observation source and ID index.
    pub fn new(namespace: Uuid, source: &'a mut dyn ObservationSource, existing: &'a dyn IdIndex) -> QpidAssigner<'a> {
        QpidAssigner { ids: IdService::new(namespace, source, existing), existing }
    }

    /// The identity of a characteristic being created. An imported QPId is kept unchanged
    /// (`IdService::import_uuid`), and the source is not touched. Otherwise one UUIDv7 is drawn
    /// (`IdService::new_v7`). Either way, an ID the project already holds fails with
    /// `id_collision`, and nothing is redrawn. Call this exactly once per characteristic;
    /// edits keep the result ([`check_unchanged`]). The caller adds the new ID to the
    /// project's index when it commits.
    pub fn assign(&mut self, imported: Option<Qpid>) -> Result<CharIdentity, IdentError> {
        match imported {
            Some(qpid) => {
                self.ids.import_uuid(qpid.uuid)?;
                Ok(CharIdentity { qpid, source: IdSource::Imported })
            }
            None => {
                let uuid = self.ids.new_v7()?;
                if self.existing.contains_id(&uuid) {
                    return Err(IdentError::Id(IdError::IdCollision(uuid)));
                }
                Ok(CharIdentity { qpid: Qpid::from_uuid(uuid), source: IdSource::Assigned })
            }
        }
    }

    /// Import-time identity of a QIF element: [`read_identity`], then [`QpidAssigner::assign`].
    /// A fresh UUIDv7 is drawn only if the element carried no identity.
    pub fn assign_on_import(&mut self, slot: QpidSlot, view: &QifIdentityView) -> Result<CharIdentity, IdentError> {
        let imported = read_identity(slot, view)?;
        self.assign(imported)
    }
}

// ------------------------------------------------------------------ edits

/// `Ok` if `new` names the same characteristic as `old` (equal UUIDs), else
/// `identity_immutable`.
pub fn check_unchanged(old: &CharIdentity, new: &CharIdentity) -> Result<(), IdentError> {
    if old == new {
        Ok(())
    } else {
        Err(IdentError::IdentityImmutable { current: old.uuid(), attempted: new.uuid() })
    }
}

/// Checks the identity field of an edit payload against the characteristic's identity.
/// No field passes, and a field naming the same UUID passes. A malformed field fails with
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

/// Reads the identity of an imported element, or `None` if it carries none. An `fx:qpid`
/// value must have the exact form [`Qpid::parse_fx`] accepts.
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
    let fx_qpid = first_fx.map(Qpid::parse_fx).transpose()?;
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
                (Some(n), Some(f)) if n != f => Err(malformed("conflicting_qpid")),
                (Some(n), _) => Ok(Some(n)),
                (None, f) => Ok(f),
            }
        }
    }
}

/// Writes `id` into the element's identity fields for export.
///
/// - `Native`: the native QPId text becomes `id`'s text (the imported text, unchanged), and
///   any `fx:qpid` attribute is removed, because the native slot is the one place the
///   identity lives.
/// - `FxAttribute`: the native QPId is cleared. The first `fx:qpid` attribute takes the
///   lowercase hyphenated UUID in place, or one is appended at the end, and any further
///   `fx:qpid` is removed.
///
/// Every other attribute is kept, in order.
pub fn write_identity(slot: QpidSlot, id: &CharIdentity, view: &mut QifIdentityView) {
    match slot {
        QpidSlot::Native => {
            view.native_qpid = Some(id.as_str().to_owned());
            view.attributes.retain(|a| a.name != FX_QPID);
        }
        QpidSlot::FxAttribute => {
            let canonical = id.uuid().to_string();
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
                a.value.clone_from(&canonical);
                true
            });
            if !seen {
                view.attributes.push(QifStrAttribute { name: FX_QPID.to_owned(), value: canonical });
            }
        }
    }
}
