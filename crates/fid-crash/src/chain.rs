//! The synthetic hash chain the harness commits (REQ-CORE.STO.SQL.CRASH-001).
//!
//! Row `seq` (from 1) is a JSON object of ASCII strings and nulls,
//! `{"blob", "format", "payload", "prev", "seq"}`, hashed in its RFC 8785 form through
//! `fid-canon`: `prev` is the hash of row `seq - 1` (null for row 1), `payload` the digest of
//! [`payload_bytes`], `blob` the digest of [`blob_bytes`] (null for every third row). Every
//! row is a pure function of `seq`, so the expected chain is known without reading the store;
//! `fixtures/core/sto/sql/crash/make_chain.py` derives the same rows independently.

use fid_canon::{digest_bytes, parse_with_budget, to_canonical_bytes, CValue};
use fid_types::Digest;
use std::collections::BTreeMap;

/// Value of the row's `format` field.
pub const ROW_FORMAT: &str = "fid-crash-row@1";
/// Longest canonical row accepted when reading a store (a real row is about 280 bytes).
pub const MAX_ROW_BYTES: usize = 1024;
/// Most JSON elements a stored row may parse into (a real row has 11).
const MAX_ROW_ELEMENTS: usize = 32;
/// Nesting budget of a stored row (a real row is one flat object).
const MAX_ROW_DEPTH: usize = 2;

/// One row of the synthetic chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntheticRow {
    pub seq: u64,
    pub prev_hash: Option<Digest>,
    pub payload_digest: Digest,
    pub blob: Option<Digest>,
}

/// SHA-256 of raw bytes as a `sha256:` digest.
pub fn sha256(bytes: &[u8]) -> Digest {
    Digest::from_bytes(digest_bytes(bytes).0)
}

/// The payload of row `seq`.
pub fn payload_bytes(seq: u64) -> Vec<u8> {
    format!("fid-crash payload {seq}").into_bytes()
}

/// The blob of row `seq`, if it has one (every row whose `seq` is not a multiple of 3).
pub fn blob_bytes(seq: u64) -> Option<Vec<u8>> {
    if seq % 3 == 0 {
        return None;
    }
    Some(format!("fid-crash blob {seq}\n").repeat(32 * (1 + (seq % 5) as usize)).into_bytes())
}

impl SyntheticRow {
    /// The expected row `seq` after the row whose hash is `prev_hash`.
    pub fn new(seq: u64, prev_hash: Option<Digest>) -> SyntheticRow {
        SyntheticRow {
            seq,
            prev_hash,
            payload_digest: sha256(&payload_bytes(seq)),
            blob: blob_bytes(seq).map(|b| sha256(&b)),
        }
    }

    fn to_value(&self) -> CValue {
        let opt = |d: &Option<Digest>| d.map_or(CValue::Null, |d| CValue::Str(d.as_str().to_string()));
        let mut o = BTreeMap::new();
        o.insert("blob".to_string(), opt(&self.blob));
        o.insert("format".to_string(), CValue::Str(ROW_FORMAT.to_string()));
        o.insert("payload".to_string(), CValue::Str(self.payload_digest.as_str().to_string()));
        o.insert("prev".to_string(), opt(&self.prev_hash));
        o.insert("seq".to_string(), CValue::Str(self.seq.to_string()));
        CValue::Obj(o)
    }

    /// RFC 8785 bytes of the row. The row holds only ASCII strings and nulls, so this cannot
    /// fail; an empty vector is returned if it ever did, which no check accepts.
    pub fn canonical(&self) -> Vec<u8> {
        to_canonical_bytes(&self.to_value()).unwrap_or_default()
    }

    /// The row's hash: SHA-256 of [`SyntheticRow::canonical`].
    pub fn hash(&self) -> Digest {
        sha256(&self.canonical())
    }

    /// Parses stored row bytes, bounded by [`MAX_ROW_BYTES`] and a small element budget, and
    /// accepts them only if they are exactly the canonical form of a well-formed row.
    pub fn parse(bytes: &[u8]) -> Result<SyntheticRow, String> {
        if bytes.len() > MAX_ROW_BYTES {
            return Err(format!("row of {} bytes exceeds {MAX_ROW_BYTES}", bytes.len()));
        }
        let v = parse_with_budget(bytes, MAX_ROW_BYTES, MAX_ROW_DEPTH, MAX_ROW_ELEMENTS)
            .map_err(|e| format!("row is not JSON: {e}"))?;
        let CValue::Obj(o) = v else { return Err("row is not an object".to_string()) };
        let keys: Vec<&str> = o.keys().map(String::as_str).collect();
        if keys != ["blob", "format", "payload", "prev", "seq"] {
            return Err(format!("row has keys {keys:?}"));
        }
        if o.get("format") != Some(&CValue::Str(ROW_FORMAT.to_string())) {
            return Err("row has the wrong format".to_string());
        }
        let digest = |k: &str| -> Result<Option<Digest>, String> {
            match o.get(k) {
                Some(CValue::Null) => Ok(None),
                Some(CValue::Str(s)) => Digest::parse(s).map(Some).map_err(|_| format!("{k} is not a digest")),
                _ => Err(format!("{k} is not a digest or null")),
            }
        };
        let seq = match o.get("seq") {
            Some(CValue::Str(s)) if !s.starts_with('0') && s.len() <= 19 && s.bytes().all(|b| b.is_ascii_digit()) => {
                s.parse::<u64>().map_err(|_| "seq out of range".to_string())?
            }
            _ => return Err("seq is not a positive decimal string".to_string()),
        };
        let row = SyntheticRow {
            seq,
            prev_hash: digest("prev")?,
            payload_digest: digest("payload")?.ok_or("payload is null")?,
            blob: digest("blob")?,
        };
        if row.canonical() != bytes {
            return Err("row bytes are not canonical".to_string());
        }
        Ok(row)
    }
}

/// The first `n` rows of the chain.
pub fn chain(n: u64) -> Vec<SyntheticRow> {
    let mut out = Vec::new();
    let mut prev = None;
    for seq in 1..=n {
        let row = SyntheticRow::new(seq, prev);
        prev = Some(row.hash());
        out.push(row);
    }
    out
}
