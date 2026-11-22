//! Checkpoint check and signed-checkpoint verification (spec node `TRUST.CHAIN.CKPT`).
//!
//! Parses `journal/checkpoints/<index>.json` following `CORE.JRN.CHAIN.CKPT`'s published
//! `linear-v1` schema (`schema/checkpoint.schema.json`), compares the checkpoint's `head`
//! digest against the recomputed entry hash at `seq` (`MATCH | MISMATCH | OUT_OF_RANGE`),
//! and verifies any attached DSSE v1.0 signature against `fid-sign-core::troot`.
//!
//! Malformed or unknown-version checkpoint files produce `ckpt.malformed` for that file
//! without stopping verification of other checkpoints (REQ-TRUST.CHAIN.CKPT-003).
//!
//! Zero CORE dependencies: canonicalisation uses `fid-trust-canon`, and signature checks
//! use `fid-sign-core`. Everything is D0 (no clock, no network, no filesystem).

use fid_sign_core::troot::{Keyring, Revocations, SignatureStatus, TrustInputs, TrustRoot, UtcTime, MAX_KEYID_TRIALS};
use fid_sign_core::{Envelope, Fingerprint, PublicKey};
use fid_trust_canon::{parse_with_limits, serialize, Limits, Value, MAX_DEPTH};
use fid_types::Digest;
use std::collections::BTreeMap;
use std::fmt;

/// The P0 checkpoint layout published by `CORE.JRN.CHAIN.CKPT` (`schema/checkpoint.schema.json`).
pub const FORMAT_LINEAR_V1: &str = "linear-v1";
/// DSSE `payloadType` for signed `linear-v1` checkpoints.
pub const CHECKPOINT_PAYLOAD_TYPE: &str = "application/vnd.metrologymaven.checkpoint+json";
/// Largest accepted checkpoint file (bare JSON or DSSE-wrapped), in bytes.
pub const MAX_CHECKPOINT_BYTES: usize = 64 * 1024;
/// Longest accepted journal stream name, in bytes (matches `schema/checkpoint.schema.json`).
pub const MAX_STREAM_BYTES: usize = 64;
/// Package directory prefix holding checkpoint files.
pub const CHECKPOINTS_PREFIX: &str = "journal/checkpoints/";
/// Stable error code for malformed or unknown-version checkpoint files.
pub const ERR_MALFORMED: &str = "ckpt.malformed";

/// Why a checkpoint was emitted (`schema/checkpoint.schema.json`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CheckpointReason {
    Signature,
    Export,
    Close,
}

impl CheckpointReason {
    /// Wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            CheckpointReason::Signature => "signature",
            CheckpointReason::Export => "export",
            CheckpointReason::Close => "close",
        }
    }

    /// Parses the wire string.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "signature" => Some(CheckpointReason::Signature),
            "export" => Some(CheckpointReason::Export),
            "close" => Some(CheckpointReason::Close),
            _ => None,
        }
    }
}

/// Optional Access Ledger head recorded inside a `linear-v1` checkpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerHead {
    pub seq: u64,
    pub head: Digest,
}

/// Parsed `linear-v1` checkpoint with its canonical body bytes and optional DSSE signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkpoint {
    pub format: String,
    pub stream: String,
    /// 1-based journal sequence index of the checkpointed entry.
    pub seq: u64,
    /// Expected SHA-256 entry hash at `seq`.
    pub head: Digest,
    /// Number of entries up to `seq` (always equals `seq` in `linear-v1`).
    pub count: u64,
    pub reason: CheckpointReason,
    /// Signer/writer-recorded whole-second UTC timestamp (`YYYY-MM-DDThh:mm:ssZ`).
    pub time: UtcTime,
    /// Exact raw timestamp string from the checkpoint.
    pub time_raw: String,
    pub ledger: Option<LedgerHead>,
    /// Canonical RFC 8785 bytes of the `linear-v1` checkpoint body (excluding any attached signature).
    pub canonical_body: Vec<u8>,
    /// Optional DSSE v1.0 signature envelope covering `canonical_body`.
    pub signature: Option<Envelope>,
}

/// Head-digest match verdict for one checkpoint (REQ-TRUST.CHAIN.CKPT-001, -003).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CheckpointStatus {
    Match,
    Mismatch,
    OutOfRange,
    Malformed,
}

impl CheckpointStatus {
    /// Wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            CheckpointStatus::Match => "MATCH",
            CheckpointStatus::Mismatch => "MISMATCH",
            CheckpointStatus::OutOfRange => "OUT_OF_RANGE",
            CheckpointStatus::Malformed => "MALFORMED",
        }
    }
}

/// Verification result for one signature on a checkpoint (REQ-TRUST.CHAIN.CKPT-002).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointSigResult {
    pub claimed_keyid: String,
    pub fingerprint: Option<String>,
    pub key_label: Option<String>,
    pub status: SignatureStatus,
    pub detail: Option<String>,
    pub notes: Vec<String>,
    /// Timestamp asserted inside the checkpoint body (D-045: claimed, not a trusted clock).
    pub claimed_signed_at: String,
}

/// Complete verification outcome for one checkpoint file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointResult {
    pub path: String,
    /// Checkpoint sequence index (`seq`), if parsing succeeded.
    pub index: Option<u64>,
    pub stream: Option<String>,
    pub head_sha256: Option<Digest>,
    pub status: CheckpointStatus,
    /// `Some("ckpt.malformed")` when `status == CheckpointStatus::Malformed`.
    pub error_code: Option<&'static str>,
    pub detail: Option<String>,
    /// Primary (first) signature status when the checkpoint carries a DSSE signature.
    pub signature: Option<CheckpointSigResult>,
    /// All signature results in envelope order when signed.
    pub signatures: Vec<CheckpointSigResult>,
}

/// Recomputed entry hashes of a journal segment keyed by 1-based sequence number.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SegmentHashes {
    by_seq: BTreeMap<u64, Digest>,
}

impl SegmentHashes {
    /// Builds a segment index from a contiguous slice starting at 1-based `start_seq`.
    pub fn from_contiguous(start_seq: u64, hashes: impl IntoIterator<Item = Digest>) -> Self {
        let mut by_seq = BTreeMap::new();
        for (offset, digest) in hashes.into_iter().enumerate() {
            if let Ok(off_u64) = u64::try_from(offset) {
                if let Some(seq) = start_seq.checked_add(off_u64) {
                    by_seq.insert(seq, digest);
                }
            }
        }
        SegmentHashes { by_seq }
    }

    /// Builds a segment index directly from a `(seq -> entry_hash)` map.
    pub fn from_map(by_seq: BTreeMap<u64, Digest>) -> Self {
        SegmentHashes { by_seq }
    }

    /// Looks up the recomputed entry hash at 1-based sequence `seq`.
    pub fn get(&self, seq: u64) -> Option<&Digest> {
        self.by_seq.get(&seq)
    }

    /// Number of entries in the segment.
    pub fn len(&self) -> usize {
        self.by_seq.len()
    }

    /// True if the segment holds no entries.
    pub fn is_empty(&self) -> bool {
        self.by_seq.is_empty()
    }
}

/// One raw checkpoint file passed to [`check_checkpoints`].
#[derive(Debug, Clone, Copy)]
pub struct CheckpointInput<'a> {
    pub path: &'a str,
    pub bytes: &'a [u8],
}

/// Parse error for a single checkpoint file (`ckpt.malformed`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CkptError {
    pub detail: String,
}

impl CkptError {
    /// Stable error code (`ckpt.malformed`).
    pub fn code(&self) -> &'static str {
        ERR_MALFORMED
    }

    fn malformed(detail: impl Into<String>) -> Self {
        CkptError { detail: detail.into() }
    }
}

impl fmt::Display for CkptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{ERR_MALFORMED}: {}", self.detail)
    }
}

impl std::error::Error for CkptError {}

// ------------------------------------------------------------------ parsing

/// Parses a checkpoint file (`journal/checkpoints/<index>.json`) under [`MAX_CHECKPOINT_BYTES`].
///
/// Accepts:
/// 1. A DSSE v1.0 [`Envelope`] (`{payload, payloadType, signatures}`) whose base64-decoded
///    `payload` is a canonical `linear-v1` checkpoint JSON object; or
/// 2. A `linear-v1` checkpoint JSON object (`schema/checkpoint.schema.json`), optionally
///    carrying a `"signature"` member holding a DSSE v1.0 [`Envelope`].
///
/// Any unknown `format` (or legacy `format_version`), missing/unknown member, non-decimal
/// sequence/count, invalid timestamp, or malformed DSSE envelope fails with `ckpt.malformed`.
pub fn parse_checkpoint(bytes: &[u8]) -> Result<Checkpoint, CkptError> {
    if bytes.len() > MAX_CHECKPOINT_BYTES {
        return Err(CkptError::malformed(format!("checkpoint exceeds maximum size of {MAX_CHECKPOINT_BYTES} bytes")));
    }
    let limits = Limits { max_bytes: MAX_CHECKPOINT_BYTES, max_depth: MAX_DEPTH };
    let root = parse_with_limits(bytes, limits).map_err(|e| CkptError::malformed(format!("invalid json: {e}")))?;
    let Value::Object(members) = root else {
        return Err(CkptError::malformed("checkpoint root must be a JSON object"));
    };

    // Detect DSSE envelope wrapper `{payload, payloadType, signatures}`.
    if is_dsse_envelope_shape(&members) {
        let env =
            Envelope::from_json(bytes).map_err(|e| CkptError::malformed(format!("invalid dsse envelope: {e}")))?;
        let payload_limits = Limits { max_bytes: MAX_CHECKPOINT_BYTES, max_depth: MAX_DEPTH };
        let payload_val = parse_with_limits(&env.payload, payload_limits)
            .map_err(|e| CkptError::malformed(format!("invalid checkpoint payload json: {e}")))?;
        let Value::Object(payload_members) = payload_val else {
            return Err(CkptError::malformed("checkpoint payload must be a JSON object"));
        };
        let mut ckpt = parse_linear_v1_body(payload_members, false)?;
        ckpt.signature = Some(env);
        return Ok(ckpt);
    }

    parse_linear_v1_body(members, true)
}

fn is_dsse_envelope_shape(members: &[(String, Value)]) -> bool {
    let has_payload = members.iter().any(|(k, _)| k == "payload");
    let has_payload_type = members.iter().any(|(k, _)| k == "payloadType");
    let has_signatures = members.iter().any(|(k, _)| k == "signatures");
    let has_format = members.iter().any(|(k, _)| k == "format" || k == "format_version");
    has_payload && has_payload_type && has_signatures && !has_format
}

fn parse_linear_v1_body(members: Vec<(String, Value)>, allow_embedded_sig: bool) -> Result<Checkpoint, CkptError> {
    // Check `format` first (matches CORE.JRN.CHAIN.CKPT's format-first gate and AC-4).
    match members.iter().find(|(k, _)| k == "format") {
        Some((_, Value::String(fmt))) if fmt == FORMAT_LINEAR_V1 => {}
        Some((_, Value::String(other))) => {
            return Err(CkptError::malformed(format!("unknown checkpoint format `{other}`")));
        }
        Some(_) => return Err(CkptError::malformed("`format` must be a string")),
        None => {
            if let Some((_, v)) = members.iter().find(|(k, _)| k == "format_version") {
                return Err(CkptError::malformed(format!("unsupported checkpoint format_version ({v:?})")));
            }
            return Err(CkptError::malformed("missing required field `format`"));
        }
    }

    let mut stream: Option<String> = None;
    let mut seq: Option<u64> = None;
    let mut head: Option<Digest> = None;
    let mut count: Option<u64> = None;
    let mut reason: Option<CheckpointReason> = None;
    let mut time_raw: Option<String> = None;
    let mut ledger: Option<LedgerHead> = None;
    let mut signature: Option<Envelope> = None;
    let mut body_members: Vec<(String, Value)> = Vec::with_capacity(members.len());

    for (k, v) in members {
        match k.as_str() {
            "format" => {
                body_members.push((k, v));
            }
            "stream" => {
                let Value::String(s) = &v else {
                    return Err(CkptError::malformed("`stream` must be a string"));
                };
                if !is_valid_stream(s) {
                    return Err(CkptError::malformed(format!("invalid `stream` `{s}`")));
                }
                stream = Some(s.clone());
                body_members.push((k, v));
            }
            "seq" => {
                let n = parse_seq_value(&v, "seq")?;
                seq = Some(n);
                body_members.push((k, v));
            }
            "count" => {
                let n = parse_seq_value(&v, "count")?;
                count = Some(n);
                body_members.push((k, v));
            }
            "head" => {
                let d = parse_digest_value(&v, "head")?;
                head = Some(d);
                body_members.push((k, v));
            }
            "reason" => {
                let Value::String(s) = &v else {
                    return Err(CkptError::malformed("`reason` must be a string"));
                };
                let parsed = CheckpointReason::parse(s)
                    .ok_or_else(|| CkptError::malformed(format!("unknown `reason` `{s}`")))?;
                reason = Some(parsed);
                body_members.push((k, v));
            }
            "time" => {
                let Value::String(s) = &v else {
                    return Err(CkptError::malformed("`time` must be a string"));
                };
                if !is_whole_second_utc(s) {
                    return Err(CkptError::malformed(format!("invalid whole-second UTC `time` `{s}`")));
                }
                time_raw = Some(s.clone());
                body_members.push((k, v));
            }
            "ledger" => {
                let lh = parse_ledger_value(&v)?;
                ledger = Some(lh);
                body_members.push((k, v));
            }
            "signature" if allow_embedded_sig => {
                let env_bytes =
                    serialize(&v).map_err(|e| CkptError::malformed(format!("cannot canonicalise `signature`: {e}")))?;
                let env = Envelope::from_json(&env_bytes)
                    .map_err(|e| CkptError::malformed(format!("invalid `signature` dsse envelope: {e}")))?;
                signature = Some(env);
            }
            other => {
                return Err(CkptError::malformed(format!("unexpected field `{other}` in linear-v1 checkpoint")));
            }
        }
    }

    let stream = stream.ok_or_else(|| CkptError::malformed("missing required field `stream`"))?;
    let seq = seq.ok_or_else(|| CkptError::malformed("missing required field `seq`"))?;
    let count = count.ok_or_else(|| CkptError::malformed("missing required field `count`"))?;
    if count != seq {
        return Err(CkptError::malformed(format!("`count` ({count}) must equal `seq` ({seq}) in linear-v1")));
    }
    let head = head.ok_or_else(|| CkptError::malformed("missing required field `head`"))?;
    let reason = reason.ok_or_else(|| CkptError::malformed("missing required field `reason`"))?;
    let time_raw = time_raw.ok_or_else(|| CkptError::malformed("missing required field `time`"))?;
    let time = UtcTime::parse(&time_raw)
        .ok_or_else(|| CkptError::malformed(format!("invalid calendar UTC `time` `{time_raw}`")))?;

    let canonical_body = serialize(&Value::Object(body_members))
        .map_err(|e| CkptError::malformed(format!("cannot canonicalise checkpoint body: {e}")))?;

    Ok(Checkpoint {
        format: FORMAT_LINEAR_V1.to_string(),
        stream,
        seq,
        head,
        count,
        reason,
        time,
        time_raw,
        ledger,
        canonical_body,
        signature,
    })
}

fn is_valid_stream(s: &str) -> bool {
    let b = s.as_bytes();
    (1..=MAX_STREAM_BYTES).contains(&b.len())
        && b.first().is_some_and(u8::is_ascii_lowercase)
        && b.iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
}

fn parse_seq_value(v: &Value, field: &str) -> Result<u64, CkptError> {
    let Value::String(s) = v else {
        return Err(CkptError::malformed(format!("`{field}` must be a decimal string")));
    };
    if s.is_empty() || s.starts_with('0') || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(CkptError::malformed(format!("`{field}` must be a positive decimal string without leading zeros")));
    }
    s.parse::<u64>().map_err(|_| CkptError::malformed(format!("`{field}` overflows u64")))
}

fn parse_digest_value(v: &Value, field: &str) -> Result<Digest, CkptError> {
    let Value::String(s) = v else {
        return Err(CkptError::malformed(format!("`{field}` must be a digest string")));
    };
    Digest::parse(s).map_err(|_| CkptError::malformed(format!("invalid sha256 digest in `{field}`")))
}

fn is_whole_second_utc(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 20
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b'T'
        && b[13] == b':'
        && b[16] == b':'
        && b[19] == b'Z'
        && [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18].iter().all(|&i| b[i].is_ascii_digit())
}

fn parse_ledger_value(v: &Value) -> Result<LedgerHead, CkptError> {
    let Value::Object(members) = v else {
        return Err(CkptError::malformed("`ledger` must be an object"));
    };
    let mut seq: Option<u64> = None;
    let mut head: Option<Digest> = None;
    for (k, val) in members {
        match k.as_str() {
            "seq" => seq = Some(parse_seq_value(val, "ledger.seq")?),
            "head" => head = Some(parse_digest_value(val, "ledger.head")?),
            other => return Err(CkptError::malformed(format!("unexpected field `ledger.{other}`"))),
        }
    }
    let seq = seq.ok_or_else(|| CkptError::malformed("missing `ledger.seq`"))?;
    let head = head.ok_or_else(|| CkptError::malformed("missing `ledger.head`"))?;
    Ok(LedgerHead { seq, head })
}

// ------------------------------------------------------------------ verification

/// Checks one checkpoint file against `segment` and optional `trust` inputs.
///
/// Never fails with an error return: a malformed checkpoint yields
/// `CheckpointResult { status: CheckpointStatus::Malformed, error_code: Some("ckpt.malformed"), .. }`.
pub fn check_checkpoint(
    path: &str,
    bytes: &[u8],
    segment: &SegmentHashes,
    trust: Option<&TrustInputs<'_>>,
) -> CheckpointResult {
    let ckpt = match parse_checkpoint(bytes) {
        Ok(c) => c,
        Err(err) => {
            return CheckpointResult {
                path: path.to_string(),
                index: None,
                stream: None,
                head_sha256: None,
                status: CheckpointStatus::Malformed,
                error_code: Some(ERR_MALFORMED),
                detail: Some(err.detail),
                signature: None,
                signatures: Vec::new(),
            };
        }
    };

    let (status, detail) = match segment.get(ckpt.seq) {
        None => (
            CheckpointStatus::OutOfRange,
            Some(format!("checkpoint index {} is outside the packaged segment", ckpt.seq)),
        ),
        Some(actual) if *actual == ckpt.head => (CheckpointStatus::Match, None),
        Some(actual) => (
            CheckpointStatus::Mismatch,
            Some(format!("at index {} checkpoint head is {}, segment hash is {actual}", ckpt.seq, ckpt.head)),
        ),
    };

    let empty_root = TrustRoot::default();
    let empty_rev = Revocations::default();
    let empty_ring = Keyring::default();
    let default_trust =
        TrustInputs { trust_root: &empty_root, revocations: &empty_rev, keyring: &empty_ring, as_of: None };
    let active_trust = trust.unwrap_or(&default_trust);

    let signatures = match &ckpt.signature {
        Some(env) => verify_checkpoint_envelope(env, &ckpt.canonical_body, &ckpt.time, &ckpt.time_raw, active_trust),
        None => Vec::new(),
    };
    let primary_sig = signatures.first().cloned();

    CheckpointResult {
        path: path.to_string(),
        index: Some(ckpt.seq),
        stream: Some(ckpt.stream),
        head_sha256: Some(ckpt.head),
        status,
        error_code: None,
        detail,
        signature: primary_sig,
        signatures,
    }
}

/// Checks every checkpoint file in deterministic path order (UTF-8 bytes).
///
/// Malformed files produce `ckpt.malformed` and never prevent other checkpoints from being checked
/// (REQ-TRUST.CHAIN.CKPT-003, AC-4).
pub fn check_checkpoints(
    inputs: &[CheckpointInput<'_>],
    segment: &SegmentHashes,
    trust: Option<&TrustInputs<'_>>,
) -> Vec<CheckpointResult> {
    let mut ordered: Vec<&CheckpointInput<'_>> = inputs.iter().collect();
    ordered.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
    ordered.into_iter().map(|i| check_checkpoint(i.path, i.bytes, segment, trust)).collect()
}

struct KeyCandidate<'a> {
    label: &'a str,
    key: &'a PublicKey,
    fingerprint: &'a Fingerprint,
    trusted: Option<&'a fid_sign_core::troot::TrustedKey>,
}

fn verify_checkpoint_envelope(
    envelope: &Envelope,
    canonical_body: &[u8],
    checkpoint_time: &UtcTime,
    checkpoint_time_raw: &str,
    inputs: &TrustInputs<'_>,
) -> Vec<CheckpointSigResult> {
    let mut candidates: Vec<KeyCandidate<'_>> =
        Vec::with_capacity(inputs.trust_root.keys.len().saturating_add(inputs.keyring.keys.len()));
    for k in &inputs.trust_root.keys {
        candidates.push(KeyCandidate { label: &k.label, key: &k.key, fingerprint: &k.fingerprint, trusted: Some(k) });
    }
    for k in &inputs.keyring.keys {
        candidates.push(KeyCandidate { label: &k.label, key: &k.key, fingerprint: &k.fingerprint, trusted: None });
    }

    let binding_err: Option<&'static str> = if envelope.payload_type != CHECKPOINT_PAYLOAD_TYPE {
        Some("payload_type")
    } else if envelope.payload != canonical_body {
        Some("payload_not_bound")
    } else {
        None
    };

    let pae = if binding_err.is_none() { envelope.pae() } else { Vec::new() };
    let mut results = Vec::with_capacity(envelope.signatures.len());

    for sig in &envelope.signatures {
        let by_keyid =
            (!sig.keyid.is_empty()).then(|| candidates.iter().find(|c| c.fingerprint.as_str() == sig.keyid)).flatten();

        if let Some(reason) = binding_err {
            results.push(CheckpointSigResult {
                claimed_keyid: sig.keyid.clone(),
                fingerprint: by_keyid.map(|c| c.fingerprint.as_str().to_owned()),
                key_label: by_keyid.map(|c| c.label.to_owned()),
                status: SignatureStatus::Invalid,
                detail: Some(reason.to_owned()),
                notes: Vec::new(),
                claimed_signed_at: checkpoint_time_raw.to_owned(),
            });
            continue;
        }

        let matched = if sig.keyid.is_empty() {
            if candidates.len() > MAX_KEYID_TRIALS {
                results.push(CheckpointSigResult {
                    claimed_keyid: String::new(),
                    fingerprint: None,
                    key_label: None,
                    status: SignatureStatus::UnknownKey,
                    detail: Some("keyid_missing".to_owned()),
                    notes: Vec::new(),
                    claimed_signed_at: checkpoint_time_raw.to_owned(),
                });
                continue;
            }
            match candidates.iter().find(|c| c.key.verify_raw(&pae, &sig.sig).is_ok()) {
                Some(c) => c,
                None => {
                    results.push(CheckpointSigResult {
                        claimed_keyid: String::new(),
                        fingerprint: None,
                        key_label: None,
                        status: SignatureStatus::Invalid,
                        detail: Some("no_listed_key_verifies".to_owned()),
                        notes: Vec::new(),
                        claimed_signed_at: checkpoint_time_raw.to_owned(),
                    });
                    continue;
                }
            }
        } else {
            let Some(c) = by_keyid else {
                results.push(CheckpointSigResult {
                    claimed_keyid: sig.keyid.clone(),
                    fingerprint: None,
                    key_label: None,
                    status: SignatureStatus::UnknownKey,
                    detail: Some("key_not_listed".to_owned()),
                    notes: Vec::new(),
                    claimed_signed_at: checkpoint_time_raw.to_owned(),
                });
                continue;
            };
            if c.key.verify_raw(&pae, &sig.sig).is_err() {
                results.push(CheckpointSigResult {
                    claimed_keyid: sig.keyid.clone(),
                    fingerprint: Some(c.fingerprint.as_str().to_owned()),
                    key_label: Some(c.label.to_owned()),
                    status: SignatureStatus::Invalid,
                    detail: Some("signature_invalid".to_owned()),
                    notes: Vec::new(),
                    claimed_signed_at: checkpoint_time_raw.to_owned(),
                });
                continue;
            }
            c
        };

        let mut notes = Vec::new();
        let (status, detail) = if let Some(rev) = inputs.revocations.find(matched.fingerprint) {
            if rev.revoked_at <= *checkpoint_time {
                (SignatureStatus::Revoked, Some("revoked_before_signing".to_owned()))
            } else {
                notes.push("revoked_after_claimed_signing".to_owned());
                classify_trusted_window(matched.trusted, checkpoint_time, inputs.as_of, &mut notes)
            }
        } else {
            classify_trusted_window(matched.trusted, checkpoint_time, inputs.as_of, &mut notes)
        };

        results.push(CheckpointSigResult {
            claimed_keyid: sig.keyid.clone(),
            fingerprint: Some(matched.fingerprint.as_str().to_owned()),
            key_label: Some(matched.label.to_owned()),
            status,
            detail,
            notes,
            claimed_signed_at: checkpoint_time_raw.to_owned(),
        });
    }

    results
}

fn classify_trusted_window(
    trusted: Option<&fid_sign_core::troot::TrustedKey>,
    checkpoint_time: &UtcTime,
    as_of: Option<&UtcTime>,
    notes: &mut Vec<String>,
) -> (SignatureStatus, Option<String>) {
    let Some(tk) = trusted else {
        return (SignatureStatus::ValidUntrusted, Some("key_not_trusted".to_owned()));
    };
    if *checkpoint_time < tk.not_before {
        return (SignatureStatus::Expired, Some("before_not_before".to_owned()));
    }
    if let Some(na) = &tk.not_after {
        if *checkpoint_time > *na {
            return (SignatureStatus::Expired, Some("after_not_after".to_owned()));
        }
        if let Some(ref_time) = as_of {
            if *na < *ref_time {
                notes.push("key_validity_ended_as_of".to_owned());
            }
        }
    }
    (SignatureStatus::ValidTrusted, None)
}
