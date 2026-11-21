//! Offline trust roots, revocation and the signature check (spec node `TRUST.SIGN.TROOT`).
//!
//! Answers "is this signature valid **and** from a key I trust, as of when?" from local
//! files only:
//!
//! - [`TrustRoot`] (`trust-root.json`): trusted keys as PEM SubjectPublicKeyInfo with a
//!   label, the fingerprint, `not_before` and an optional `not_after` (RFC 3339 UTC).
//! - [`Revocations`] (`revoked.json`): fingerprint, `revoked_at` and reason. Rotation is a
//!   new key in the trust root plus a revocation of the old one.
//! - [`Keyring`] (optional): keys that are known but not trusted, so a valid signature by
//!   such a key can be reported `VALID_UNTRUSTED`. A Result Package carries no public keys,
//!   so without a listed key a signature cannot be checked at all (`UNKNOWN_KEY`).
//!
//! All three files are parsed with bounds (16 MiB, [`MAX_KEYS`] entries, duplicate JSON
//! members, unknown fields and duplicate fingerprints refused) and every failure is
//! `troot.malformed` with a reason ([`TrootError`]); a listed fingerprint that does not match
//! its key is refused too.
//!
//! [`check_envelope`] classifies each signature of a DSSE envelope found in a package. The
//! envelope payload must be bound to the package's `statement.json`: either it *is* those
//! bytes, or it is a signature statement (in-toto Statement v1 with the single subject
//! `statement.json` at the statement's SHA-256, the multi-signer form of `TRUST.ESIG.MULTI`)
//! whose predicate carries `signed_at`. The time rules use that signer-asserted time, never
//! the verifier's clock, so the report names it `claimed_signed_at` (D-045: a claimed value
//! never looks measured); a payload that is the bare statement has no signing time and can
//! at most be `VALID_UNTRUSTED`.
//!
//! `UNKNOWN_KEY` and `INVALID` both fail closed: neither ever counts toward a passing
//! verification (maintainer decision on #408). The work per package is bounded before any
//! cryptography runs; see [`check_envelope`].
//!
//! The command `verify.check_signature@1` with its CLI and MCP surfaces is [`command`].
//! Everything here is D0: no clock, no network, no file system (the command reads the
//! files through the caller's opener).

pub mod command;
mod time;

pub use command::{
    check_package, check_signature, declarations, invoke_cli, invoke_mcp, CheckError, CheckParams, CliOutput,
    PackageStatus, SignatureReport, CLI_CHECK, COMMAND_ID, MAX_SIGNATURE_ENTRIES, MCP_CHECK,
};
pub use time::UtcTime;

use std::fmt;

use fid_registry::FidSchema;
use fid_trust_canon::Value;
use sha2::{Digest as _, Sha256};

use crate::{Envelope, Fingerprint, PublicKey, IN_TOTO_PAYLOAD_TYPE};

/// Largest accepted trust-root, revocation or keyring file, in bytes (REQ-TRUST.SIGN.TROOT-005).
pub const MAX_FILE_BYTES: usize = 16 * 1024 * 1024;
/// Most keys or revocation entries in one file.
pub const MAX_KEYS: usize = 10_000;
/// Longest key label, in bytes.
pub const MAX_LABEL_BYTES: usize = 256;
/// Longest revocation reason, in bytes.
pub const MAX_REASON_BYTES: usize = 4096;
/// Longest claimed meaning code echoed from a signature statement, in bytes.
pub const MAX_MEANING_BYTES: usize = 256;
/// `_type` of an in-toto Statement v1.
pub const STATEMENT_TYPE: &str = "https://in-toto.io/Statement/v1";
/// The one subject name of a signature statement.
pub const SIGNED_SUBJECT: &str = "statement.json";
/// `predicateType` of a signature statement, under the internal working name. Owned by
/// `TRUST.ESIG.BIND`, which may rename it (renamed globally at publication); any other
/// predicate type is `INVALID` with detail `predicate_type`.
pub const SIGNATURE_PREDICATE_TYPE: &str = "https://metrologymaven.io/signature/predicate/v1";
/// Largest payload parsed as a signature statement, in bytes (the 1 MiB bound of
/// `TRUST.ESIG.BIND`); a larger payload that is not the statement itself is
/// `payload_too_large`, refused before parsing or verification.
pub const MAX_SIGNATURE_STATEMENT_BYTES: usize = 1024 * 1024;
/// Most signatures checked in one package, over all envelopes; more is `sig.malformed`.
pub const MAX_SIGNATURES_PER_PACKAGE: usize = 64;
/// An empty keyid is tried against the listed keys only when at most this many are listed
/// (trust root plus keyring); otherwise it is `UNKNOWN_KEY` `keyid_missing`, untried.
pub const MAX_KEYID_TRIALS: usize = 16;

/// `troot.malformed`: a trust-root, revocation or keyring file is refused. No signature is
/// reported when this happens (REQ-TRUST.SIGN.TROOT-005).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrootError {
    /// Stable reason: `io`, `too_large`, `not_json`, `schema`, `key`, `fingerprint_mismatch`,
    /// `duplicate_fingerprint`, `time` or `window`.
    pub reason: &'static str,
    /// Human-readable detail (which file, which entry).
    pub detail: String,
}

/// Every reason a [`TrootError`] can carry.
pub const TROOT_REASONS: [&str; 9] =
    ["io", "too_large", "not_json", "schema", "key", "fingerprint_mismatch", "duplicate_fingerprint", "time", "window"];

impl TrootError {
    /// Stable machine-readable code.
    pub fn code(&self) -> &'static str {
        "troot.malformed"
    }

    fn new(reason: &'static str, detail: impl Into<String>) -> TrootError {
        TrootError { reason, detail: detail.into() }
    }
}

impl fmt::Display for TrootError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "troot.malformed ({}): {}", self.reason, self.detail)
    }
}

impl std::error::Error for TrootError {}

// ------------------------------------------------------------------ files

/// A trusted key with its validity window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedKey {
    pub label: String,
    pub key: PublicKey,
    pub fingerprint: Fingerprint,
    pub not_before: UtcTime,
    pub not_after: Option<UtcTime>,
}

/// The trust-root file: `{"keys":[{label, spki_pem, fingerprint, not_before, not_after?}]}`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TrustRoot {
    pub keys: Vec<TrustedKey>,
}

/// A known but untrusted key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownKey {
    pub label: String,
    pub key: PublicKey,
    pub fingerprint: Fingerprint,
}

/// The optional keyring file: `{"keys":[{label, spki_pem, fingerprint}]}`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Keyring {
    pub keys: Vec<KnownKey>,
}

/// One revocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revocation {
    pub fingerprint: Fingerprint,
    pub revoked_at: UtcTime,
    pub reason: String,
}

/// The revocation file: `{"entries":[{fingerprint, revoked_at, reason}]}`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Revocations {
    pub entries: Vec<Revocation>,
}

fn top(bytes: &[u8], what: &str, list: &str) -> Result<Vec<Value>, TrootError> {
    if bytes.len() > MAX_FILE_BYTES {
        return Err(TrootError::new("too_large", format!("{what}: larger than 16 MiB")));
    }
    let members = match fid_trust_canon::parse(bytes) {
        Ok(Value::Object(m)) => m,
        Ok(_) => return Err(TrootError::new("schema", format!("{what}: not a JSON object"))),
        Err(e) => return Err(TrootError::new("not_json", format!("{what}: {e}"))),
    };
    let mut items = None;
    for (name, value) in members {
        match (name == list, value) {
            (true, Value::Array(a)) => items = Some(a),
            (true, _) => return Err(TrootError::new("schema", format!("{what}: `{list}` is not an array"))),
            (false, _) => return Err(TrootError::new("schema", format!("{what}: unknown member `{name}`"))),
        }
    }
    let items = items.ok_or_else(|| TrootError::new("schema", format!("{what}: `{list}` missing")))?;
    if items.len() > MAX_KEYS {
        return Err(TrootError::new("schema", format!("{what}: more than {MAX_KEYS} entries")));
    }
    Ok(items)
}

/// The string members of one entry object, in `fields` order; `None` for an absent optional
/// field. Unknown members, non-string values and absent required fields are `schema`.
fn fields(item: Value, at: &str, required: &[&str], optional: &[&str]) -> Result<Vec<Option<String>>, TrootError> {
    let Value::Object(members) = item else {
        return Err(TrootError::new("schema", format!("{at}: not an object")));
    };
    let names: Vec<&str> = required.iter().chain(optional).copied().collect();
    let mut out: Vec<Option<String>> = vec![None; names.len()];
    for (name, value) in members {
        let Some(i) = names.iter().position(|n| *n == name) else {
            return Err(TrootError::new("schema", format!("{at}: unknown member `{name}`")));
        };
        let Value::String(s) = value else {
            return Err(TrootError::new("schema", format!("{at}: `{name}` is not a string")));
        };
        if let Some(slot) = out.get_mut(i) {
            *slot = Some(s);
        }
    }
    for (i, name) in required.iter().enumerate() {
        if !matches!(out.get(i), Some(Some(_))) {
            return Err(TrootError::new("schema", format!("{at}: `{name}` missing")));
        }
    }
    Ok(out)
}

fn label(text: String, at: &str) -> Result<String, TrootError> {
    if text.is_empty() || text.len() > MAX_LABEL_BYTES || text.chars().any(char::is_control) {
        return Err(TrootError::new(
            "schema",
            format!("{at}: label must be 1 to 256 bytes without control characters"),
        ));
    }
    Ok(text)
}

fn fingerprint(text: &str, at: &str) -> Result<Fingerprint, TrootError> {
    Fingerprint::parse(text)
        .ok_or_else(|| TrootError::new("schema", format!("{at}: fingerprint is not 64 lowercase hex digits")))
}

fn utc(text: &str, at: &str, name: &str) -> Result<UtcTime, TrootError> {
    UtcTime::parse(text).ok_or_else(|| TrootError::new("time", format!("{at}: `{name}` is not an RFC 3339 UTC time")))
}

/// Parses the PEM key and checks the listed fingerprint against it.
fn listed_key(pem: &str, fp: &str, at: &str) -> Result<(PublicKey, Fingerprint), TrootError> {
    let listed = fingerprint(fp, at)?;
    let key = PublicKey::from_pem(pem).map_err(|e| TrootError::new("key", format!("{at}: {e}")))?;
    let actual = key.fingerprint();
    if actual != listed {
        return Err(TrootError::new(
            "fingerprint_mismatch",
            format!("{at}: listed fingerprint {listed} but the key's is {actual}"),
        ));
    }
    Ok((key, actual))
}

fn no_duplicates<'a>(fps: impl Iterator<Item = &'a Fingerprint>, what: &str) -> Result<(), TrootError> {
    let mut seen: Vec<&Fingerprint> = fps.collect();
    seen.sort();
    if let Some(w) = seen.windows(2).find(|w| w.first() == w.get(1)) {
        let fp = w.first().map(|f| f.as_str()).unwrap_or_default();
        return Err(TrootError::new("duplicate_fingerprint", format!("{what}: fingerprint {fp} listed twice")));
    }
    Ok(())
}

fn take(v: &mut [Option<String>], i: usize) -> String {
    v.get_mut(i).and_then(Option::take).unwrap_or_default()
}

impl TrustRoot {
    /// Parses `trust-root.json` (REQ-TRUST.SIGN.TROOT-005).
    pub fn parse(bytes: &[u8]) -> Result<TrustRoot, TrootError> {
        let mut keys = Vec::new();
        for (i, item) in top(bytes, "trust root", "keys")?.into_iter().enumerate() {
            let at = format!("trust root key {i}");
            let mut f = fields(item, &at, &["label", "spki_pem", "fingerprint", "not_before"], &["not_after"])?;
            let label = label(take(&mut f, 0), &at)?;
            let (key, fingerprint) = listed_key(&take(&mut f, 1), &take(&mut f, 2), &at)?;
            let not_before = utc(&take(&mut f, 3), &at, "not_before")?;
            let not_after = match f.get_mut(4).and_then(Option::take) {
                Some(t) => Some(utc(&t, &at, "not_after")?),
                None => None,
            };
            if not_after.as_ref().is_some_and(|na| *na < not_before) {
                return Err(TrootError::new("window", format!("{at}: not_after is before not_before")));
            }
            keys.push(TrustedKey { label, key, fingerprint, not_before, not_after });
        }
        no_duplicates(keys.iter().map(|k| &k.fingerprint), "trust root")?;
        Ok(TrustRoot { keys })
    }

    /// The trusted key with this fingerprint.
    pub fn find(&self, fp: &Fingerprint) -> Option<&TrustedKey> {
        self.keys.iter().find(|k| k.fingerprint == *fp)
    }
}

impl Keyring {
    /// Parses a keyring file.
    pub fn parse(bytes: &[u8]) -> Result<Keyring, TrootError> {
        let mut keys = Vec::new();
        for (i, item) in top(bytes, "keyring", "keys")?.into_iter().enumerate() {
            let at = format!("keyring key {i}");
            let mut f = fields(item, &at, &["label", "spki_pem", "fingerprint"], &[])?;
            let label = label(take(&mut f, 0), &at)?;
            let (key, fingerprint) = listed_key(&take(&mut f, 1), &take(&mut f, 2), &at)?;
            keys.push(KnownKey { label, key, fingerprint });
        }
        no_duplicates(keys.iter().map(|k| &k.fingerprint), "keyring")?;
        Ok(Keyring { keys })
    }
}

impl Revocations {
    /// Parses `revoked.json` (REQ-TRUST.SIGN.TROOT-005).
    pub fn parse(bytes: &[u8]) -> Result<Revocations, TrootError> {
        let mut entries = Vec::new();
        for (i, item) in top(bytes, "revocations", "entries")?.into_iter().enumerate() {
            let at = format!("revocation {i}");
            let mut f = fields(item, &at, &["fingerprint", "revoked_at", "reason"], &[])?;
            let fingerprint = fingerprint(&take(&mut f, 0), &at)?;
            let revoked_at = utc(&take(&mut f, 1), &at, "revoked_at")?;
            let reason = take(&mut f, 2);
            if reason.len() > MAX_REASON_BYTES {
                return Err(TrootError::new("schema", format!("{at}: reason longer than 4096 bytes")));
            }
            entries.push(Revocation { fingerprint, revoked_at, reason });
        }
        no_duplicates(entries.iter().map(|e| &e.fingerprint), "revocations")?;
        Ok(Revocations { entries })
    }

    /// The revocation of this fingerprint.
    pub fn find(&self, fp: &Fingerprint) -> Option<&Revocation> {
        self.entries.iter().find(|e| e.fingerprint == *fp)
    }
}

// ------------------------------------------------------------------ report types

/// Status of one signature (REQ-TRUST.SIGN.TROOT-001 to -003). `UNKNOWN_KEY` is an addition
/// to the spec's list: no listed key carries the signature's keyid, so it cannot be checked.
#[derive(FidSchema, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SignatureStatus {
    #[fid(rename = "VALID_TRUSTED")]
    ValidTrusted,
    #[fid(rename = "VALID_UNTRUSTED")]
    ValidUntrusted,
    #[fid(rename = "REVOKED")]
    Revoked,
    #[fid(rename = "EXPIRED")]
    Expired,
    #[fid(rename = "INVALID")]
    Invalid,
    #[fid(rename = "UNKNOWN_KEY")]
    UnknownKey,
}

impl SignatureStatus {
    /// Wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            SignatureStatus::ValidTrusted => "VALID_TRUSTED",
            SignatureStatus::ValidUntrusted => "VALID_UNTRUSTED",
            SignatureStatus::Revoked => "REVOKED",
            SignatureStatus::Expired => "EXPIRED",
            SignatureStatus::Invalid => "INVALID",
            SignatureStatus::UnknownKey => "UNKNOWN_KEY",
        }
    }
}

/// Every `detail` code a [`SignatureEntry`] can carry, sorted.
pub const DETAILS: [&str; 18] = [
    "after_not_after",
    "before_not_before",
    "key_fingerprint_mismatch",
    "key_not_listed",
    "key_not_trusted",
    "keyid_missing",
    "no_listed_key_verifies",
    "no_signing_time",
    "payload_not_bound",
    "payload_too_large",
    "payload_type",
    "predicate_type",
    "revoked_before_signing",
    "revoked_no_signing_time",
    "signature_invalid",
    "signed_at_invalid",
    "signed_at_missing",
    "statement_missing",
];

/// Every `notes` code a [`SignatureEntry`] can carry, sorted. Notes never change the status.
pub const NOTES: [&str; 2] = ["key_validity_ended_as_of", "revoked_after_claimed_signing"];

/// One signature of the package and its status.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct SignatureEntry {
    #[fid(description = "Position of the signature over all envelopes, in envelope-name order.")]
    pub index: u32,
    #[fid(description = "The package entry holding the envelope (under signatures/).")]
    pub envelope: String,
    #[fid(description = "The keyid written in the envelope, as claimed by the signer; may be empty.")]
    pub claimed_keyid: String,
    #[fid(
        description = "Fingerprint of the listed key this signature was matched to (computed from the key).",
        pattern = "^[0-9a-f]{64}$"
    )]
    pub fingerprint: Option<String>,
    #[fid(description = "Label of that key in the trust root or keyring.")]
    pub key_label: Option<String>,
    pub status: SignatureStatus,
    #[fid(description = "Why the status is not VALID_TRUSTED (one of the documented detail codes).")]
    pub detail: Option<String>,
    #[fid(
        description = "Facts that do not change the status (documented note codes), e.g. a revocation after the claimed signing time."
    )]
    pub notes: Vec<String>,
    #[fid(description = "The signing time the signed statement asserts (signer-asserted, not a trusted timestamp).")]
    pub claimed_signed_at: Option<String>,
    #[fid(description = "The meaning code the signed statement asserts.")]
    pub claimed_meaning_code: Option<String>,
}

// ------------------------------------------------------------------ the check

/// How an envelope's payload is bound to the package's `statement.json`.
enum Binding {
    /// The payload is the statement itself: no signing time.
    Direct,
    /// A signature statement over the statement.
    Signed { signed_at: UtcTime, key_fingerprint: Option<String>, meaning_code: Option<String> },
    /// Not bound; the detail code.
    Unbound(&'static str),
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(char::from(HEX[usize::from(b >> 4)]));
        out.push(char::from(HEX[usize::from(b & 15)]));
    }
    out
}

fn member<'a>(members: &'a [(String, Value)], name: &str) -> Option<&'a Value> {
    members.iter().find(|(k, _)| k == name).map(|(_, v)| v)
}

fn str_member<'a>(members: &'a [(String, Value)], name: &str) -> Option<&'a str> {
    match member(members, name) {
        Some(Value::String(s)) => Some(s),
        _ => None,
    }
}

/// Decides how the payload is bound before any signature is verified, so an unbound payload
/// costs one hash (and at most a 1 MiB parse), never a verification.
fn binding(envelope: &Envelope, statement_sha256: Option<&str>) -> Binding {
    let Some(statement) = statement_sha256 else { return Binding::Unbound("statement_missing") };
    if envelope.payload_type != IN_TOTO_PAYLOAD_TYPE {
        return Binding::Unbound("payload_type");
    }
    if hex(&Sha256::digest(&envelope.payload)) == statement {
        return Binding::Direct;
    }
    if envelope.payload.len() > MAX_SIGNATURE_STATEMENT_BYTES {
        return Binding::Unbound("payload_too_large");
    }
    let unbound = Binding::Unbound("payload_not_bound");
    let Ok(Value::Object(st)) = fid_trust_canon::parse(&envelope.payload) else { return unbound };
    if str_member(&st, "_type") != Some(STATEMENT_TYPE) {
        return unbound;
    }
    match str_member(&st, "predicateType") {
        Some(SIGNATURE_PREDICATE_TYPE) => {}
        Some(_) => return Binding::Unbound("predicate_type"),
        None => return unbound,
    }
    let subject_ok = match member(&st, "subject") {
        Some(Value::Array(subjects)) => match subjects.as_slice() {
            [Value::Object(s)] => {
                str_member(s, "name") == Some(SIGNED_SUBJECT)
                    && matches!(member(s, "digest"), Some(Value::Object(d)) if str_member(d, "sha256") == Some(statement))
            }
            _ => false,
        },
        _ => false,
    };
    if !subject_ok {
        return unbound;
    }
    let Some(Value::Object(pred)) = member(&st, "predicate") else { return unbound };
    let signed_at = match member(pred, "signed_at") {
        None => return Binding::Unbound("signed_at_missing"),
        Some(Value::String(s)) => match UtcTime::parse(s) {
            Some(t) => t,
            None => return Binding::Unbound("signed_at_invalid"),
        },
        Some(_) => return Binding::Unbound("signed_at_invalid"),
    };
    let key_fingerprint = match member(pred, "key_fingerprint") {
        None => None,
        Some(Value::String(s)) => Some(s.clone()),
        Some(_) => return unbound,
    };
    let meaning_code = match member(pred, "meaning_code") {
        None => None,
        Some(Value::String(s)) if s.len() <= MAX_MEANING_BYTES && !s.chars().any(char::is_control) => Some(s.clone()),
        Some(_) => return unbound,
    };
    Binding::Signed { signed_at, key_fingerprint, meaning_code }
}

/// The inputs a check is made against.
#[derive(Debug, Clone, Copy)]
pub struct TrustInputs<'a> {
    pub trust_root: &'a TrustRoot,
    pub revocations: &'a Revocations,
    pub keyring: &'a Keyring,
    /// A caller-supplied reference time (never the clock). When given, a signature whose key's
    /// `not_after` lies before it gets the note `key_validity_ended_as_of`.
    pub as_of: Option<&'a UtcTime>,
}

/// A listed key: trusted (with its window) or only known.
struct Candidate<'a> {
    label: &'a str,
    key: &'a PublicKey,
    fingerprint: &'a Fingerprint,
    trusted: Option<&'a TrustedKey>,
}

impl<'a> TrustInputs<'a> {
    /// Trust-root keys first, then keyring keys, each in file order.
    fn candidates(&self) -> impl Iterator<Item = Candidate<'a>> {
        let trusted = self.trust_root.keys.iter().map(|k| Candidate {
            label: &k.label,
            key: &k.key,
            fingerprint: &k.fingerprint,
            trusted: Some(k),
        });
        let known = self.keyring.keys.iter().map(|k| Candidate {
            label: &k.label,
            key: &k.key,
            fingerprint: &k.fingerprint,
            trusted: None,
        });
        trusted.chain(known)
    }

    fn listed(&self) -> usize {
        self.trust_root.keys.len().saturating_add(self.keyring.keys.len())
    }
}

/// Classifies every signature of `envelope` (the package entry `envelope_path`) against the
/// package statement digest (lowercase hex, `None` when the package has no `statement.json`)
/// and the trust inputs. `first_index` is the index of its first signature.
///
/// The work is bounded before any cryptography runs: the binding is decided first (an
/// unbound payload is `INVALID` with no verification; a payload that is not the statement
/// itself is parsed only up to [`MAX_SIGNATURE_STATEMENT_BYTES`]), a signature with a keyid is
/// verified against that one key, and an empty keyid is tried against the listed keys only
/// when there are at most [`MAX_KEYID_TRIALS`] of them (`UNKNOWN_KEY` `keyid_missing`
/// otherwise). The caller caps the signatures per package ([`MAX_SIGNATURES_PER_PACKAGE`]).
///
/// Per signature, in order: the binding; find the key; verify over PAE; the predicate
/// `key_fingerprint`; then the time rules: revoked at or before the claimed signing time is
/// `REVOKED`; outside the inclusive window `[not_before, not_after]` is `EXPIRED`; a key only
/// in the keyring, or no signing time, is `VALID_UNTRUSTED`. A key revoked after the claimed
/// time keeps its status and gets the note `revoked_after_claimed_signing`. Claimed values
/// (signing time, meaning code) are echoed only for a signature that verified, is bound to the
/// statement and whose predicate names its key.
///
/// Two envelopes may carry the same signature; each is reported. Catching a replayed signer
/// is `TRUST.ESIG.MULTI`'s job (its `sequence_index` and `prior` chain), not this check's.
pub fn check_envelope(
    envelope: &Envelope,
    envelope_path: &str,
    statement_sha256: Option<&str>,
    inputs: &TrustInputs<'_>,
    first_index: u32,
) -> Vec<SignatureEntry> {
    let bound = binding(envelope, statement_sha256);
    let message = match bound {
        Binding::Unbound(_) => Vec::new(),
        _ => envelope.pae(),
    };
    let mut out = Vec::with_capacity(envelope.signatures.len());
    for (i, sig) in envelope.signatures.iter().enumerate() {
        let mut entry = SignatureEntry {
            index: first_index.saturating_add(u32::try_from(i).unwrap_or(u32::MAX)),
            envelope: envelope_path.to_owned(),
            claimed_keyid: sig.keyid.clone(),
            fingerprint: None,
            key_label: None,
            status: SignatureStatus::UnknownKey,
            detail: None,
            notes: Vec::new(),
            claimed_signed_at: None,
            claimed_meaning_code: None,
        };
        let finish = |mut e: SignatureEntry, status, detail: Option<&str>| {
            e.status = status;
            e.detail = detail.map(str::to_owned);
            e
        };
        let by_keyid = (!sig.keyid.is_empty())
            .then(|| inputs.candidates().find(|c| c.fingerprint.as_str() == sig.keyid))
            .flatten();
        if let Binding::Unbound(detail) = &bound {
            // No verification: name the key only when the keyid lists one (a lookup).
            if let Some(c) = &by_keyid {
                entry.fingerprint = Some(c.fingerprint.as_str().to_owned());
                entry.key_label = Some(c.label.to_owned());
            }
            out.push(finish(entry, SignatureStatus::Invalid, Some(*detail)));
            continue;
        }
        let candidate = if sig.keyid.is_empty() {
            if inputs.listed() > MAX_KEYID_TRIALS {
                out.push(finish(entry, SignatureStatus::UnknownKey, Some("keyid_missing")));
                continue;
            }
            match inputs.candidates().find(|c| c.key.verify_raw(&message, &sig.sig).is_ok()) {
                Some(c) => c,
                None => {
                    out.push(finish(entry, SignatureStatus::UnknownKey, Some("no_listed_key_verifies")));
                    continue;
                }
            }
        } else {
            match by_keyid {
                Some(c) => c,
                None => {
                    out.push(finish(entry, SignatureStatus::UnknownKey, Some("key_not_listed")));
                    continue;
                }
            }
        };
        entry.fingerprint = Some(candidate.fingerprint.as_str().to_owned());
        entry.key_label = Some(candidate.label.to_owned());
        // With an empty keyid the search above already verified this key.
        if !sig.keyid.is_empty() && candidate.key.verify_raw(&message, &sig.sig).is_err() {
            out.push(finish(entry, SignatureStatus::Invalid, Some("signature_invalid")));
            continue;
        }
        let signed_at = match &bound {
            Binding::Signed { signed_at, key_fingerprint, meaning_code } => {
                if key_fingerprint.as_deref().is_some_and(|f| f != candidate.fingerprint.as_str()) {
                    out.push(finish(entry, SignatureStatus::Invalid, Some("key_fingerprint_mismatch")));
                    continue;
                }
                entry.claimed_signed_at = Some(signed_at.as_str().to_owned());
                entry.claimed_meaning_code = meaning_code.clone();
                Some(signed_at)
            }
            _ => None,
        };
        let (status, detail) = classify_time(candidate.fingerprint, candidate.trusted, signed_at, inputs.revocations);
        if status != SignatureStatus::Revoked && inputs.revocations.find(candidate.fingerprint).is_some() {
            entry.notes.push("revoked_after_claimed_signing".to_owned());
        }
        let ended = candidate.trusted.and_then(|k| k.not_after.as_ref());
        if let (Some(na), Some(now)) = (ended, inputs.as_of) {
            if na < now {
                entry.notes.push("key_validity_ended_as_of".to_owned());
            }
        }
        out.push(finish(entry, status, detail));
    }
    out
}

fn classify_time(
    fp: &Fingerprint,
    trusted: Option<&TrustedKey>,
    signed_at: Option<&UtcTime>,
    revocations: &Revocations,
) -> (SignatureStatus, Option<&'static str>) {
    if let Some(rv) = revocations.find(fp) {
        match signed_at {
            None => return (SignatureStatus::Revoked, Some("revoked_no_signing_time")),
            Some(t) if *t >= rv.revoked_at => return (SignatureStatus::Revoked, Some("revoked_before_signing")),
            Some(_) => {}
        }
    }
    let Some(key) = trusted else { return (SignatureStatus::ValidUntrusted, Some("key_not_trusted")) };
    let Some(t) = signed_at else { return (SignatureStatus::ValidUntrusted, Some("no_signing_time")) };
    if *t < key.not_before {
        return (SignatureStatus::Expired, Some("before_not_before"));
    }
    if key.not_after.as_ref().is_some_and(|na| t > na) {
        return (SignatureStatus::Expired, Some("after_not_after"));
    }
    (SignatureStatus::ValidTrusted, None)
}
