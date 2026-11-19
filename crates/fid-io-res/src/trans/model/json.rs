//! Canonical JSON (RFC 8785, `fid-canon`) of the transcription types. Every key is always
//! written; an absent value is `null`. Positions are decimal strings (CORE.JRN.CANON).

use std::collections::BTreeMap;

use fid_canon::CValue;
use fid_types::Digest;

use super::{SourceText, TransError, TranscribedResult, Transcription, TranscriptionRecord};
use super::{DECISION_RULE, RECORD_SCHEMA, RESULT_SCHEMA, STATUS_TRANSCRIBED, TRANSCRIPTION_SCHEMA};

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect::<BTreeMap<_, _>>())
}

fn s(v: &str) -> CValue {
    CValue::Str(v.to_owned())
}

fn t(v: &SourceText) -> CValue {
    s(v.raw())
}

fn opt(v: Option<&SourceText>) -> CValue {
    v.map_or(CValue::Null, t)
}

fn d(v: &Digest) -> CValue {
    s(v.as_str())
}

pub(super) fn record_id_inputs(file: &Digest) -> CValue {
    obj([("kind", s("transcription_record")), ("file_sha256", d(file))])
}

pub(super) fn result_id_inputs(file: &Digest, characteristic: &str, part: Option<&str>) -> CValue {
    obj([
        ("kind", s("transcribed_result")),
        ("file_sha256", d(file)),
        ("characteristic", s(characteristic)),
        ("part", part.map_or(CValue::Null, s)),
    ])
}

pub(super) fn record_value(r: &TranscriptionRecord) -> CValue {
    obj([
        ("schema", s(RECORD_SCHEMA)),
        ("id", s(&r.id.to_string())),
        ("source_software", t(&r.source_software)),
        ("source_version", t(&r.source_version)),
        ("file_sha256", d(&r.file_sha256)),
        ("imported_at", s(&r.imported_at)),
        ("format", s(&r.format)),
    ])
}

/// The statement as `fid-kunc` wrote it, embedded as a JSON object.
fn uncertainty(r: &TranscribedResult) -> Result<CValue, TransError> {
    fid_canon::parse(r.uncertainty.canonical_json()).map_err(|_| TransError::Internal("uncertainty_statement"))
}

pub(super) fn result_value(r: &TranscribedResult) -> Result<CValue, TransError> {
    let a = &r.as_reported;
    Ok(obj([
        ("schema", s(RESULT_SCHEMA)),
        ("id", s(&r.id.to_string())),
        ("record", obj([("id", s(&r.record.to_string())), ("digest", d(&r.record_digest))])),
        ("source_row", CValue::U64(r.source_row as u64)),
        ("characteristic", t(&r.characteristic)),
        ("part", opt(r.part.as_ref())),
        ("measurand", opt(r.measurand.as_ref())),
        ("evaluation_basis", s(r.evaluation_basis.as_str())),
        ("status", s(STATUS_TRANSCRIBED)),
        ("value", obj([("text", t(&r.value)), ("unit", opt(r.unit.as_ref()))])),
        ("uncertainty", uncertainty(r)?),
        ("uncertainty_digest", d(&r.uncertainty.digest())),
        (
            "decision",
            obj([
                ("rule", s(DECISION_RULE)),
                ("outcome", s(r.outcome.as_str())),
                ("reason", r.reason.map_or(CValue::Null, |c| s(c.as_str()))),
            ]),
        ),
        ("side", s(r.side.as_str())),
        (
            "claim",
            obj([
                ("claimed", CValue::Bool(r.is_claimed())),
                ("declared_input", CValue::Bool(r.rests_on_declared_input())),
                ("uncharacterized_input", CValue::Bool(r.rests_on_uncharacterized_input())),
            ]),
        ),
        ("spec_context", CValue::Null),
        ("algorithm", CValue::Null),
        ("kernel_version", CValue::Null),
        ("input_digests", CValue::Arr(r.input_digests().iter().map(d).collect())),
        (
            "as_reported",
            obj([
                ("value", t(&a.value)),
                ("unit", opt(a.unit.as_ref())),
                ("status", opt(a.status.as_ref())),
                ("uncertainty", opt(a.uncertainty.as_ref())),
                ("decision_rule", opt(a.decision_rule.as_ref())),
                ("outcome", opt(a.outcome.as_ref())),
                ("lower_limit", opt(a.lower_limit.as_ref())),
                ("upper_limit", opt(a.upper_limit.as_ref())),
            ]),
        ),
    ]))
}

pub(super) fn transcription_value(x: &Transcription) -> Result<CValue, TransError> {
    let results = x.results.iter().map(result_value).collect::<Result<Vec<_>, _>>()?;
    Ok(obj([
        ("schema", s(TRANSCRIPTION_SCHEMA)),
        ("record", record_value(&x.record)),
        ("results", CValue::Arr(results)),
        (
            "dropped",
            CValue::Arr(
                x.dropped
                    .iter()
                    .map(|e| obj([("row", CValue::U64(e.row as u64)), ("reason", s(e.reason.as_str()))]))
                    .collect(),
            ),
        ),
    ]))
}

pub(super) fn bytes_of(v: &CValue) -> Result<Vec<u8>, TransError> {
    fid_canon::to_canonical_bytes(v).map_err(|_| TransError::Internal("canonical_json"))
}

pub(super) fn digest_of(v: &CValue) -> Result<Digest, TransError> {
    Ok(Digest::from_bytes(fid_canon::digest_bytes(&bytes_of(v)?).0))
}
