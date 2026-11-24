//! Typed `instrument-profile/v1` records, their reader and their wire form.

use super::{
    ProfileError, ProfileErrorKind as K, MAX_DEVICE_CLASS, MAX_FRAMES, MAX_KEYID_BYTES, MAX_MPE_TERMS, MAX_SIG_CHARS,
    MAX_SYMBOL_BYTES, MAX_TOKEN_BYTES, MAX_UEC, MAX_UNTRUSTED_TEXT_BYTES, SCHEMA_TAG, SPEC_SHEET_UEC_CAP,
};
use crate::package::is_semver;
use crate::stamp::{child, digest_of, UntrustedText};
use crate::units::{f64_bits, FrameKind, Quantity, SiUnit};
use fid_canon::CValue;
use fid_types::Digest;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

// ------------------------------------------------------------------ identifiers

/// `true` for a lowercase name: `[a-z0-9][a-z0-9._-]*`, 1 to [`MAX_TOKEN_BYTES`] bytes.
fn is_name(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= MAX_TOKEN_BYTES
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'.' | b'_' | b'-'))
}

/// Splits `<prefix><name>@<semver>`; `None` unless both halves are valid.
fn split_versioned<'a>(s: &'a str, prefix: &str) -> Option<(&'a str, &'a str)> {
    let rest = s.strip_prefix(prefix)?;
    let (name, version) = rest.split_once('@')?;
    (is_name(name) && is_semver(version)).then_some((name, version))
}

/// A profile identity, `<name>@<semver>` (for example `arm-demo-01@1.0.0`). The name is
/// lowercase ASCII (`[a-z0-9][a-z0-9._-]*`, at most 64 bytes); the version is SemVer 2.0.0.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProfileId(String);

impl ProfileId {
    /// Parses the wire form; anything else is `profile_value_invalid`.
    pub fn parse(s: &str) -> Result<ProfileId, ProfileError> {
        split_versioned(s, "").map(|_| ProfileId(s.to_string())).ok_or_else(|| ProfileError::new(K::ValueInvalid, ""))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The part before `@`.
    pub fn name(&self) -> &str {
        self.0.split_once('@').map_or("", |(n, _)| n)
    }

    /// The SemVer after `@`.
    pub fn version(&self) -> &str {
        self.0.split_once('@').map_or("", |(_, v)| v)
    }
}

impl fmt::Display for ProfileId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The ID of the uncertainty contributor an MPE term maps to, `uc:<name>@<semver>`
/// (`UNC.MODEL.CONTRIB`). Opaque here: UNC resolves it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContributorId(String);

impl ContributorId {
    /// Parses the wire form; anything else is `profile_value_invalid`.
    pub fn parse(s: &str) -> Result<ContributorId, ProfileError> {
        split_versioned(s, "uc:")
            .map(|_| ContributorId(s.to_string()))
            .ok_or_else(|| ProfileError::new(K::ValueInvalid, ""))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The consuming uncertainty model: its ID `im:<name>@<semver>` (`UNC.INST.SCHEMA`) and the
/// digest of its canonical form. Wire form: `{"digest": "sha256:…", "id": "im:…"}`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModelRef {
    id: String,
    pub digest: Digest,
}

impl ModelRef {
    /// A reference; an `id` that is not `im:<name>@<semver>` is `profile_value_invalid`.
    pub fn new(id: &str, digest: Digest) -> Result<ModelRef, ProfileError> {
        split_versioned(id, "im:")
            .map(|_| ModelRef { id: id.to_string(), digest })
            .ok_or_else(|| ProfileError::new(K::ValueInvalid, "/id"))
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    /// The `id@digest` spelling, for logs and messages.
    pub fn to_ref_string(&self) -> String {
        format!("{}@{}", self.id, self.digest)
    }
}

// ------------------------------------------------------------------ records

/// A standard reference: designator, edition and clause only (REQ-002). There is no field for
/// standards text, and each part is at most 64 bytes, so none can be stored.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StandardRef {
    /// For example `ISO 10360-12`.
    pub designator: String,
    /// For example `2016`.
    pub edition: String,
    /// For example `6.2`.
    pub clause: String,
}

/// One maximum-permissible-error term (REQ-002). `value` is the limit the source states, not a
/// measured error.
#[derive(Debug, Clone, PartialEq)]
pub struct MpeTerm {
    pub standard: StandardRef,
    /// The term symbol, `[A-Za-z][A-Za-z0-9_.]*`, at most 32 bytes (for example `E_UNI`).
    pub symbol: String,
    /// Non-negative and finite, in an SI unit.
    pub value: Quantity,
    pub contributor: ContributorId,
}

/// One frame of the instrument's frame chain, from the sensor outward.
///
/// The chain starts at a `sensor` frame and its kinds never go back along sensor → mount →
/// device → machine (a kind may repeat, for example two mounts). `part` frames belong to a
/// session, not to an instrument, and are refused. Names are lowercase and unique within the
/// chain. Session frame IDs (`HAL.IDL.UNITS` `FrameId`) are derived from these names when a
/// session opens; transforms are calibration data and are not part of a profile.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProfileFrame {
    pub name: String,
    pub kind: FrameKind,
}

/// A closed interval of one quantity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    pub lower: Quantity,
    pub upper: Quantity,
}

/// Where the profile's figures hold (REQ-005): the measuring range (metres or radians) and the
/// ambient temperature (kelvin). Lower ≤ upper for both, in one unit each.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ValidityEnvelope {
    pub range: Limits,
    pub temperature: Limits,
}

/// What the profile's figures come from (REQ-003).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SourceKind {
    /// A maker's published specification for the model line.
    SpecSheet,
    /// An acceptance-test certificate of one instrument.
    AcceptanceCertificate,
    /// An instrument error model parameter set.
    Iemps,
}

impl SourceKind {
    /// Every kind, in declaration order.
    pub const ALL: [SourceKind; 3] = [SourceKind::SpecSheet, SourceKind::AcceptanceCertificate, SourceKind::Iemps];

    /// The wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            SourceKind::SpecSheet => "spec-sheet",
            SourceKind::AcceptanceCertificate => "acceptance-certificate",
            SourceKind::Iemps => "iemps",
        }
    }

    /// Parses the wire string, case-sensitively.
    pub fn parse(s: &str) -> Option<SourceKind> {
        SourceKind::ALL.into_iter().find(|k| k.as_str() == s)
    }

    /// Whether the figures are only a maker's declared claim, with no test of any instrument
    /// behind them (D-045: a declared capability never looks characterized or measured). Such
    /// a profile is capped at UEC-1.
    pub fn is_declared(self) -> bool {
        self == SourceKind::SpecSheet
    }

    /// The highest `uec_max` this source allows: UEC-1 for a spec sheet (REQ-004), UEC-3
    /// otherwise. Whether `acceptance-certificate` sources get a lower cap is a UNC decision
    /// (see the leaf's open questions).
    pub fn uec_cap(self) -> u8 {
        if self.is_declared() {
            SPEC_SHEET_UEC_CAP
        } else {
            MAX_UEC
        }
    }
}

impl fmt::Display for SourceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The source block (REQ-003): kind, the digest of the source document, and a citation kept as
/// untrusted text (never interpreted).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProfileSource {
    pub kind: SourceKind,
    pub digest: Digest,
    pub citation: UntrustedText,
}

/// The signature block, opaque here: a signer key reference and a padded standard base64
/// signature value, the shape of a DSSE signature entry. Only presence and shape are checked;
/// `TRUST` verifies it. It is excluded from the digest (REQ-006).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SignatureBlock {
    /// The signer reference: 1 to 256 bytes, no control characters.
    pub keyid: String,
    /// Padded standard base64 (RFC 4648 §4), 1 to 8192 characters.
    pub sig: String,
}

/// An `instrument-profile/v1` descriptor (REQ-001). Build one with [`super::validate`]; a
/// value assembled in code is re-validated by [`InstrumentProfile::digest`] and
/// [`InstrumentProfile::canonical_bytes`].
#[derive(Debug, Clone, PartialEq)]
pub struct InstrumentProfile {
    pub profile_id: ProfileId,
    /// Capability names the source declares (for example `PortablePose`), strictly ascending.
    /// A declaration, not the result of negotiation (`HAL.IDL.NEG`) with any device.
    pub device_class: Vec<String>,
    pub make: UntrustedText,
    pub model: UntrustedText,
    pub mpe_terms: Vec<MpeTerm>,
    pub frame_chain: Vec<ProfileFrame>,
    pub validity: ValidityEnvelope,
    pub source: ProfileSource,
    pub um_model: ModelRef,
    /// The highest evidence class (0 to 3) a result using this profile may reach; a ceiling,
    /// never a class reached.
    pub uec_max: u8,
    pub signature: SignatureBlock,
}

impl InstrumentProfile {
    /// The wire value, signature included.
    pub fn to_value(&self) -> CValue {
        let mut o = self.unsigned_map();
        o.insert("signature".into(), obj([("keyid", st(&self.signature.keyid)), ("sig", st(&self.signature.sig))]));
        CValue::Obj(o)
    }

    /// The wire value without the `signature` member: the digest preimage.
    pub fn unsigned_value(&self) -> CValue {
        CValue::Obj(self.unsigned_map())
    }

    /// RFC 8785 canonical bytes of the whole profile, after re-validating it.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ProfileError> {
        let checked = super::validate_value(self.to_value())?;
        canonical(&checked.to_value())
    }

    /// RFC 8785 canonical bytes without the signature block, after re-validating the profile.
    pub fn unsigned_canonical_bytes(&self) -> Result<Vec<u8>, ProfileError> {
        let checked = super::validate_value(self.to_value())?;
        canonical(&checked.unsigned_value())
    }

    /// The descriptor digest (REQ-006): SHA-256 over the canonical JSON without the signature
    /// block, as `sha256:<hex>`. This is the sensor-model reference a stamp carries.
    pub fn digest(&self) -> Result<Digest, ProfileError> {
        self.unsigned_canonical_bytes().map(|b| digest_of(&b))
    }

    fn unsigned_map(&self) -> BTreeMap<String, CValue> {
        let terms = self
            .mpe_terms
            .iter()
            .map(|t| {
                obj([
                    ("contributor", st(t.contributor.as_str())),
                    (
                        "standard",
                        obj([
                            ("clause", st(&t.standard.clause)),
                            ("designator", st(&t.standard.designator)),
                            ("edition", st(&t.standard.edition)),
                        ]),
                    ),
                    ("symbol", st(&t.symbol)),
                    ("value", t.value.to_value()),
                ])
            })
            .collect();
        let frames =
            self.frame_chain.iter().map(|f| obj([("kind", st(f.kind.as_str())), ("name", st(&f.name))])).collect();
        let limits = |l: &Limits| obj([("lower", l.lower.to_value()), ("upper", l.upper.to_value())]);
        let pairs = [
            ("device_class", CValue::Arr(self.device_class.iter().map(|c| st(c)).collect())),
            ("frame_chain", CValue::Arr(frames)),
            ("make", st(self.make.raw())),
            ("model", st(self.model.raw())),
            ("mpe_terms", CValue::Arr(terms)),
            ("profile_id", st(self.profile_id.as_str())),
            ("schema", st(SCHEMA_TAG)),
            (
                "source",
                obj([
                    ("citation", st(self.source.citation.raw())),
                    ("digest", st(self.source.digest.as_str())),
                    ("kind", st(self.source.kind.as_str())),
                ]),
            ),
            ("uec_max", CValue::Num(f64::from(self.uec_max))),
            ("um_model", obj([("digest", st(self.um_model.digest.as_str())), ("id", st(self.um_model.id()))])),
            (
                "validity",
                obj([("range", limits(&self.validity.range)), ("temperature", limits(&self.validity.temperature))]),
            ),
        ];
        pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
    }

    // -------------------------------------------------------------- reading

    pub(crate) fn read(value: CValue) -> Result<InstrumentProfile, ProfileError> {
        let CValue::Obj(mut o) = value else {
            return Err(ProfileError::new(K::ValueInvalid, ""));
        };
        // The tag first, so a future major version reports its version, not its new fields.
        let tag = o.remove("schema").ok_or_else(|| ProfileError::new(K::FieldMissing, "/schema"))?;
        check_tag(tag)?;
        let mut r = Rd::from_obj(o, "", &FIELDS)?;
        // Declared fields in sorted key order (the module's error order).
        let device_class = read_device_class(r.take("device_class")?)?;
        let frame_chain = read_frame_chain(r.take("frame_chain")?)?;
        let make = untrusted(r.take("make")?)?;
        let model = untrusted(r.take("model")?)?;
        let mpe_terms = read_mpe_terms(r.take("mpe_terms")?)?;
        let (v, p) = r.take("profile_id")?;
        let profile_id = ProfileId::parse(&string(v, &p)?).map_err(|_| ProfileError::new(K::ValueInvalid, &p))?;
        let signature = read_signature(r.take("signature")?)?;
        let source = read_source(r.take("source")?)?;
        let uec_max = read_uec(r.take("uec_max")?)?;
        let um_model = read_model_ref(r.take("um_model")?)?;
        let validity = read_validity(r.take("validity")?)?;
        Ok(InstrumentProfile {
            profile_id,
            device_class,
            make,
            model,
            mpe_terms,
            frame_chain,
            validity,
            source,
            um_model,
            uec_max,
            signature,
        })
    }

    /// The cross-field rules, after the profile is structurally valid.
    pub(crate) fn semantic(&self) -> Result<(), ProfileError> {
        check_range(&self.validity.range)?;
        check_temperature(&self.validity.temperature)?;
        if self.uec_max > self.source.kind.uec_cap() {
            return Err(ProfileError::new(K::UecExceedsSource, "/uec_max"));
        }
        Ok(())
    }
}

/// The top-level fields other than `schema`, sorted.
const FIELDS: [&str; 11] = [
    "device_class",
    "frame_chain",
    "make",
    "model",
    "mpe_terms",
    "profile_id",
    "signature",
    "source",
    "uec_max",
    "um_model",
    "validity",
];

fn check_tag(tag: CValue) -> Result<(), ProfileError> {
    let s = string(tag, "/schema")?;
    if s == SCHEMA_TAG {
        return Ok(());
    }
    let other_major = s
        .strip_prefix("instrument-profile/v")
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()) && !n.starts_with('0'));
    let kind = if other_major { K::SchemaVersionUnsupported } else { K::ValueInvalid };
    Err(ProfileError::new(kind, "/schema"))
}

fn check_range(l: &Limits) -> Result<(), ProfileError> {
    let unit = l.lower.unit();
    // Quantities are finite, so `<=` is a total order here.
    let ok =
        unit == l.upper.unit() && matches!(unit, SiUnit::Metre | SiUnit::Radian) && l.lower.value() <= l.upper.value();
    if ok {
        Ok(())
    } else {
        Err(ProfileError::new(K::EnvelopeInvalid, "/validity/range"))
    }
}

fn check_temperature(l: &Limits) -> Result<(), ProfileError> {
    let ok = l.lower.unit() == SiUnit::Kelvin
        && l.upper.unit() == SiUnit::Kelvin
        && l.lower.value() >= 0.0
        && l.lower.value() <= l.upper.value();
    if ok {
        Ok(())
    } else {
        Err(ProfileError::new(K::EnvelopeInvalid, "/validity/temperature"))
    }
}

// ------------------------------------------------------------------ reader helpers

/// An object being read, with its JSON pointer.
struct Rd {
    o: BTreeMap<String, CValue>,
    ptr: String,
}

impl Rd {
    /// An object whose keys are all in `fields` (sorted); else the first unknown key.
    fn new(v: (CValue, String), fields: &[&str]) -> Result<Rd, ProfileError> {
        let (v, ptr) = v;
        let CValue::Obj(o) = v else {
            return Err(ProfileError::new(K::ValueInvalid, &ptr));
        };
        Rd::from_obj(o, &ptr, fields)
    }

    fn from_obj(o: BTreeMap<String, CValue>, ptr: &str, fields: &[&str]) -> Result<Rd, ProfileError> {
        if let Some(k) = o.keys().find(|k| !fields.contains(&k.as_str())) {
            return Err(ProfileError::new(K::FieldUnknown, &child(ptr, k)));
        }
        Ok(Rd { o, ptr: ptr.to_string() })
    }

    /// Removes a declared field, with its pointer; absent is `profile_field_missing`.
    fn take(&mut self, field: &str) -> Result<(CValue, String), ProfileError> {
        let p = child(&self.ptr, field);
        match self.o.remove(field) {
            Some(v) => Ok((v, p)),
            None => Err(ProfileError::new(K::FieldMissing, &p)),
        }
    }
}

fn st(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn canonical(v: &CValue) -> Result<Vec<u8>, ProfileError> {
    fid_canon::to_canonical_bytes(v).map_err(|_| ProfileError::new(K::ValueInvalid, ""))
}

fn string(v: CValue, ptr: &str) -> Result<String, ProfileError> {
    match v {
        CValue::Str(s) => Ok(s),
        _ => Err(ProfileError::new(K::ValueInvalid, ptr)),
    }
}

/// A short printable token: 1 to `max` bytes, no control characters.
fn token((v, p): (CValue, String), max: usize) -> Result<String, ProfileError> {
    let s = string(v, &p)?;
    if s.is_empty() || s.len() > max || s.chars().any(char::is_control) {
        return Err(ProfileError::new(K::ValueInvalid, &p));
    }
    Ok(s)
}

fn untrusted((v, p): (CValue, String)) -> Result<UntrustedText, ProfileError> {
    let s = string(v, &p)?;
    if s.is_empty() {
        return Err(ProfileError::new(K::ValueInvalid, &p));
    }
    if s.len() > MAX_UNTRUSTED_TEXT_BYTES {
        return Err(ProfileError::new(K::TextTooLong, &p));
    }
    Ok(UntrustedText::new(&s))
}

fn digest((v, p): (CValue, String)) -> Result<Digest, ProfileError> {
    Digest::parse(&string(v, &p)?).map_err(|_| ProfileError::new(K::ValueInvalid, &p))
}

fn array((v, p): (CValue, String), min: usize, max: usize) -> Result<(Vec<CValue>, String), ProfileError> {
    match v {
        CValue::Arr(a) if a.len() >= min && a.len() <= max => Ok((a, p)),
        _ => Err(ProfileError::new(K::ValueInvalid, &p)),
    }
}

fn quantity(v: (CValue, String)) -> Result<Quantity, ProfileError> {
    let mut r = Rd::new(v, &["unit", "value"])?;
    let (u, up) = r.take("unit")?;
    let unit = SiUnit::parse(&string(u, &up)?).map_err(|_| ProfileError::new(K::UnitUnsupported, &up))?;
    let (val, vp) = r.take("value")?;
    let value = f64_bits("value", val).map_err(|_| ProfileError::new(K::ValueInvalid, &vp))?;
    Quantity::new(value, unit).map_err(|_| ProfileError::new(K::QuantityNotFinite, &vp))
}

fn limits(v: (CValue, String)) -> Result<Limits, ProfileError> {
    let mut r = Rd::new(v, &["lower", "upper"])?;
    let lower = quantity(r.take("lower")?)?;
    let upper = quantity(r.take("upper")?)?;
    Ok(Limits { lower, upper })
}

/// `true` for a capability name: `[A-Z][A-Za-z0-9]*`, at most 64 bytes (the `HAL.IDL.STAMP`
/// capability-name pattern).
fn is_capability_name(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty() && b.len() <= 64 && b[0].is_ascii_uppercase() && b.iter().all(u8::is_ascii_alphanumeric)
}

fn read_device_class(v: (CValue, String)) -> Result<Vec<String>, ProfileError> {
    let (items, p) = array(v, 1, MAX_DEVICE_CLASS)?;
    let mut out: Vec<String> = Vec::with_capacity(items.len());
    for (i, item) in items.into_iter().enumerate() {
        let ip = child(&p, &i.to_string());
        let s = string(item, &ip)?;
        if !is_capability_name(&s) {
            return Err(ProfileError::new(K::ValueInvalid, &ip));
        }
        out.push(s);
    }
    if out.windows(2).any(|w| w[0] >= w[1]) {
        return Err(ProfileError::new(K::ValueInvalid, &p));
    }
    Ok(out)
}

fn read_frame_chain(v: (CValue, String)) -> Result<Vec<ProfileFrame>, ProfileError> {
    let (items, p) = array(v, 1, MAX_FRAMES)?;
    let mut out: Vec<ProfileFrame> = Vec::with_capacity(items.len());
    let mut names = BTreeSet::new();
    for (i, item) in items.into_iter().enumerate() {
        let mut r = Rd::new((item, child(&p, &i.to_string())), &["kind", "name"])?;
        let (k, kp) = r.take("kind")?;
        let kind = FrameKind::parse(&string(k, &kp)?).map_err(|_| ProfileError::new(K::ValueInvalid, &kp))?;
        let first_ok = i > 0 || kind == FrameKind::Sensor;
        let order_ok = out.last().map_or(true, |prev| prev.kind <= kind);
        if kind == FrameKind::Part || !first_ok || !order_ok {
            return Err(ProfileError::new(K::ValueInvalid, &kp));
        }
        let (n, np) = r.take("name")?;
        let name = string(n, &np)?;
        if !is_name(&name) || !names.insert(name.clone()) {
            return Err(ProfileError::new(K::ValueInvalid, &np));
        }
        out.push(ProfileFrame { name, kind });
    }
    Ok(out)
}

/// `true` for an MPE term symbol: `[A-Za-z][A-Za-z0-9_.]*`, at most 32 bytes.
fn is_symbol(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= MAX_SYMBOL_BYTES
        && b[0].is_ascii_alphabetic()
        && b.iter().all(|&c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.'))
}

fn read_mpe_terms(v: (CValue, String)) -> Result<Vec<MpeTerm>, ProfileError> {
    let (items, p) = array(v, 0, MAX_MPE_TERMS)?;
    let mut out = Vec::with_capacity(items.len());
    for (i, item) in items.into_iter().enumerate() {
        let mut r = Rd::new((item, child(&p, &i.to_string())), &["contributor", "standard", "symbol", "value"])?;
        let (c, cp) = r.take("contributor")?;
        let contributor =
            ContributorId::parse(&string(c, &cp)?).map_err(|_| ProfileError::new(K::ValueInvalid, &cp))?;
        let mut s = Rd::new(r.take("standard")?, &["clause", "designator", "edition"])?;
        let clause = token(s.take("clause")?, MAX_TOKEN_BYTES)?;
        let designator = token(s.take("designator")?, MAX_TOKEN_BYTES)?;
        let edition = token(s.take("edition")?, MAX_TOKEN_BYTES)?;
        let (sy, sp) = r.take("symbol")?;
        let symbol = string(sy, &sp)?;
        if !is_symbol(&symbol) {
            return Err(ProfileError::new(K::ValueInvalid, &sp));
        }
        let (qv, qp) = r.take("value")?;
        let value = quantity((qv, qp.clone()))?;
        // An MPE is a limit: never negative, and -0 has a second spelling of 0.
        if value.value().is_sign_negative() {
            return Err(ProfileError::new(K::ValueInvalid, &child(&qp, "value")));
        }
        out.push(MpeTerm { standard: StandardRef { designator, edition, clause }, symbol, value, contributor });
    }
    Ok(out)
}

/// `true` for padded standard base64 (RFC 4648 §4): a non-empty multiple of four characters,
/// with at most two `=` and only at the end.
fn is_base64(s: &str) -> bool {
    let b = s.as_bytes();
    if b.is_empty() || b.len() % 4 != 0 || b.len() > MAX_SIG_CHARS {
        return false;
    }
    let body = s.trim_end_matches('=');
    b.len() - body.len() <= 2 && body.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'+' || c == b'/')
}

fn read_signature(v: (CValue, String)) -> Result<SignatureBlock, ProfileError> {
    let mut r = Rd::new(v, &["keyid", "sig"])?;
    let keyid = token(r.take("keyid")?, MAX_KEYID_BYTES)?;
    let (s, sp) = r.take("sig")?;
    let sig = string(s, &sp)?;
    if !is_base64(&sig) {
        return Err(ProfileError::new(K::ValueInvalid, &sp));
    }
    Ok(SignatureBlock { keyid, sig })
}

fn read_source(v: (CValue, String)) -> Result<ProfileSource, ProfileError> {
    let mut r = Rd::new(v, &["citation", "digest", "kind"])?;
    let citation = untrusted(r.take("citation")?)?;
    let digest = digest(r.take("digest")?)?;
    let (k, kp) = r.take("kind")?;
    let kind = SourceKind::parse(&string(k, &kp)?).ok_or_else(|| ProfileError::new(K::ValueInvalid, &kp))?;
    Ok(ProfileSource { kind, digest, citation })
}

fn read_uec((v, p): (CValue, String)) -> Result<u8, ProfileError> {
    let CValue::Num(f) = v else {
        return Err(ProfileError::new(K::ValueInvalid, &p));
    };
    (0..=MAX_UEC).find(|&u| f64::from(u) == f).ok_or_else(|| ProfileError::new(K::ValueInvalid, &p))
}

fn read_model_ref(v: (CValue, String)) -> Result<ModelRef, ProfileError> {
    let mut r = Rd::new(v, &["digest", "id"])?;
    let digest = digest(r.take("digest")?)?;
    let (id, ip) = r.take("id")?;
    ModelRef::new(&string(id, &ip)?, digest).map_err(|_| ProfileError::new(K::ValueInvalid, &ip))
}

fn read_validity(v: (CValue, String)) -> Result<ValidityEnvelope, ProfileError> {
    let mut r = Rd::new(v, &["range", "temperature"])?;
    let range = limits(r.take("range")?)?;
    let temperature = limits(r.take("temperature")?)?;
    Ok(ValidityEnvelope { range, temperature })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_versioned_ids() {
        assert!(is_name("arm-demo-01") && is_name("0") && is_name("a.b_c-d"));
        for bad in ["", "Arm", "-arm", "arm demo", "arm/x", &"a".repeat(65)] {
            assert!(!is_name(bad), "{bad:?}");
        }
        assert_eq!(split_versioned("uc:x@1.2.3", "uc:"), Some(("x", "1.2.3")));
        assert_eq!(split_versioned("im:x@1.2.3", "uc:"), None);
        assert_eq!(split_versioned("x@1.2", ""), None);
        assert_eq!(split_versioned("x@1.2.3@4.5.6", ""), None);
        let parts =
            ProfileId::parse("arm-demo-01@1.0.0-rc.1").map(|id| (id.name().to_owned(), id.version().to_owned()));
        assert_eq!(parts, Ok(("arm-demo-01".to_owned(), "1.0.0-rc.1".to_owned())));
    }

    #[test]
    fn base64_shape() {
        for ok in ["QUJD", "QUI=", "QQ==", "ab+/"] {
            assert!(is_base64(ok), "{ok}");
        }
        for bad in ["", "QUJ", "Q===", "QU=D", "QUJD\n", "ab-_", "QUJD====", "=QUJ"] {
            assert!(!is_base64(bad), "{bad:?}");
        }
    }

    #[test]
    fn spec_sheet_is_declared_and_capped() {
        assert!(SourceKind::SpecSheet.is_declared());
        assert!(!SourceKind::AcceptanceCertificate.is_declared() && !SourceKind::Iemps.is_declared());
        assert_eq!(SourceKind::SpecSheet.uec_cap(), 1);
        assert_eq!(SourceKind::Iemps.uec_cap(), MAX_UEC);
        for k in SourceKind::ALL {
            assert_eq!(SourceKind::parse(k.as_str()), Some(k));
        }
        assert_eq!(SourceKind::parse("Spec-Sheet"), None);
    }
}
