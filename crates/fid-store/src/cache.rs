//! Evictable cache store for memoised results and snapshot bytes (spec node `CORE.STO.CACHE`).
//!
//! The cache holds only what can always be re-derived, so it never fails a command: every
//! problem (I/O, a damaged item, an unusable directory) is a miss plus a [`CacheWarning`],
//! and deleting the whole cache directory only costs time. Verifier replay uses a
//! [disabled](CacheMode::Disabled) handle, which never touches the disk.
//!
//! Layout below the cache directory (by default `<per-user cache dir>/fiducial/cache/<project
//! uuid>/`, from [`ProjectLocation::cache_dir`], which is never inside the project):
//!
//! | entry | content |
//! |---|---|
//! | `items/<h0h1>/<hex>` | one item: [`ITEM_TAG`] line, key digest line, content digest line, content |
//! | `tmp/` | files being written; anything left there is deleted when the cache is loaded |
//! | `index` | [`INDEX_TAG`] line, `next=<counter>`, then `<hex> <size> <counter>` per item |
//!
//! `<hex>` is the SHA-256 of the key's canonical JSON ([`CacheKey::canonical_bytes`]). Only a
//! [`CacheKey`] can name an item and it has exactly two forms, so an unkeyed put does not
//! compile:
//!
//! ```compile_fail
//! let mut cache = fid_store::cache::Cache::disabled();
//! cache.put(b"result bytes"); // no key: there is no such method
//! ```
//!
//! Eviction is least-recently-used by a monotonic access counter (a logical clock, never the
//! wall clock): every put and every hit takes the next counter value, and after a put the
//! item with the smallest counter is deleted until the accounted size is at most the limit.
//! An item is accounted at its file size (header plus content). The item being put is never
//! evicted by its own put; an item larger than the limit is not stored at all.
//!
//! Crash safety: items and the index are written to `tmp/` and renamed into place, so no
//! name ever holds a partial file. Size accounting never trusts the index: when a handle
//! first touches the disk it scans `items/` and takes every size from the file system, so
//! the accounted total is the real total whatever point a previous process crashed at. The
//! index only carries access counters; it is written by [`Cache::flush`], on drop and after
//! every [`FLUSH_EVERY`] changes, and a missing or damaged index only resets recency. The
//! number of items is capped at [`MAX_ENTRIES`] and the index file size with it, so memory
//! and the index stay bounded. Item writes are not flushed to stable storage: after a power
//! loss a renamed item may hold garbage, which the digest check on read turns into a miss.

use crate::layout::ProjectLocation;
use fid_canon::CValue;
use fid_types::{CommandName, Digest};
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::ops::Bound;
use std::path::{Path, PathBuf};

/// Default size limit: 10 GiB (REQ-CORE.STO.CACHE-003).
pub const DEFAULT_LIMIT: u64 = 10 * 1024 * 1024 * 1024;
/// Most items a cache holds; beyond it the least recently used items are evicted.
pub const MAX_ENTRIES: usize = 1 << 20;
/// Most input digests a memo key may carry.
pub const MAX_INPUT_DIGESTS: usize = 4096;
/// Value of the `key` field of every canonical key.
pub const KEY_FORMAT: &str = "fid-cache-key@1";
/// First line of every item file.
pub const ITEM_TAG: &str = "fid-cache-item@1";
/// First line of the index file.
pub const INDEX_TAG: &str = "fid-cache-index@1";
/// Bytes of an item file before its content: tag line plus two `sha256:` digest lines.
pub const ITEM_HEADER_LEN: u64 = (ITEM_TAG.len() + 1 + 72 + 72) as u64;
/// Directory of the item files.
pub const ITEMS_DIR: &str = "items";
/// Directory of files being written.
pub const TMP_DIR: &str = "tmp";
/// Index file.
pub const INDEX_FILE: &str = "index";
/// Changes (puts, removals, evictions, hits) after which the index is written.
pub const FLUSH_EVERY: u32 = 1024;
/// Most warnings a handle keeps until [`Cache::take_warnings`]; later ones are counted only.
pub const MAX_WARNINGS: usize = 256;

/// Largest index file read: one line of at most 107 bytes per item, plus the header.
const MAX_INDEX_BYTES: u64 = MAX_ENTRIES as u64 * 112 + 1024;

/// The key of a cached item (REQ-CORE.STO.CACHE-001).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheKey {
    /// A memoised command result, keyed by its full derivation.
    Memo {
        command: CommandName,
        major: u32,
        args_digest: Digest,
        /// In the order the command consumed them (order is part of the key).
        input_digests: Vec<Digest>,
        kernel_digest: Digest,
    },
    /// Materialised snapshot bytes of the state after a journal entry.
    Snapshot { entry_hash: Digest, format: u32 },
}

impl CacheKey {
    /// RFC 8785 canonical JSON of the key (integers as decimal strings, per `fid-canon`):
    /// memo `{args_digest, command, input_digests, kernel_digest, key, kind: "memo", major}`,
    /// snapshot `{entry_hash, format, key, kind: "snapshot"}`, with `key` = [`KEY_FORMAT`].
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        let s = |d: &Digest| CValue::Str(d.as_str().to_string());
        let mut obj = BTreeMap::new();
        obj.insert("key".to_string(), CValue::Str(KEY_FORMAT.to_string()));
        match self {
            CacheKey::Memo { command, major, args_digest, input_digests, kernel_digest } => {
                if input_digests.len() > MAX_INPUT_DIGESTS {
                    return Err(format!("{} input digests, limit {MAX_INPUT_DIGESTS}", input_digests.len()));
                }
                obj.insert("kind".to_string(), CValue::Str("memo".to_string()));
                obj.insert("command".to_string(), CValue::Str(command.as_str().to_string()));
                obj.insert("major".to_string(), CValue::U64(u64::from(*major)));
                obj.insert("args_digest".to_string(), s(args_digest));
                obj.insert("input_digests".to_string(), CValue::Arr(input_digests.iter().map(s).collect()));
                obj.insert("kernel_digest".to_string(), s(kernel_digest));
            }
            CacheKey::Snapshot { entry_hash, format } => {
                obj.insert("kind".to_string(), CValue::Str("snapshot".to_string()));
                obj.insert("entry_hash".to_string(), s(entry_hash));
                obj.insert("format".to_string(), CValue::U64(u64::from(*format)));
            }
        }
        fid_canon::to_canonical_bytes(&CValue::Obj(obj)).map_err(|e| format!("canonical form: {e}"))
    }

    /// SHA-256 of [`CacheKey::canonical_bytes`]; its hex digits name the item file.
    pub fn digest(&self) -> Result<Digest, String> {
        Ok(Digest::from_bytes(Sha256::digest(self.canonical_bytes()?).into()))
    }
}

/// Whether a handle uses the disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CacheMode {
    #[default]
    Enabled,
    /// Every get misses and every put is ignored (verifier replay, REQ-CORE.STO.CACHE-005).
    Disabled,
}

/// Options of [`Cache::open`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheConfig {
    /// Size limit in bytes, items accounted at their file size.
    pub limit: u64,
    pub mode: CacheMode,
}

impl Default for CacheConfig {
    fn default() -> Self {
        CacheConfig { limit: DEFAULT_LIMIT, mode: CacheMode::Enabled }
    }
}

/// Why a cache operation missed or did less than asked. Never an error of the command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheWarning {
    /// An I/O problem; the operation was treated as a miss or skipped.
    Io(String),
    /// A stored item failed its check (digest, header or size); it was deleted. Carries the key
    /// digest.
    Integrity(Digest),
    /// An item larger than the limit was not stored.
    TooLarge { size: u64, limit: u64 },
    /// A key could not be put in canonical form (for example too many input digests).
    KeyInvalid(String),
    /// The index was missing a header or unreadable and recency was reset; sizes are unaffected.
    IndexRebuilt(String),
    /// The cache directory cannot be used; the handle behaves as disabled.
    Unavailable(String),
}

impl CacheWarning {
    /// Stable snake_case code.
    pub fn code(&self) -> &'static str {
        match self {
            CacheWarning::Io(_) => "io",
            CacheWarning::Integrity(_) => "integrity",
            CacheWarning::TooLarge { .. } => "too_large",
            CacheWarning::KeyInvalid(_) => "key_invalid",
            CacheWarning::IndexRebuilt(_) => "index_rebuilt",
            CacheWarning::Unavailable(_) => "unavailable",
        }
    }
}

impl fmt::Display for CacheWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CacheWarning::Io(m) => write!(f, "io: {m}"),
            CacheWarning::Integrity(k) => write!(f, "integrity: cached item {k} was damaged and deleted"),
            CacheWarning::TooLarge { size, limit } => write!(f, "too_large: {size} bytes, limit {limit}"),
            CacheWarning::KeyInvalid(m) => write!(f, "key_invalid: {m}"),
            CacheWarning::IndexRebuilt(m) => write!(f, "index_rebuilt: {m}"),
            CacheWarning::Unavailable(m) => write!(f, "unavailable: {m}"),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Entry {
    size: u64,
    tick: u64,
}

/// In-memory index: sizes from the file system, access counters from the index file.
#[derive(Debug, Default)]
struct Index {
    entries: BTreeMap<String, Entry>,
    /// `(tick, name)` of every entry, least recently used first.
    order: BTreeSet<(u64, String)>,
    total: u64,
    next: u64,
    changes: u32,
}

impl Index {
    fn insert(&mut self, name: &str, size: u64, tick: u64) {
        self.remove(name);
        self.entries.insert(name.to_string(), Entry { size, tick });
        self.order.insert((tick, name.to_string()));
        self.total = self.total.saturating_add(size);
        self.changes = self.changes.saturating_add(1);
    }

    fn remove(&mut self, name: &str) -> bool {
        match self.entries.remove(name) {
            Some(e) => {
                self.order.remove(&(e.tick, name.to_string()));
                self.total = self.total.saturating_sub(e.size);
                self.changes = self.changes.saturating_add(1);
                true
            }
            None => false,
        }
    }

    fn tick(&mut self) -> u64 {
        let t = self.next;
        self.next = self.next.saturating_add(1);
        t
    }
}

#[derive(Debug)]
enum State {
    /// The disk has not been read yet.
    Unloaded,
    Loaded(Index),
    /// The directory cannot be used; behaves as disabled.
    Broken,
}

/// A handle on one cache directory. Not shared between threads; open one handle per user.
#[derive(Debug)]
pub struct Cache {
    /// `None` for a disabled handle.
    dir: Option<PathBuf>,
    limit: u64,
    state: State,
    warnings: Vec<CacheWarning>,
    dropped_warnings: u64,
}

enum ReadOutcome {
    Hit(Vec<u8>),
    Absent,
    Damaged,
}

impl Cache {
    /// A handle on the cache in `dir`. Nothing is read or created until the first operation;
    /// a directory that cannot be used then turns the handle into a disabled one with an
    /// [`CacheWarning::Unavailable`]. The caller keeps `dir` outside every project
    /// (REQ-CORE.STO.CACHE-006); [`Cache::for_project`] does so.
    pub fn open(dir: &Path, config: CacheConfig) -> Cache {
        let mut c = Cache::disabled();
        c.limit = config.limit;
        if config.mode == CacheMode::Disabled {
            return c;
        }
        match std::path::absolute(dir) {
            Ok(d) => c.dir = Some(d),
            Err(e) => c.warn(CacheWarning::Unavailable(format!("cache directory: {e}"))),
        }
        c
    }

    /// The cache of `project`, in the per-user cache directory outside it
    /// ([`ProjectLocation::cache_dir`]); disabled with a warning if there is none.
    pub fn for_project(project: &ProjectLocation, config: CacheConfig) -> Cache {
        match project.cache_dir() {
            Some(dir) => Cache::open(&dir, config),
            None => {
                let mut c = Cache::disabled();
                c.limit = config.limit;
                c.warn(CacheWarning::Unavailable("no per-user cache directory outside the project".to_string()));
                c
            }
        }
    }

    /// A disabled handle: every get misses, every put is ignored, the disk is never touched.
    pub fn disabled() -> Cache {
        Cache { dir: None, limit: DEFAULT_LIMIT, state: State::Unloaded, warnings: Vec::new(), dropped_warnings: 0 }
    }

    /// True for a disabled handle, or one whose directory turned out unusable.
    pub fn is_disabled(&self) -> bool {
        self.dir.is_none() || matches!(self.state, State::Broken)
    }

    /// The cache directory (`None` when disabled).
    pub fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    /// The size limit in bytes.
    pub fn limit(&self) -> u64 {
        self.limit
    }

    /// Where the item of `key` is stored (`None` when disabled or the key is invalid).
    pub fn path_of(&self, key: &CacheKey) -> Option<PathBuf> {
        let dir = self.dir.as_deref()?;
        let d = key.digest().ok()?;
        Some(item_path(dir, d.hex()))
    }

    /// The bytes stored under `key`, or `None` for a miss. A damaged item is deleted and
    /// reported as a miss (REQ-CORE.STO.CACHE-004); a hit makes the item most recently used.
    pub fn get(&mut self, key: &CacheKey) -> Option<Vec<u8>> {
        let (dir, name) = self.target(key)?;
        // Load first: an unusable directory is reported once, as `unavailable`.
        self.index()?;
        let path = item_path(&dir, &name);
        let outcome = read_item(&path, &name, self.limit);
        let mut warn = None;
        let result = {
            let idx = self.index()?;
            match outcome {
                Ok(ReadOutcome::Hit(content)) => {
                    let size = ITEM_HEADER_LEN.saturating_add(content.len() as u64);
                    let t = idx.tick();
                    idx.insert(&name, size, t);
                    Some(content)
                }
                Ok(ReadOutcome::Absent) => {
                    idx.remove(&name);
                    None
                }
                Ok(ReadOutcome::Damaged) => {
                    let removed = remove_file(&path);
                    idx.remove(&name);
                    warn = Some(match removed {
                        Ok(()) => CacheWarning::Integrity(digest_of_hex(&name)),
                        Err(e) => CacheWarning::Io(format!("deleting damaged item {name}: {e}")),
                    });
                    None
                }
                Err(e) => {
                    warn = Some(CacheWarning::Io(format!("reading item {name}: {e}")));
                    None
                }
            }
        };
        if let Some(w) = warn {
            self.warn(w);
        }
        // A hit on an item another handle stored may push the total over the limit.
        self.evict(&dir, Some(&name));
        self.maybe_flush();
        result
    }

    /// Whether an item is stored under `key`, without reading or touching it.
    pub fn contains(&mut self, key: &CacheKey) -> bool {
        match self.target(key) {
            Some((_, name)) => self.index().is_some_and(|idx| idx.entries.contains_key(&name)),
            None => false,
        }
    }

    /// Stores `bytes` under `key`, replacing an earlier item, then evicts least recently used
    /// items until the cache is within its limit. Returns whether the item was stored; a
    /// disabled handle ignores every put (REQ-CORE.STO.CACHE-005).
    pub fn put(&mut self, key: &CacheKey, bytes: &[u8]) -> bool {
        let Some((dir, name)) = self.target(key) else { return false };
        let size = ITEM_HEADER_LEN.saturating_add(bytes.len() as u64);
        if size > self.limit {
            self.warn(CacheWarning::TooLarge { size, limit: self.limit });
            return false;
        }
        if self.index().is_none() {
            return false;
        }
        if let Err(e) = write_item(&dir, &name, bytes) {
            self.warn(CacheWarning::Io(format!("writing item {name}: {e}")));
            return false;
        }
        if let Some(idx) = self.index() {
            let t = idx.tick();
            idx.insert(&name, size, t);
        }
        self.evict(&dir, Some(&name));
        self.maybe_flush();
        true
    }

    /// Deletes the item stored under `key`, if any.
    pub fn remove(&mut self, key: &CacheKey) {
        let Some((dir, name)) = self.target(key) else { return };
        if self.index().is_none() {
            return;
        }
        match remove_file(&item_path(&dir, &name)) {
            Ok(()) => {
                if let Some(idx) = self.index() {
                    idx.remove(&name);
                }
            }
            Err(e) => self.warn(CacheWarning::Io(format!("removing item {name}: {e}"))),
        }
        self.maybe_flush();
    }

    /// Accounted size of all items in bytes (0 when disabled).
    pub fn total_bytes(&mut self) -> u64 {
        if self.dir.is_none() {
            return 0;
        }
        self.index().map_or(0, |idx| idx.total)
    }

    /// Key digests of all items, least recently used first (empty when disabled).
    pub fn lru_order(&mut self) -> Vec<Digest> {
        if self.dir.is_none() {
            return Vec::new();
        }
        self.index().map_or_else(Vec::new, |idx| idx.order.iter().map(|(_, n)| digest_of_hex(n)).collect())
    }

    /// Writes the index if anything changed since it was last written.
    pub fn flush(&mut self) {
        let Some(dir) = self.dir.clone() else { return };
        let State::Loaded(idx) = &mut self.state else { return };
        if idx.changes == 0 {
            return;
        }
        match write_index(&dir, idx) {
            Ok(()) => idx.changes = 0,
            Err(e) => self.warn(CacheWarning::Io(format!("writing index: {e}"))),
        }
    }

    /// The warnings raised since the last call, oldest first. If more than [`MAX_WARNINGS`]
    /// were raised, the later ones were dropped and a final `io` warning says how many.
    pub fn take_warnings(&mut self) -> Vec<CacheWarning> {
        let mut out = std::mem::take(&mut self.warnings);
        if self.dropped_warnings > 0 {
            out.push(CacheWarning::Io(format!("{} further warnings dropped", self.dropped_warnings)));
            self.dropped_warnings = 0;
        }
        out
    }

    fn warn(&mut self, w: CacheWarning) {
        if self.warnings.len() < MAX_WARNINGS {
            self.warnings.push(w);
        } else {
            self.dropped_warnings = self.dropped_warnings.saturating_add(1);
        }
    }

    /// The directory and item name for `key`, or `None` (disabled, unusable or invalid key).
    fn target(&mut self, key: &CacheKey) -> Option<(PathBuf, String)> {
        if self.is_disabled() {
            return None;
        }
        let dir = self.dir.clone()?;
        match key.digest() {
            Ok(d) => Some((dir, d.hex().to_string())),
            Err(e) => {
                self.warn(CacheWarning::KeyInvalid(e));
                None
            }
        }
    }

    /// The index, loading it from the disk on first use.
    fn index(&mut self) -> Option<&mut Index> {
        if matches!(self.state, State::Unloaded) {
            let dir = self.dir.clone()?;
            let mut w = Vec::new();
            self.state = match load(&dir, &mut w) {
                Ok(idx) => State::Loaded(idx),
                Err(e) => {
                    w.push(CacheWarning::Unavailable(format!("{}: {e}", dir.display())));
                    State::Broken
                }
            };
            for x in w {
                self.warn(x);
            }
            if let Some(d) = self.dir.clone() {
                self.evict(&d, None);
            }
        }
        match &mut self.state {
            State::Loaded(idx) => Some(idx),
            _ => None,
        }
    }

    /// Deletes least recently used items (never `keep`) until the total is within the limit
    /// and the entry count within [`MAX_ENTRIES`]. An item that cannot be deleted stays
    /// accounted and the next one is tried.
    fn evict(&mut self, dir: &Path, keep: Option<&str>) {
        let limit = self.limit;
        let mut failures = Vec::new();
        if let State::Loaded(idx) = &mut self.state {
            let mut cursor: Option<(u64, String)> = None;
            while idx.total > limit || idx.entries.len() > MAX_ENTRIES {
                let next = match &cursor {
                    None => idx.order.iter().next(),
                    Some(c) => idx.order.range((Bound::Excluded(c.clone()), Bound::Unbounded)).next(),
                };
                let Some((tick, name)) = next.cloned() else { break };
                cursor = Some((tick, name.clone()));
                if Some(name.as_str()) == keep {
                    continue;
                }
                match remove_file(&item_path(dir, &name)) {
                    Ok(()) => {
                        idx.remove(&name);
                    }
                    Err(e) => failures.push(CacheWarning::Io(format!("evicting item {name}: {e}"))),
                }
            }
        }
        for w in failures {
            self.warn(w);
        }
    }

    fn maybe_flush(&mut self) {
        if matches!(&self.state, State::Loaded(idx) if idx.changes >= FLUSH_EVERY) {
            self.flush();
        }
    }
}

impl Drop for Cache {
    fn drop(&mut self) {
        self.flush();
    }
}

fn item_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(ITEMS_DIR).join(name.get(..2).unwrap_or("00")).join(name)
}

fn digest_of_hex(hex: &str) -> Digest {
    Digest::parse(&format!("sha256:{hex}")).unwrap_or(Digest::TOMBSTONE)
}

fn is_hex(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Deletes a file; a file that is already gone counts as deleted.
fn remove_file(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Creates `path` as a directory if missing; refuses a link or a file in its place.
fn ensure_dir(path: &Path) -> io::Result<()> {
    match fs::create_dir(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => real_dir(path),
        Err(e) => Err(e),
    }
}

fn real_dir(path: &Path) -> io::Result<()> {
    let m = fs::symlink_metadata(path)?;
    if m.is_dir() && !m.file_type().is_symlink() {
        Ok(())
    } else {
        Err(io::Error::new(io::ErrorKind::InvalidData, format!("{} is a link or not a directory", path.display())))
    }
}

/// Reads the cache state from `dir` (creating it if missing): clears `tmp/`, takes sizes
/// from the item files and access counters from the index.
fn load(dir: &Path, warnings: &mut Vec<CacheWarning>) -> io::Result<Index> {
    match fs::symlink_metadata(dir) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => fs::create_dir_all(dir)?,
        Err(e) => return Err(e),
        Ok(_) => real_dir(dir)?,
    }
    let items = dir.join(ITEMS_DIR);
    let tmp = dir.join(TMP_DIR);
    ensure_dir(&items)?;
    ensure_dir(&tmp)?;
    for e in fs::read_dir(&tmp)? {
        let e = e?;
        if e.file_type().is_ok_and(|t| t.is_file()) {
            // Leftovers of an interrupted write; a failure only leaves a file behind.
            let _ = fs::remove_file(e.path());
        }
    }
    let (prev, next) = match read_index(dir) {
        Ok(x) => x,
        Err(why) => {
            warnings.push(CacheWarning::IndexRebuilt(why));
            (BTreeMap::new(), 0)
        }
    };
    let mut idx = Index { next, ..Index::default() };
    for d1 in fs::read_dir(&items)? {
        let d1 = d1?;
        let fan = d1.file_name().to_string_lossy().into_owned();
        if !is_hex(&fan, 2) || !d1.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        for f in fs::read_dir(d1.path())? {
            let f = f?;
            let name = f.file_name().to_string_lossy().into_owned();
            if !is_hex(&name, 64) || !name.starts_with(&fan) || !f.file_type().is_ok_and(|t| t.is_file()) {
                continue;
            }
            let size = f.metadata()?.len();
            // Items the index does not know (stored before a crash) count as oldest.
            let tick = prev.get(&name).map_or(0, |p| p.1);
            idx.insert(&name, size, tick);
            idx.next = idx.next.max(tick.saturating_add(1));
        }
    }
    let same = prev.len() == idx.entries.len()
        && idx.entries.iter().all(|(n, e)| prev.get(n).is_some_and(|p| p.0 == e.size && p.1 == e.tick));
    idx.changes = if same { 0 } else { 1 };
    Ok(idx)
}

type Recorded = BTreeMap<String, (u64, u64)>;

/// Parses the index file: `(name -> (size, tick), next)`. Missing index: empty. Anything
/// malformed is an error message (the caller resets recency).
fn read_index(dir: &Path) -> Result<(Recorded, u64), String> {
    let path = dir.join(INDEX_FILE);
    let mut f = match File::open(&path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok((BTreeMap::new(), 0)),
        Err(e) => return Err(format!("index: {e}")),
    };
    let mut bytes = Vec::new();
    (&mut f).take(MAX_INDEX_BYTES + 1).read_to_end(&mut bytes).map_err(|e| format!("index: {e}"))?;
    if bytes.len() as u64 > MAX_INDEX_BYTES {
        return Err("index: too large".to_string());
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| "index: not UTF-8".to_string())?;
    let mut lines = text.lines();
    if lines.next() != Some(INDEX_TAG) {
        return Err("index: missing tag".to_string());
    }
    let next = lines
        .next()
        .and_then(|l| l.strip_prefix("next="))
        .and_then(parse_decimal)
        .ok_or_else(|| "index: bad next counter".to_string())?;
    let mut out = BTreeMap::new();
    for (i, line) in lines.enumerate() {
        let bad = || format!("index: bad line {}", i + 3);
        let mut parts = line.split(' ');
        let (Some(name), Some(size), Some(tick), None) = (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(bad());
        };
        let size = parse_decimal(size).ok_or_else(bad)?;
        let tick = parse_decimal(tick).ok_or_else(bad)?;
        if !is_hex(name, 64) || tick >= next || out.insert(name.to_string(), (size, tick)).is_some() {
            return Err(bad());
        }
        if out.len() > MAX_ENTRIES {
            return Err("index: too many entries".to_string());
        }
    }
    Ok((out, next))
}

fn parse_decimal(s: &str) -> Option<u64> {
    if s.is_empty() || s.len() > 20 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

fn write_index(dir: &Path, idx: &Index) -> io::Result<()> {
    let mut text = format!("{INDEX_TAG}\nnext={}\n", idx.next);
    for (name, e) in &idx.entries {
        text.push_str(&format!("{name} {} {}\n", e.size, e.tick));
    }
    let tmp = dir.join(TMP_DIR).join(crate::cas::temp_name());
    replace_via(&tmp, &dir.join(INDEX_FILE), &[text.as_bytes()])
}

/// Writes `parts` into the new file `tmp` and renames it to `target`.
fn replace_via(tmp: &Path, target: &Path, parts: &[&[u8]]) -> io::Result<()> {
    let renamed = write_new(tmp, parts).and_then(|()| fs::rename(tmp, target));
    if renamed.is_err() {
        let _ = fs::remove_file(tmp);
    }
    renamed
}

fn write_new(path: &Path, parts: &[&[u8]]) -> io::Result<()> {
    let mut f = OpenOptions::new().write(true).create_new(true).open(path)?;
    for p in parts {
        f.write_all(p)?;
    }
    Ok(())
}

fn write_item(dir: &Path, name: &str, content: &[u8]) -> io::Result<()> {
    let target = item_path(dir, name);
    if let Some(parent) = target.parent() {
        ensure_dir(parent)?;
    }
    let content_digest = Digest::from_bytes(Sha256::digest(content).into());
    let header = format!("{ITEM_TAG}\nsha256:{name}\n{content_digest}\n");
    let tmp = dir.join(TMP_DIR).join(crate::cas::temp_name());
    replace_via(&tmp, &target, &[header.as_bytes(), content])
}

/// Reads and checks one item. Damaged: wrong header, wrong key line, content digest
/// mismatch, or larger than any item this cache stores.
fn read_item(path: &Path, name: &str, limit: u64) -> io::Result<ReadOutcome> {
    let meta = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(ReadOutcome::Absent),
        Err(e) => return Err(e),
    };
    if !meta.is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "item is not a regular file"));
    }
    let mut f = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(ReadOutcome::Absent),
        Err(e) => return Err(e),
    };
    let mut bytes = Vec::new();
    (&mut f).take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
    drop(f);
    if bytes.len() as u64 > limit {
        return Ok(ReadOutcome::Damaged);
    }
    let header_len = ITEM_HEADER_LEN as usize;
    let Some(header) = bytes.get(..header_len) else { return Ok(ReadOutcome::Damaged) };
    let expected_start = format!("{ITEM_TAG}\nsha256:{name}\n");
    let Some(digest_line) = header.strip_prefix(expected_start.as_bytes()) else {
        return Ok(ReadOutcome::Damaged);
    };
    let stored = std::str::from_utf8(digest_line)
        .ok()
        .and_then(|l| l.strip_suffix('\n'))
        .and_then(|l| Digest::parse(l).ok());
    let content = bytes.split_off(header_len);
    match stored {
        Some(d) if d == Digest::from_bytes(Sha256::digest(&content).into()) => Ok(ReadOutcome::Hit(content)),
        _ => Ok(ReadOutcome::Damaged),
    }
}
