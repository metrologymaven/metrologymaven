//! JSON side of the entry model: reading drafts, and checking headers against the rules
//! of `schema/entry.schema.json` (REQ-011). The checks mirror the published schema one
//! for one; `tools/check_entries.py` runs the same fixtures through the schema itself.

use super::{
    parse_digest, parse_principal_kind, AiAssistance, CommandDraft, CommandHeader, ConfirmMethod, DraftBody,
    EntryDraft, EntryError, EntryHeader, EntryKind, EntryTime, EventSource, HeadMove, HeaderBody, Manifests,
    OutputDigest, PrincipalCommit, PrincipalDraft, SessionInfo, TaggedDigest, UpgradeDraft, UpgradeHeader,
    R1_RESULT_TAG, SCHEMA_V1, SCHEMA_V2, TOMBSTONE,
};
use fid_canon::{digest_bytes, to_canonical_bytes, CValue, Digest};
use fid_kci::AlgorithmId;
use fid_types::{CommandName, Determinism, SideEffect};
use std::collections::BTreeMap;

use super::shape::{
    ObjectShape, COMMAND_SHAPE, HEADER_SHAPE, HEAD_MOVE_SHAPE, INPUT_SHAPE, MANIFESTS_SHAPE, OUTPUT_SHAPE,
    PRINCIPAL_SHAPE, SESSION_SHAPE, TIME_SHAPE, UPGRADE_SHAPE,
};

type Obj = BTreeMap<String, CValue>;

fn err(at: &str, msg: &str) -> EntryError {
    EntryError::Schema(format!("{at}: {msg}"))
}

fn obj<'a>(v: &'a CValue, at: &str) -> Result<&'a Obj, EntryError> {
    match v {
        CValue::Obj(o) => Ok(o),
        _ => Err(err(at, "expected an object")),
    }
}

/// Every `required` key is present and no key outside `required` and `optional` is.
fn fields(o: &Obj, at: &str, required: &[&str], optional: &[&str]) -> Result<(), EntryError> {
    for r in required {
        if !o.contains_key(*r) {
            return Err(err(at, &format!("missing field {r}")));
        }
    }
    for k in o.keys() {
        if !required.contains(&k.as_str()) && !optional.contains(&k.as_str()) {
            return Err(err(at, &format!("unknown field {k}")));
        }
    }
    Ok(())
}

/// [`fields`] with the key lists of a published object shape.
fn shaped(o: &Obj, at: &str, shape: &ObjectShape) -> Result<(), EntryError> {
    fields(o, at, shape.required, shape.optional)
}

fn get<'a>(o: &'a Obj, k: &str, at: &str) -> Result<&'a CValue, EntryError> {
    o.get(k).ok_or_else(|| err(at, &format!("missing field {k}")))
}

fn as_str<'a>(v: &'a CValue, at: &str) -> Result<&'a str, EntryError> {
    match v {
        CValue::Str(s) => Ok(s),
        _ => Err(err(at, "expected a string")),
    }
}

fn string<'a>(o: &'a Obj, k: &str, at: &str) -> Result<&'a str, EntryError> {
    as_str(get(o, k, at)?, &format!("{at}.{k}"))
}

fn array<'a>(o: &'a Obj, k: &str, at: &str) -> Result<&'a [CValue], EntryError> {
    match get(o, k, at)? {
        CValue::Arr(a) => Ok(a),
        _ => Err(err(&format!("{at}.{k}"), "expected an array")),
    }
}

/// A non-empty string (`$defs/text`).
fn text_value(v: &CValue, at: &str) -> Result<String, EntryError> {
    let s = as_str(v, at)?;
    if s.is_empty() {
        return Err(err(at, "must not be empty"));
    }
    Ok(s.to_string())
}

fn text(o: &Obj, k: &str, at: &str) -> Result<String, EntryError> {
    text_value(get(o, k, at)?, &format!("{at}.{k}"))
}

fn opt_text(o: &Obj, k: &str, at: &str) -> Result<Option<String>, EntryError> {
    match o.get(k) {
        Some(v) => text_value(v, &format!("{at}.{k}")).map(Some),
        None => Ok(None),
    }
}

/// `^[a-z][a-z0-9_]*$` (`$defs/ident`).
fn is_ident(s: &str) -> bool {
    let mut b = s.bytes();
    matches!(b.next(), Some(c) if c.is_ascii_lowercase())
        && b.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
}

fn ident(o: &Obj, k: &str, at: &str) -> Result<String, EntryError> {
    let s = string(o, k, at)?;
    if !is_ident(s) {
        return Err(err(&format!("{at}.{k}"), "expected a lowercase identifier"));
    }
    Ok(s.to_string())
}

/// `^[A-Za-z0-9][A-Za-z0-9._/-]*$` (`$defs/branch`).
fn is_branch(s: &str) -> bool {
    let mut b = s.bytes();
    matches!(b.next(), Some(c) if c.is_ascii_alphanumeric())
        && b.all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'/' | b'-'))
}

/// RFC 3339 UTC instant: `YYYY-MM-DDTHH:MM:SS[.fraction]Z`, 1 to 9 fraction digits (`$defs/utc`).
fn is_utc(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() < 20 || b[b.len() - 1] != b'Z' {
        return false;
    }
    for (i, c) in b"dddd-dd-ddTdd:dd:dd".iter().enumerate() {
        let ok = if *c == b'd' { b[i].is_ascii_digit() } else { b[i] == *c };
        if !ok {
            return false;
        }
    }
    let frac = &b[19..b.len() - 1];
    frac.is_empty() || (frac[0] == b'.' && (2..=10).contains(&frac.len()) && frac[1..].iter().all(u8::is_ascii_digit))
}

fn utc(o: &Obj, k: &str, at: &str) -> Result<String, EntryError> {
    let s = string(o, k, at)?;
    if !is_utc(s) {
        return Err(err(&format!("{at}.{k}"), "expected an RFC 3339 UTC instant ending in Z"));
    }
    Ok(s.to_string())
}

fn digest_value(v: &CValue, at: &str) -> Result<Digest, EntryError> {
    parse_digest(as_str(v, at)?).ok_or_else(|| err(at, "expected sha256: and 64 lowercase hex digits"))
}

fn digest(o: &Obj, k: &str, at: &str) -> Result<Digest, EntryError> {
    digest_value(get(o, k, at)?, &format!("{at}.{k}"))
}

fn opt_digest(o: &Obj, k: &str, at: &str) -> Result<Option<Digest>, EntryError> {
    match o.get(k) {
        Some(v) => digest_value(v, &format!("{at}.{k}")).map(Some),
        None => Ok(None),
    }
}

/// A required field that is a digest or `null`.
fn digest_or_null(o: &Obj, k: &str, at: &str) -> Result<Option<Digest>, EntryError> {
    match get(o, k, at)? {
        CValue::Null => Ok(None),
        v => digest_value(v, &format!("{at}.{k}")).map(Some),
    }
}

/// A 64-bit unsigned integer as a decimal string without leading zeros (`$defs/u64`).
fn u64_field(o: &Obj, k: &str, at: &str) -> Result<u64, EntryError> {
    match get(o, k, at)? {
        CValue::U64(n) => Ok(*n),
        CValue::Str(s) => {
            let canonical = !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()) && (s == "0" || !s.starts_with('0'));
            match s.parse::<u64>() {
                Ok(n) if canonical => Ok(n),
                _ => Err(err(&format!("{at}.{k}"), "expected a decimal string of a 64-bit unsigned integer")),
            }
        }
        _ => Err(err(&format!("{at}.{k}"), "expected a decimal string")),
    }
}

fn u32_field(o: &Obj, k: &str, at: &str) -> Result<u32, EntryError> {
    match get(o, k, at)? {
        CValue::Num(f) if f.fract() == 0.0 && *f >= 0.0 && *f <= f64::from(u32::MAX) => Ok(*f as u32),
        _ => Err(err(&format!("{at}.{k}"), "expected an integer from 0 to 4294967295")),
    }
}

fn algorithms(o: &Obj, at: &str) -> Result<Vec<AlgorithmId>, EntryError> {
    let mut out = Vec::new();
    for (i, v) in array(o, "algorithms", at)?.iter().enumerate() {
        let here = format!("{at}.algorithms[{i}]");
        out.push(
            AlgorithmId::parse(as_str(v, &here)?).map_err(|_| err(&here, "expected module.feature.method@x.y.z"))?,
        );
    }
    Ok(out)
}

fn command_id(o: &Obj, at: &str) -> Result<String, EntryError> {
    let id = string(o, "id", at)?;
    if !CommandName::is_valid(id) {
        return Err(err(&format!("{at}.id"), "expected a dotted lowercase command ID"));
    }
    Ok(id.to_string())
}

fn class(o: &Obj, at: &str) -> Result<SideEffect, EntryError> {
    SideEffect::parse(string(o, "class", at)?).map_err(|_| err(&format!("{at}.class"), "unknown side-effect class"))
}

fn determinism(o: &Obj, at: &str) -> Result<Determinism, EntryError> {
    Determinism::parse(string(o, "determinism", at)?)
        .map_err(|_| err(&format!("{at}.determinism"), "unknown determinism class"))
}

/// `command.event_sources`: an array of [`EventSource`] wire strings.
fn event_sources(c: &Obj, at: &str) -> Result<Vec<EventSource>, EntryError> {
    let mut out = Vec::new();
    for (i, v) in array(c, "event_sources", at)?.iter().enumerate() {
        let here = format!("{at}.event_sources[{i}]");
        out.push(EventSource::parse(as_str(v, &here)?).map_err(|_| err(&here, "unknown event source"))?);
    }
    Ok(out)
}

/// The schema version of a header: 1 or 2.
fn version(o: &Obj, at: &str) -> Result<u32, EntryError> {
    match get(o, "schema_version", at)? {
        CValue::Num(f) if *f == f64::from(SCHEMA_V1) => Ok(SCHEMA_V1),
        CValue::Num(f) if *f == f64::from(SCHEMA_V2) => Ok(SCHEMA_V2),
        _ => Err(err(at, "schema_version must be 1 or 2")),
    }
}

/// A field that exists only from schema version 2 on: absent in version 1, required in
/// version 2.
fn v2_field<'a>(o: &'a Obj, k: &str, version: u32, at: &str) -> Result<Option<&'a CValue>, EntryError> {
    match (o.get(k), version) {
        (None, SCHEMA_V1) => Ok(None),
        (None, _) => Err(err(at, &format!("missing field {k} (schema version 2)"))),
        (Some(_), SCHEMA_V1) => Err(err(&format!("{at}.{k}"), "not a field of schema version 1")),
        (Some(v), _) => Ok(Some(v)),
    }
}

fn tagged_inputs(o: &Obj, at: &str) -> Result<Vec<TaggedDigest>, EntryError> {
    let mut out = Vec::new();
    for (i, v) in array(o, "inputs", at)?.iter().enumerate() {
        let here = format!("{at}.inputs[{i}]");
        let t = obj(v, &here)?;
        shaped(t, &here, &INPUT_SHAPE)?;
        out.push(TaggedDigest { tag: ident(t, "tag", &here)?, digest: digest(t, "digest", &here)? });
    }
    Ok(out)
}

fn time(o: &Obj, at: &str) -> Result<EntryTime, EntryError> {
    let here = format!("{at}.time");
    let t = obj(get(o, "time", at)?, &here)?;
    shaped(t, &here, &TIME_SHAPE)?;
    Ok(EntryTime {
        utc: utc(t, "utc", &here)?,
        mono_ns: u64_field(t, "mono_ns", &here)?,
        sync: text(t, "sync", &here)?,
    })
}

fn manifests(o: &Obj, at: &str) -> Result<Manifests, EntryError> {
    let here = format!("{at}.manifests");
    let m = obj(get(o, "manifests", at)?, &here)?;
    shaped(m, &here, &MANIFESTS_SHAPE)?;
    Ok(Manifests { registry: digest(m, "registry", &here)?, kernel: digest(m, "kernel", &here)? })
}

fn head_move(o: &Obj, at: &str) -> Result<HeadMove, EntryError> {
    let here = format!("{at}.head_move");
    let h = obj(get(o, "head_move", at)?, &here)?;
    shaped(h, &here, &HEAD_MOVE_SHAPE)?;
    Ok(HeadMove { from: digest(h, "from", &here)?, to: digest(h, "to", &here)? })
}

fn session(o: &Obj, at: &str) -> Result<SessionInfo, EntryError> {
    let here = format!("{at}.session");
    let s = obj(get(o, "session", at)?, &here)?;
    shaped(s, &here, &SESSION_SHAPE)?;
    Ok(SessionInfo { session_id: text(s, "session_id", &here)?, app_version: text(s, "app_version", &here)? })
}

/// `logical_parent`: a digest or `null`, never the zero digest (one encoding for "none").
fn logical_parent(o: &Obj, at: &str) -> Result<Option<Digest>, EntryError> {
    match digest_or_null(o, "logical_parent", at)? {
        Some(d) if d == TOMBSTONE => Err(err(&format!("{at}.logical_parent"), "use null, not the zero digest")),
        other => Ok(other),
    }
}

/// `seq`: a 64-bit decimal string starting at 1.
fn seq(o: &Obj, at: &str) -> Result<u64, EntryError> {
    match u64_field(o, "seq", at)? {
        0 => Err(err(&format!("{at}.seq"), "sequences start at 1")),
        n => Ok(n),
    }
}

/// The key that holds the kind-specific part.
fn body_key(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::Command => "command",
        EntryKind::SessionHeader => "session",
        EntryKind::PrincipalUpgrade => "upgrade",
        EntryKind::HeadMove => "head_move",
    }
}

// ------------------------------------------------------------------ schema constants
//
// `schema/entry.schema.json` must agree with these; a test checks it.

/// `$defs/digest`.
pub const DIGEST_PATTERN: &str = "^sha256:[0-9a-f]{64}$";
/// `$defs/u64`: a decimal string from 0 to 18446744073709551615 without leading zeros.
pub const U64_PATTERN: &str = "^(0|[1-9][0-9]{0,18}|1[0-7][0-9]{18}|18[0-3][0-9]{17}|184[0-3][0-9]{16}|1844[0-5][0-9]{15}|18446[0-6][0-9]{14}|184467[0-3][0-9]{13}|1844674[0-3][0-9]{12}|184467440[0-6][0-9]{10}|1844674407[0-2][0-9]{9}|18446744073[0-6][0-9]{8}|1844674407370[0-8][0-9]{6}|18446744073709[0-4][0-9]{5}|184467440737095[0-4][0-9]{4}|18446744073709550[0-9]{3}|18446744073709551[0-5][0-9]{2}|1844674407370955160[0-9]|1844674407370955161[0-4]|18446744073709551615)$";
/// `$defs/seq`: as `$defs/u64` but starting at 1.
pub const SEQ_PATTERN: &str = "^([1-9][0-9]{0,18}|1[0-7][0-9]{18}|18[0-3][0-9]{17}|184[0-3][0-9]{16}|1844[0-5][0-9]{15}|18446[0-6][0-9]{14}|184467[0-3][0-9]{13}|1844674[0-3][0-9]{12}|184467440[0-6][0-9]{10}|1844674407[0-2][0-9]{9}|18446744073[0-6][0-9]{8}|1844674407370[0-8][0-9]{6}|18446744073709[0-4][0-9]{5}|184467440737095[0-4][0-9]{4}|18446744073709550[0-9]{3}|18446744073709551[0-5][0-9]{2}|1844674407370955160[0-9]|1844674407370955161[0-4]|18446744073709551615)$";
/// `$defs/ident`.
pub const IDENT_PATTERN: &str = "^[a-z][a-z0-9_]*$";
/// `$defs/branch`.
pub const BRANCH_PATTERN: &str = "^[A-Za-z0-9][A-Za-z0-9._/-]*$";
/// `$defs/utc`.
pub const UTC_PATTERN: &str = "^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(\\.[0-9]{1,9})?Z$";
/// `$defs/command_id`, the registry's command-ID pattern.
pub const COMMAND_ID_PATTERN: &str = CommandName::PATTERN;
/// `$defs/algorithm`: `module.feature.method@x.y.z`, each version part a u32.
pub const ALGORITHM_PATTERN: &str = "^[a-z][a-z0-9_]*\\.[a-z][a-z0-9_]*\\.[a-z][a-z0-9_]*@(0|[1-9][0-9]{0,8}|[1-3][0-9]{9}|4[0-1][0-9]{8}|42[0-8][0-9]{7}|429[0-3][0-9]{6}|4294[0-8][0-9]{5}|42949[0-5][0-9]{4}|429496[0-6][0-9]{3}|4294967[0-1][0-9]{2}|42949672[0-8][0-9]|429496729[0-4]|4294967295)\\.(0|[1-9][0-9]{0,8}|[1-3][0-9]{9}|4[0-1][0-9]{8}|42[0-8][0-9]{7}|429[0-3][0-9]{6}|4294[0-8][0-9]{5}|42949[0-5][0-9]{4}|429496[0-6][0-9]{3}|4294967[0-1][0-9]{2}|42949672[0-8][0-9]|429496729[0-4]|4294967295)\\.(0|[1-9][0-9]{0,8}|[1-3][0-9]{9}|4[0-1][0-9]{8}|42[0-8][0-9]{7}|429[0-3][0-9]{6}|4294[0-8][0-9]{5}|42949[0-5][0-9]{4}|429496[0-6][0-9]{3}|4294967[0-1][0-9]{2}|42949672[0-8][0-9]|429496729[0-4]|4294967295)$";

/// Keys of the kind-specific bodies; exactly one is present.
pub const BODY_KEYS: [&str; 4] = ["command", "session", "upgrade", "head_move"];

// ------------------------------------------------------------------ headers

/// Checks a header value against the rules of `schema/entry.schema.json`; any
/// violation is a `schema` error (REQ-011).
pub fn validate_header_value(v: &CValue) -> Result<(), EntryError> {
    let at = "header";
    let o = obj(v, at)?;
    let kind = EntryKind::parse(string(o, "kind", at)?)?;
    let body = body_key(kind);
    let mut required = HEADER_SHAPE.required.to_vec();
    required.push(body);
    let optional: Vec<&str> = HEADER_SHAPE.optional.iter().copied().filter(|k| !BODY_KEYS.contains(k)).collect();
    fields(o, at, &required, &optional)?;
    let version = version(o, at)?;
    let coedited = match v2_field(o, "agent_coedited", version, at)? {
        None | Some(CValue::Bool(false)) => false,
        Some(CValue::Bool(true)) => true,
        Some(_) => return Err(err("header.agent_coedited", "expected a boolean")),
    };
    // Whether some event was added by a bus stage, not by the handler (version 2 only).
    let mut bus_added = false;
    ident(o, "stream", at)?;
    seq(o, at)?;
    digest(o, "prev", at)?;
    let branch = string(o, "branch", at)?;
    if !is_branch(branch) {
        return Err(err("header.branch", "expected a branch name"));
    }
    logical_parent(o, at)?;
    time(o, at)?;
    let p = obj(get(o, "principal", at)?, "header.principal")?;
    shaped(p, "header.principal", &PRINCIPAL_SHAPE)?;
    parse_principal_kind(string(p, "kind", "header.principal")?)?;
    digest(p, "commit", "header.principal")?;
    opt_digest(p, "on_behalf_of_commit", "header.principal")?;
    AiAssistance::parse(string(o, "ai_assistance", at)?)?;
    manifests(o, at)?;
    opt_digest(o, "reason_commit", at)?;
    opt_text(o, "human_assertion", at)?;
    digest(o, "hash", at)?;
    let here = format!("{at}.{body}");
    match kind {
        EntryKind::Command => {
            let c = obj(get(o, body, at)?, &here)?;
            shaped(c, &here, &COMMAND_SHAPE)?;
            command_id(c, &here)?;
            u32_field(c, "major", &here)?;
            class(c, &here)?;
            determinism(c, &here)?;
            u64_field(c, "seed", &here)?;
            digest(c, "args_commit", &here)?;
            digest(c, "events_commit", &here)?;
            tagged_inputs(c, &here)?;
            for (i, v) in array(c, "outputs", &here)?.iter().enumerate() {
                let oat = format!("{here}.outputs[{i}]");
                let out = obj(v, &oat)?;
                shaped(out, &oat, &OUTPUT_SHAPE)?;
                let d = digest(out, "digest", &oat)?;
                if opt_text(out, "entity", &oat)?.is_none() && d == TOMBSTONE {
                    return Err(err(&oat, "the tombstone digest needs an entity"));
                }
            }
            algorithms(c, &here)?;
            if v2_field(c, "event_sources", version, &here)?.is_some() {
                bus_added = event_sources(c, &here)?.iter().any(|e| *e != EventSource::Handler);
            }
        }
        EntryKind::SessionHeader => {
            session(o, at)?;
        }
        EntryKind::PrincipalUpgrade => {
            let u = obj(get(o, body, at)?, &here)?;
            shaped(u, &here, &UPGRADE_SHAPE)?;
            text(u, "session_id", &here)?;
            ConfirmMethod::parse(string(u, "method", &here)?)?;
            digest(u, "identity_commit", &here)?;
            utc(u, "confirmed_at", &here)?;
        }
        EntryKind::HeadMove => {
            head_move(o, at)?;
        }
    }
    // One encoding per content: a version 2 header records something version 1 cannot.
    if version == SCHEMA_V2 && !coedited && !bus_added {
        return Err(err(
            at,
            "schema version 2 with agent_coedited false and every event from the handler; write version 1",
        ));
    }
    Ok(())
}

/// Checks a recorded header on its own (no payload needed): the schema rules, then
/// that `hash` equals SHA-256 of the RFC 8785 header without `hash`. Returns the hash.
pub fn verify_header_value(v: &CValue) -> Result<Digest, EntryError> {
    validate_header_value(v)?;
    let mut o = obj(v, "header")?.clone();
    let recorded = match o.remove("hash") {
        Some(h) => digest_value(&h, "header.hash")?,
        None => return Err(err("header", "missing field hash")),
    };
    if digest_bytes(&to_canonical_bytes(&CValue::Obj(o))?) == recorded {
        Ok(recorded)
    } else {
        Err(EntryError::HashMismatch)
    }
}

/// [`EntryHeader::from_value`]: [`verify_header_value`], then the typed fields, then a check
/// that the typed header writes back exactly the bytes it was read from.
pub(super) fn header_from_value(v: &CValue) -> Result<EntryHeader, EntryError> {
    let hash = verify_header_value(v)?;
    let at = "header";
    let o = obj(v, at)?;
    let kind = EntryKind::parse(string(o, "kind", at)?)?;
    let pat = "header.principal";
    let p = obj(get(o, "principal", at)?, pat)?;
    let principal = PrincipalCommit {
        kind: parse_principal_kind(string(p, "kind", pat)?)?,
        commit: digest(p, "commit", pat)?,
        on_behalf_of_commit: opt_digest(p, "on_behalf_of_commit", pat)?,
    };
    let here = format!("{at}.{}", body_key(kind));
    let body = match kind {
        EntryKind::Command => {
            let c = obj(get(o, "command", at)?, &here)?;
            let mut outputs = Vec::new();
            for (i, v) in array(c, "outputs", &here)?.iter().enumerate() {
                let oat = format!("{here}.outputs[{i}]");
                let out = obj(v, &oat)?;
                outputs.push(OutputDigest {
                    entity: opt_text(out, "entity", &oat)?,
                    digest: digest(out, "digest", &oat)?,
                });
            }
            let sources = if c.contains_key("event_sources") { Some(event_sources(c, &here)?) } else { None };
            HeaderBody::Command(CommandHeader {
                id: command_id(c, &here)?,
                major: u32_field(c, "major", &here)?,
                class: class(c, &here)?,
                determinism: determinism(c, &here)?,
                seed: u64_field(c, "seed", &here)?,
                args_commit: digest(c, "args_commit", &here)?,
                events_commit: digest(c, "events_commit", &here)?,
                inputs: tagged_inputs(c, &here)?,
                outputs,
                algorithms: algorithms(c, &here)?,
                event_sources: sources,
            })
        }
        EntryKind::SessionHeader => HeaderBody::SessionHeader(session(o, at)?),
        EntryKind::PrincipalUpgrade => {
            let u = obj(get(o, "upgrade", at)?, &here)?;
            HeaderBody::PrincipalUpgrade(UpgradeHeader {
                session_id: text(u, "session_id", &here)?,
                method: ConfirmMethod::parse(string(u, "method", &here)?)?,
                identity_commit: digest(u, "identity_commit", &here)?,
                confirmed_at: utc(u, "confirmed_at", &here)?,
            })
        }
        EntryKind::HeadMove => HeaderBody::HeadMove(head_move(o, at)?),
    };
    let header = EntryHeader {
        stream: string(o, "stream", at)?.to_string(),
        seq: seq(o, at)?,
        prev: digest(o, "prev", at)?,
        branch: string(o, "branch", at)?.to_string(),
        logical_parent: logical_parent(o, at)?,
        time: time(o, at)?,
        principal,
        ai_assistance: AiAssistance::parse(string(o, "ai_assistance", at)?)?,
        manifests: manifests(o, at)?,
        reason_commit: opt_digest(o, "reason_commit", at)?,
        human_assertion: opt_text(o, "human_assertion", at)?,
        agent_coedited: matches!(o.get("agent_coedited"), Some(CValue::Bool(true))),
        body,
        hash,
    };
    if to_canonical_bytes(&header.to_value())? != to_canonical_bytes(v)? {
        return Err(err(at, "the header does not read back to the same bytes"));
    }
    Ok(header)
}

// ------------------------------------------------------------------ drafts

pub(super) fn draft_from_value(v: &CValue) -> Result<EntryDraft, EntryError> {
    let at = "draft";
    let o = obj(v, at)?;
    let kind = EntryKind::parse(string(o, "kind", at)?)?;
    let key = body_key(kind);
    let required =
        ["kind", "stream", "branch", "logical_parent", "time", "principal", "ai_assistance", "manifests", key];
    fields(o, at, &required, &["reason", "human_assertion", "agent_coedited"])?;
    let agent_coedited = match o.get("agent_coedited") {
        None | Some(CValue::Bool(false)) => false,
        Some(CValue::Bool(true)) => true,
        Some(_) => return Err(err("draft.agent_coedited", "expected a boolean")),
    };

    let pat = "draft.principal";
    let p = obj(get(o, "principal", at)?, pat)?;
    fields(p, pat, &["kind", "identity"], &["on_behalf_of"])?;
    let principal = PrincipalDraft {
        kind: parse_principal_kind(string(p, "kind", pat)?)?,
        identity: get(p, "identity", pat)?.clone(),
        on_behalf_of: p.get("on_behalf_of").cloned(),
    };

    let here = format!("{at}.{key}");
    let body = match kind {
        EntryKind::Command => {
            let c = obj(get(o, key, at)?, &here)?;
            fields(
                c,
                &here,
                &["id", "major", "class", "determinism", "seed", "args", "events", "inputs", "outputs", "algorithms"],
                &["consumed_r1", "event_sources"],
            )?;
            let inputs = tagged_inputs(c, &here)?;
            if inputs.iter().any(|t| t.tag == R1_RESULT_TAG) {
                return Err(err(&format!("{here}.inputs"), "tag r1_result comes only from consumed_r1"));
            }
            let consumed_r1 = match c.get("consumed_r1") {
                None => Vec::new(),
                Some(_) => array(c, "consumed_r1", &here)?
                    .iter()
                    .enumerate()
                    .map(|(i, d)| digest_value(d, &format!("{here}.consumed_r1[{i}]")))
                    .collect::<Result<_, _>>()?,
            };
            let mut outputs = Vec::new();
            for (i, v) in array(c, "outputs", &here)?.iter().enumerate() {
                let oat = format!("{here}.outputs[{i}]");
                let out = obj(v, &oat)?;
                fields(out, &oat, &[], &["entity", "digest", "deleted"])?;
                let entity = opt_text(out, "entity", &oat)?;
                let deleted = match out.get("deleted") {
                    None | Some(CValue::Bool(false)) => false,
                    Some(CValue::Bool(true)) => true,
                    Some(_) => return Err(err(&oat, "deleted must be a boolean")),
                };
                let d = match (deleted, out.contains_key("digest"), &entity) {
                    (true, false, Some(_)) => TOMBSTONE,
                    (true, _, _) => return Err(err(&oat, "a deleted output names its entity and has no digest")),
                    (false, _, _) => digest(out, "digest", &oat)?,
                };
                outputs.push(OutputDigest { entity, digest: d });
            }
            DraftBody::Command(CommandDraft {
                id: command_id(c, &here)?,
                major: u32_field(c, "major", &here)?,
                class: class(c, &here)?,
                determinism: determinism(c, &here)?,
                seed: u64_field(c, "seed", &here)?,
                args: get(c, "args", &here)?.clone(),
                events: array(c, "events", &here)?.to_vec(),
                inputs,
                consumed_r1,
                outputs,
                algorithms: algorithms(c, &here)?,
                event_sources: if c.contains_key("event_sources") { event_sources(c, &here)? } else { Vec::new() },
            })
        }
        EntryKind::SessionHeader => DraftBody::SessionHeader(session(o, at)?),
        EntryKind::PrincipalUpgrade => {
            let u = obj(get(o, key, at)?, &here)?;
            fields(u, &here, &["session_id", "method", "identity", "confirmed_at"], &[])?;
            DraftBody::PrincipalUpgrade(UpgradeDraft {
                session_id: text(u, "session_id", &here)?,
                method: ConfirmMethod::parse(string(u, "method", &here)?)?,
                identity: get(u, "identity", &here)?.clone(),
                confirmed_at: utc(u, "confirmed_at", &here)?,
            })
        }
        EntryKind::HeadMove => DraftBody::HeadMove(head_move(o, at)?),
    };

    let branch = string(o, "branch", at)?.to_string();
    Ok(EntryDraft {
        stream: string(o, "stream", at)?.to_string(),
        branch,
        logical_parent: logical_parent(o, at)?,
        time: time(o, at)?,
        principal,
        ai_assistance: AiAssistance::parse(string(o, "ai_assistance", at)?)?,
        manifests: manifests(o, at)?,
        reason: o.get("reason").map(|r| as_str(r, "draft.reason").map(str::to_string)).transpose()?,
        human_assertion: opt_text(o, "human_assertion", at)?,
        agent_coedited,
        body,
    })
}
