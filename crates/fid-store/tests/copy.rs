//! Spec node CORE.STO.LAYOUT, REQ-011 to REQ-013 and AC-8 to AC-10: projects opened from
//! verified folders, the read-only open, and the atomic project copy (`Store::copy_to`).
//!
//! AC-9 kills a child process (this test binary, re-run with `FID_COPY_CHILD_*` set) at 50
//! copy steps chosen with seed 11 by `std::process::abort` from the copy hook, instead of the
//! timed kills of `fid-crash`; every step boundary of the copy is reachable that way.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_store::layout::{set_copy_hook, CopyStep, COPY_PREFIX, DB_FILE};
use fid_store::{CopyTarget, FolderId, OpenMode, Store, StoreError, TxnError, Value, VerifiedFolder};
use fid_types::Digest;
use sha2::{Digest as _, Sha256};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};

// ------------------------------------------------------------------ helpers

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        static N: AtomicU32 = AtomicU32::new(0);
        let name = format!("fid-copy-{tag}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst));
        let p = std::fs::canonicalize(std::env::temp_dir()).unwrap().join(name);
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Target {
    parent: PathBuf,
    id: FolderId,
    name: String,
}

impl Target {
    fn new(parent: &Path, name: &str) -> Target {
        Target { parent: parent.to_path_buf(), id: FolderId::of_path(parent).unwrap(), name: name.into() }
    }
}

impl CopyTarget for Target {
    fn parent_path(&self) -> &Path {
        &self.parent
    }
    fn parent_id(&self) -> FolderId {
        self.id
    }
    fn target_name(&self) -> &str {
        &self.name
    }
}

struct Folder {
    path: PathBuf,
    id: FolderId,
    read_only: bool,
}

impl VerifiedFolder for Folder {
    fn folder_path(&self) -> &Path {
        &self.path
    }
    fn folder_id(&self) -> FolderId {
        self.id
    }
    fn read_only(&self) -> bool {
        self.read_only
    }
}

fn digest_of(text: &str) -> String {
    let hex: String = Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
    format!("sha256:{hex}")
}

/// Pseudo-random bytes (xorshift, fixed seed per call).
fn bytes(seed: u64, len: usize) -> Vec<u8> {
    let mut s = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
    (0..len)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            (s >> 24) as u8
        })
        .collect()
}

/// Commits entry `i` referencing the given blob contents.
fn commit_entry(store: &Store, i: u64, blobs: &[Vec<u8>]) {
    let h = digest_of(&format!("entry-{i}"));
    let mut txn = store.transaction();
    let mut refs = Vec::new();
    for b in blobs {
        refs.push(txn.put_blob(b.clone()).unwrap().digest());
    }
    txn.insert(
        "entries",
        &[
            ("hash", Value::from(h.as_str())),
            ("stream", Value::from("record")),
            ("branch", Value::from("main")),
            ("kind", Value::from("command")),
            ("header", Value::Blob(format!("header {i}").into_bytes())),
        ],
    );
    for (n, d) in refs.iter().enumerate() {
        txn.insert(
            "entry_blobs",
            &[
                ("entry_hash", Value::from(h.as_str())),
                ("role", Value::from(format!("r{n}").as_str())),
                ("digest", Value::from(*d)),
            ],
        );
    }
    txn.commit().unwrap();
}

/// A project with `entries` entries and `blobs` small blobs, plus `chunked` objects larger
/// than one chunk.
fn source(dir: &Path, entries: u64, blobs: u64, chunked: u64) -> Store {
    let store = Store::create(&dir.join("src")).unwrap();
    let mut k = 0;
    for i in 0..entries {
        let mut contents = Vec::new();
        while k < blobs && contents.len() < 3 {
            contents.push(bytes(k + 1, 100 + k as usize));
            k += 1;
        }
        if i < chunked {
            contents.push(bytes(1000 + i, 8 * 1024 * 1024 + 100));
        }
        commit_entry(&store, i, &contents);
    }
    while k < blobs {
        commit_entry(&store, 10_000 + k, &[bytes(k + 1, 100 + k as usize)]);
        k += 1;
    }
    store
}

fn entries(store: &Store) -> Vec<(i64, String)> {
    store
        .read(|c| {
            let mut st = c.prepare("SELECT seq, hash FROM entries ORDER BY seq")?;
            let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect()
        })
        .unwrap()
}

fn referenced(store: &Store) -> Vec<String> {
    store
        .read(|c| {
            let mut st = c.prepare("SELECT DISTINCT digest FROM entry_blobs ORDER BY 1")?;
            let rows = st.query_map([], |r| r.get(0))?;
            rows.collect()
        })
        .unwrap()
}

fn temps(parent: &Path) -> Vec<String> {
    std::fs::read_dir(parent)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(COPY_PREFIX))
        .collect()
}

fn plain_publish(rename: &dyn Fn() -> Result<(), StoreError>) -> Result<(), TxnError> {
    rename().map_err(TxnError::from)
}

fn code(e: &TxnError) -> &'static str {
    e.code()
}

/// Checks a published copy: integrity, a prefix of the source's entries, every object sound.
fn check_copy(copy: &Store, source_entries: &[(i64, String)]) {
    let ok: String = copy.read(|c| c.query_row("PRAGMA integrity_check", [], |r| r.get(0))).unwrap();
    assert_eq!(ok, "ok");
    let got = entries(copy);
    assert!(!got.is_empty());
    assert_eq!(got.as_slice(), &source_entries[..got.len()], "the copy is not a prefix of the source");
    for d in referenced(copy) {
        let d = Digest::parse(&d).unwrap();
        copy.blobs().read_object(&d, 64 * 1024 * 1024).unwrap();
    }
}

// ------------------------------------------------------------------ AC-8

/// AC-8: a project with 20 entities and 50 blobs (two chunked) copied while another thread
/// keeps committing gives a sound, prefix-consistent copy with its own store-instance UUID.
#[test]
fn ac8_copy_while_committing() {
    let t = TempDir::new("ac8");
    let store = source(&t.0, 20, 50, 2);
    let out = t.0.join("out");
    std::fs::create_dir(&out).unwrap();
    let stop = AtomicBool::new(false);
    let copy = std::thread::scope(|s| {
        s.spawn(|| {
            let mut i = 0;
            while !stop.load(Ordering::SeqCst) && i < 200 {
                commit_entry(&store, 50_000 + i, &[bytes(90_000 + i, 64)]);
                i += 1;
            }
        });
        let copy = store.copy_to(&Target::new(&out, "p_copy"), |_| Ok(()), plain_publish);
        stop.store(true, Ordering::SeqCst);
        copy
    })
    .unwrap();
    check_copy(&copy, &entries(&store));
    assert_ne!(copy.location().uuid(), store.location().uuid());
    assert_eq!(copy.location().root(), out.join("p_copy"));
    assert!(temps(&out).is_empty());
    // Both chunked objects arrived with their chunks.
    let chunked = referenced(&copy)
        .into_iter()
        .filter(|d| {
            copy.blobs().read_object(&Digest::parse(d).unwrap(), 64 * 1024 * 1024).unwrap().len() > 8 * 1024 * 1024
        })
        .count();
    assert_eq!(chunked, 2);
}

// ------------------------------------------------------------------ AC-9

const CHILD_SRC: &str = "FID_COPY_CHILD_SRC";
const CHILD_OUT: &str = "FID_COPY_CHILD_OUT";
const CHILD_AT: &str = "FID_COPY_CHILD_AT";
const ABORT_MARKER: &str = "ABORT_AT";

/// The child of AC-9: copies the source and aborts at step number `FID_COPY_CHILD_AT`. Does
/// nothing when the variables are not set.
#[test]
fn copy_child() {
    let (Some(src), Some(out), Some(at)) =
        (std::env::var_os(CHILD_SRC), std::env::var_os(CHILD_OUT), std::env::var(CHILD_AT).ok())
    else {
        return;
    };
    let at: u32 = at.parse().unwrap();
    let store = Store::open(Path::new(&src), OpenMode::Write).unwrap();
    let n = Rc::new(RefCell::new(0u32));
    let seen = n.clone();
    set_copy_hook(Some(Box::new(move |s| {
        *seen.borrow_mut() += 1;
        if *seen.borrow() == at {
            // The parent checks this marker, so a child that fails earlier (a panic, a failed
            // open) does not pass for one killed at this step.
            eprintln!("{ABORT_MARKER} {at} {s:?}");
            std::process::abort();
        }
    })));
    let _ = store.copy_to(&Target::new(Path::new(&out), "p_copy"), |_| Ok(()), plain_publish);
}

fn open_retry(root: &Path) -> Store {
    let start = Instant::now();
    loop {
        match Store::open(root, OpenMode::Write) {
            Ok(s) => return s,
            Err(TxnError::Store(StoreError::Locked(_))) if start.elapsed() < Duration::from_secs(10) => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => panic!("open failed: {e}"),
        }
    }
}

/// AC-9: a copy killed at 50 steps (seed 11) never leaves a partial project at the target; the
/// next copy removes the dead temporary folder, and the source is unchanged.
#[test]
fn ac9_crash_mid_copy_leaves_no_partial_project() {
    let t = TempDir::new("ac9");
    let src_root = t.0.join("src");
    let before = {
        let store = source(&t.0, 6, 12, 0);
        entries(&store)
    };
    // Dry run: how many steps a copy reports.
    let total = {
        let store = open_retry(&src_root);
        let dry = t.0.join("dry");
        std::fs::create_dir(&dry).unwrap();
        let n = Rc::new(RefCell::new(0u32));
        let seen = n.clone();
        set_copy_hook(Some(Box::new(move |_| *seen.borrow_mut() += 1)));
        store.copy_to(&Target::new(&dry, "p_copy"), |_| Ok(()), plain_publish).unwrap();
        set_copy_hook(None);
        let total = *n.borrow();
        total
    };
    assert!(total >= 10, "{total} steps");
    let mut s: u64 = 11;
    let exe = std::env::current_exe().unwrap();
    for run in 0..50 {
        s = s.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        let at = 1 + ((s >> 33) % u64::from(total)) as u32;
        let out = t.0.join(format!("out{run}"));
        std::fs::create_dir(&out).unwrap();
        let child = Command::new(&exe)
            .args(["--exact", "copy_child", "--nocapture", "--test-threads=1"])
            .env(CHILD_SRC, &src_root)
            .env(CHILD_OUT, &out)
            .env(CHILD_AT, at.to_string())
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&child.stderr);
        assert!(
            !child.status.success() && stderr.contains(&format!("{ABORT_MARKER} {at} ")),
            "run {run}: the child was not killed at step {at}: {}\n{stderr}",
            child.status
        );
        let target = out.join("p_copy");
        let store = open_retry(&src_root);
        if target.exists() {
            // Only a completed rename publishes: the copy is whole.
            let copy = open_retry(&target);
            check_copy(&copy, &before);
        }
        // The next copy into the same parent removes the dead temporary folder.
        drop(store.copy_to(&Target::new(&out, "again"), |_| Ok(()), plain_publish).unwrap());
        assert!(temps(&out).is_empty(), "run {run} (step {at}): {:?}", temps(&out));
        assert_eq!(entries(&store), before, "run {run}: the source changed");
    }
}

// ------------------------------------------------------------------ AC-10 and REQ-011

/// AC-10: an existing target, or a blob that no longer matches its name, fails the copy;
/// nothing appears at the target and no temporary folder remains.
#[test]
fn ac10_existing_target_and_corrupt_blob() {
    let t = TempDir::new("ac10");
    let store = source(&t.0, 3, 6, 0);
    let out = t.0.join("out");
    std::fs::create_dir_all(out.join("taken")).unwrap();
    let e = store.copy_to(&Target::new(&out, "taken"), |_| Ok(()), plain_publish).unwrap_err();
    assert_eq!(code(&e), "not_empty");
    assert!(temps(&out).is_empty());

    let victim = Digest::parse(&referenced(&store)[0]).unwrap();
    let path = store.blobs().path_of(&victim);
    let mut perm = std::fs::metadata(&path).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perm.set_readonly(false);
    std::fs::set_permissions(&path, perm).unwrap();
    std::fs::write(&path, b"tampered").unwrap();
    let e = store.copy_to(&Target::new(&out, "p_copy"), |_| Ok(()), plain_publish).unwrap_err();
    assert_eq!(code(&e), "blob_corrupt");
    assert!(!out.join("p_copy").exists());
    assert!(temps(&out).is_empty());
}

/// REQ-012 on every platform (Windows included): a non-empty folder that appears at the target
/// after the check is never replaced; the rename fails and the temporary folder is removed.
#[test]
fn nonempty_target_appearing_before_rename_is_refused() {
    let t = TempDir::new("race");
    let store = source(&t.0, 2, 3, 0);
    let out = t.0.join("out");
    std::fs::create_dir(&out).unwrap();
    let target = out.join("p_copy");
    let planted = target.clone();
    set_copy_hook(Some(Box::new(move |s| {
        if s == CopyStep::BeforeRename {
            std::fs::create_dir(&planted).unwrap();
            std::fs::write(planted.join("keep.txt"), b"mine").unwrap();
        }
    })));
    let e = store.copy_to(&Target::new(&out, "p_copy"), |_| Ok(()), plain_publish).unwrap_err();
    set_copy_hook(None);
    assert_eq!(code(&e), "not_empty");
    assert_eq!(std::fs::read(target.join("keep.txt")).unwrap(), b"mine");
    assert_eq!(std::fs::read_dir(&target).unwrap().count(), 1);
    assert!(temps(&out).is_empty());
}

/// REQ-012: the post-rename identity check runs on every published copy, after the rename.
#[test]
fn post_rename_identity_check_runs() {
    let t = TempDir::new("ident");
    let store = source(&t.0, 2, 3, 0);
    let out = t.0.join("out");
    std::fs::create_dir(&out).unwrap();
    let log = Rc::new(RefCell::new(Vec::new()));
    let sink = log.clone();
    set_copy_hook(Some(Box::new(move |s| sink.borrow_mut().push(s))));
    let copy = store.copy_to(&Target::new(&out, "p_copy"), |_| Ok(()), plain_publish).unwrap();
    set_copy_hook(None);
    let log = log.borrow();
    let pos = |s| log.iter().position(|x| *x == s).unwrap_or_else(|| panic!("{s:?} missing: {log:?}"));
    assert!(pos(CopyStep::Closed) < pos(CopyStep::BeforeRename));
    assert!(pos(CopyStep::Renamed) < pos(CopyStep::IdentityChecked));
    assert_eq!(copy.location().folder_id().unwrap(), FolderId::of_path(&out.join("p_copy")).unwrap());
}

/// REQ-013: `publish` that refuses (no rename) or fails after the rename leaves nothing.
#[test]
fn publish_refusal_and_failure_leave_nothing() {
    let t = TempDir::new("pub");
    let store = source(&t.0, 2, 3, 0);
    let out = t.0.join("out");
    std::fs::create_dir(&out).unwrap();
    let e = store
        .copy_to(&Target::new(&out, "p_copy"), |_| Ok(()), |_rename| Err(StoreError::NotEmpty.into()))
        .unwrap_err();
    assert_eq!(code(&e), "not_empty");
    assert!(!out.join("p_copy").exists() && temps(&out).is_empty());
    let e = store
        .copy_to(
            &Target::new(&out, "p_copy"),
            |_| Ok(()),
            |rename| {
                rename().unwrap();
                Err(TxnError::Io(std::io::Error::other("source commit failed")))
            },
        )
        .unwrap_err();
    assert_eq!(code(&e), "io");
    assert!(!out.join("p_copy").exists() && temps(&out).is_empty());
    // finish runs on the copy before it is published; its error aborts the copy.
    let e = store
        .copy_to(&Target::new(&out, "p_copy"), |_| Err(TxnError::Io(std::io::Error::other("no"))), plain_publish)
        .unwrap_err();
    assert_eq!(code(&e), "io");
    assert!(!out.join("p_copy").exists() && temps(&out).is_empty());
}

/// REQ-011: a folder whose identity is not the verified one is refused (`outside_root`), when
/// opened and when checked again after the database is open.
#[test]
fn req011_swapped_folder_is_refused() {
    let t = TempDir::new("swap");
    drop(Store::create(&t.0.join("a")).unwrap());
    drop(Store::create(&t.0.join("b")).unwrap());
    let a = t.0.join("a");
    let b_id = FolderId::of_path(&t.0.join("b")).unwrap();
    let wrong = Folder { path: a.clone(), id: b_id, read_only: false };
    assert_eq!(Store::open_at(&wrong, OpenMode::Write).unwrap_err().code(), "outside_root");
    let right = Folder { path: a.clone(), id: FolderId::of_path(&a).unwrap(), read_only: false };
    let store = Store::open_at(&right, OpenMode::Read).unwrap();
    // Swap: move `a` away and put `b` in its place while the store is open (Unix allows it).
    if cfg!(unix) {
        std::fs::rename(&a, t.0.join("a-moved")).unwrap();
        std::fs::rename(t.0.join("b"), &a).unwrap();
        assert_eq!(store.location().check_identity(right.id).unwrap_err().code(), "outside_root");
    }
}

/// REQ-011: a read-only folder opens with no lock and no write; a write open of it, or one
/// that would need the WAL, is refused.
#[test]
fn req011_read_only_open_writes_nothing() {
    let t = TempDir::new("ro");
    let root = t.0.join("p");
    drop(source(&t.0, 3, 4, 0));
    std::fs::rename(t.0.join("src"), &root).unwrap();
    // The listing, and every file's bytes and modification time. Folder modification times
    // are left out: NTFS updates them lazily, so a `DirEntry` can report a stale one.
    let snapshot = |p: &Path| -> Vec<(String, Vec<u8>, Option<std::time::SystemTime>)> {
        let mut out = Vec::new();
        let mut stack = vec![p.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap() {
                let e = e.unwrap();
                let m = std::fs::symlink_metadata(e.path()).unwrap();
                if m.is_dir() {
                    stack.push(e.path());
                    out.push((e.path().to_string_lossy().into_owned(), Vec::new(), None));
                } else {
                    out.push((
                        e.path().to_string_lossy().into_owned(),
                        std::fs::read(e.path()).unwrap(),
                        Some(m.modified().unwrap()),
                    ));
                }
            }
        }
        out.sort();
        out
    };
    let before = snapshot(&root);
    let ro = Folder { path: root.clone(), id: FolderId::of_path(&root).unwrap(), read_only: true };
    {
        let store = Store::open_at(&ro, OpenMode::Read).unwrap();
        assert_eq!(store.mode(), OpenMode::Read);
        assert_eq!(entries(&store).len(), 3);
    }
    assert_eq!(snapshot(&root), before);
    assert_eq!(Store::open_at(&ro, OpenMode::Write).unwrap_err().code(), "invariant_violated");
    std::fs::write(root.join(format!("{DB_FILE}-wal")), b"").unwrap();
    assert_eq!(Store::open_at(&ro, OpenMode::Read).unwrap_err().code(), "read_only_unavailable");
}

/// REQ-011: a read-only open probes the project's `lock` without writing; a writer holding it
/// makes the open, and a copy from an `immutable` source, `read_only_unavailable`.
#[test]
fn req011_read_only_open_refuses_a_live_writer() {
    let t = TempDir::new("rolock");
    let root = t.0.join("src");
    drop(source(&t.0, 3, 4, 0));
    let ro = Folder { path: root.clone(), id: FolderId::of_path(&root).unwrap(), read_only: true };
    {
        let writer = Store::open(&root, OpenMode::Write).unwrap();
        assert_eq!(Store::open_at(&ro, OpenMode::Read).unwrap_err().code(), "read_only_unavailable");
        drop(writer);
    }
    let out = t.0.join("out");
    std::fs::create_dir(&out).unwrap();
    let snapshot = Store::open_at(&ro, OpenMode::Read).unwrap();
    // No writer: the copy from the import-root source succeeds.
    check_copy(&snapshot.copy_to(&Target::new(&out, "ok"), |_| Ok(()), plain_publish).unwrap(), &entries(&snapshot));
    // A writer appears while the snapshot is open: the copy is refused and leaves nothing.
    let writer = Store::open(&root, OpenMode::Write).unwrap();
    let e = snapshot.copy_to(&Target::new(&out, "p_copy"), |_| Ok(()), plain_publish).unwrap_err();
    assert_eq!(code(&e), "read_only_unavailable");
    assert!(!out.join("p_copy").exists() && temps(&out).is_empty());
    drop(writer);
}

/// REQ-013 with two copiers into the same parent: a second copy, whose stale cleanup runs
/// while the first is inside `publish` (its temporary folder already closed), never makes the
/// first publish a partial project. On Unix the first copy holds its lock through the rename
/// and is published; on Windows cleanup claims the unlocked folder and the first copy's rename
/// fails cleanly. Leftover temporary and claimed folders are removed by the next copy.
#[test]
fn stale_cleanup_during_publish_never_publishes_a_partial_project() {
    let t = TempDir::new("two");
    let store = source(&t.0, 4, 8, 1);
    let before = entries(&store);
    let out = t.0.join("out");
    std::fs::create_dir(&out).unwrap();
    // A dead copier's folder, unlocked and half written, and an unfinished claim.
    let dead = out.join(format!("{COPY_PREFIX}{}", "0123456789abcdef0123456789abcdef"));
    std::fs::create_dir_all(dead.join("blobs")).unwrap();
    std::fs::write(dead.join(DB_FILE), b"half").unwrap();
    let claimed = out.join(format!("{COPY_PREFIX}dead-{}", "fedcba9876543210fedcba9876543210"));
    std::fs::create_dir_all(claimed.join("tmp")).unwrap();

    let log = Rc::new(RefCell::new(Vec::new()));
    let sink = log.clone();
    set_copy_hook(Some(Box::new(move |s| sink.borrow_mut().push(s))));
    let first = store.copy_to(
        &Target::new(&out, "a"),
        |_| Ok(()),
        |rename| {
            // The second copier runs its stale cleanup now, while the first copy's temporary
            // folder is closed and waiting for its rename.
            let second = store.copy_to(&Target::new(&out, "b"), |_| Ok(()), plain_publish).unwrap();
            check_copy(&second, &before);
            rename().map_err(TxnError::from)
        },
    );
    set_copy_hook(None);
    let claims = log.borrow().iter().filter(|s| **s == CopyStep::StaleClaimed).count();
    match first {
        Ok(copy) => {
            if !cfg!(unix) {
                panic!("the first copy should have been claimed on this platform");
            }
            check_copy(&copy, &before);
            assert_eq!(claims, 1, "only the dead copier's folder is claimed");
        }
        Err(e) => {
            if cfg!(unix) {
                panic!("the first copy failed on Unix: {e}");
            }
            assert_eq!(code(&e), "io", "{e}");
            assert!(!out.join("a").exists(), "a partial project was published");
            assert_eq!(claims, 2);
        }
    }
    assert!(!dead.exists() && !claimed.exists());
    assert!(temps(&out).is_empty(), "{:?}", temps(&out));
    // The next copy into the same parent is unaffected.
    check_copy(&store.copy_to(&Target::new(&out, "c"), |_| Ok(()), plain_publish).unwrap(), &before);
    assert!(temps(&out).is_empty());
}

/// REQ-013: when `publish` fails after the rename, the copy is taken back only if the folder
/// at the target is still the copy; another folder there is left alone.
#[test]
fn rollback_after_publish_failure_checks_the_target_identity() {
    let t = TempDir::new("back");
    let store = source(&t.0, 2, 3, 0);
    let out = t.0.join("out");
    std::fs::create_dir(&out).unwrap();
    let target = out.join("p_copy");
    let e = store
        .copy_to(
            &Target::new(&out, "p_copy"),
            |_| Ok(()),
            |rename| {
                rename().unwrap();
                // Someone moves the published copy away and puts their own folder there.
                std::fs::rename(&target, out.join("moved")).unwrap();
                std::fs::create_dir(&target).unwrap();
                std::fs::write(target.join("keep.txt"), b"mine").unwrap();
                Err(TxnError::Io(std::io::Error::other("source commit failed")))
            },
        )
        .unwrap_err();
    assert_eq!(code(&e), "io");
    assert_eq!(std::fs::read(target.join("keep.txt")).unwrap(), b"mine");
    assert!(out.join("moved").join("format").exists(), "the moved copy is not touched either");
    assert!(temps(&out).is_empty());
}
