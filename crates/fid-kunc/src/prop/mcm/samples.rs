//! Retained Monte Carlo sample sets, format [`FORMAT`] = `mcm-samples@1`, and their digest.
//! Spec node `UNC.PROP.MCM.SAMP`.
//!
//! A sample set keeps the output values of every trial of a run so that `UNC.DEC`
//! (conformance probability) and `UNC.XFM` (fields) can use the distribution itself rather
//! than a summary (JCGM 101:2008, clause 7.5 [unverified: clause numbering]). The set is
//! addressed by the SHA-256 digest of its bytes; storing the bytes is the command layer's
//! job (`UNC.PROP.MCM.CMD`), not this module's.
//!
//! # Format (pinned; all integers little-endian)
//!
//! | Bytes | Content |
//! |---|---|
//! | 8 | `H`, the header length in bytes (`u64`) |
//! | `H` | the header: RFC 8785 JSON, see below |
//! | `8 (T - F)` per output | the output's values as IEEE-754 doubles in global trial order, failed trials omitted; outputs in header order |
//! | `8 F` | the failed global trial indices (`u64`), strictly ascending |
//!
//! The header is the canonical JSON (sorted keys, no whitespace, every integer a decimal
//! string) of `{"failed":F,"inputs","model","outputs":[...],"profile","seed","trials":T,
//! "version":"mcm-samples@1"}`: `inputs`, `model` and `profile` are `sha256:` digests (the
//! sampling plan's, the model's and the adaptive profile's), `outputs` the output node IDs
//! in ascending byte order (the column order of [`BlockOut::values`]). The encoded size is
//! therefore `8 + H + 8 (K (T - F) + F)` for `K` outputs.
//!
//! The value columns hold only the trials that succeeded. Consumers must read `failed` and
//! treat the values as conditional on success: a statistic over the columns describes the
//! successful trials, not all `T` trials. Only the failed trial *indices* are retained; the
//! per-trial failure codes stay in the blocks and the run record.
//!
//! # Encoding ([`encode`])
//!
//! The blocks are taken in ascending block index, whatever their order in the slice, so
//! blocks computed in any order give the same bytes (REQ-UNC.PROP.MCM.SAMP-001). They must
//! be exactly the blocks `0 .. n` of one run: every block has the same size `s` (values per
//! column plus failures), one column per header output, finite values, and failed indices
//! strictly ascending inside its range `b s .. (b + 1) s`; the header's `trials` and `failed`
//! must equal the blocks' totals. Anything else is `samples_invalid{detail}` and nothing is
//! written. If the encoded size would exceed the cap, the result is
//! `samples_not_retained{size}` with that size and no bytes are produced
//! (REQ-UNC.PROP.MCM.SAMP-002). [`DEFAULT_CAP`] is 64 MiB [unverified: unmeasured against
//! DEC and XFM needs]. Encoding does no floating-point arithmetic (D0); the content is
//! seeded (D2).
//!
//! # Decoding ([`decode`], bounded, fails closed)
//!
//! In this order, each failure being `samples_invalid` with the detail named: input longer
//! than the caller's cap (`too_large`); fewer than 8 bytes, or a header that runs past the
//! end (`length`); a header longer than [`MAX_HEADER_BYTES`], not readable as JSON, not in
//! canonical form, or with a missing, extra or malformed member (`header`); a `version`
//! other than [`FORMAT`] (`version`); outputs that are empty, not strictly ascending or not
//! printable ASCII IDs of at most [`MAX_ID_BYTES`] bytes (`outputs`); `trials` of 0 or
//! `failed > trials` (`counts`); a body whose length differs from what the counts declare
//! (`length`, REQ-UNC.PROP.MCM.SAMP-003); a NaN or infinite value (`non_finite`); failed
//! indices not strictly ascending or not below `trials` (`failed`).
//!
//! KNP-1: no floating-point arithmetic, no hash containers, no panics; a NaN is an error.

use fid_kci::kci::json::{to_canonical, Json};
use fid_kci::kci::sha256_digest;
use fid_types::Digest;

use super::block::BlockOut;
use super::McmError;
use crate::model::MAX_ID_BYTES;

/// Format name and version, the header's `version` member.
pub const FORMAT: &str = "mcm-samples@1";
/// Default retention cap: 64 MiB [unverified].
pub const DEFAULT_CAP: u64 = 64 * 1024 * 1024;
/// Longest header [`decode`] reads: 1 MiB.
pub const MAX_HEADER_BYTES: u64 = 1024 * 1024;

/// Bytes of the header length prefix, of one value and of one failed index.
const WORD: u64 = 8;
/// Members of the header object.
const HEADER_MEMBERS: usize = 8;

fn invalid(detail: &'static str) -> McmError {
    McmError::SamplesInvalid { detail }
}

fn alloc<T>(v: &mut Vec<T>, n: usize) -> Result<(), McmError> {
    v.try_reserve_exact(n).map_err(|_| McmError::Allocation)
}

/// The header of a sample set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SampleSetHeader {
    /// Digest of the model.
    pub model_digest: Digest,
    /// Digest of the sampling plan (the blocks' inputs digest).
    pub inputs_digest: Digest,
    /// RNG key of the run.
    pub seed: u64,
    /// Digest of the adaptive profile the run used.
    pub profile_digest: Digest,
    /// Output node IDs in ascending byte order, one per column.
    pub outputs: Vec<String>,
    /// Trials run, failed ones included.
    pub trials: u64,
    /// Failed trials.
    pub failed: u64,
}

impl SampleSetHeader {
    /// Checks the outputs and the counts (see the module documentation).
    fn check(&self) -> Result<(), McmError> {
        if self.outputs.is_empty() {
            return Err(invalid("outputs"));
        }
        for name in &self.outputs {
            let ok = !name.is_empty()
                && name.len() <= MAX_ID_BYTES
                && name.bytes().all(|c| (0x20..=0x7e).contains(&c) && c != b'"' && c != b'\\');
            if !ok {
                return Err(invalid("outputs"));
            }
        }
        if self.outputs.windows(2).any(|w| w[0].as_bytes() >= w[1].as_bytes()) {
            return Err(invalid("outputs"));
        }
        if self.trials == 0 || self.failed > self.trials {
            return Err(invalid("counts"));
        }
        Ok(())
    }

    /// The header's canonical JSON.
    pub fn canonical_json(&self) -> Result<Vec<u8>, McmError> {
        self.check()?;
        let mut outs = Vec::new();
        alloc(&mut outs, self.outputs.len())?;
        outs.extend(self.outputs.iter().map(|s| Json::Str(s.clone())));
        let s = |x: &str| Json::Str(x.to_owned());
        let doc = Json::Obj(vec![
            ("failed".to_owned(), s(&self.failed.to_string())),
            ("inputs".to_owned(), s(self.inputs_digest.as_str())),
            ("model".to_owned(), s(self.model_digest.as_str())),
            ("outputs".to_owned(), Json::Arr(outs)),
            ("profile".to_owned(), s(self.profile_digest.as_str())),
            ("seed".to_owned(), s(&self.seed.to_string())),
            ("trials".to_owned(), s(&self.trials.to_string())),
            ("version".to_owned(), s(FORMAT)),
        ]);
        to_canonical(&doc).map_err(|_| McmError::Internal { detail: "samples_header_encoding" })
    }

    /// Values per column, `trials - failed`.
    fn kept(&self) -> u64 {
        self.trials.saturating_sub(self.failed)
    }

    /// Body bytes the counts declare, `8 (K (T - F) + F)`; `None` on overflow.
    fn body_len(&self) -> Option<u64> {
        let k = u64::try_from(self.outputs.len()).ok()?;
        k.checked_mul(self.kept())?.checked_add(self.failed)?.checked_mul(WORD)
    }
}

/// A decoded (or assembled) sample set.
#[derive(Debug, Clone, PartialEq)]
pub struct SampleSet {
    pub header: SampleSetHeader,
    /// One column per header output, each `trials - failed` values in global trial order.
    pub outputs: Vec<Vec<f64>>,
    /// Failed global trial indices, strictly ascending.
    pub failed: Vec<u64>,
}

impl SampleSet {
    /// Encodes the set (same checks and cap rule as [`encode`]).
    pub fn encode(&self, cap: u64) -> Result<Encoded, McmError> {
        self.check()?;
        emit(&self.header, cap, |out| {
            for col in &self.outputs {
                for x in col {
                    out.extend_from_slice(&x.to_le_bytes());
                }
            }
            for t in &self.failed {
                out.extend_from_slice(&t.to_le_bytes());
            }
        })
    }

    fn check(&self) -> Result<(), McmError> {
        let h = &self.header;
        h.check()?;
        let kept = usize::try_from(h.kept()).map_err(|_| invalid("length"))?;
        if self.outputs.len() != h.outputs.len()
            || self.outputs.iter().any(|c| c.len() != kept)
            || u64::try_from(self.failed.len()).ok() != Some(h.failed)
        {
            return Err(invalid("length"));
        }
        if self.outputs.iter().flatten().any(|x| !x.is_finite()) {
            return Err(invalid("non_finite"));
        }
        check_failed(&self.failed, 0, h.trials)
    }
}

/// Failed indices strictly ascending inside `start .. end`.
fn check_failed(failed: &[u64], start: u64, end: u64) -> Result<(), McmError> {
    let mut next = start;
    for &t in failed {
        if t < next || t >= end {
            return Err(invalid("failed"));
        }
        next = t + 1;
    }
    Ok(())
}

/// Encoded bytes of a sample set and their SHA-256 digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Encoded {
    pub bytes: Vec<u8>,
    pub digest: Digest,
}

/// Writes the prefix and header, checks the cap, then lets `body` write the rest.
fn emit(header: &SampleSetHeader, cap: u64, body: impl FnOnce(&mut Vec<u8>)) -> Result<Encoded, McmError> {
    let json = header.canonical_json()?;
    let h = u64::try_from(json.len()).map_err(|_| McmError::Allocation)?;
    let size = header.body_len().and_then(|b| b.checked_add(h)).and_then(|b| b.checked_add(WORD)).unwrap_or(u64::MAX);
    if size > cap {
        return Err(McmError::SamplesNotRetained { size });
    }
    let n = usize::try_from(size).map_err(|_| McmError::Allocation)?;
    let mut bytes = Vec::new();
    alloc(&mut bytes, n)?;
    bytes.extend_from_slice(&h.to_le_bytes());
    bytes.extend_from_slice(&json);
    body(&mut bytes);
    if bytes.len() != n {
        return Err(McmError::Internal { detail: "samples_size" });
    }
    let digest = sha256_digest(&bytes);
    Ok(Encoded { bytes, digest })
}

/// Encodes the blocks of one run under `header` (REQ-UNC.PROP.MCM.SAMP-001, -002; see the
/// module documentation for the checks).
pub fn encode(blocks: &[BlockOut], header: &SampleSetHeader, cap: u64) -> Result<Encoded, McmError> {
    header.check()?;
    let mut order: Vec<&BlockOut> = Vec::new();
    alloc(&mut order, blocks.len())?;
    order.extend(blocks.iter());
    order.sort_by_key(|b| b.block_index);
    let first = order.first().ok_or(invalid("blocks"))?;
    let k = header.outputs.len();
    let size = first.values.first().map_or(0, Vec::len).checked_add(first.failed.len()).ok_or(invalid("block_size"))?;
    let size = u64::try_from(size).map_err(|_| invalid("block_size"))?;
    if size == 0 {
        return Err(invalid("block_size"));
    }
    let (mut trials, mut failed) = (0u64, 0u64);
    if order.iter().enumerate().any(|(i, b)| u64::try_from(i).ok() != Some(b.block_index)) {
        return Err(invalid("blocks"));
    }
    for b in &order {
        if b.values.len() != k {
            return Err(invalid("columns"));
        }
        let n = b.values.first().map_or(0, Vec::len);
        if b.values.iter().any(|c| c.len() != n) {
            return Err(invalid("columns"));
        }
        if n.checked_add(b.failed.len()).and_then(|x| u64::try_from(x).ok()) != Some(size) {
            return Err(invalid("block_size"));
        }
        if b.values.iter().flatten().any(|x| !x.is_finite()) {
            return Err(invalid("non_finite"));
        }
        let start = b.block_index.checked_mul(size).ok_or(invalid("block_size"))?;
        let end = start.checked_add(size).ok_or(invalid("block_size"))?;
        let idx: Vec<u64> = b.failed.iter().map(|f| f.index).collect();
        check_failed(&idx, start, end)?;
        trials = end;
        failed = u64::try_from(b.failed.len()).ok().and_then(|x| failed.checked_add(x)).ok_or(invalid("block_size"))?;
    }
    if trials != header.trials {
        return Err(invalid("trials"));
    }
    if failed != header.failed {
        return Err(invalid("failed_count"));
    }
    emit(header, cap, |out| {
        for c in 0..k {
            for b in &order {
                if let Some(col) = b.values.get(c) {
                    for x in col {
                        out.extend_from_slice(&x.to_le_bytes());
                    }
                }
            }
        }
        for b in &order {
            for f in &b.failed {
                out.extend_from_slice(&f.index.to_le_bytes());
            }
        }
    })
}

/// SHA-256 of encoded bytes, the sample set's address.
pub fn digest(bytes: &[u8]) -> Digest {
    sha256_digest(bytes)
}

// ------------------------------------------------------------------ decoding

fn word(bytes: &[u8], at: usize) -> Option<[u8; 8]> {
    let s = bytes.get(at..at.checked_add(8)?)?;
    let mut w = [0u8; 8];
    w.copy_from_slice(s);
    Some(w)
}

/// A canonical decimal `u64` (no sign, no leading zero).
fn decimal(s: &str) -> Option<u64> {
    let canonical = !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()) && (s == "0" || !s.starts_with('0'));
    if canonical {
        s.parse().ok()
    } else {
        None
    }
}

fn member<'a>(m: &'a [(String, Json)], key: &str) -> Result<&'a Json, McmError> {
    m.iter().find(|(k, _)| k == key).map(|(_, v)| v).ok_or(invalid("header"))
}

fn text<'a>(m: &'a [(String, Json)], key: &str) -> Result<&'a str, McmError> {
    match member(m, key)? {
        Json::Str(s) => Ok(s),
        _ => Err(invalid("header")),
    }
}

fn header_digest(m: &[(String, Json)], key: &str) -> Result<Digest, McmError> {
    Digest::parse(text(m, key)?).map_err(|_| invalid("header"))
}

fn header_u64(m: &[(String, Json)], key: &str) -> Result<u64, McmError> {
    decimal(text(m, key)?).ok_or(invalid("header"))
}

fn parse_header(h: &[u8]) -> Result<SampleSetHeader, McmError> {
    let json = crate::model::reader::read(h).map_err(|_| invalid("header"))?;
    let canonical = to_canonical(&json).map_err(|_| invalid("header"))?;
    if canonical != h {
        return Err(invalid("header"));
    }
    let Json::Obj(m) = json else {
        return Err(invalid("header"));
    };
    if text(&m, "version")? != FORMAT {
        return Err(invalid("version"));
    }
    if m.len() != HEADER_MEMBERS {
        return Err(invalid("header"));
    }
    let Json::Arr(outs) = member(&m, "outputs")? else {
        return Err(invalid("header"));
    };
    let mut outputs = Vec::new();
    alloc(&mut outputs, outs.len())?;
    for o in outs {
        match o {
            Json::Str(s) => outputs.push(s.clone()),
            _ => return Err(invalid("header")),
        }
    }
    let header = SampleSetHeader {
        model_digest: header_digest(&m, "model")?,
        inputs_digest: header_digest(&m, "inputs")?,
        seed: header_u64(&m, "seed")?,
        profile_digest: header_digest(&m, "profile")?,
        outputs,
        trials: header_u64(&m, "trials")?,
        failed: header_u64(&m, "failed")?,
    };
    header.check()?;
    Ok(header)
}

/// Decodes `bytes`, refusing input longer than `cap` (REQ-UNC.PROP.MCM.SAMP-003; see the
/// module documentation for the checks and their order).
pub fn decode(bytes: &[u8], cap: u64) -> Result<SampleSet, McmError> {
    let len = u64::try_from(bytes.len()).map_err(|_| invalid("too_large"))?;
    if len > cap {
        return Err(invalid("too_large"));
    }
    let h = u64::from_le_bytes(word(bytes, 0).ok_or(invalid("length"))?);
    if h > MAX_HEADER_BYTES {
        return Err(invalid("header"));
    }
    let body_at = usize::try_from(h).ok().and_then(|h| h.checked_add(8)).ok_or(invalid("header"))?;
    let header_bytes = bytes.get(8..body_at).ok_or(invalid("length"))?;
    let header = parse_header(header_bytes)?;
    let body = bytes.get(body_at..).ok_or(invalid("length"))?;
    if header.body_len() != u64::try_from(body.len()).ok() {
        return Err(invalid("length"));
    }
    // The body length matches the counts, so every count below fits in usize.
    let kept = usize::try_from(header.kept()).map_err(|_| invalid("length"))?;
    let nfailed = usize::try_from(header.failed).map_err(|_| invalid("length"))?;
    let mut at = 0usize;
    let mut outputs = Vec::new();
    alloc(&mut outputs, header.outputs.len())?;
    for _ in &header.outputs {
        let mut col = Vec::new();
        alloc(&mut col, kept)?;
        for _ in 0..kept {
            let x = f64::from_le_bytes(word(body, at).ok_or(invalid("length"))?);
            if !x.is_finite() {
                return Err(invalid("non_finite"));
            }
            col.push(x);
            at += 8;
        }
        outputs.push(col);
    }
    let mut failed = Vec::new();
    alloc(&mut failed, nfailed)?;
    for _ in 0..nfailed {
        failed.push(u64::from_le_bytes(word(body, at).ok_or(invalid("length"))?));
        at += 8;
    }
    check_failed(&failed, 0, header.trials)?;
    Ok(SampleSet { header, outputs, failed })
}
