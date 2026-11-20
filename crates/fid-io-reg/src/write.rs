//! Atomic export writes and overwrite protection (spec node `IO.REG.WRITE`,
//! REQ-IO.REG-016, REQ-IO.REG-017).
//!
//! [`AtomicWriter`] is the one code path through which a writer puts bytes on disk inside
//! an export root. Every file-system call goes through the folder capability of the
//! [`ExportTarget`] that `IO.REG.ROOTS` resolved, so no path can leave that folder.
//!
//! 1. **Check** ([`AtomicWriter::create`]): an existing target without `overwrite` fails
//!    `target_exists` before any byte is written (REQ-IO.REG.WRITE-001). With `overwrite`,
//!    the existing target must be a regular file: a symbolic link, junction or other
//!    reparse point, or a folder, fails `io{InvalidInput}`; a read-only file fails
//!    `io{PermissionDenied}` on every platform (POSIX would replace it, Windows would not;
//!    one rule keeps the outcome platform-independent).
//! 2. **Sweep**: stale temp files in the folder are removed (best effort, at most
//!    [`MAX_STALE_SCAN`] folder entries looked at). Only regular files (not links) named
//!    exactly `.fid-tmp-<lowercase UUIDv7>` qualify, and only when both the UUIDv7
//!    timestamp and the modification time are at least [`STALE_AFTER`] old; a user file
//!    such as `.fid-tmp-notes.txt` is never touched.
//! 3. **Write**: bytes go to `.fid-tmp-<uuidv7>` in the target's folder, created
//!    exclusively, while SHA-256 and the byte count are taken. More than
//!    [`WriteOptions::max_bytes`] bytes fail `limit_exceeded` (`bytes`). The first error
//!    poisons the writer; [`AtomicWriter::commit`] then returns it.
//! 4. **Commit**: flush, `sync_all` on the temp file (an error fails the commit), close,
//!    then move into place: without `overwrite` by a hard link, which fails `target_exists`
//!    if a target appeared meanwhile (no clobbering). Any other hard-link error fails the
//!    commit with that error's kind (fail closed: a file system without hard links gets
//!    no replacing rename as a fallback). With `overwrite` the move is a rename, which
//!    replaces the folder entry and never writes through it. On Windows a rename refused
//!    for a sharing or lock violation is retried for at most [`RENAME_RETRY_MS`] in total,
//!    then fails `io`. After the move, the folder is synced on POSIX (best effort, as the
//!    spec says). On Windows the rename is std's (no `MOVEFILE_WRITE_THROUGH`) and there
//!    is no folder sync: durability of the new entry is best effort, which the spec allows.
//! 5. **Cleanup**: on any error, on [`AtomicWriter::cancel`] and on drop before commit,
//!    the temp file is closed and removed; the target is never touched before the final
//!    move (REQ-IO.REG.WRITE-002, -003).
//!
//! Writing into a local export root is W1 with `fs_effect: write_root` (CONSTITUTION
//! §3.1). Bytes and digest are D0; temp names are not part of any record.

use crate::error::{IoError, Position};
use crate::limits::{LimitKind, DEFAULT_BYTES};
use crate::roots::ExportTarget;
use cap_std::fs::{Dir, Metadata};
use fid_types::Digest;
use sha2::{Digest as _, Sha256};
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs::File;
use std::hash::{BuildHasher, Hasher};
use std::io::{self, BufWriter, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Prefix of every temp file name.
pub const TMP_PREFIX: &str = ".fid-tmp-";

/// Age (by modification time) from which a leftover temp file is removed.
pub const STALE_AFTER: Duration = Duration::from_secs(3600);

/// Most folder entries one stale sweep looks at.
pub const MAX_STALE_SCAN: usize = 65_536;

/// Total time a Windows rename refused for a sharing or lock violation is retried, in ms.
pub const RENAME_RETRY_MS: u64 = 315;

/// Write buffer size.
const BUF_CAP: usize = 64 * 1024;

/// Temp names tried before giving up (each is a fresh UUIDv7).
const TMP_ATTEMPTS: u64 = 8;

/// Per-process sequence mixed into temp names.
static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// How a write treats an existing target, and how large it may get.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteOptions {
    /// Replace an existing regular file at the target.
    pub overwrite: bool,
    /// Most bytes the file may hold; more fail `limit_exceeded` (`bytes`).
    pub max_bytes: u64,
}

impl Default for WriteOptions {
    /// No overwrite; the reader input limit ([`DEFAULT_BYTES`]) as size bound.
    fn default() -> WriteOptions {
        WriteOptions { overwrite: false, max_bytes: DEFAULT_BYTES }
    }
}

impl WriteOptions {
    /// The defaults with `overwrite` set as given.
    pub fn overwrite(overwrite: bool) -> WriteOptions {
        WriteOptions { overwrite, ..WriteOptions::default() }
    }
}

/// What a committed write produced, for the export journal event (REQ-IO.REG.WRITE-004).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteReceipt {
    /// SHA-256 of the committed file's bytes.
    pub sha256: Digest,
    /// Its size in bytes.
    pub bytes: u64,
}

/// The temp file under the write buffer. `fail_after` injects an I/O error once that many
/// bytes have reached the file (test seam, settable only with the `test-hooks` feature).
struct Sink {
    file: File,
    written: u64,
    fail_after: Option<u64>,
}

impl Write for Sink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut buf = buf;
        if let Some(n) = self.fail_after {
            let room = n.saturating_sub(self.written);
            if room == 0 && !buf.is_empty() {
                return Err(io::Error::other("injected write failure"));
            }
            let room = usize::try_from(room).unwrap_or(usize::MAX);
            if room < buf.len() {
                buf = &buf[..room];
            }
        }
        let k = self.file.write(buf)?;
        self.written = self.written.saturating_add(k as u64);
        Ok(k)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

/// Writes one export file atomically: the target is either the complete new file or
/// untouched. See the [module documentation](self) for the steps.
pub struct AtomicWriter {
    dir: Dir,
    target: OsString,
    tmp: OsString,
    out: Option<BufWriter<Sink>>,
    hasher: Sha256,
    bytes: u64,
    opts: WriteOptions,
    failed: Option<IoError>,
    done: bool,
    sealed: Option<WriteReceipt>,
    fail_hard_link: bool,
}

impl fmt::Debug for AtomicWriter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AtomicWriter")
            .field("target", &self.target)
            .field("tmp", &self.tmp)
            .field("bytes", &self.bytes)
            .field("opts", &self.opts)
            .field("failed", &self.failed)
            .field("done", &self.done)
            .finish()
    }
}

impl AtomicWriter {
    /// Checks the target, sweeps stale temp files and creates the temp file next to the
    /// target. Fails `target_exists` (target present, `overwrite` not set), `io` (target not
    /// a replaceable regular file, or a file-system error) before any byte is written.
    pub fn create(target: ExportTarget, opts: WriteOptions) -> Result<AtomicWriter, IoError> {
        let ExportTarget { dir, name, final_link, .. } = target;
        if final_link {
            // The handle named a link; ROOTS resolved it to its target. Never written through.
            return Err(if opts.overwrite { kind_err(io::ErrorKind::InvalidInput) } else { IoError::TargetExists });
        }
        check_target(&dir, &name, opts.overwrite)?;
        sweep_stale(&dir, SystemTime::now());
        let (file, tmp) = create_tmp(&dir)?;
        let sink = Sink { file, written: 0, fail_after: None };
        Ok(AtomicWriter {
            dir,
            target: name,
            tmp,
            out: Some(BufWriter::with_capacity(BUF_CAP, sink)),
            hasher: Sha256::new(),
            bytes: 0,
            opts,
            failed: None,
            done: false,
            sealed: None,
            fail_hard_link: false,
        })
    }

    /// Step 1 alone, for a dry run: the checks [`AtomicWriter::create`] makes on `target`
    /// (`target_exists`, a link or folder or read-only file under `overwrite`), with no
    /// stale sweep and no file created, removed or changed.
    pub fn check(target: &ExportTarget, opts: WriteOptions) -> Result<(), IoError> {
        if target.final_link {
            return Err(if opts.overwrite { kind_err(io::ErrorKind::InvalidInput) } else { IoError::TargetExists });
        }
        check_target(&target.dir, &target.name, opts.overwrite)
    }

    /// The target file name inside the export folder.
    pub fn target_name(&self) -> &OsStr {
        &self.target
    }

    /// The temp file name inside the export folder.
    pub fn tmp_name(&self) -> &OsStr {
        &self.tmp
    }

    /// Bytes accepted so far.
    pub fn bytes_written(&self) -> u64 {
        self.bytes
    }

    /// Test seam for AC-3: the temp file fails with an I/O error once `n` bytes have
    /// reached it. Exists only with the `test-hooks` feature.
    #[cfg(any(test, feature = "test-hooks"))]
    #[doc(hidden)]
    pub fn fail_after(&mut self, n: u64) {
        if let Some(out) = self.out.as_mut() {
            out.get_mut().fail_after = Some(n);
        }
    }

    /// Test seam: the hard link of a commit without `overwrite` fails `Unsupported`, as on
    /// a file system without hard links. Exists only with the `test-hooks` feature.
    #[cfg(any(test, feature = "test-hooks"))]
    #[doc(hidden)]
    pub fn fail_hard_link(&mut self) {
        self.fail_hard_link = true;
    }

    fn fail(&mut self, e: IoError) -> IoError {
        if self.failed.is_none() {
            self.failed = Some(e.clone());
        }
        e
    }

    fn io_at(&self, e: &io::Error) -> IoError {
        IoError::Io { kind: e.kind(), at: Position::byte(self.bytes) }
    }

    /// Appends `buf`. Fails `limit_exceeded` past [`WriteOptions::max_bytes`] and `io` on a
    /// file-system error; after any failure every call fails with the first error.
    pub fn write_bytes(&mut self, buf: &[u8]) -> Result<(), IoError> {
        if let Some(e) = &self.failed {
            return Err(e.clone());
        }
        if self.sealed.is_some() {
            return Err(self.fail(kind_err(io::ErrorKind::Other)));
        }
        let end = u64::try_from(buf.len()).ok().and_then(|n| self.bytes.checked_add(n));
        let end = match end {
            Some(end) if end <= self.opts.max_bytes => end,
            _ => {
                let e = IoError::LimitExceeded {
                    limit: LimitKind::Bytes,
                    value: self.opts.max_bytes,
                    at: Position::byte(self.bytes),
                };
                return Err(self.fail(e));
            }
        };
        let r = match self.out.as_mut() {
            Some(out) => out.write_all(buf),
            None => Err(io::Error::from(io::ErrorKind::Other)),
        };
        if let Err(e) = r {
            let e = self.io_at(&e);
            return Err(self.fail(e));
        }
        self.hasher.update(buf);
        self.bytes = end;
        Ok(())
    }

    /// Flushes, syncs and closes the temp file and returns its digest and size, without
    /// touching the target: the bytes are durable under the temp name, ready for
    /// [`AtomicWriter::commit`]. Calling it again returns the same receipt; writing after it
    /// fails. On any error the writer is poisoned (the temp file is removed on drop).
    pub fn prepare(&mut self) -> Result<WriteReceipt, IoError> {
        if let Some(r) = self.sealed {
            return Ok(r);
        }
        if let Some(e) = &self.failed {
            return Err(e.clone());
        }
        let out = match self.out.take() {
            Some(out) => out,
            None => return Err(self.fail(kind_err(io::ErrorKind::Other))),
        };
        let sink = match out.into_inner() {
            Ok(sink) => sink,
            Err(e) => {
                let e = self.io_at(e.error());
                return Err(self.fail(e));
            }
        };
        let file = sink.file;
        if let Err(e) = file.sync_all() {
            let e = self.io_at(&e);
            return Err(self.fail(e));
        }
        drop(file);
        let sha256 = Digest::from_bytes(std::mem::take(&mut self.hasher).finalize().into());
        let r = WriteReceipt { sha256, bytes: self.bytes };
        self.sealed = Some(r);
        Ok(r)
    }

    /// [`AtomicWriter::prepare`] (if not done yet), then moves the temp file onto the
    /// target and returns its digest and size. On any error the temp file is removed and the
    /// target is unchanged.
    pub fn commit(mut self) -> Result<WriteReceipt, IoError> {
        let r = self.prepare()?;
        self.place()?;
        self.done = true;
        sync_dir(&self.dir);
        Ok(r)
    }

    /// Moves the closed, synced temp file onto the target.
    fn place(&mut self) -> Result<(), IoError> {
        if self.opts.overwrite {
            // A link or folder put there since `create` is refused; a rename would replace
            // a link entry anyway, never write through it.
            check_target(&self.dir, &self.target, true)?;
            return rename(&self.dir, &self.tmp, &self.target).map_err(|e| self.io_at(&e));
        }
        let linked = if self.fail_hard_link {
            Err(io::Error::from(io::ErrorKind::Unsupported))
        } else {
            self.dir.hard_link(&self.tmp, &self.dir, &self.target)
        };
        match linked {
            Ok(()) => {
                // The target is complete; a temp name left behind (removal refused) is a
                // second link to it and is swept once stale.
                let _ = self.dir.remove_file(&self.tmp);
                Ok(())
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Err(IoError::TargetExists),
            // Fail closed: a file system without hard links (FAT, exFAT, some network or
            // FUSE mounts) gets no no-replace move; a rename could clobber a target created
            // meanwhile. See the leaf's open questions.
            Err(e) => Err(self.io_at(&e)),
        }
    }

    /// Abandons the write: the temp file is removed and the target is unchanged.
    pub fn cancel(self) {
        drop(self);
    }
}

impl Write for AtomicWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.write_bytes(buf).map(|()| buf.len()).map_err(IoError::into_io)
    }

    fn flush(&mut self) -> io::Result<()> {
        if let Some(e) = &self.failed {
            return Err(e.clone().into_io());
        }
        let r = match self.out.as_mut() {
            Some(out) => out.flush(),
            None => Ok(()),
        };
        match r {
            Ok(()) => Ok(()),
            Err(e) => {
                let e = self.io_at(&e);
                Err(self.fail(e).into_io())
            }
        }
    }
}

impl Drop for AtomicWriter {
    fn drop(&mut self) {
        if self.done {
            return;
        }
        if let Some(out) = self.out.take() {
            // Close without flushing the buffer: these bytes are being discarded.
            let (sink, _) = out.into_parts();
            drop(sink);
        }
        let _ = self.dir.remove_file(&self.tmp);
    }
}

fn kind_err(kind: io::ErrorKind) -> IoError {
    IoError::Io { kind, at: Position::default() }
}

fn io_err(e: &io::Error) -> IoError {
    kind_err(e.kind())
}

/// True for a symbolic link, a junction or any other reparse point.
fn is_link(m: &Metadata) -> bool {
    m.file_type().is_symlink() || reparse_point(m)
}

#[cfg(windows)]
fn reparse_point(m: &Metadata) -> bool {
    use cap_std::fs::MetadataExt;
    m.file_attributes() & crate::roots::FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn reparse_point(_m: &Metadata) -> bool {
    false
}

/// The target check of step 1, looked at without following links.
fn check_target(dir: &Dir, name: &OsStr, overwrite: bool) -> Result<(), IoError> {
    let m = match dir.symlink_metadata(name) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(io_err(&e)),
    };
    if !overwrite {
        return Err(IoError::TargetExists);
    }
    if is_link(&m) || !m.is_file() {
        return Err(kind_err(io::ErrorKind::InvalidInput));
    }
    if m.permissions().readonly() {
        return Err(kind_err(io::ErrorKind::PermissionDenied));
    }
    Ok(())
}

/// Removes `.fid-tmp-*` regular files at least [`STALE_AFTER`] old (best effort).
fn sweep_stale(dir: &Dir, now: SystemTime) {
    let entries = match dir.entries() {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.take(MAX_STALE_SCAN) {
        let name = match entry {
            Ok(entry) => entry.file_name(),
            Err(_) => continue,
        };
        let born = match tmp_name_unix_ms(name.as_encoded_bytes()) {
            Some(ms) => UNIX_EPOCH + Duration::from_millis(ms),
            None => continue,
        };
        if !now.duration_since(born).is_ok_and(|age| age >= STALE_AFTER) {
            continue;
        }
        let m = match dir.symlink_metadata(&name) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if is_link(&m) || !m.is_file() {
            continue;
        }
        let stale = m.modified().is_ok_and(|t| now.duration_since(t.into_std()).is_ok_and(|age| age >= STALE_AFTER));
        if stale {
            let _ = dir.remove_file(&name);
        }
    }
}

/// The UUIDv7 timestamp (Unix ms) of a name of the exact form `.fid-tmp-<lowercase
/// hyphenated UUIDv7>`; `None` for any other name.
fn tmp_name_unix_ms(name: &[u8]) -> Option<u64> {
    let u = name.strip_prefix(TMP_PREFIX.as_bytes())?;
    if u.len() != 36 {
        return None;
    }
    let mut ms = 0u64;
    for (i, &b) in u.iter().enumerate() {
        if matches!(i, 8 | 13 | 18 | 23) {
            if b != b'-' {
                return None;
            }
            continue;
        }
        let v = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            _ => return None,
        };
        if i < 13 {
            // The first 12 hex digits (8, then 4 after the hyphen) are the 48-bit time.
            ms = (ms << 4) | u64::from(v);
        }
    }
    let version_ok = u[14] == b'7';
    let variant_ok = matches!(u[19], b'8' | b'9' | b'a' | b'b');
    if version_ok && variant_ok {
        Some(ms)
    } else {
        None
    }
}

/// Creates the temp file exclusively under a fresh name.
fn create_tmp(dir: &Dir) -> Result<(File, OsString), IoError> {
    let mut opts = cap_std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    for _ in 0..TMP_ATTEMPTS {
        let name = OsString::from(format!("{TMP_PREFIX}{}", uuid_v7_text()));
        match dir.open_with(&name, &opts) {
            Ok(f) => return Ok((f.into_std(), name)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(io_err(&e)),
        }
    }
    Err(kind_err(io::ErrorKind::AlreadyExists))
}

/// 64 bits of per-call entropy from the standard library's randomly keyed hasher, mixed
/// with a sequence number, the process ID and the time. Not secret: the exclusive create
/// is what prevents clashes.
fn entropy(seq: u64, nanos: u32, lane: u8) -> u64 {
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u64(seq);
    h.write_u32(nanos);
    h.write_u32(std::process::id());
    h.write_u8(lane);
    h.finish()
}

/// The 16 bytes of an RFC 9562 UUIDv7 for `unix_ms`.
fn uuid_v7_bytes(unix_ms: u64, rand_a: u64, rand_b: u64) -> [u8; 16] {
    let mut b = [0u8; 16];
    b[..6].copy_from_slice(&(unix_ms & 0xFFFF_FFFF_FFFF).to_be_bytes()[2..]);
    let ver = 0x7000u16 | (rand_a & 0x0FFF) as u16;
    b[6..8].copy_from_slice(&ver.to_be_bytes());
    let var = (rand_b & 0x3FFF_FFFF_FFFF_FFFF) | 0x8000_0000_0000_0000;
    b[8..].copy_from_slice(&var.to_be_bytes());
    b
}

/// Lowercase hyphenated text of a UUID.
fn uuid_text(b: &[u8; 16]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(36);
    for (i, byte) in b.iter().enumerate() {
        if matches!(i, 4 | 6 | 8 | 10) {
            s.push('-');
        }
        s.push(char::from(HEX[usize::from(byte >> 4)]));
        s.push(char::from(HEX[usize::from(byte & 0x0F)]));
    }
    s
}

fn uuid_v7_text() -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let ms = u64::try_from(now.as_millis()).unwrap_or(u64::MAX);
    let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let nanos = now.subsec_nanos();
    uuid_text(&uuid_v7_bytes(ms, entropy(seq, nanos, 1), entropy(seq, nanos, 2)))
}

/// Renames within the folder.
#[cfg(not(windows))]
fn rename(dir: &Dir, from: &OsStr, to: &OsStr) -> io::Result<()> {
    dir.rename(from, dir, to)
}

/// Renames within the folder; a sharing or lock violation (often a virus scanner or
/// indexer holding the target briefly) is retried with back-off for at most
/// [`RENAME_RETRY_MS`] in total.
#[cfg(windows)]
fn rename(dir: &Dir, from: &OsStr, to: &OsStr) -> io::Result<()> {
    let mut r = dir.rename(from, dir, to);
    let mut wait = 5u64;
    let mut spent = 0u64;
    while spent + wait <= RENAME_RETRY_MS {
        match &r {
            Err(e) if transient_windows(e) => {}
            _ => break,
        }
        std::thread::sleep(Duration::from_millis(wait));
        spent += wait;
        wait *= 2;
        r = dir.rename(from, dir, to);
    }
    r
}

/// `ERROR_ACCESS_DENIED` (5), `ERROR_SHARING_VIOLATION` (32), `ERROR_LOCK_VIOLATION` (33).
#[cfg(windows)]
fn transient_windows(e: &io::Error) -> bool {
    matches!(e.raw_os_error(), Some(5) | Some(32) | Some(33))
}

/// Best-effort folder sync after the rename, so the new entry survives a power cut.
#[cfg(unix)]
fn sync_dir(dir: &Dir) {
    if let Ok(d) = dir.open(".") {
        let _ = d.into_std().sync_all();
    }
}

/// Windows has no folder sync; NTFS journals the rename.
#[cfg(not(unix))]
fn sync_dir(_dir: &Dir) {}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn uuid_v7_layout() {
        // Hand-labelled: 2022-02-22T19:22:22Z = 0x017F22E279B0 ms (RFC 9562 appendix A.6
        // example time), rand_a 0xCC3, rand_b 0x18C4DC0C0C07398F.
        let b = uuid_v7_bytes(0x017F_22E2_79B0, 0xCC3, 0x18C4_DC0C_0C07_398F);
        assert_eq!(uuid_text(&b), "017f22e2-79b0-7cc3-98c4-dc0c0c07398f");
        // Out-of-range inputs are masked into their fields.
        let b = uuid_v7_bytes(u64::MAX, u64::MAX, u64::MAX);
        assert_eq!(uuid_text(&b), "ffffffff-ffff-7fff-bfff-ffffffffffff");
        let b = uuid_v7_bytes(0, 0, 0);
        assert_eq!(uuid_text(&b), "00000000-0000-7000-8000-000000000000");
    }

    #[test]
    fn temp_name_timestamps() {
        let t = |s: &str| tmp_name_unix_ms(s.as_bytes());
        assert_eq!(t(".fid-tmp-017f22e2-79b0-7cc3-98c4-dc0c0c07398f"), Some(0x017F_22E2_79B0));
        assert_eq!(t(".fid-tmp-00000000-0000-7000-8000-000000000000"), Some(0));
        for bad in [
            ".fid-tmp-notes.txt",
            ".fid-tmp-",
            "fid-tmp-017f22e2-79b0-7cc3-98c4-dc0c0c07398f",
            ".fid-tmp-017F22E2-79B0-7CC3-98C4-DC0C0C07398F",
            ".fid-tmp-017f22e2-79b0-4cc3-98c4-dc0c0c07398f",
            ".fid-tmp-017f22e2-79b0-7cc3-c8c4-dc0c0c07398f",
            ".fid-tmp-017f22e2-79b0-7cc3-98c4-dc0c0c07398f.bak",
            ".fid-tmp-017f22e2079b0-7cc3-98c4-dc0c0c07398f",
            ".fid-tmp-017f22e2-79b0-7cc3-98c4-dc0c0c07398g",
        ] {
            assert_eq!(t(bad), None, "{bad}");
        }
        let fresh = uuid_v7_text();
        assert!(t(&format!("{TMP_PREFIX}{fresh}")).is_some());
    }

    #[test]
    fn temp_names_differ() {
        let a = uuid_v7_text();
        let b = uuid_v7_text();
        assert_ne!(a, b);
        assert_eq!(a.len(), 36);
        assert_eq!(&a[14..15], "7");
    }

    #[test]
    fn injected_failure_cuts_at_the_byte() {
        let dir = std::env::temp_dir().join(format!("fid-io-write-sink-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = File::create(dir.join("s")).unwrap();
        let mut s = Sink { file, written: 0, fail_after: Some(5) };
        assert_eq!(s.write(b"abc").unwrap(), 3);
        assert_eq!(s.write(b"defg").unwrap(), 2);
        assert!(s.write(b"h").is_err());
        assert!(s.write(b"").is_ok());
        drop(s);
        assert_eq!(std::fs::read(dir.join("s")).unwrap(), b"abcde");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
