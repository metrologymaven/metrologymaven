//! Atomic project copy (spec node `CORE.STO.LAYOUT` REQ-012, REQ-013).
//!
//! [`Store::copy_to`] writes the copy into a temporary sibling `.fid-copy-<32 hex>` of the
//! target, in the target's parent folder, and creates the target only by renaming that folder
//! there without replacing anything:
//!
//! 1. create the temporary folder and lock its `lock` at once;
//! 2. copy `project.db` with the SQLite online backup API in one step (never as raw bytes);
//! 3. copy every CAS object the copied database references (`entry_blobs`, `snapshots`),
//!    transitively through chunk manifests, each blob read back and re-hashed; copy `kernels/`
//!    byte for byte and compare hashes;
//! 4. write `format` with a new store-instance UUID, open the copy (schema and tamper checks,
//!    `PRAGMA integrity_check`) and run the caller's `finish`;
//! 5. fsync every file and folder, then close the copy's connection, lock and handles (Windows
//!    cannot rename a folder with open handles in it);
//! 6. the caller's `publish` runs its commit-stage check and calls the rename at most once;
//!    after the rename the target's identity is compared with the temporary folder's;
//! 7. on any failure the temporary folder is removed (and, if `publish` fails after a
//!    successful rename, the copy is renamed back to a temporary name and removed). After a
//!    crash, the next copy into the same parent removes every `.fid-copy-*` sibling whose
//!    `lock` no live process holds.

use super::{
    acquire, format_text, new_uuid, sys, FolderId, HexName, OpenMode, ProjectLocation, StoreError, BLOBS_DIR, DB_FILE,
    FORMAT_FILE, KERNELS_DIR, LAYOUT_VERSION, LOCK_FILE, TMP_DIR,
};
use crate::cas::{Address, BlobRef, BlobStore, CasError, PutOpts, Stored};
use crate::layout::CopyTarget;
use crate::txn::{Store, TxnError};
use fid_types::Digest;
use rusqlite::{Connection, OpenFlags};
use sha2::{Digest as _, Sha256};
use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Prefix of a temporary copy folder; `CORE.BUS.POL.FS` refuses it as a handle name.
pub const COPY_PREFIX: &str = ".fid-copy-";
/// Largest file copied from `kernels/`.
const MAX_KERNEL_FILE: u64 = 256 * 1024 * 1024;
/// Deepest folder nesting copied from `kernels/`.
const MAX_KERNEL_DEPTH: u32 = 16;

/// A step of [`Store::copy_to`], reported to the test hook ([`set_copy_hook`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyStep {
    /// The temporary folder exists and its lock is held.
    TempLocked,
    /// `project.db` was copied by the backup API.
    DatabaseCopied,
    /// One CAS blob (object, manifest or chunk) was copied and verified.
    BlobCopied,
    /// `format` is written and the copy passed its checks.
    Opened,
    /// The caller's `finish` returned.
    Finished,
    /// Every file and folder was flushed.
    Synced,
    /// The copy's connection, lock and handles are closed.
    Closed,
    /// Just before the no-replace rename.
    BeforeRename,
    /// The rename returned.
    Renamed,
    /// The target's identity matched the temporary folder's.
    IdentityChecked,
}

type Hook = Box<dyn Fn(CopyStep)>;

thread_local! {
    static HOOK: RefCell<Option<Hook>> = const { RefCell::new(None) };
}

/// Installs (or with `None` removes) this thread's copy hook. Tests use it to observe the steps,
/// to abort a child process at one of them, or to change the file system between them.
pub fn set_copy_hook(hook: Option<Box<dyn Fn(CopyStep)>>) {
    HOOK.with(|h| *h.borrow_mut() = hook);
}

fn step(s: CopyStep) {
    HOOK.with(|h| {
        if let Some(f) = h.borrow().as_ref() {
            f(s);
        }
    });
}

fn cas(e: CasError) -> StoreError {
    match e {
        CasError::Io(e) => StoreError::Io(e),
        other => StoreError::BlobCorrupt(other.to_string()),
    }
}

/// Whether `name` is a temporary copy folder name: the prefix and 32 lowercase hex digits.
fn is_temp_name(name: &str) -> bool {
    name.strip_prefix(COPY_PREFIX)
        .is_some_and(|h| h.len() == 32 && h.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
}

/// One plain name in the parent folder.
fn plain_target(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', ':', '\0'])
        && !name.to_ascii_lowercase().starts_with(COPY_PREFIX)
}

fn temp_name() -> String {
    format!("{COPY_PREFIX}{}", new_uuid().replace('-', ""))
}

/// Removes every `.fid-copy-*` folder in `parent` whose lock no live process holds.
fn remove_stale(parent: &sys::Dir, parent_path: &Path) -> Result<(), StoreError> {
    for (name, is_dir) in parent.entries()? {
        if !is_dir || !is_temp_name(&name) {
            continue;
        }
        let Ok(dir) = parent.sub(&name, false) else { continue };
        match dir.try_lock(LOCK_FILE) {
            Ok(sys::TryLock::Acquired(h)) => {
                h.release();
                drop(dir);
                // `remove_dir_all` does not follow links inside the folder.
                let _ = fs::remove_dir_all(parent_path.join(&name));
            }
            Ok(sys::TryLock::Busy) | Err(_) => {}
        }
    }
    Ok(())
}

/// Removes the temporary folder on drop unless disarmed.
struct TempGuard {
    path: PathBuf,
    armed: bool,
}

impl Drop for TempGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

/// fsync of a folder, tolerating the refusals `CORE.STO.CAS` tolerates on Windows.
fn sync_tree(path: &Path, depth: u32) -> io::Result<()> {
    if depth > MAX_KERNEL_DEPTH + 8 {
        return Err(io::Error::other("copy nested too deeply"));
    }
    for ent in fs::read_dir(path)? {
        let ent = ent?;
        let t = ent.file_type()?;
        if t.is_symlink() {
            return Err(io::Error::other("link in the copy"));
        }
        if t.is_dir() {
            sync_tree(&ent.path(), depth + 1)?;
        } else {
            fs::OpenOptions::new().write(true).open(ent.path())?.sync_all()?;
        }
    }
    crate::cas::sync_dir(path)
}

fn hash(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

/// Copies `kernels/` of `src` into `dst` (both `kernels/` folders), comparing hashes.
fn copy_kernels(src: &sys::Dir, dst: &sys::Dir, depth: u32) -> Result<(), StoreError> {
    if depth > MAX_KERNEL_DEPTH {
        return Err(StoreError::Io(io::Error::other("kernels/ nested too deeply")));
    }
    for (name, is_dir) in src.entries()? {
        let name = HexName::new(&name)?;
        if is_dir {
            copy_kernels(&src.sub(name.as_str(), false)?, &dst.sub(name.as_str(), true)?, depth + 1)?;
        } else {
            let bytes = src.read_file(name.as_str(), MAX_KERNEL_FILE)?;
            dst.write_file(name.as_str(), &bytes)?;
            if hash(&dst.read_file(name.as_str(), MAX_KERNEL_FILE)?) != hash(&bytes) {
                return Err(StoreError::BlobCorrupt(format!("kernel file {} did not copy intact", name.as_str())));
            }
        }
    }
    Ok(())
}

/// Copies one referenced digest and, for a chunked object, its chunks; each blob is checked by
/// the CAS, read back verified and stored again under the same digest.
fn copy_object(src: &BlobStore, dst: &BlobStore, d: Digest, seen: &mut BTreeSet<String>) -> Result<(), StoreError> {
    if seen.contains(d.as_str()) {
        return Ok(());
    }
    let list = match src.check_stored(&d).map_err(cas)? {
        Stored::Sound(list) => list,
        Stored::Missing(m) => return Err(StoreError::BlobCorrupt(format!("{m} is referenced but not stored"))),
        Stored::Corrupt(c) => return Err(StoreError::BlobCorrupt(format!("{c} does not match its digest"))),
    };
    for b in list {
        if !seen.insert(b.as_str().to_string()) {
            continue;
        }
        let size = fs::symlink_metadata(src.path_of(&b))?.len();
        let bytes = src.get(&Address::Blob(BlobRef { digest: b, size })).map_err(cas)?;
        let got = dst.put_bytes(&bytes, PutOpts { durable: true }).map_err(cas)?;
        if got.digest() != b {
            return Err(StoreError::BlobCorrupt(format!("{b} was stored under another address")));
        }
        step(CopyStep::BlobCopied);
    }
    Ok(())
}

/// The digests the copied database references.
fn referenced(db: &Path) -> Result<Vec<Digest>, StoreError> {
    let conn = Connection::open_with_flags(
        db,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .map_err(|e| StoreError::Io(io::Error::other(e)))?;
    let mut st = conn
        .prepare("SELECT digest FROM entry_blobs UNION SELECT content_digest FROM snapshots ORDER BY 1")
        .map_err(|e| StoreError::Io(io::Error::other(e)))?;
    let rows = st.query_map([], |r| r.get::<_, String>(0)).map_err(|e| StoreError::Io(io::Error::other(e)))?;
    let mut out = Vec::new();
    for r in rows {
        let s = r.map_err(|e| StoreError::Io(io::Error::other(e)))?;
        out.push(Digest::parse(&s).map_err(|_| StoreError::BlobCorrupt("a referenced digest is malformed".into()))?);
    }
    Ok(out)
}

impl Store {
    /// Copies this project to `target` (REQ-012, REQ-013; module documentation, steps 1 to 7).
    ///
    /// `finish` runs on the copy, opened for writing, before it is flushed and closed (for
    /// example `CORE.PRJ.CONT.LIFE`'s session header and genesis entry). `publish` receives the
    /// rename and must call it at most once, after its own commit-stage check; if it returns
    /// without calling it, or fails, nothing appears at the target. Returns the copy, reopened
    /// for writing after the identity check.
    ///
    /// Errors: `not_empty` for an existing target (commands map it to
    /// `policy/would_overwrite`), `outside_root` for a swapped parent or target, `blob_corrupt`,
    /// `io`, and the schema and transaction errors of the copy's checks; or `publish`'s error.
    pub fn copy_to(
        &self,
        target: &dyn CopyTarget,
        finish: impl FnOnce(&Store) -> Result<(), TxnError>,
        publish: impl FnOnce(&dyn Fn() -> Result<(), StoreError>) -> Result<(), TxnError>,
    ) -> Result<Store, TxnError> {
        let name = target.target_name().to_string();
        if !plain_target(&name) {
            return Err(StoreError::InvalidName(name).into());
        }
        let parent_path = super::absolute_checked(target.parent_path())?;
        let parent = sys::Dir::open(&parent_path, false).map_err(StoreError::from)?;
        if parent.identity().map_err(StoreError::from)? != target.parent_id() {
            return Err(StoreError::FolderSwapped.into());
        }
        remove_stale(&parent, &parent_path)?;
        if fs::symlink_metadata(parent_path.join(&name)).is_ok() {
            return Err(StoreError::NotEmpty.into());
        }

        // 1. Temporary folder, locked at once.
        let temp = temp_name();
        let temp_path = parent_path.join(&temp);
        fs::create_dir(&temp_path).map_err(StoreError::from)?;
        let mut guard = TempGuard { path: temp_path.clone(), armed: true };
        let dir = parent.sub(&temp, false).map_err(StoreError::from)?;
        let lock = acquire(&dir)?;
        let temp_id: FolderId = dir.identity().map_err(StoreError::from)?;
        step(CopyStep::TempLocked);

        // 2. Database: one backup step, so the copy is one consistent snapshot.
        let db_path = temp_path.join(DB_FILE);
        {
            let mut dst = Connection::open_with_flags(
                &db_path,
                OpenFlags::SQLITE_OPEN_READ_WRITE
                    | OpenFlags::SQLITE_OPEN_CREATE
                    | OpenFlags::SQLITE_OPEN_NO_MUTEX
                    | OpenFlags::SQLITE_OPEN_NOFOLLOW,
            )
            .map_err(|e| StoreError::Io(io::Error::other(e)))?;
            self.read(|src| {
                let b = rusqlite::backup::Backup::new(src, &mut dst)?;
                match b.step(-1)? {
                    rusqlite::backup::StepResult::Done => Ok(()),
                    _ => Err(rusqlite::Error::SqliteFailure(
                        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
                        Some("the backup did not finish in one step".into()),
                    )),
                }
            })?;
        }
        step(CopyStep::DatabaseCopied);

        // 3. Objects, transitively, and kernels/.
        for d in [BLOBS_DIR, KERNELS_DIR, TMP_DIR] {
            dir.sub(d, true).map_err(StoreError::from)?;
        }
        let dst_blobs = BlobStore::open(&temp_path.join(BLOBS_DIR)).map_err(cas)?;
        let mut seen = BTreeSet::new();
        for d in referenced(&db_path)? {
            copy_object(self.blobs(), &dst_blobs, d, &mut seen)?;
        }
        copy_kernels(
            &self.location().dir.sub(KERNELS_DIR, false).map_err(StoreError::from)?,
            &dir.sub(KERNELS_DIR, false).map_err(StoreError::from)?,
            0,
        )?;

        // 4. format, open with every check, then the caller's finish.
        let uuid = new_uuid();
        dir.write_file(FORMAT_FILE, format_text(LAYOUT_VERSION, &uuid).as_bytes()).map_err(StoreError::from)?;
        let loc = ProjectLocation { root: temp_path.clone(), uuid, layout_version: LAYOUT_VERSION, dir };
        let copy = Store::open_locked(loc, lock)?;
        let ok: String = copy.read(|c| c.query_row("PRAGMA integrity_check", [], |r| r.get(0)))?;
        if ok != "ok" {
            return Err(StoreError::BlobCorrupt(format!("the copied database failed integrity_check: {ok}")).into());
        }
        step(CopyStep::Opened);
        finish(&copy)?;
        step(CopyStep::Finished);

        // 5. Flush, then close everything inside the temporary folder.
        sync_tree(&temp_path, 0).map_err(StoreError::from)?;
        crate::cas::sync_dir(&parent_path).map_err(StoreError::from)?;
        step(CopyStep::Synced);
        drop(copy);
        step(CopyStep::Closed);

        // 6. Publish: the caller's check, then at most one no-replace rename.
        let calls = Cell::new(0u32);
        let renamed = Cell::new(false);
        let rename = || -> Result<(), StoreError> {
            calls.set(calls.get() + 1);
            if calls.get() > 1 {
                return Err(StoreError::Io(io::Error::other("the rename may be called only once")));
            }
            step(CopyStep::BeforeRename);
            match parent.rename_noreplace(&temp, &name) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => return Err(StoreError::NotEmpty),
                Err(e) if fs::symlink_metadata(parent_path.join(&name)).is_ok() && fs::metadata(&temp_path).is_ok() => {
                    // The target appeared (Windows reports a non-empty folder as access denied).
                    let _ = e;
                    return Err(StoreError::NotEmpty);
                }
                Err(e) => return Err(StoreError::Io(e)),
            }
            renamed.set(true);
            step(CopyStep::Renamed);
            if FolderId::of_path(&parent_path.join(&name)).map_err(StoreError::Io)? != temp_id {
                return Err(StoreError::FolderSwapped);
            }
            step(CopyStep::IdentityChecked);
            crate::cas::sync_dir(&parent_path)?;
            Ok(())
        };
        let published = publish(&rename);
        let target_path = parent_path.join(&name);
        match (published, renamed.get()) {
            (Ok(()), true) => {}
            (Ok(()), false) => {
                return Err(StoreError::Io(io::Error::other("publish returned without renaming the copy")).into())
            }
            (Err(e), false) => return Err(e),
            (Err(e), true) => {
                // The copy was published but the caller's commit failed: take it back.
                let back = temp_name();
                if parent.rename_noreplace(&name, &back).is_ok() {
                    let _ = fs::remove_dir_all(parent_path.join(&back));
                }
                return Err(e);
            }
        }
        guard.armed = false;

        // 7. Reopen the published copy and check it is the folder that was renamed.
        let (loc, lock) = ProjectLocation::open(&target_path, OpenMode::Write)?;
        if loc.dir.identity().map_err(StoreError::from)? != temp_id {
            return Err(StoreError::FolderSwapped.into());
        }
        let lock = lock.ok_or(StoreError::LockLost)?;
        let copy = Store::open_locked(loc, lock)?;
        copy.location().check_identity(temp_id)?;
        Ok(copy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_names() {
        let n = temp_name();
        assert!(is_temp_name(&n), "{n}");
        assert!(!is_temp_name(".fid-copy-xyz") && !is_temp_name("p1"));
        assert!(!plain_target(&n) && !plain_target("..") && !plain_target("a/b") && !plain_target(""));
        assert!(!plain_target(".FID-COPY-0123") && plain_target("p_copy"));
    }
}
