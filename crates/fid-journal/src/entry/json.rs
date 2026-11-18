//! JSON side of the entry model: reading drafts, and checking headers against the rules
//! of `schema/entry.schema.json` (REQ-011). The checks mirror the published schema one
//! for one; `tools/check_entries.py` runs the same fixtures through the schema itself.

use super::{
    parse_digest, AiAssistance, CommandDraft, ConfirmMethod, DraftBody, EntryDraft, EntryError, EntryKind, EntryTime,
    HeadMove, Manifests, OutputDigest, PrincipalDraft, PrincipalKind, SessionInfo, TaggedDigest, UpgradeDraft,
    SCHEMA_VERSION, TOMBSTONE,
};
use fid_canon::{digest_bytes, to_canonical_bytes, CValue, Digest};
use fid_kci::AlgorithmId;
use fid_registry::{Determinism, SideEffect};
use std::collections::BTreeMap;

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

/// `^[a-z0-9]([a-z0-9_.]*[a-z0-9])?$`, the registry's command-ID pattern.
fn is_command_id(s: &str) -> bool {
    let end_ok = |c: u8| c.is_ascii_lowercase() || c.is_ascii_digit();
    let b = s.as_bytes();
    match (b.first(), b.last()) {
        (Some(f), Some(l)) => {
            end_ok(*f) && end_ok(*l) && b.iter().all(|c| end_ok(*c) || *c == b'_' || *c == b'.')
        }
        _ => false,
    }
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
        out.push(AlgorithmId::parse(as_str(v, &here)?).map_err(|_| err(&here, "expected module.feature.method@x.y.z"))?);
    }
    Ok(out)
}

fn command_id(o: &Obj, at: &str) -> Result<String, EntryError> {
    let id = string(o, "id", at)?;
    if !is_command_id(id) {
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

fn tagged_inputs(o: &Obj, at: &str) -> Result<Vec<TaggedDigest>, EntryError> {
    let mut out = Vec::new();
    for (i, v) in array(o, "inputs", at)?.iter().enumerate() {
        let here = format!("{at}.inputs[{i}]");
        let t = obj(v, &here)?;
        fields(t, &here, &["tag", "digest"], &[])?;
        out.push(TaggedDigest { tag: ident(t, "tag", &here)?, digest: digest(t, "digest", &here)? });
    }
    Ok(out)
}

fn time(o: &Obj, at: &str) -> Result<EntryTime, EntryError> {
    let here = format!("{at}.time");
    let t = obj(get(o, "time", at)?, &here)?;
    fields(t, &here, &["utc", "mono_ns", "sync"], &[])?;
    Ok(EntryTime { utc: utc(t, "utc", &here)?, mono_ns: u64_field(t, "mono_ns", &here)?, sync: text(t, "sync", &here)? })
}

fn manifests(o: &Obj, at: &str) -> Result<Manifests, EntryError> {
    let here = format!("{at}.manifests");
    let m = obj(get(o, "manifests", at)?, &here)?;
    fields(m, &here, &["registry", "kernel"], &[])?;
    Ok(Manifests { registry: digest(m, "registry", &here)?, kernel: digest(m, "kernel", &here)? })
}

fn head_move(o: &Obj, at: &str) -> Result<HeadMove, EntryError> {
    let here = format!("{at}.head_move");
    let h = obj(get(o, "head_move", at)?, &here)?;
    fields(h, &here, &["from", "to"], &[])?;
    Ok(HeadMove { from: digest(h, "from", &here)?, to: digest(h, "to", &here)? })
}

fn session(o: &Obj, at: &str) -> Result<SessionInfo, EntryError> {
    let here = format!("{at}.session");
    let s = obj(get(o, "session", at)?, &here)?;
    fields(s, &here, &["session_id", "app_version"], &[])?;
    Ok(SessionInfo { session_id: text(s, "session_id", &here)?, app_version: text(s, "app_version", &here)? })
}

fn signatures(o: &Obj, at: &str) -> Result<Vec<String>, EntryError> {
    let mut out = Vec::new();
    for (i, v) in array(o, "signatures", at)?.iter().enumerate() {
        out.push(text_value(v, &format!("{at}.signatures[{i}]"))?);
    }
    Ok(out)
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

// ------------------------------------------------------------------ headers

const HEADER_REQUIRED: [&str; 13] = [
    "schema_version",
    "kind",
    "stream",
    "seq",
    "prev",
    "branch",
    "logical_parent",
    "time",
    "principal",
    "ai_assistance",
    "manifests",
    "signatures",
    "hash",
];

/// Checks a header value against the rules of `schema/entry.schema.json`; any
/// violation is a `schema` error (REQ-011).
pub fn validate_header_value(v: &CValue) -> Result<(), EntryError> {
    let at = "header";
    let o = obj(v, at)?;
    let kind = EntryKind::parse(string(o, "kind", at)?)?;
    let body = body_key(kind);
    let mut required = HEADER_REQUIRED.to_vec();
    required.push(body);
    fields(o, at, &required, &["reason_commit", "human_assertion"])?;
    match get(o, "schema_version", at)? {
        CValue::Num(f) if *f == f64::from(SCHEMA_VERSION) => {}
        _ => return Err(err(at, "schema_version must be 1")),
    }
    ident(o, "stream", at)?;
    u64_field(o, "seq", at)?;
    digest_or_null(o, "prev", at)?;
    let branch = string(o, "branch", at)?;
    if !is_branch(branch) {
        return Err(err("header.branch", "expected a branch name"));
    }
    digest_or_null(o, "logical_parent", at)?;
    time(o, at)?;
    let p = obj(get(o, "principal", at)?, "header.principal")?;
    fields(p, "header.principal", &["kind", "commit"], &["on_behalf_of_commit"])?;
    PrincipalKind::parse(string(p, "kind", "header.principal")?)?;
    digest(p, "commit", "header.principal")?;
    opt_digest(p, "on_behalf_of_commit", "header.principal")?;
    AiAssistance::parse(string(o, "ai_assistance", at)?)?;
    manifests(o, at)?;
    opt_digest(o, "reason_commit", at)?;
    opt_text(o, "human_assertion", at)?;
    signatures(o, at)?;
    digest(o, "hash", at)?;
    let here = format!("{at}.{body}");
    match kind {
        EntryKind::Command => {
            let c = obj(get(o, body, at)?, &here)?;
            fields(
                c,
                &here,
                &[
                    "id",
                    "major",
                    "class",
                    "determinism",
                    "seed",
                    "args_commit",
                    "events_commit",
                    "inputs",
                    "outputs",
                    "algorithms",
                ],
                &[],
            )?;
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
                fields(out, &oat, &["digest"], &["entity"])?;
                digest(out, "digest", &oat)?;
                opt_text(out, "entity", &oat)?;
            }
            algorithms(c, &here)?;
        }
        EntryKind::SessionHeader => {
            session(o, at)?;
        }
        EntryKind::PrincipalUpgrade => {
            let u = obj(get(o, body, at)?, &here)?;
            fields(u, &here, &["session_id", "method", "identity_commit", "confirmed_at"], &[])?;
            text(u, "session_id", &here)?;
            ConfirmMethod::parse(string(u, "method", &here)?)?;
            digest(u, "identity_commit", &here)?;
            utc(u, "confirmed_at", &here)?;
        }
        EntryKind::HeadMove => {
            head_move(o, at)?;
        }
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

// ------------------------------------------------------------------ drafts

pub(super) fn draft_from_value(v: &CValue) -> Result<EntryDraft, EntryError> {
    let at = "draft";
    let o = obj(v, at)?;
    let kind = EntryKind::parse(string(o, "kind", at)?)?;
    let key = body_key(kind);
    let required = ["kind", "stream", "branch", "logical_parent", "time", "principal", "ai_assistance", "manifests", key];
    fields(o, at, &required, &["reason", "human_assertion", "signatures"])?;

    let pat = "draft.principal";
    let p = obj(get(o, "principal", at)?, pat)?;
    fields(p, pat, &["kind", "identity"], &["on_behalf_of"])?;
    let principal = PrincipalDraft {
        kind: PrincipalKind::parse(string(p, "kind", pat)?)?,
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
                &["consumed_r1"],
            )?;
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
                inputs: tagged_inputs(c, &here)?,
                consumed_r1,
                outputs,
                algorithms: algorithms(c, &here)?,
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
        logical_parent: digest_or_null(o, "logical_parent", at)?,
        time: time(o, at)?,
        principal,
        ai_assistance: AiAssistance::parse(string(o, "ai_assistance", at)?)?,
        manifests: manifests(o, at)?,
        reason: o.get("reason").map(|r| as_str(r, "draft.reason").map(str::to_string)).transpose()?,
        human_assertion: opt_text(o, "human_assertion", at)?,
        signatures: match o.get("signatures") {
            Some(_) => signatures(o, at)?,
            None => Vec::new(),
        },
        body,
    })
}
