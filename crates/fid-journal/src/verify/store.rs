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
//! - **from_seq.** Anchored as [`super::resolve_start`] pins (a trusted head at `from_seq - 1`,
//!   or a checkpoint row at `from_seq - 1` matching the stored hash, read by `entry_hash`);
//!   anything else is `validation/bad_range`.
//! - **Head cache.** After a successful walk the `heads` rows are checked with
//!   [`super::check_heads`], each entry looked up by its stored hash.
//! - **Verification blocks appends.** The whole walk, blob checks included, runs inside one
//!   [`fid_store::Store::read`], which holds the store's connection mutex: appends and other
//!   reads of the same open project wait until it ends (other processes read the WAL snapshot).
//!   A bounded-window verification that releases the lock between windows is a follow-up.
//! - **Branch.** A `branch` filter naming no entry is `not_found/unknown_branch`.

use super::{
    check_heads, is_stream_name, list_rows, plan_list, resolve_start, verify_rows, BlobSource, BlobState,
    CheckpointRow, ListPage, ListQuery, ReadError, StoredEntry, VerifyOptions, VerifyReport, MAX_HEADER_BYTES,
    MAX_PAYLOAD_BYTES, MAX_SCAN,
};
use crate::append::Journal;
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

/// `seq, hash, stream, branch, kind, prev_hash, header (if within the bound), length(header)`.
/// Every unverified column is cast to bytes (a value of another type cannot fail the read) and
/// cut one byte past its longest valid length, so an over-long value stays longer than any
/// valid one and never matches: digests 71 bytes, stream 64, kind 16, branch at most the
/// header bound (a branch is part of its header).
const ROWS_SQL: &str = concat!(
    "SELECT seq, substr(CAST(hash AS BLOB), 1, 72), substr(CAST(stream AS BLOB), 1, 65), ",
    "substr(CAST(branch AS BLOB), 1, 1048577), substr(CAST(kind AS BLOB), 1, 17), ",
    "substr(CAST(prev_hash AS BLOB), 1, 72), ",
    "CASE WHEN length(CAST(header AS BLOB)) <= ?2 THEN CAST(header AS BLOB) ELSE NULL END, ",
    "length(CAST(header AS BLOB)) FROM entries WHERE seq >= ?1 ORDER BY seq LIMIT ?3"
);
/// At most `MAX_ENTRY_REFS` + 1 rows; role cut at 17 bytes, digest at 72.
const REFS_SQL: &str = concat!(
    "SELECT substr(CAST(role AS BLOB), 1, 17), substr(CAST(digest AS BLOB), 1, 72) FROM entry_blobs ",
    "WHERE entry_hash = ?1 ORDER BY role, digest LIMIT 4098"
);
/// At most `MAX_CHECKPOINTS` + 1 rows; stream cut at 65 bytes, entry hash at 72, body after
/// `MAX_CHECKPOINT_BYTES` + 1.
const CHECKPOINTS_SQL: &str = concat!(
    "SELECT id, substr(CAST(stream AS BLOB), 1, 65), substr(CAST(entry_hash AS BLOB), 1, 72), ",
    "substr(CAST(body AS BLOB), 1, 4097) FROM checkpoints ORDER BY id LIMIT 65537"
);
/// Anchor candidates: checkpoint rows naming one entry hash, at most `MAX_ANCHOR_CANDIDATES`.
const ANCHOR_SQL: &str = concat!(
    "SELECT id, substr(CAST(stream AS BLOB), 1, 65), substr(CAST(entry_hash AS BLOB), 1, 72), ",
    "substr(CAST(body AS BLOB), 1, 4097) FROM checkpoints WHERE entry_hash = ?1 ORDER BY id LIMIT 16"
);
/// The head cache in branch order, at most `MAX_HEADS` + 1 rows.
const HEADS_SQL: &str = concat!(
    "SELECT substr(CAST(branch AS BLOB), 1, 1048577), substr(CAST(entry_hash AS BLOB), 1, 72) ",
    "FROM heads ORDER BY branch LIMIT 65537"
);
/// The branch column of the entry with a given stored hash.
const BRANCH_OF_SQL: &str = "SELECT substr(CAST(branch AS BLOB), 1, 1048577) FROM entries WHERE hash = ?1 LIMIT 1";

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
/// Holds the store's connection mutex for the whole walk, so appends through the same open
/// store wait until it returns.
pub fn verify_store(store: &Store, stream: &str, opts: &VerifyOptions) -> Result<VerifyReport, ReadError> {
    check_stream(stream, opts.stream.as_ref())?;
    let tail = tail_seq(store)?;
    let from = opts.from_seq.unwrap_or(1);
    let max_entries = to_i64(opts.max_entries.saturating_add(1));
    let mut blobs = StoreBlobs { store };
    store
        .read(|conn| {
            // The anchor of a partial verification.
            let below: Bytes = if from > 1 {
                conn.query_row(
                    "SELECT substr(CAST(hash AS BLOB), 1, 72) FROM entries WHERE seq = ?1",
                    [to_i64(from - 1)],
                    |r| r.get(0),
                )
                .unwrap_or(None)
            } else {
                None
            };
            let below = below.map(|b| text_of(Some(b)));
            let mut anchor_st = conn.prepare(ANCHOR_SQL)?;
            let candidates = anchor_st.query_map([below.as_deref().unwrap_or("")], |r| {
                Ok(CheckpointRow {
                    id: r.get(0)?,
                    stream: text_of(r.get(1)?),
                    entry_hash: text_of(r.get(2)?),
                    body: r.get::<_, Bytes>(3)?.unwrap_or_default(),
                })
            })?;
            let candidates = candidates.map(|r| r.map_err(|e| storage("read checkpoints", e)));
            let start = match resolve_start(stream, from, tail, opts.trusted, below.as_deref(), candidates) {
                Ok(s) => s,
                Err(e) => return Ok(Err(e)),
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
            // A full verification reads every row, so a row stored below sequence 1 is seen (and
            // fails as `sequence/reorder`) instead of being skipped by the range.
            let first = if start.seq == 1 { i64::MIN } else { to_i64(start.seq) };
            let rows = st.query_map([first, header_cap, max_entries], |r| {
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
            let mut report = match verify_rows(stream, start, opts, checkpoints, rows, &mut blobs) {
                Ok(r) => r,
                Err(e) => return Ok(Err(e)),
            };
            // The head cache.
            let mut heads_st = conn.prepare(HEADS_SQL)?;
            let heads = heads_st.query_map([], |r| Ok((text_of(r.get(0)?), text_of(r.get(1)?))))?;
            let heads = heads.map(|r| r.map_err(|e| storage("read heads", e)));
            let mut branch_st = conn.prepare(BRANCH_OF_SQL)?;
            let lookup = |hash: &str| -> Result<Option<String>, ReadError> {
                let mut rows =
                    branch_st.query_map([hash], |r| r.get::<_, Bytes>(0)).map_err(|e| storage("read entries", e))?;
                match rows.next() {
                    None => Ok(None),
                    Some(b) => Ok(Some(text_of(b.map_err(|e| storage("read entries", e))?))),
                }
            };
            if let Err(e) = check_heads(&mut report, heads, lookup) {
                return Ok(Err(e));
            }
            Ok(Ok(report))
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
    /// `journal.verify_chain@1` over this journal's stream ([`verify_store`]). Holds the store's
    /// connection for the whole walk: appends to this journal wait until it returns.
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
/// A SQLite row id as stored: any 64-bit integer, so a tampered id is still reported.
const ROW_ID: &str = "^(0|-?[1-9][0-9]{0,18})$";

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
        ("required", strs(&["anchor", "checked", "from_seq", "ok"])),
        (
            "properties",
            obj([
                ("ok", obj([("type", text("boolean")), ("description", text("True when the chain verified."))])),
                ("from_seq", string(SEQ, "First sequence verified; 1 for a full verification.")),
                (
                    "anchor",
                    object(
                        &["kind"],
                        obj([
                            (
                                "kind",
                                obj([
                                    ("type", text("string")),
                                    ("enum", strs(&["genesis", "checkpoint", "trusted"])),
                                    ("description", text("What the first verified entry links to.")),
                                ]),
                            ),
                            ("seq", string(SEQ, "Sequence of the anchoring entry (from_seq - 1).")),
                            ("checkpoint", string(ROW_ID, "Row id of the anchoring checkpoint.")),
                        ]),
                    ),
                ),
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
                                    ("description", text("Sequence of the first failure; null when not at an entry.")),
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
                            (
                                "checkpoint",
                                string(ROW_ID, "Checkpoint row id of a checkpoint failure, as stored (untrusted)."),
                            ),
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
