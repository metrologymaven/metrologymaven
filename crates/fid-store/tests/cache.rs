//! Acceptance criteria of spec node CORE.STO.CACHE (AC-1 .. AC-5) and its requirements.
//!
//! Expected values come from `fixtures/core/sto/cache/make_fixtures.py` (Python hashlib and
//! json), never from this crate. AC-5 (no unkeyed put) is the `compile_fail` doctest in
//! `src/cache.rs`. No test reads the wall clock: recency is the cache's access counter.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_store::cache::{CacheWarning, ITEMS_DIR, ITEM_HEADER_LEN, MAX_INPUT_DIGESTS, TMP_DIR};
use fid_store::{BlobStore, Cache, CacheConfig, CacheKey, CacheMode, OpenMode, ProjectLocation, PutOpts};
use fid_types::{CommandName, Digest};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/core/sto/cache");

/// A unique directory under the system temp dir, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        static N: AtomicU32 = AtomicU32::new(0);
        let name = format!("fid-cache-{tag}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst));
        let p = std::path::absolute(std::env::temp_dir().join(name)).unwrap();
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

fn h(text: &str) -> Digest {
    Digest::from_bytes(Sha256::digest(text.as_bytes()).into())
}

fn snap(i: usize) -> CacheKey {
    CacheKey::Snapshot { entry_hash: h(&format!("test item {i}")), format: 1 }
}

fn open(dir: &Path, limit: u64) -> Cache {
    Cache::open(dir, CacheConfig { limit, mode: CacheMode::Enabled })
}

fn fixture(rel: &str) -> String {
    std::fs::read_to_string(Path::new(FIXTURES).join(rel)).unwrap()
}

/// Regular files below `root` (relative path -> bytes).
fn tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let e = e.unwrap();
            let p = e.path();
            if e.file_type().unwrap().is_dir() {
                walk(base, &p, out);
            } else {
                let rel = p.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/");
                out.insert(rel, std::fs::read(&p).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    if root.exists() {
        walk(root, root, &mut out);
    }
    out
}

// ---------------------------------------------------------------- key format (REQ-001)

#[test]
fn key_file_names_match_reference() {
    let dir = TempDir::new("keys");
    let cache = open(&dir.0, 1 << 20);
    let mut n = 0;
    for line in fixture("key_names.txt").lines().filter(|l| !l.starts_with('#')) {
        let f: Vec<&str> = line.split(' ').collect();
        let (key, name) = match f[0] {
            "memo" => {
                let inputs =
                    if f[5] == "-" { Vec::new() } else { f[5].split(',').map(|d| Digest::parse(d).unwrap()).collect() };
                let key = CacheKey::Memo {
                    command: CommandName::parse(f[1]).unwrap(),
                    major: f[2].parse().unwrap(),
                    args_digest: Digest::parse(f[3]).unwrap(),
                    input_digests: inputs,
                    kernel_digest: Digest::parse(f[4]).unwrap(),
                };
                (key, f[6])
            }
            "snapshot" => {
                (CacheKey::Snapshot { entry_hash: Digest::parse(f[1]).unwrap(), format: f[2].parse().unwrap() }, f[3])
            }
            other => panic!("unknown line kind {other}"),
        };
        assert_eq!(key.digest().unwrap().hex(), name, "{line}");
        let path = cache.path_of(&key).unwrap();
        assert_eq!(path.file_name().unwrap().to_string_lossy(), name);
        assert_eq!(path.parent().unwrap().file_name().unwrap().to_string_lossy(), &name[..2]);
        n += 1;
    }
    assert_eq!(n, 7);
}

#[test]
fn input_order_is_part_of_the_key_and_oversized_keys_are_refused() {
    let memo = |inputs: Vec<Digest>| CacheKey::Memo {
        command: CommandName::parse("test.step").unwrap(),
        major: 1,
        args_digest: h("a"),
        input_digests: inputs,
        kernel_digest: h("k"),
    };
    assert_ne!(memo(vec![h("x"), h("y")]).digest().unwrap(), memo(vec![h("y"), h("x")]).digest().unwrap());
    let dir = TempDir::new("bigkey");
    let mut cache = open(&dir.0, 1 << 20);
    let big = memo(vec![h("x"); MAX_INPUT_DIGESTS + 1]);
    assert!(!cache.put(&big, b"r"));
    assert_eq!(cache.get(&big), None);
    assert!(cache.take_warnings().iter().all(|w| w.code() == "key_invalid"));
}

// ---------------------------------------------------------------- AC-1 (REQ-002)

struct Session {
    command: CommandName,
    major: u32,
    kernel: Digest,
    genesis: Digest,
    /// (args digest, entry hash) per commit.
    commits: Vec<(Digest, Digest)>,
    /// (commits before it, entry hash, format) per snapshot.
    snapshots: Vec<(usize, Digest, u32)>,
}

fn parse_session(text: &str) -> Session {
    let mut s = Session {
        command: CommandName::parse("x").unwrap(),
        major: 0,
        kernel: Digest::TOMBSTONE,
        genesis: Digest::TOMBSTONE,
        commits: Vec::new(),
        snapshots: Vec::new(),
    };
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let f: Vec<&str> = line.split(' ').collect();
        match f[0] {
            "command" => {
                s.command = CommandName::parse(f[1]).unwrap();
                s.major = f[2].parse().unwrap();
            }
            "kernel" => s.kernel = Digest::parse(f[1]).unwrap(),
            "genesis" => s.genesis = Digest::parse(f[1]).unwrap(),
            "commit" => {
                assert_eq!(f[1].parse::<usize>().unwrap(), s.commits.len() + 1);
                s.commits.push((Digest::parse(f[2]).unwrap(), Digest::parse(f[3]).unwrap()));
            }
            "snapshot" => s.snapshots.push((s.commits.len(), Digest::parse(f[1]).unwrap(), f[2].parse().unwrap())),
            other => panic!("unknown session line {other}"),
        }
    }
    s
}

#[derive(Debug, Default, PartialEq)]
struct Replay {
    /// Commits covered by the snapshot the replay started from (0: genesis).
    from_snapshot: usize,
    memo_hits: usize,
    computed: usize,
}

/// Test-kit reconstruction: start from the newest cached snapshot (else genesis) and replay
/// the remaining commits, taking each result from the memo cache when present.
fn reconstruct(s: &Session, cache: &mut Cache) -> (Digest, Replay) {
    let mut stats = Replay::default();
    let mut state = s.genesis;
    for (after, entry, format) in s.snapshots.iter().rev() {
        let key = CacheKey::Snapshot { entry_hash: *entry, format: *format };
        if let Some(d) = cache.get(&key).and_then(|b| Digest::parse(std::str::from_utf8(&b).ok()?).ok()) {
            state = d;
            stats.from_snapshot = *after;
            break;
        }
    }
    for (i, (args, _)) in s.commits.iter().enumerate().skip(stats.from_snapshot) {
        let args = *args;
        let key = CacheKey::Memo {
            command: s.command.clone(),
            major: s.major,
            args_digest: args,
            input_digests: vec![state],
            kernel_digest: s.kernel,
        };
        let cached = cache.get(&key).and_then(|b| Digest::parse(std::str::from_utf8(&b).ok()?).ok());
        let result = match cached {
            Some(r) => {
                stats.memo_hits += 1;
                r
            }
            None => {
                stats.computed += 1;
                let r = h(&format!("result\n{args}\n{state}"));
                cache.put(&key, r.as_str().as_bytes());
                r
            }
        };
        state = h(&format!("{state}\n{result}"));
        for (after, entry, format) in &s.snapshots {
            if *after == i + 1 {
                cache.put(&CacheKey::Snapshot { entry_hash: *entry, format: *format }, state.as_str().as_bytes());
            }
        }
    }
    (state, stats)
}

#[test]
fn ac1_deleting_the_cache_changes_no_document_state() {
    let tmp = TempDir::new("ac1");
    let project_dir = tmp.0.join("project");
    let session_text = fixture("session_200/session.txt");
    let expected = Digest::parse(fixture("session_200.state_digest").trim_end()).unwrap();

    // The fixture project: its journal stand-in (the session) is a blob in the project.
    let addr = {
        let (loc, _lock) = ProjectLocation::create(&project_dir).unwrap();
        BlobStore::for_project(&loc).unwrap().put_bytes(session_text.as_bytes(), PutOpts { durable: true }).unwrap()
    };
    let (loc, none) = ProjectLocation::open(&project_dir, OpenMode::Read).unwrap();
    assert!(none.is_none());
    // Stand-in for the per-user cache directory, laid out like ProjectLocation::cache_dir.
    let cache_dir = tmp.0.join("user-cache").join("fiducial").join("cache").join(loc.uuid());
    assert!(!cache_dir.starts_with(loc.root()));
    let session = parse_session(&String::from_utf8(BlobStore::for_project(&loc).unwrap().get(&addr).unwrap()).unwrap());
    assert_eq!((session.commits.len(), session.snapshots.len()), (200, 2));
    let project_before = tree(&project_dir);

    // Cold: everything computed, results memoised, two snapshots cached.
    let mut cache = open(&cache_dir, CacheConfig::default().limit);
    let (d, st) = reconstruct(&session, &mut cache);
    assert_eq!(d, expected);
    assert_eq!(st, Replay { from_snapshot: 0, memo_hits: 0, computed: 200 });
    drop(cache);

    // Reopen with a warm cache: the newest snapshot covers everything.
    let mut cache = open(&cache_dir, CacheConfig::default().limit);
    assert_eq!(reconstruct(&session, &mut cache), (expected, Replay { from_snapshot: 200, ..Replay::default() }));
    // Without the newest snapshot: the older one plus 100 memoised results.
    cache.remove(&CacheKey::Snapshot { entry_hash: session.snapshots[1].1, format: 1 });
    assert_eq!(
        reconstruct(&session, &mut cache),
        (expected, Replay { from_snapshot: 100, memo_hits: 100, computed: 0 })
    );
    drop(cache);

    // Delete the whole cache directory: no document state changes, and reopening the project
    // reconstructs the same state digest from scratch.
    std::fs::remove_dir_all(&cache_dir).unwrap();
    drop(loc);
    assert_eq!(tree(&project_dir), project_before);
    let (loc, _) = ProjectLocation::open(&project_dir, OpenMode::Read).unwrap();
    let session = parse_session(&String::from_utf8(BlobStore::for_project(&loc).unwrap().get(&addr).unwrap()).unwrap());
    let mut cache = open(&cache_dir, CacheConfig::default().limit);
    assert_eq!(reconstruct(&session, &mut cache), (expected, Replay { computed: 200, ..Replay::default() }));
    drop(cache);
    // Verifier replay (disabled handle) never reads the cache and gets the same digest.
    let mut verifier = Cache::open(&cache_dir, CacheConfig { mode: CacheMode::Disabled, ..CacheConfig::default() });
    assert_eq!(reconstruct(&session, &mut verifier), (expected, Replay { computed: 200, ..Replay::default() }));
    assert_eq!(tree(&project_dir), project_before);
}

// ---------------------------------------------------------------- AC-2 (REQ-003)

fn num(v: &CValue) -> u64 {
    match v {
        CValue::Num(f) => {
            assert!(*f >= 0.0 && f.fract() == 0.0);
            *f as u64
        }
        other => panic!("expected a number, got {other:?}"),
    }
}

fn field<'a>(obj: &'a BTreeMap<String, CValue>, k: &str) -> &'a CValue {
    obj.get(k).unwrap_or_else(|| panic!("missing {k}"))
}

fn list(v: &CValue) -> &[CValue] {
    match v {
        CValue::Arr(a) => a,
        other => panic!("expected an array, got {other:?}"),
    }
}

fn lru_item(i: usize) -> CacheKey {
    CacheKey::Snapshot { entry_hash: h(&format!("lru item {i}")), format: 1 }
}

#[test]
fn ac2_lru_eviction_keeps_the_most_recently_used_items() {
    let CValue::Obj(exp) = fid_canon::parse(fixture("lru_expected.json").as_bytes()).unwrap() else { panic!() };
    let limit = num(field(&exp, "limit"));
    let item_size = num(field(&exp, "item_size")) as usize;
    assert_eq!(num(field(&exp, "header")), ITEM_HEADER_LEN);
    let dir = TempDir::new("ac2");
    let mut cache = open(&dir.0, limit);
    let mut gets = Vec::new();
    for op in list(field(&exp, "ops")) {
        let CValue::Str(op) = op else { panic!() };
        let (verb, i) = op.split_once(' ').unwrap();
        let i: usize = i.parse().unwrap();
        let content = vec![i as u8; item_size];
        match verb {
            "put" => assert!(cache.put(&lru_item(i), &content)),
            "get" => {
                let got = cache.get(&lru_item(i));
                if let Some(b) = &got {
                    assert_eq!(b, &content);
                }
                gets.push(CValue::Bool(got.is_some()));
            }
            other => panic!("unknown op {other}"),
        }
        assert!(cache.total_bytes() <= limit);
    }
    assert_eq!(gets.as_slice(), list(field(&exp, "gets")));
    assert!(cache.take_warnings().is_empty());

    let remaining: Vec<u64> = list(field(&exp, "remaining")).iter().map(num).collect();
    let order: Vec<u64> = list(field(&exp, "lru_order")).iter().map(num).collect();
    let present: Vec<u64> = (0..50).filter(|i| cache.contains(&lru_item(*i as usize))).collect();
    assert_eq!(present, remaining);
    let index_of: BTreeMap<Digest, u64> = (0..50).map(|i| (lru_item(i as usize).digest().unwrap(), i)).collect();
    let actual_order: Vec<u64> = cache.lru_order().iter().map(|d| index_of[d]).collect();
    assert_eq!(actual_order, order);
    let per_item = ITEM_HEADER_LEN + item_size as u64;
    assert_eq!(cache.total_bytes(), per_item * remaining.len() as u64);
    drop(cache);

    // On disk: exactly the remaining items; recency survives a reopen.
    let files = tree(&dir.0.join(ITEMS_DIR));
    assert_eq!(files.len(), remaining.len());
    assert!(files.values().all(|b| b.len() as u64 == per_item));
    let mut cache = open(&dir.0, limit);
    let actual_order: Vec<u64> = cache.lru_order().iter().map(|d| index_of[d]).collect();
    assert_eq!(actual_order, order);
    // The next put evicts the least recently used survivor first.
    assert!(cache.put(&lru_item(99), &vec![0u8; item_size]));
    assert!(!cache.contains(&lru_item(order[0] as usize)));
    assert!(cache.contains(&lru_item(order[1] as usize)));
}

#[test]
fn eviction_never_removes_the_item_being_put_and_refuses_oversized_items() {
    let dir = TempDir::new("keep");
    let limit = 2 * (ITEM_HEADER_LEN + 100);
    let mut cache = open(&dir.0, limit);
    assert!(cache.put(&snap(1), &[1; 100]));
    assert!(cache.put(&snap(2), &[2; 100]));
    // Larger than the limit: not stored, and nothing else is evicted for it.
    assert!(!cache.put(&snap(3), &[3; 400]));
    assert_eq!(cache.take_warnings().iter().map(CacheWarning::code).collect::<Vec<_>>(), ["too_large"]);
    assert!(cache.contains(&snap(1)) && cache.contains(&snap(2)) && !cache.contains(&snap(3)));
    // Exactly the limit: everything else goes, the new item stays.
    assert!(cache.put(&snap(4), &vec![4; (limit - ITEM_HEADER_LEN) as usize]));
    assert!(!cache.contains(&snap(1)) && !cache.contains(&snap(2)));
    assert_eq!(cache.get(&snap(4)).map(|b| b.len() as u64), Some(limit - ITEM_HEADER_LEN));
    assert_eq!(cache.total_bytes(), limit);
    // Replacing an item re-accounts it rather than counting it twice.
    assert!(cache.put(&snap(4), &[5; 10]));
    assert_eq!(cache.total_bytes(), ITEM_HEADER_LEN + 10);
    assert_eq!(cache.get(&snap(4)), Some(vec![5; 10]));
}

// ---------------------------------------------------------------- AC-3 (REQ-004)

#[test]
fn ac3_altered_item_is_a_miss_and_is_deleted() {
    type Damage = fn(&mut Vec<u8>);
    let cases: [(&str, Damage); 5] = [
        ("flip a content byte", |b| {
            let last = b.len() - 1;
            b[last] ^= 0x01;
        }),
        ("append a byte", |b| b.push(b'x')),
        ("truncate into the header", |b| b.truncate(40)),
        ("change the stored digest", |b| b[ITEM_HEADER_LEN as usize - 2] ^= 0x01),
        ("change the key line", |b| b[30] ^= 0x01),
    ];
    for (what, damage) in cases {
        let dir = TempDir::new("ac3");
        let mut cache = open(&dir.0, 1 << 20);
        let key = snap(7);
        assert!(cache.put(&key, b"snapshot bytes of entry seven"));
        assert!(cache.put(&snap(8), b"another item"));
        let path = cache.path_of(&key).unwrap();
        let mut bytes = std::fs::read(&path).unwrap();
        damage(&mut bytes);
        std::fs::write(&path, &bytes).unwrap();

        assert_eq!(cache.get(&key), None, "{what}");
        assert!(!path.exists(), "{what}: damaged item must be deleted");
        let warnings = cache.take_warnings();
        assert_eq!(warnings, [CacheWarning::Integrity(key.digest().unwrap())], "{what}");
        assert!(!cache.contains(&key));
        assert_eq!(cache.total_bytes(), ITEM_HEADER_LEN + 12, "{what}");
        assert_eq!(cache.get(&snap(8)), Some(b"another item".to_vec()));
    }
}

// ---------------------------------------------------------------- AC-4 (REQ-005)

#[test]
fn ac4_disabled_handle_misses_and_ignores_puts() {
    let dir = TempDir::new("ac4");
    let mut cache = open(&dir.0, 1 << 20);
    assert!(cache.put(&snap(1), b"cached"));
    drop(cache);
    let before = tree(&dir.0);

    let mut off = Cache::open(&dir.0, CacheConfig { mode: CacheMode::Disabled, ..CacheConfig::default() });
    assert!(off.is_disabled());
    assert_eq!(off.get(&snap(1)), None);
    assert!(!off.put(&snap(2), b"ignored"));
    assert!(!off.contains(&snap(1)));
    off.remove(&snap(1));
    assert_eq!(off.total_bytes(), 0);
    drop(off);
    assert_eq!(tree(&dir.0), before, "a disabled handle never touches the disk");

    let mut on = open(&dir.0, 1 << 20);
    assert_eq!(on.get(&snap(1)), Some(b"cached".to_vec()));
    assert_eq!(on.get(&snap(2)), None);

    let mut none = Cache::disabled();
    assert_eq!(none.dir(), None);
    assert_eq!(none.get(&snap(1)), None);
    assert!(!none.put(&snap(1), b"x"));
}

// ---------------------------------------------------------------- REQ-006

#[test]
fn project_cache_lives_outside_the_project() {
    let tmp = TempDir::new("req6");
    let (loc, _lock) = ProjectLocation::create(&tmp.0.join("p")).unwrap();
    // No get or put: that would write into this machine's real per-user cache directory.
    let cache = Cache::for_project(&loc, CacheConfig::default());
    match cache.dir() {
        Some(d) => {
            assert!(!d.starts_with(loc.root()));
            assert!(d.ends_with(loc.uuid()));
            assert_eq!(Some(d.to_path_buf()), loc.cache_dir().map(|p| std::path::absolute(p).unwrap()));
        }
        None => assert!(cache.is_disabled()),
    }
    assert_eq!(cache.limit(), fid_store::cache::DEFAULT_LIMIT);
}

// ---------------------------------------------------------------- crash safety, bounded accounting

#[test]
fn accounting_recovers_from_a_crash_at_any_point() {
    let dir = TempDir::new("crash");
    let mut cache = open(&dir.0, 1 << 20);
    for i in 0..3 {
        assert!(cache.put(&snap(i), &vec![i as u8; 100 + i]));
    }
    cache.flush();
    for i in 3..6 {
        assert!(cache.put(&snap(i), &vec![i as u8; 100 + i]));
    }
    // Crash: no flush, so the index lists only items 0..3 and their old recency.
    std::mem::forget(cache);
    // A write interrupted between temp file and rename, and an item deleted by hand.
    // It is two hours old, far past STALE_TEMP_SECS, so the test has no timing race.
    let stale = dir.0.join(TMP_DIR).join("0123456789abcdef0123456789abcdef");
    std::fs::write(&stale, [0u8; 5000]).unwrap();
    age(&stale, 7200);
    // A write still in progress in another process (fresh): left alone.
    let fresh = dir.0.join(TMP_DIR).join("fedcba9876543210fedcba9876543210");
    std::fs::write(&fresh, [0u8; 10]).unwrap();
    let gone = open(&dir.0, 1 << 20).path_of(&snap(0)).unwrap();
    std::fs::remove_file(&gone).unwrap();

    let mut cache = open(&dir.0, 1 << 20);
    let on_disk: u64 = tree(&dir.0.join(ITEMS_DIR)).values().map(|b| b.len() as u64).sum();
    assert_eq!(cache.total_bytes(), on_disk);
    assert_eq!(on_disk, (1..6).map(|i| ITEM_HEADER_LEN + 100 + i).sum::<u64>());
    assert!(!stale.exists(), "abandoned temp files are cleared");
    assert!(fresh.exists(), "fresh temp files may be another process's write");
    std::fs::remove_file(&fresh).unwrap();
    assert_eq!(cache.get(&snap(0)), None);
    for i in 1..6 {
        assert_eq!(cache.get(&snap(i)), Some(vec![i as u8; 100 + i]));
    }
    drop(cache);

    // A garbled index only resets recency; sizes still come from the files.
    std::fs::write(dir.0.join("index"), b"fid-cache-index@1\nnext=zz\n").unwrap();
    let mut cache = open(&dir.0, 1 << 20);
    assert_eq!(cache.total_bytes(), on_disk);
    assert_eq!(cache.take_warnings().iter().map(CacheWarning::code).collect::<Vec<_>>(), ["index_rebuilt"]);
    drop(cache);

    // Reopening with a smaller limit evicts down to it straight away.
    let small = 2 * (ITEM_HEADER_LEN + 105);
    let mut cache = open(&dir.0, small);
    assert!(cache.total_bytes() <= small);
    assert_eq!(cache.lru_order().len(), 2);
    let on_disk: u64 = tree(&dir.0.join(ITEMS_DIR)).values().map(|b| b.len() as u64).sum();
    assert_eq!(cache.total_bytes(), on_disk);
}

#[test]
fn unusable_cache_directory_degrades_to_misses() {
    let dir = TempDir::new("broken");
    let file = dir.0.join("not-a-dir");
    std::fs::write(&file, b"user file").unwrap();
    let mut cache = open(&file, 1 << 20);
    assert_eq!(cache.get(&snap(1)), None);
    assert!(cache.is_disabled());
    assert!(!cache.put(&snap(1), b"x"));
    assert_eq!(cache.take_warnings().iter().map(CacheWarning::code).collect::<Vec<_>>(), ["unavailable"]);
    drop(cache);
    assert_eq!(std::fs::read(&file).unwrap(), b"user file", "the cache never removes what it did not write");
}

#[test]
fn eviction_order_is_deterministic_for_an_access_sequence() {
    let run = || {
        let dir = TempDir::new("det");
        let mut cache = open(&dir.0, 4 * (ITEM_HEADER_LEN + 8));
        for i in [0, 1, 2, 3] {
            cache.put(&snap(i), &[i as u8; 8]);
        }
        cache.get(&snap(0));
        cache.get(&snap(2));
        cache.put(&snap(4), &[4; 8]);
        cache.put(&snap(5), &[5; 8]);
        cache.lru_order()
    };
    let a = run();
    assert_eq!(a, run());
    let expect: Vec<Digest> = [0, 2, 4, 5].iter().map(|i| snap(*i).digest().unwrap()).collect();
    assert_eq!(a, expect);
}

/// Sets a file's modification time `secs` seconds into the past.
fn age(path: &Path, secs: u64) {
    let f = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    f.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(secs)).unwrap();
}

#[test]
fn two_handles_share_one_cache_directory() {
    let dir = TempDir::new("shared");
    let limit = 3 * (ITEM_HEADER_LEN + 10);
    let mut a = open(&dir.0, limit);
    let mut b = open(&dir.0, limit);
    assert!(a.put(&snap(1), &[1; 10]));
    // Each handle sees what the other stored and what it deleted.
    assert_eq!(b.get(&snap(1)), Some(vec![1; 10]));
    assert!(b.put(&snap(2), &[2; 10]));
    assert_eq!(a.get(&snap(2)), Some(vec![2; 10]));
    b.remove(&snap(1));
    assert_eq!(a.get(&snap(1)), None);
    assert!(a.put(&snap(3), &[3; 10]));
    assert!(a.put(&snap(4), &[4; 10]));
    // b does not know 3 and 4 yet: the directory may briefly exceed the limit.
    assert!(b.put(&snap(5), &[5; 10]));
    assert!(a.total_bytes() <= limit && b.total_bytes() <= limit);
    assert_eq!(a.get(&snap(5)), Some(vec![5; 10]));
    assert_eq!(b.get(&snap(3)), Some(vec![3; 10]));
    assert!(a.take_warnings().is_empty());
    assert!(b.take_warnings().is_empty());
    drop(a);
    drop(b);
    // A fresh handle accounts the real files and evicts down to the limit.
    let mut c = open(&dir.0, limit);
    let on_disk: u64 = tree(&dir.0.join(ITEMS_DIR)).values().map(|b| b.len() as u64).sum();
    assert_eq!(c.total_bytes(), on_disk);
    assert!(on_disk <= limit);
    assert_eq!(c.lru_order().len(), 3);
    for i in 2..6 {
        if let Some(v) = c.get(&snap(i)) {
            assert_eq!(v, vec![i as u8; 10]);
        }
    }
    assert!(c.take_warnings().is_empty());
}

#[test]
fn access_counters_are_renumbered_before_they_run_out() {
    let dir = TempDir::new("renumber");
    let mut cache = open(&dir.0, 1 << 20);
    for i in 0..3 {
        assert!(cache.put(&snap(i), &[i as u8; 4]));
    }
    drop(cache);
    let hex = |i: usize| snap(i).digest().unwrap().hex().to_string();
    let size = ITEM_HEADER_LEN + 4;
    let max = u64::MAX;
    let index = format!(
        "fid-cache-index@1\nnext={}\n{} {size} {}\n{} {size} {}\n{} {size} {}\n",
        max - 2,
        hex(0),
        max - 5,
        hex(1),
        max - 3,
        hex(2),
        max - 10
    );
    std::fs::write(dir.0.join("index"), index).unwrap();
    let mut cache = open(&dir.0, 1 << 20);
    let order = |c: &mut Cache| c.lru_order().iter().map(|d| d.hex().to_string()).collect::<Vec<_>>();
    assert_eq!(order(&mut cache), [hex(2), hex(0), hex(1)]);
    assert!(cache.put(&snap(3), &[3; 4]));
    assert_eq!(order(&mut cache), [hex(2), hex(0), hex(1), hex(3)]);
    assert!(cache.take_warnings().is_empty());
    drop(cache);
    let text = std::fs::read_to_string(dir.0.join("index")).unwrap();
    assert!(text.starts_with("fid-cache-index@1\nnext=4\n"), "{text}");
    let mut cache = open(&dir.0, 1 << 20);
    assert_eq!(order(&mut cache), [hex(2), hex(0), hex(1), hex(3)]);
}

#[test]
fn stray_files_in_the_items_directory_are_removed() {
    let dir = TempDir::new("stray");
    let mut cache = open(&dir.0, 1 << 20);
    assert!(cache.put(&snap(1), b"item"));
    let fan = cache.path_of(&snap(1)).unwrap().parent().unwrap().to_path_buf();
    drop(cache);
    let items = dir.0.join(ITEMS_DIR);
    std::fs::write(items.join("notes.txt"), b"stray").unwrap();
    std::fs::write(fan.join("notes.txt"), b"stray").unwrap();
    std::fs::create_dir(items.join("keep")).unwrap();
    std::fs::write(items.join("keep").join("x"), b"not ours to judge").unwrap();
    let mut cache = open(&dir.0, 1 << 20);
    assert_eq!(cache.total_bytes(), ITEM_HEADER_LEN + 4);
    assert!(!items.join("notes.txt").exists() && !fan.join("notes.txt").exists());
    assert!(items.join("keep").join("x").exists(), "unknown directories are left alone");
    assert_eq!(cache.get(&snap(1)), Some(b"item".to_vec()));
}

#[cfg(unix)]
#[test]
fn a_linked_fan_out_directory_is_never_followed() {
    let dir = TempDir::new("link");
    let outside = TempDir::new("outside");
    let mut cache = open(&dir.0, 1 << 20);
    assert!(cache.put(&snap(1), b"item"));
    let path = cache.path_of(&snap(1)).unwrap();
    let fan = path.parent().unwrap().to_path_buf();
    let target = outside.0.join(path.file_name().unwrap());
    std::fs::copy(&path, &target).unwrap();
    std::fs::remove_dir_all(&fan).unwrap();
    std::os::unix::fs::symlink(&outside.0, &fan).unwrap();

    assert_eq!(cache.get(&snap(1)), None);
    cache.remove(&snap(1));
    assert!(!cache.put(&snap(1), b"other"));
    let codes: Vec<&str> = cache.take_warnings().iter().map(CacheWarning::code).collect();
    assert_eq!(codes, ["io", "io", "io"]);
    assert_eq!(std::fs::read_dir(&outside.0).unwrap().count(), 1);
    assert!(target.exists(), "nothing is read or deleted through a link");
    drop(cache);

    // On reload the link itself is removed; its target is untouched.
    let mut cache = open(&dir.0, 1 << 20);
    assert_eq!(cache.total_bytes(), 0);
    assert!(std::fs::symlink_metadata(&fan).is_err());
    assert!(target.exists());
}
