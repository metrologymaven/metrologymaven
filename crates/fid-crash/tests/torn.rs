//! Torn writes and tampered files.
//!
//! Torn WAL: a writer commits 8 rows, reporting the WAL length after each commit returned,
//! and is killed with the WAL still in place. Copies of that store with the WAL cut at every
//! frame boundary (and one byte either side, the header and seeded points between) must
//! recover exactly the commits whose frames lie wholly inside the cut: the oracle is the
//! recorded lengths, not SQLite's recovery code. Flipped WAL bytes, damaged database pages,
//! damaged or missing blobs and edited rows must be refused (fail closed, with a v3 reason)
//! or, where the damage touched nothing the store uses, recover the full oracle state;
//! never a wrong state reported as good.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::*;
use fid_crash::adapter::{CrashAdapter, StoreAdapter, WriterMode};
use fid_crash::chain::{blob_bytes, chain, sha256, MAX_ROW_BYTES};
use fid_crash::plan::splitmix64;
use fid_crash::{verify_store, Ack, CheckLimits, CrashFailure, FailureKind, Writer};
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const COMMITS: u64 = 8;
/// A digest no row of the chain has.
const OTHER: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn verify(root: &Path, acked: &[u64]) -> Result<u64, CrashFailure> {
    verify_store(root, acked, CheckLimits::default(), true)
}

/// A killed writer's store with its WAL, and the WAL length after each commit.
fn killed_store(t: &TempDir) -> (PathBuf, Vec<u64>) {
    let root = t.0.join("template");
    StoreAdapter::new(WriterMode::Correct).create(&root).unwrap();
    let mut w = Writer::spawn(&writer("correct", &["--max-commits", "8", "--wal-sizes"]), &root).unwrap();
    w.wait_for("DONE").unwrap();
    let (acks, lines) = w.kill().unwrap();
    assert_eq!(acks, (1..=COMMITS).collect::<Vec<_>>());
    let ends: Vec<u64> = lines
        .iter()
        .filter_map(|l| l.strip_prefix("WAL "))
        .map(|l| l.split(' ').nth(1).unwrap().parse().unwrap())
        .collect();
    assert_eq!(ends.len() as u64, COMMITS, "{lines:?}");
    assert!(ends.windows(2).all(|w| w[0] < w[1]), "WAL did not grow at every commit: {ends:?}");
    (root, ends)
}

fn wal(root: &Path) -> PathBuf {
    root.join("project.db-wal")
}

fn copy_of(template: &Path, t: &TempDir, name: &str) -> PathBuf {
    let root = t.0.join(name);
    copy_dir(template, &root);
    root
}

fn set_len(path: &Path, len: u64) {
    OpenOptions::new().write(true).open(path).unwrap().set_len(len).unwrap();
}

fn flip(path: &Path, at: u64) {
    let mut f = OpenOptions::new().read(true).write(true).open(path).unwrap();
    let mut b = [0u8; 1];
    f.seek(SeekFrom::Start(at)).unwrap();
    f.read_exact(&mut b).unwrap();
    f.seek(SeekFrom::Start(at)).unwrap();
    f.write_all(&[b[0] ^ 0x5a]).unwrap();
}

/// The outcome of a damaged store: refused with a v3 reason, or exactly the full chain.
fn refused_or_whole(r: Result<u64, CrashFailure>, why: &str) -> bool {
    match r {
        Ok(n) => {
            assert_eq!(n, COMMITS, "{why}: wrong state accepted");
            false
        }
        Err(f) => {
            assert_refused(&f, why);
            true
        }
    }
}

#[test]
fn torn_wal_recovers_exactly_the_whole_commits() {
    let t = TempDir::new("torn");
    let (template, ends) = killed_store(&t);
    let full = std::fs::metadata(wal(&template)).unwrap().len();
    assert_eq!(*ends.last().unwrap(), full);
    assert_eq!(verify(&copy_of(&template, &t, "whole"), &(1..=COMMITS).collect::<Vec<_>>()), Ok(COMMITS));

    let mut cuts: Vec<u64> = vec![0, 1, 31, 32, 33, 56, 4096];
    for &e in &ends {
        cuts.extend([e.saturating_sub(1), e, e + 1]);
    }
    for k in 0..24 {
        cuts.push(splitmix64(k) % (full + 1));
    }
    cuts.retain(|&c| c <= full);
    cuts.sort_unstable();
    cuts.dedup();
    for (i, &cut) in cuts.iter().enumerate() {
        let want = ends.iter().filter(|&&e| e <= cut).count() as u64;
        let root = copy_of(&template, &t, &format!("cut{i}"));
        set_len(&wal(&root), cut);
        let acked: Vec<u64> = (1..=want).collect();
        assert_eq!(verify(&root, &acked), Ok(want), "WAL cut at {cut} of {full} (commit ends {ends:?})");
        let _ = std::fs::remove_dir_all(&root);
    }
}

#[test]
fn flipped_wal_bytes_are_refused_when_commits_were_acknowledged() {
    let t = TempDir::new("walflip");
    let (template, ends) = killed_store(&t);
    let full = *ends.last().unwrap();
    let all: Vec<u64> = (1..=COMMITS).collect();
    let mut points: Vec<u64> = vec![0, 8, 24, 31, 32, 40, 56, full - 1];
    for k in 100..124 {
        points.push(splitmix64(k) % full);
    }
    let mut refusals = 0;
    for (i, &at) in points.iter().enumerate() {
        let root = copy_of(&template, &t, &format!("flip{i}"));
        flip(&wal(&root), at);
        if refused_or_whole(verify(&root, &all), &format!("WAL byte {at} flipped")) {
            refusals += 1;
        }
        let _ = std::fs::remove_dir_all(&root);
    }
    // Every frame and the header are checksummed, so damage early in the WAL loses
    // acknowledged commits and must be refused.
    assert!(refusals >= 8, "only {refusals} refusals");
}

/// A closed store holding the 8-row chain (checkpointed: no WAL).
fn closed_store(t: &TempDir) -> PathBuf {
    let root = t.0.join("closed");
    let mut a = StoreAdapter::new(WriterMode::Correct);
    a.create(&root).unwrap();
    a.open_writer(&root).unwrap();
    let mut sink = Vec::new();
    for seq in 1..=COMMITS {
        a.commit_next(seq, &mut Ack::new(&mut sink)).unwrap();
    }
    a.close();
    assert_eq!(verify(&root, &[COMMITS]), Ok(COMMITS));
    root
}

fn blob_path(root: &Path, seq: u64) -> PathBuf {
    let d = sha256(&blob_bytes(seq).unwrap());
    let hex = d.hex().to_string();
    root.join("blobs").join("sha256").join(&hex[0..2]).join(&hex[2..4]).join(hex)
}

fn expect(r: Result<u64, CrashFailure>, kind: FailureKind, why: &str) {
    match r {
        Err(f) => {
            assert_eq!(f.kind, kind, "{why}: {f}");
            assert_refused(&f, why);
        }
        Ok(n) => panic!("{why}: accepted with {n} rows"),
    }
}

/// Runs `sql` on a closed store's database directly, with the append-only triggers of
/// `entries` and `entry_blobs` dropped for the edit and then restored with their exact text
/// (so the store's schema check still passes and the edit reaches the row checks).
fn raw_edit(root: &Path, sql: &str) {
    let c = rusqlite::Connection::open(root.join("project.db")).unwrap();
    let names = ["entries_no_update", "entries_no_delete", "entry_blobs_no_delete"];
    let mut saved = Vec::new();
    for n in names {
        let text: String = c
            .query_row("SELECT sql FROM sqlite_master WHERE type = 'trigger' AND name = ?1", [n], |r| r.get(0))
            .unwrap();
        saved.push(text);
        c.execute_batch(&format!("DROP TRIGGER {n}")).unwrap();
    }
    c.execute_batch(sql).unwrap();
    for text in saved {
        c.execute_batch(&text).unwrap();
    }
}

#[test]
fn damaged_blobs_are_refused() {
    let t = TempDir::new("blobs");
    let template = closed_store(&t);
    let all: Vec<u64> = (1..=COMMITS).collect();

    let r = copy_of(&template, &t, "flip");
    flip(&blob_path(&r, 2), 17);
    expect(verify(&r, &all), FailureKind::BlobCorrupt, "flipped blob byte");

    let r = copy_of(&template, &t, "gone");
    std::fs::remove_file(blob_path(&r, 4)).unwrap();
    expect(verify(&r, &all), FailureKind::BlobMissing, "deleted blob");

    let r = copy_of(&template, &t, "short");
    let p = blob_path(&r, 5);
    set_len(&p, std::fs::metadata(&p).unwrap().len() / 2);
    expect(verify(&r, &all), FailureKind::BlobCorrupt, "torn blob");

    let r = copy_of(&template, &t, "empty");
    set_len(&blob_path(&r, 7), 0);
    expect(verify(&r, &all), FailureKind::BlobCorrupt, "empty blob");

    // A huge file under a blob's address is refused without reading it whole.
    let r = copy_of(&template, &t, "huge");
    set_len(&blob_path(&r, 1), 64 * 1024 * 1024);
    expect(verify(&r, &all), FailureKind::BlobCorrupt, "oversized blob");

    let r = copy_of(&template, &t, "dir");
    let p = blob_path(&r, 8);
    std::fs::remove_file(&p).unwrap();
    std::fs::create_dir(&p).unwrap();
    let f = verify(&r, &all).unwrap_err();
    assert_refused(&f, "directory in place of a blob");
}

#[test]
fn edited_rows_are_refused() {
    let t = TempDir::new("rows");
    let template = closed_store(&t);
    let all: Vec<u64> = (1..=COMMITS).collect();
    let rows = chain(COMMITS);
    let edits: Vec<(&str, String)> = vec![
        ("header seq", "UPDATE entries SET header = CAST(replace(CAST(header AS TEXT), '\"seq\":\"3\"', '\"seq\":\"9\"') AS BLOB) WHERE seq = 3".into()),
        ("header of another row", "UPDATE entries SET header = (SELECT header FROM entries WHERE seq = 2) WHERE seq = 5".into()),
        ("hash column", format!("UPDATE entries SET hash = '{OTHER}' WHERE seq = 4")),
        ("prev_hash column", "UPDATE entries SET prev_hash = NULL WHERE seq = 6".into()),
        ("stream column", "UPDATE entries SET stream = 'x' WHERE seq = 1".into()),
        ("header as text", "UPDATE entries SET header = CAST(header AS TEXT) WHERE seq = 2".into()),
        ("oversized header", format!("UPDATE entries SET header = zeroblob({}) WHERE seq = 7", MAX_ROW_BYTES * 4096)),
        ("gap in seq", "DELETE FROM entry_blobs WHERE entry_hash = (SELECT hash FROM entries WHERE seq = 8); DELETE FROM entries WHERE seq = 8; UPDATE heads SET entry_hash = (SELECT hash FROM entries WHERE seq = 7)".into()),
        ("seq renumbered", "UPDATE entries SET seq = 100 WHERE seq = 8".into()),
        ("removed blob reference", "DELETE FROM entry_blobs WHERE entry_hash = (SELECT hash FROM entries WHERE seq = 1)".into()),
        ("orphan blob reference", format!("INSERT INTO entry_blobs (entry_hash, role, digest) VALUES ('{}', 'blob', '{}')", rows[2].hash(), rows[0].blob.unwrap())),
        ("head moved back", format!("UPDATE heads SET entry_hash = '{}'", rows[3].hash())),
        ("extra head", format!("INSERT INTO heads (branch, entry_hash) VALUES ('b', '{}')", rows[3].hash())),
    ];
    for (i, (why, sql)) in edits.iter().enumerate() {
        let r = copy_of(&template, &t, &format!("edit{i}"));
        raw_edit(&r, sql);
        let res = verify(&r, &all);
        let f = res.as_ref().err().unwrap_or_else(|| panic!("{why}: accepted"));
        assert!(matches!(f.kind, FailureKind::ChainBroken | FailureKind::AckMissing), "{why}: {f}");
        assert_refused(f, why);
        let _ = std::fs::remove_dir_all(&r);
    }
    // A row appended past the chain's end that is not the chain's next row.
    let r = copy_of(&template, &t, "append");
    raw_edit(
        &r,
        &format!(
            "INSERT INTO entries (seq, hash, stream, branch, kind, prev_hash, header)              VALUES (9, '{OTHER}', 'fid-crash', 'main', 'fid-crash-row@1', '{}', X'7B7D')",
            rows[7].hash()
        ),
    );
    expect(verify(&r, &all), FailureKind::ChainBroken, "foreign row appended");
}

#[test]
fn damaged_database_files_are_refused_or_recover_whole() {
    let t = TempDir::new("db");
    let template = closed_store(&t);
    let all: Vec<u64> = (1..=COMMITS).collect();
    let db = |r: &Path| r.join("project.db");

    let r = copy_of(&template, &t, "header");
    flip(&db(&r), 0);
    expect(verify(&r, &all), FailureKind::StoreRefused, "database header magic");

    let r = copy_of(&template, &t, "half");
    let len = std::fs::metadata(db(&r)).unwrap().len();
    set_len(&db(&r), len / 2);
    let f = verify(&r, &all).unwrap_err();
    assert_refused(&f, "database cut in half");

    let r = copy_of(&template, &t, "zero");
    set_len(&db(&r), 0);
    let f = verify(&r, &all).unwrap_err();
    assert_refused(&f, "empty database");

    let r = copy_of(&template, &t, "format");
    std::fs::write(r.join("format"), b"garbage").unwrap();
    expect(verify(&r, &all), FailureKind::StoreRefused, "garbled format file");

    // One flipped byte in every page: refused, or (damage in unused space) the whole chain.
    let pages = len / 4096;
    assert!(pages >= 2);
    let mut refusals = 0;
    for p in 0..pages {
        for (j, off) in [100u64, 2048, 4000].iter().enumerate() {
            let at = p * 4096 + (off + splitmix64(p * 3 + j as u64) % 64);
            let r = copy_of(&template, &t, &format!("page{p}-{j}"));
            flip(&db(&r), at);
            if refused_or_whole(verify(&r, &all), &format!("database byte {at} flipped")) {
                refusals += 1;
            }
            let _ = std::fs::remove_dir_all(&r);
        }
    }
    assert!(refusals > 0);

    // Over the row limit: refused before the rows are read.
    let r = copy_of(&template, &t, "limit");
    let f = verify_store(&r, &all, CheckLimits { max_rows: 5 }, true).unwrap_err();
    assert_eq!(f.kind, FailureKind::StoreRefused, "{f}");
}
