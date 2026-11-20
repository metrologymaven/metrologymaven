//! Verification and listing over a project database (feature `store`).
//!
//! # Pinned rules
//!
//! - **Read only (REQ-006).** Both commands only run `SELECT`s through
//!   [`fid_store::Store::read`] and read blob files; they open no transaction that commits
//!   and write nothing to the Record Journal, the head cache or the blob store. They work on
//!   a project opened read-only ([`fid_store::OpenMode::Read`]) and do not need a [`Journal`]
//!   (whose open refuses a broken chain): [`verify_store`] and [`list_store`] take the store.
//! - **Rows.** Entries are streamed in `seq` order from the first sequence verified, one row
//!   in memory at a time; a header longer than the bound is not loaded (its length is read
//!   with `length(header)`). Columns are read by type without trusting the declared types: a
//!   text column that is not text reads as empty, which never matches a header. The
//!   `entry_blobs` rows of an entry are read by its stored `hash` column (at most
//!   [`super::MAX_ENTRY_REFS`] + 1). Checkpoint rows are read in id order (at most
//!   [`super::MAX_CHECKPOINTS`] + 1), bodies cut after 4097 bytes.
//! - **Blobs.** A referenced blob is checked with the store's pre-commit check
//!   ([`fid_store::Txn::check_blob`]: the file hashes to its address; for a chunked object the
//!   manifest and every chunk), on a transaction that is never committed. A payload blob is
//!   read from its file (at most [`super::MAX_PAYLOAD_BYTES`] + 1 bytes; a link or other
//!   non-regular file reads as corrupt) and hashed by the verifier.
//! - **Stream.** One stream per database (`CORE.JRN.CHAIN.APPEND`): the journal's stream is
//!   given by the caller (a [`Journal`]'s own); a `stream` parameter naming another is
//!   `not_found/unknown_stream`.
//! - **from_seq.** 1 to the tail sequence plus one; else `validation/bad_range`. Above 1 the
//!   walk links to the stored `hash` column of entry `from_seq - 1`.
//! - **Branch.** A `branch` filter naming no entry is `not_found/unknown_branch`.

use super::{
    is_stream_name, list_rows, plan_list, verify_rows, BlobSource, BlobState, CheckpointRow, ListPage, ListQuery,
    ReadError, Start, StoredEntry, VerifyOptions, VerifyReport, MAX_HEADER_BYTES, MAX_PAYLOAD_BYTES, MAX_SCAN,
};
use crate::append::Journal;
use crate::entry::parse_digest;
use fid_canon::{CValue, Digest};
use fid_registry::{CommandDef, DefError};
use fid_store::{Store, TxnError};
use fid_types::Digest as StoreDigest;
use std::fs;
use std::io::Read;

/// Command ID of the verify command.
pub const VERIFY_CHAIN: &str = "journal.verify_chain";
/// Command ID of the list command.
pub const LIST_ENTRIES: &str = "journal.list_entries";
const LIFECYCLE_SINCE: &str = "2026-11-20";

fn storage(what: &str, e: impl std::fmt::Display) -> ReadError {
    ReadError::Storage(format!("{what}: {e}"))
}

/// A column read as bytes (`CAST(.. AS BLOB)`): UTF-8 text, else empty.
fn text_of(v: Option<Vec<u8>>) -> String {
    v.and_then(|b| String::from_utf8(b).ok()).unwrap_or_default()
}

/// `seq, hash, stream, branch, kind, prev_hash, header (if within the bound), length(header)`,
/// every text column cast to bytes so a value of another type cannot fail the read.
const ROWS_SQL: &str = "SELECT seq, CAST(hash AS BLOB), CAST(stream AS BLOB), CAST(branch AS BLOB),      CAST(kind AS BLOB), CAST(prev_hash AS BLOB),      CASE WHEN length(CAST(header AS BLOB)) <= ?2 THEN CAST(header AS BLOB) ELSE NULL END,      length(CAST(header AS BLOB)) FROM entries WHERE seq >= ?1 ORDER BY seq LIMIT ?3";
/// At most `MAX_ENTRY_REFS` + 1 rows.
const REFS_SQL: &str = "SELECT CAST(role AS BLOB), CAST(digest AS BLOB) FROM entry_blobs      WHERE entry_hash = ?1 ORDER BY role, digest LIMIT 4098";
/// At most `MAX_CHECKPOINTS` + 1 rows, bodies cut after `MAX_CHECKPOINT_BYTES` + 1 bytes.
const CHECKPOINTS_SQL: &str = "SELECT id, CAST(stream AS BLOB), CAST(entry_hash AS BLOB),      substr(CAST(body AS BLOB), 1, 4097) FROM checkpoints ORDER BY id LIMIT 65537";

type Bytes = Option<Vec<u8>>;
type RowTuple = (i64, Bytes, Bytes, Bytes, Bytes, Bytes, Bytes, Option<i64>);

fn to_i64(n: u64) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

fn stored(t: RowTuple) -> StoredEntry {
    let (seq, hash, stream, branch, kind, prev_hash, header, len) = t;
    StoredEntry {
        seq,
        hash: text_of(hash),
        stream: text_of(stream),
        branch: text_of(branch),
        kind: text_of(kind),
        prev_hash: prev_hash.map(|b| text_of(Some(b))),
        header: header.unwrap_or_default(),
        header_len: len.and_then(|n| u64::try_from(n).ok()).unwrap_or(0),
        refs: Vec::new(),
    }
}

/// The tail sequence (0 for an empty journal).
fn tail_seq(store: &Store) -> Result<u64, ReadError> {
    let max: i64 = store
        .read(|conn| conn.query_row("SELECT coalesce(max(seq), 0) FROM entries", [], |r| r.get(0)))
        .map_err(|e| storage("read tail", e))?;
    u64::try_from(max).map_err(|_| ReadError::Storage("negative tail sequence".into()))
}

fn check_stream(stream: &str, asked: Option<&String>) -> Result<(), ReadError> {
    if !is_stream_name(stream) {
        return Err(ReadError::BadParam(format!("journal stream {stream:?} is not an identifier")));
    }
    match asked {
        Some(s) if s != stream => Err(ReadError::UnknownStream(s.chars().take(64).collect())),
        _ => Ok(()),
    }
}

/// The store's blob files as a [`BlobSource`].
struct StoreBlobs<'a> {
    store: &'a Store,
}

impl BlobSource for StoreBlobs<'_> {
    fn check(&mut self, digest: &Digest) -> Result<BlobState, ReadError> {
        let txn = self.store.transaction();
        let r = txn.check_blob(&StoreDigest::from_bytes(digest.0));
        txn.abort();
        match r {
            Ok(()) => Ok(BlobState::Sound),
            Err(TxnError::DanglingBlob(_)) => Ok(BlobState::Missing),
            Err(TxnError::CorruptBlob(_)) => Ok(BlobState::Corrupt),
            Err(e) => Err(storage(&format!("check blob {digest}"), e)),
        }
    }

    fn payload(&mut self, digest: &Digest) -> Result<Option<Vec<u8>>, ReadError> {
        let path = self.store.blobs().path_of(&StoreDigest::from_bytes(digest.0));
        match fs::symlink_metadata(&path) {
            Ok(m) if m.is_file() => {}
            // A link, directory or device where the blob should be: not the blob's content.
            Ok(_) => return Ok(Some(Vec::new())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(storage(&format!("stat payload {digest}"), e)),
        }
        let file = fs::File::open(&path).map_err(|e| storage(&format!("open payload {digest}"), e))?;
        let mut bytes = Vec::new();
        let limit = u64::try_from(MAX_PAYLOAD_BYTES).unwrap_or(u64::MAX).saturating_add(1);
        file.take(limit).read_to_end(&mut bytes).map_err(|e| storage(&format!("read payload {digest}"), e))?;
        Ok(Some(bytes))
    }
}

/// Verifies the chain of `stream` in `store` (module documentation; [`super::verify_rows`]).
pub fn verify_store(store: &Store, stream: &str, opts: &VerifyOptions) -> Result<VerifyReport, ReadError> {
    check_stream(stream, opts.stream.as_ref())?;
    let tail = tail_seq(store)?;
    let from = opts.from_seq.unwrap_or(1);
    if from == 0 || from > tail.saturating_add(1) {
        return Err(ReadError::BadRange(format!("from_seq {from}, journal tail {tail}")));
    }
    let max_entries = to_i64(opts.max_entries.saturating_add(1));
    let mut blobs = StoreBlobs { store };
    store
        .read(|conn| {
            let start = if from == 1 {
                Start::GENESIS
            } else {
                let below: Bytes = conn
                    .query_row("SELECT CAST(hash AS BLOB) FROM entries WHERE seq = ?1", [to_i64(from - 1)], |r| {
                        r.get(0)
                    })
                    .unwrap_or(None);
                match parse_digest(&text_of(below)) {
                    Some(prev) => Start { seq: from, prev },
                    None => {
                        return Ok(Err(ReadError::BadRange(format!(
                            "entry {} below from_seq has no readable hash",
                            from - 1
                        ))))
                    }
                }
            };
            let mut cps = conn.prepare(CHECKPOINTS_SQL)?;
            let checkpoints = cps.query_map([], |r| {
                Ok(CheckpointRow {
                    id: r.get(0)?,
                    stream: text_of(r.get(1)?),
                    entry_hash: text_of(r.get(2)?),
                    body: r.get::<_, Bytes>(3)?.unwrap_or_default(),
                })
            })?;
            let checkpoints = checkpoints.map(|r| r.map_err(|e| storage("read checkpoints", e)));
            let mut refs_st = conn.prepare(REFS_SQL)?;
            let mut st = conn.prepare(ROWS_SQL)?;
            let header_cap = to_i64(u64::try_from(MAX_HEADER_BYTES).unwrap_or(1 << 20));
            let rows = st.query_map([to_i64(from), header_cap, max_entries], |r| {
                Ok(stored((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?)))
            })?;
            let rows = rows.map(|row| -> Result<StoredEntry, ReadError> {
                let mut row = row.map_err(|e| storage("read entries", e))?;
                let refs = refs_st
                    .query_map([row.hash.as_str()], |r| Ok((text_of(r.get(0)?), text_of(r.get(1)?))))
                    .map_err(|e| storage("read entry_blobs", e))?;
                for r in refs {
                    row.refs.push(r.map_err(|e| storage("read entry_blobs", e))?);
                }
                Ok(row)
            });
            Ok(verify_rows(stream, start, opts, checkpoints, rows, &mut blobs))
        })
        .map_err(|e| storage("verify", e))?
}

/// One page of `journal.list_entries@1` over `stream` in `store` (module documentation;
/// [`super::list_rows`]).
pub fn list_store(store: &Store, stream: &str, q: &ListQuery) -> Result<ListPage, ReadError> {
    check_stream(stream, q.stream.as_ref())?;
    let tail = tail_seq(store)?;
    let plan = plan_list(stream, tail, q)?;
    if let Some(b) = plan.branch() {
        let known: bool = store
            .read(|conn| conn.query_row("SELECT EXISTS (SELECT 1 FROM entries WHERE branch = ?1)", [b], |r| r.get(0)))
            .map_err(|e| storage("read branches", e))?;
        if !known {
            return Err(ReadError::UnknownBranch(b.chars().take(64).collect()));
        }
    }
    let scan = to_i64(u64::try_from(MAX_SCAN).unwrap_or(100_000).saturating_add(1));
    store
        .read(|conn| {
            let mut st = conn.prepare(ROWS_SQL)?;
            let header_cap = to_i64(u64::try_from(MAX_HEADER_BYTES).unwrap_or(1 << 20));
            let rows = st.query_map([to_i64(plan.start), header_cap, scan], |r| {
                Ok(stored((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?)))
            })?;
            Ok(list_rows(&plan, rows.map(|r| r.map_err(|e| storage("read entries", e)))))
        })
        .map_err(|e| storage("list", e))?
}

impl Journal {
    /// `journal.verify_chain@1` over this journal's stream ([`verify_store`]).
    pub fn verify_chain(&self, opts: &VerifyOptions) -> Result<VerifyReport, ReadError> {
        self.check_poison().map_err(|e| ReadError::Storage(e.to_string()))?;
        verify_store(&self.store, &self.stream, opts)
    }

    /// `journal.list_entries@1` over this journal's stream ([`list_store`]).
    pub fn list_entries(&self, q: &ListQuery) -> Result<ListPage, ReadError> {
        self.check_poison().map_err(|e| ReadError::Storage(e.to_string()))?;
        list_store(&self.store, &self.stream, q)
    }
}

// ------------------------------------------------------------------ registry definitions

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn text(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

fn strs(items: &[&str]) -> CValue {
    CValue::Arr(items.iter().map(|s| text(s)).collect())
}

fn string(pattern: &str, description: &str) -> CValue {
    obj([("type", text("string")), ("pattern", text(pattern)), ("description", text(description))])
}

fn object(required: &[&str], properties: CValue) -> CValue {
    obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(required)),
        ("properties", properties),
    ])
}

const SEQ: &str = "^[1-9][0-9]{0,19}$";
const COUNT: &str = "^(0|[1-9][0-9]{0,19})$";
const STREAM: &str = "^[a-z][a-z0-9_]{0,63}$";

fn verify_params() -> CValue {
    object(
        &[],
        obj([
            ("stream", string(STREAM, "Stream to verify; the project's Record Journal when omitted.")),
            ("from_seq", string(SEQ, "First sequence to verify (decimal); 1 when omitted.")),
        ]),
    )
}

fn verify_result() -> CValue {
    let count = |d: &str| string(COUNT, d);
    obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["checked", "ok"])),
        (
            "properties",
            obj([
                ("ok", obj([("type", text("boolean")), ("description", text("True when the chain verified."))])),
                (
                    "failure",
                    object(
                        &["kind", "reason", "seq", "stream"],
                        obj([
                            ("stream", string(STREAM, "The verified stream.")),
                            (
                                "seq",
                                obj([
                                    ("type", strs(&["string", "null"])),
                                    ("description", text("Sequence of the first failure; null for a checkpoint row.")),
                                ]),
                            ),
                            (
                                "kind",
                                obj([
                                    ("type", text("string")),
                                    ("enum", strs(&["hash", "link", "sequence", "blob", "checkpoint"])),
                                    ("description", text("What failed.")),
                                ]),
                            ),
                            ("reason", string("^[a-z][a-z_]*$", "Finer reason within the kind.")),
                            ("checkpoint", string(SEQ, "Checkpoint row id of a checkpoint failure.")),
                        ]),
                    ),
                ),
                (
                    "checked",
                    object(
                        &["blobs", "checkpoints", "entries", "without_payload"],
                        obj([
                            ("entries", count("Entries checked.")),
                            ("blobs", count("Distinct referenced blobs checked.")),
                            ("checkpoints", count("Checkpoints checked.")),
                            ("without_payload", count("Entries whose payload has been removed.")),
                        ]),
                    ),
                ),
            ]),
        ),
    ])
}

fn list_params() -> CValue {
    object(
        &[],
        obj([
            ("stream", string(STREAM, "Stream to list; the project's Record Journal when omitted.")),
            ("branch", string("^[A-Za-z0-9][A-Za-z0-9._/-]*$", "Only entries on this branch.")),
            ("from_seq", string(SEQ, "First sequence (decimal), inclusive.")),
            ("to_seq", string(SEQ, "Last sequence (decimal), inclusive.")),
            ("command", string("^[a-z0-9]([a-z0-9_.]*[a-z0-9])?@[0-9]{1,5}$", "Only entries of this command ID.")),
            (
                "principal_kind",
                obj([
                    ("type", text("string")),
                    ("enum", strs(&["human", "agent", "script", "service", "device", "system"])),
                    ("description", text("Only entries by this principal kind.")),
                ]),
            ),
            (
                "page_token",
                string("^jl1\\.[0-9a-f]{16}\\.[1-9][0-9]{0,19}$", "Continuation token of the previous page."),
            ),
        ]),
    )
}

fn list_result() -> CValue {
    object(
        &["headers"],
        obj([
            (
                "headers",
                obj([
                    ("type", text("array")),
                    ("maxItems", CValue::Num(1000.0)),
                    ("items", obj([("type", text("object")), ("description", text("An entry header."))])),
                    ("description", text("Entry headers in sequence order, at most 1000.")),
                ]),
            ),
            ("next", string("^jl1\\.[0-9a-f]{16}\\.[1-9][0-9]{0,19}$", "Token of the next page, when there is one.")),
        ]),
    )
}

/// The registry definitions of `journal.verify_chain@1` and `journal.list_entries@1`, both
/// R0 and D0 on the CLI and MCP (REQ-006), built through [`CommandDef::from_value`].
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    let specs = [
        (
            VERIFY_CHAIN,
            "Verify journal chain",
            "Recomputes every entry hash and checks every link, sequence step, referenced blob and checkpoint; \
             reports the first failure (stream, sequence, kind) or success with counts.",
            verify_params(),
            verify_result(),
        ),
        (
            LIST_ENTRIES,
            "List journal entries",
            "Lists entry headers in sequence order filtered by stream, branch, sequence range, command and \
             principal kind, at most 1000 per page with a continuation token.",
            list_params(),
            list_result(),
        ),
    ];
    specs
        .into_iter()
        .map(|(id, title, description, params, result)| {
            CommandDef::from_value(obj([
                ("id", text(id)),
                ("major", CValue::Num(1.0)),
                ("title", text(title)),
                ("description", text(description)),
                ("params_schema", params),
                ("result_schema", result),
                ("side_effect", text("R0")),
                ("mcp_projection", text("exposed")),
                ("determinism", text("D0")),
                ("idempotency", text("natural")),
                ("fs_effect", text("none")),
                ("undo", text("not_undoable")),
                ("preconditions", CValue::Arr(Vec::new())),
                ("effects", CValue::Arr(Vec::new())),
                ("algorithms", CValue::Arr(Vec::new())),
                ("exposure", strs(&["cli", "mcp"])),
                ("lifecycle", obj([("state", text("active")), ("since", text(LIFECYCLE_SINCE))])),
                ("human_only", CValue::Bool(false)),
                ("network", text("none")),
                ("web", text("available")),
                ("cli_bare", CValue::Bool(false)),
            ]))
        })
        .collect()
}
