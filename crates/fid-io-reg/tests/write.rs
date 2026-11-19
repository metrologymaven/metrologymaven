//! Acceptance criteria of spec node IO.REG.WRITE (AC-1 .. AC-5) and the requirements
//! behind them, plus the platform cases (links, junctions, read-only files, Windows
//! sharing violations). Every fixture is built per test in a fresh temporary folder.
//! Expected digests and sizes come from `fixtures/io/reg/write/expected.txt`, written by
//! the independent Python script next to it (`make_expected.py`, hashlib); expected
//! outcomes are hand-labelled from the spec's rules.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_io_reg::write::{STALE_AFTER, TMP_PREFIX};
use fid_io_reg::{
    AtomicWriter, ExportTarget, IoError, LimitKind, Position, Principal, RootAccess, RootId, RootSet, WriteOptions,
    WriteReceipt,
};
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

const OLD: &[u8] = b"solid old\nendsolid old\n";
const HALF: usize = 512 * 1024;
const CHILD_ENV: &str = "FID_IO_REG_WRITE_CHILD_DIR";

/// `name -> (bytes, "sha256:<hex>")` from the Python-generated fixture.
fn expected() -> BTreeMap<String, (u64, String)> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/io/reg/write/expected.txt");
    let text = fs::read_to_string(&p).unwrap();
    let mut out = BTreeMap::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let f: Vec<&str> = line.split_whitespace().collect();
        assert_eq!(f.len(), 3, "{line}");
        out.insert(f[0].to_string(), (f[1].parse().unwrap(), format!("sha256:{}", f[2])));
    }
    out
}

fn assert_receipt(r: &WriteReceipt, name: &str) {
    let (bytes, sha) = expected().remove(name).unwrap();
    assert_eq!(r.bytes, bytes, "{name}");
    assert_eq!(r.sha256.as_str(), sha, "{name}");
}

/// The `mib` payload: byte i = (31*i + 7*(i >> 8)) mod 256 (rule shared with the script).
fn mib() -> Vec<u8> {
    (0..1u64 << 20).map(|i| ((31 * i + 7 * (i >> 8)) & 0xFF) as u8).collect()
}

/// A temporary folder `raw` holding the export root `raw/R`; removed on drop.
struct Tmp {
    raw: PathBuf,
}

impl Tmp {
    fn new(name: &str) -> Tmp {
        let raw = std::env::temp_dir().join(format!("fid-io-write-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&raw);
        fs::create_dir_all(raw.join("R")).unwrap();
        Tmp { raw }
    }

    fn root(&self) -> PathBuf {
        self.raw.join("R")
    }

    fn set(&self) -> RootSet {
        let mut s = RootSet::new();
        s.insert(RootId::new("exp").unwrap(), RootAccess::Export, self.root());
        s
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.raw);
    }
}

fn target(s: &RootSet, rel: &str) -> ExportTarget {
    s.resolve_export(Principal::Agent, &format!("root://exp/{rel}")).unwrap()
}

fn human(s: &RootSet, p: &Path) -> ExportTarget {
    s.resolve_export(Principal::Human, &p.to_string_lossy()).unwrap()
}

fn write_all(t: ExportTarget, opts: WriteOptions, data: &[u8]) -> Result<WriteReceipt, IoError> {
    let mut w = AtomicWriter::create(t, opts)?;
    w.write_bytes(data)?;
    w.commit()
}

/// Names of `.fid-tmp-*` entries in `dir`, sorted.
fn temps(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(TMP_PREFIX))
        .collect();
    v.sort();
    v
}

fn io_kind(e: &IoError) -> io::ErrorKind {
    match e {
        IoError::Io { kind, .. } => *kind,
        other => panic!("expected io, got {other:?}"),
    }
}

/// True for `.fid-tmp-` followed by a lowercase hyphenated RFC 9562 version-7 UUID.
fn is_tmp_v7(name: &str) -> bool {
    let u = match name.strip_prefix(TMP_PREFIX) {
        Some(u) => u.as_bytes(),
        None => return false,
    };
    let hex = |b: &u8| b.is_ascii_digit() || (b'a'..=b'f').contains(b);
    u.len() == 36
        && u.iter().enumerate().all(|(i, b)| if matches!(i, 8 | 13 | 18 | 23) { *b == b'-' } else { hex(b) })
        && u[14] == b'7'
        && matches!(u[19], b'8' | b'9' | b'a' | b'b')
}

/// Outside CI a link the platform refuses (Windows without the symlink privilege) skips
/// the case; on CI it fails the test.
fn symlink_file(target: &Path, link: &Path) -> bool {
    #[cfg(unix)]
    let r = std::os::unix::fs::symlink(target, link);
    #[cfg(windows)]
    let r = std::os::windows::fs::symlink_file(target, link);
    match r {
        Ok(()) => true,
        Err(e) if std::env::var_os("CI").is_some() => panic!("cannot create link {}: {e}", link.display()),
        Err(_) => false,
    }
}

#[allow(clippy::permissions_set_readonly_false)]
fn set_readonly(p: &Path, ro: bool) {
    let mut perm = fs::metadata(p).unwrap().permissions();
    perm.set_readonly(ro);
    fs::set_permissions(p, perm).unwrap();
}

fn age(p: &Path, by: Duration) {
    let f = fs::OpenOptions::new().write(true).open(p).unwrap();
    f.set_modified(SystemTime::now() - by).unwrap();
}

// ------------------------------------------------------------------ fixture sanity

#[test]
fn fixture_payload_rules_match_the_sizes() {
    let e = expected();
    assert_eq!(e["mib"].0, mib().len() as u64);
    assert_eq!(e["half"].0, HALF as u64);
    assert_eq!(e["old"].0, OLD.len() as u64);
    // Hand-labelled FIPS 180-2 values, independent of the script.
    assert_eq!(e["abc"].1, "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    assert_eq!(e["empty"].1, "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
}

// ------------------------------------------------------------------ AC-1

#[test]
fn ac1_one_mib_commits_with_digest_and_no_temp_left() {
    let t = Tmp::new("ac1");
    let s = t.set();
    let data = mib();
    let mut w = AtomicWriter::create(target(&s, "out.stl"), WriteOptions::default()).unwrap();
    // Odd chunk sizes cross the write buffer boundary at varying offsets.
    for chunk in data.chunks(1000) {
        w.write_all(chunk).unwrap();
    }
    assert_eq!(w.bytes_written(), data.len() as u64);
    // REQ-002: bytes go to a `.fid-tmp-<uuidv7>` file next to the target; the target does
    // not exist before the commit.
    let tmp = w.tmp_name().to_string_lossy().into_owned();
    assert!(is_tmp_v7(&tmp), "{tmp}");
    assert_eq!(temps(&t.root()), [tmp]);
    assert!(!t.root().join("out.stl").exists());
    let r = w.commit().unwrap();
    assert_receipt(&r, "mib");
    assert_eq!(fs::read(t.root().join("out.stl")).unwrap(), data);
    assert!(temps(&t.root()).is_empty());
}

#[test]
fn empty_and_small_files_commit() {
    let t = Tmp::new("small");
    let s = t.set();
    let r = write_all(target(&s, "empty.stl"), WriteOptions::default(), b"").unwrap();
    assert_receipt(&r, "empty");
    assert_eq!(fs::read(t.root().join("empty.stl")).unwrap(), b"");
    let r = write_all(target(&s, "abc.txt"), WriteOptions::default(), b"abc").unwrap();
    assert_receipt(&r, "abc");
    assert!(temps(&t.root()).is_empty());
}

#[test]
fn writes_into_a_sub_folder_of_the_root() {
    let t = Tmp::new("sub");
    fs::create_dir_all(t.root().join("a").join("b")).unwrap();
    let s = t.set();
    let r = write_all(target(&s, "a/b/out.stl"), WriteOptions::default(), b"abc").unwrap();
    assert_receipt(&r, "abc");
    assert_eq!(fs::read(t.root().join("a").join("b").join("out.stl")).unwrap(), b"abc");
    assert!(temps(&t.root().join("a").join("b")).is_empty());
}

// ------------------------------------------------------------------ AC-2 / REQ-001

#[test]
fn ac2_existing_target_without_overwrite_fails_before_writing() {
    let t = Tmp::new("ac2");
    let out = t.root().join("out.stl");
    fs::write(&out, OLD).unwrap();
    let s = t.set();
    let e = AtomicWriter::create(target(&s, "out.stl"), WriteOptions::default()).unwrap_err();
    assert_eq!(e, IoError::TargetExists);
    assert_eq!(e.code(), "target_exists");
    // Raw path of a human: same rule.
    let e = AtomicWriter::create(human(&s, &out), WriteOptions::default()).unwrap_err();
    assert_eq!(e, IoError::TargetExists);
    // The file is unchanged (its bytes, hence its digest) and no temp file was created.
    assert_eq!(fs::read(&out).unwrap(), OLD);
    assert_eq!(expected()["old"].0, OLD.len() as u64);
    assert!(temps(&t.root()).is_empty());
}

#[test]
fn target_created_during_the_write_is_not_clobbered() {
    let t = Tmp::new("noclobber");
    let out = t.root().join("out.stl");
    let s = t.set();
    let mut w = AtomicWriter::create(target(&s, "out.stl"), WriteOptions::default()).unwrap();
    w.write_bytes(b"abc").unwrap();
    fs::write(&out, OLD).unwrap();
    assert_eq!(w.commit().unwrap_err(), IoError::TargetExists);
    assert_eq!(fs::read(&out).unwrap(), OLD);
    assert!(temps(&t.root()).is_empty());
}

#[test]
fn two_writers_without_overwrite_first_commit_wins() {
    let t = Tmp::new("race2");
    let s = t.set();
    let mut a = AtomicWriter::create(target(&s, "out.stl"), WriteOptions::default()).unwrap();
    let mut b = AtomicWriter::create(target(&s, "out.stl"), WriteOptions::default()).unwrap();
    a.write_bytes(b"abc").unwrap();
    b.write_bytes(OLD).unwrap();
    assert_receipt(&a.commit().unwrap(), "abc");
    assert_eq!(b.commit().unwrap_err(), IoError::TargetExists);
    assert_eq!(fs::read(t.root().join("out.stl")).unwrap(), b"abc");
    assert!(temps(&t.root()).is_empty());
}

#[test]
fn overwrite_replaces_a_regular_file() {
    let t = Tmp::new("over");
    let out = t.root().join("out.stl");
    fs::write(&out, OLD).unwrap();
    let s = t.set();
    let r = write_all(target(&s, "out.stl"), WriteOptions::overwrite(true), b"abc").unwrap();
    assert_receipt(&r, "abc");
    assert_eq!(fs::read(&out).unwrap(), b"abc");
    // Last committed writer wins, in full.
    let mut a = AtomicWriter::create(target(&s, "out.stl"), WriteOptions::overwrite(true)).unwrap();
    let mut b = AtomicWriter::create(target(&s, "out.stl"), WriteOptions::overwrite(true)).unwrap();
    a.write_bytes(OLD).unwrap();
    b.write_bytes(b"").unwrap();
    assert_receipt(&a.commit().unwrap(), "old");
    assert_receipt(&b.commit().unwrap(), "empty");
    assert_eq!(fs::read(&out).unwrap(), b"");
    assert!(temps(&t.root()).is_empty());
}

// ------------------------------------------------------------------ AC-3 / REQ-003

#[test]
fn ac3_injected_io_error_after_512_kib_leaves_old_target() {
    let t = Tmp::new("ac3");
    let out = t.root().join("out.stl");
    fs::write(&out, OLD).unwrap();
    let s = t.set();
    let data = mib();
    let mut w = AtomicWriter::create(target(&s, "out.stl"), WriteOptions::overwrite(true)).unwrap();
    w.fail_after(HALF as u64);
    let mut failed = None;
    for chunk in data.chunks(4096) {
        if let Err(e) = w.write_bytes(chunk) {
            failed = Some(e);
            break;
        }
    }
    let e = failed.expect("the injected error must surface while writing 1 MiB");
    assert_eq!(e.code(), "io");
    assert_eq!(io_kind(&e), io::ErrorKind::Other);
    assert!(e.position().unwrap().byte >= HALF as u64);
    // The writer is poisoned: later writes and the commit return the first error.
    assert_eq!(w.write_bytes(b"x").unwrap_err(), e);
    assert_eq!(w.commit().unwrap_err(), e);
    assert_eq!(fs::read(&out).unwrap(), OLD);
    assert!(temps(&t.root()).is_empty());
}

#[test]
fn ac3_error_surfacing_at_commit_flush_leaves_old_target() {
    let t = Tmp::new("ac3flush");
    let out = t.root().join("out.stl");
    fs::write(&out, OLD).unwrap();
    let s = t.set();
    let data = mib();
    let mut w = AtomicWriter::create(target(&s, "out.stl"), WriteOptions::overwrite(true)).unwrap();
    w.fail_after(HALF as u64);
    // 512 KiB + 8 KiB: the last bytes sit in the write buffer until the commit flushes.
    for chunk in data[..HALF + 8192].chunks(4096) {
        w.write_bytes(chunk).unwrap();
    }
    let e = w.commit().unwrap_err();
    assert_eq!(e.code(), "io");
    assert_eq!(fs::read(&out).unwrap(), OLD);
    assert!(temps(&t.root()).is_empty());
}

/// A serialiser-side `Write` wrapper that fails after `left` bytes.
struct FailAfter<'a> {
    inner: &'a mut AtomicWriter,
    left: usize,
}

impl Write for FailAfter<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.left == 0 {
            return Err(io::Error::other("serialiser failed"));
        }
        let n = buf.len().min(self.left);
        self.inner.write_all(&buf[..n])?;
        self.left -= n;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[test]
fn ac3_serialiser_error_then_drop_leaves_old_target() {
    let t = Tmp::new("ac3ser");
    let out = t.root().join("out.stl");
    fs::write(&out, OLD).unwrap();
    let s = t.set();
    let mut w = AtomicWriter::create(target(&s, "out.stl"), WriteOptions::overwrite(true)).unwrap();
    let r = FailAfter { inner: &mut w, left: HALF }.write_all(&mib());
    assert!(r.is_err());
    assert_eq!(w.bytes_written(), HALF as u64);
    drop(w);
    assert_eq!(fs::read(&out).unwrap(), OLD);
    assert!(temps(&t.root()).is_empty());
}

#[test]
fn size_bound_fails_limit_exceeded() {
    let t = Tmp::new("bound");
    let s = t.set();
    let opts = WriteOptions { overwrite: false, max_bytes: 1000 };
    let mut w = AtomicWriter::create(target(&s, "out.stl"), opts).unwrap();
    w.write_bytes(&[7u8; 600]).unwrap();
    let e = w.write_bytes(&[7u8; 401]).unwrap_err();
    assert_eq!(e, IoError::LimitExceeded { limit: LimitKind::Bytes, value: 1000, at: Position::byte(600) });
    assert_eq!(w.commit().unwrap_err(), e);
    assert!(!t.root().join("out.stl").exists());
    assert!(temps(&t.root()).is_empty());
    // Exactly at the bound is allowed.
    let r = write_all(target(&s, "out.stl"), opts, &[7u8; 1000]).unwrap();
    assert_eq!(r.bytes, 1000);
}

// ------------------------------------------------------------------ AC-4

#[test]
fn ac4_dropped_or_cancelled_writer_leaves_no_temp() {
    let t = Tmp::new("ac4");
    let s = t.set();
    let mut w = AtomicWriter::create(target(&s, "new.stl"), WriteOptions::default()).unwrap();
    w.write_bytes(&mib()[..HALF]).unwrap();
    assert_eq!(temps(&t.root()).len(), 1);
    drop(w);
    assert!(temps(&t.root()).is_empty());
    assert!(!t.root().join("new.stl").exists());

    let out = t.root().join("out.stl");
    fs::write(&out, OLD).unwrap();
    let mut w = AtomicWriter::create(target(&s, "out.stl"), WriteOptions::overwrite(true)).unwrap();
    w.write_all(b"abc").unwrap();
    w.flush().unwrap();
    w.cancel();
    assert!(temps(&t.root()).is_empty());
    assert_eq!(fs::read(&out).unwrap(), OLD);
}

// ------------------------------------------------------------------ AC-5

/// Child half of AC-5: run only when the parent test sets `CHILD_ENV`. Writes 512 KiB to
/// a writer over `R/out.stl`, signals, then waits to be killed.
#[test]
fn ac5_child_writer_helper() {
    let dir = match std::env::var_os(CHILD_ENV) {
        Some(d) => PathBuf::from(d),
        None => return,
    };
    let mut s = RootSet::new();
    s.insert(RootId::new("exp").unwrap(), RootAccess::Export, dir.join("R"));
    let mut w = AtomicWriter::create(target(&s, "out.stl"), WriteOptions::overwrite(true)).unwrap();
    w.write_all(&mib()[..HALF]).unwrap();
    w.flush().unwrap();
    fs::write(dir.join("ready"), b"ready").unwrap();
    for _ in 0..1200 {
        std::thread::sleep(Duration::from_millis(100));
    }
    // Not reached when the parent kills this process; exit without running destructors.
    std::process::exit(3);
}

#[test]
fn ac5_killed_writer_leaves_old_target_and_stale_temp_is_swept() {
    let t = Tmp::new("ac5");
    let root = t.root();
    let out = root.join("out.stl");
    fs::write(&out, OLD).unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["ac5_child_writer_helper", "--exact", "--nocapture", "--test-threads=1"])
        .env(CHILD_ENV, &t.raw)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let start = Instant::now();
    while !t.raw.join("ready").exists() {
        if let Some(st) = child.try_wait().unwrap() {
            panic!("child exited early: {st}");
        }
        if start.elapsed() > Duration::from_secs(120) {
            let _ = child.kill();
            panic!("child never got ready");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    child.kill().unwrap();
    child.wait().unwrap();

    // The target is the old file; the killed writer's temp file is left behind.
    assert_eq!(fs::read(&out).unwrap(), OLD);
    let left = temps(&root);
    assert_eq!(left.len(), 1, "{left:?}");
    assert!(is_tmp_v7(&left[0]), "{left:?}");
    assert_eq!(fs::read(root.join(&left[0])).unwrap(), &mib()[..HALF]);

    // Not yet stale: the next writer keeps it.
    AtomicWriter::create(target(&t.set(), "other.stl"), WriteOptions::default()).unwrap().cancel();
    assert_eq!(temps(&root), left);

    // Older than an hour: the next writer in that folder removes it. A fresh temp file and
    // an old file without the prefix stay.
    let over = STALE_AFTER + Duration::from_secs(60);
    age(&root.join(&left[0]), over);
    fs::write(root.join(".fid-tmp-fresh"), b"x").unwrap();
    fs::write(root.join("keep.bin"), b"x").unwrap();
    age(&root.join("keep.bin"), over);
    AtomicWriter::create(target(&t.set(), "other.stl"), WriteOptions::default()).unwrap().cancel();
    assert_eq!(temps(&root), [".fid-tmp-fresh"]);
    assert!(root.join("keep.bin").exists());
    assert_eq!(fs::read(&out).unwrap(), OLD);
}

#[test]
fn stale_sweep_is_per_folder_and_keeps_folders() {
    let t = Tmp::new("sweep");
    let root = t.root();
    fs::create_dir_all(root.join("a")).unwrap();
    fs::create_dir_all(root.join(".fid-tmp-folder")).unwrap();
    let over = STALE_AFTER + Duration::from_secs(60);
    fs::write(root.join(".fid-tmp-top"), b"x").unwrap();
    age(&root.join(".fid-tmp-top"), over);
    fs::write(root.join("a").join(".fid-tmp-sub"), b"x").unwrap();
    age(&root.join("a").join(".fid-tmp-sub"), over);
    // A writer in `a` sweeps `a` only.
    AtomicWriter::create(target(&t.set(), "a/x.stl"), WriteOptions::default()).unwrap().cancel();
    assert!(temps(&root.join("a")).is_empty());
    assert_eq!(temps(&root), [".fid-tmp-folder", ".fid-tmp-top"]);
    AtomicWriter::create(target(&t.set(), "x.stl"), WriteOptions::default()).unwrap().cancel();
    assert_eq!(temps(&root), [".fid-tmp-folder"]);
}

// ------------------------------------------------------------------ links, folders, read-only

#[test]
fn link_at_the_target_is_never_written_through() {
    let t = Tmp::new("link");
    let root = t.root();
    fs::write(root.join("real.stl"), OLD).unwrap();
    if !symlink_file(Path::new("real.stl"), &root.join("out.stl")) {
        return;
    }
    let s = t.set();
    // Through a handle (ROOTS resolves the in-root link) and through a human's raw path.
    let both = || [target(&s, "out.stl"), human(&s, &root.join("out.stl"))];
    for tg in both() {
        assert_eq!(AtomicWriter::create(tg, WriteOptions::default()).unwrap_err(), IoError::TargetExists);
    }
    for tg in both() {
        let e = AtomicWriter::create(tg, WriteOptions::overwrite(true)).unwrap_err();
        assert_eq!(io_kind(&e), io::ErrorKind::InvalidInput);
    }
    assert_eq!(fs::read(root.join("real.stl")).unwrap(), OLD);
    assert!(fs::symlink_metadata(root.join("out.stl")).unwrap().file_type().is_symlink());
    assert!(temps(&root).is_empty());
}

#[test]
fn dangling_link_at_the_target_is_not_followed() {
    let t = Tmp::new("dangle");
    let root = t.root();
    if !symlink_file(Path::new("nowhere.stl"), &root.join("out.stl")) {
        return;
    }
    let s = t.set();
    assert!(target(&s, "out.stl").is_link());
    assert_eq!(write_all(target(&s, "out.stl"), WriteOptions::default(), b"abc").unwrap_err(), IoError::TargetExists);
    assert_eq!(
        write_all(human(&s, &root.join("out.stl")), WriteOptions::default(), b"abc").unwrap_err(),
        IoError::TargetExists
    );
    let e = write_all(target(&s, "out.stl"), WriteOptions::overwrite(true), b"abc").unwrap_err();
    assert_eq!(io_kind(&e), io::ErrorKind::InvalidInput);
    assert!(!root.join("nowhere.stl").exists());
    assert!(temps(&root).is_empty());
}

#[test]
fn link_swapped_in_before_commit_is_refused() {
    let t = Tmp::new("swap");
    let root = t.root();
    let out = root.join("out.stl");
    fs::write(root.join("real.stl"), OLD).unwrap();
    fs::write(&out, OLD).unwrap();
    let s = t.set();
    let mut w = AtomicWriter::create(target(&s, "out.stl"), WriteOptions::overwrite(true)).unwrap();
    w.write_bytes(b"abc").unwrap();
    fs::remove_file(&out).unwrap();
    if !symlink_file(Path::new("real.stl"), &out) {
        return;
    }
    let e = w.commit().unwrap_err();
    assert_eq!(io_kind(&e), io::ErrorKind::InvalidInput);
    assert_eq!(fs::read(root.join("real.stl")).unwrap(), OLD);
    assert!(fs::symlink_metadata(&out).unwrap().file_type().is_symlink());
    assert!(temps(&root).is_empty());
}

#[test]
fn folder_at_the_target_is_refused() {
    let t = Tmp::new("folder");
    fs::create_dir_all(t.root().join("out.stl")).unwrap();
    let s = t.set();
    assert_eq!(write_all(target(&s, "out.stl"), WriteOptions::default(), b"abc").unwrap_err(), IoError::TargetExists);
    let e = write_all(target(&s, "out.stl"), WriteOptions::overwrite(true), b"abc").unwrap_err();
    assert_eq!(io_kind(&e), io::ErrorKind::InvalidInput);
    assert!(t.root().join("out.stl").is_dir());
    assert!(temps(&t.root()).is_empty());
}

/// Creates a Windows junction (no privilege needed).
#[cfg(windows)]
fn junction(target: &Path, link: &Path) {
    let st = Command::new("cmd").arg("/C").arg("mklink").arg("/J").arg(link).arg(target).status().unwrap();
    assert!(st.success(), "mklink /J {} {}", link.display(), target.display());
}

#[cfg(windows)]
#[test]
fn windows_junction_at_the_target_is_refused() {
    let t = Tmp::new("junction");
    let root = t.root();
    fs::create_dir_all(root.join("sub")).unwrap();
    let canon = fs::canonicalize(root.join("sub")).unwrap();
    let plain = PathBuf::from(canon.to_string_lossy().trim_start_matches(r"\\?\"));
    junction(&plain, &root.join("j"));
    let s = t.set();
    assert!(target(&s, "j").is_link());
    for tg in [target(&s, "j"), human(&s, &root.join("j"))] {
        assert_eq!(AtomicWriter::create(tg, WriteOptions::default()).unwrap_err(), IoError::TargetExists);
    }
    for tg in [target(&s, "j"), human(&s, &root.join("j"))] {
        let e = AtomicWriter::create(tg, WriteOptions::overwrite(true)).unwrap_err();
        assert_eq!(io_kind(&e), io::ErrorKind::InvalidInput);
    }
    assert!(fs::symlink_metadata(root.join("j")).unwrap().file_type().is_symlink());
    assert!(root.join("sub").is_dir());
    assert!(temps(&root).is_empty());
}

#[test]
fn read_only_target_is_refused_on_every_platform() {
    let t = Tmp::new("ro");
    let out = t.root().join("out.stl");
    fs::write(&out, OLD).unwrap();
    set_readonly(&out, true);
    let s = t.set();
    let e = write_all(target(&s, "out.stl"), WriteOptions::overwrite(true), b"abc").unwrap_err();
    assert_eq!(io_kind(&e), io::ErrorKind::PermissionDenied);
    assert_eq!(fs::read(&out).unwrap(), OLD);
    assert!(temps(&t.root()).is_empty());
    set_readonly(&out, false);
    assert_receipt(&write_all(target(&s, "out.stl"), WriteOptions::overwrite(true), b"abc").unwrap(), "abc");
}

#[cfg(unix)]
#[test]
fn unix_read_only_folder_fails_before_writing() {
    use std::os::unix::fs::PermissionsExt;
    let t = Tmp::new("rodir");
    let root = t.root();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o555)).unwrap();
    // A privileged user writes anyway; then there is nothing to test.
    if fs::write(root.join("probe"), b"x").is_ok() {
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
        return;
    }
    let s = t.set();
    let e = write_all(target(&s, "out.stl"), WriteOptions::default(), b"abc").unwrap_err();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(io_kind(&e), io::ErrorKind::PermissionDenied);
    assert!(!root.join("out.stl").exists());
    assert!(temps(&root).is_empty());
}

/// A target held open without `FILE_SHARE_DELETE` cannot be replaced: the write fails
/// `io` (after the bounded retry), the target is unchanged, no temp file remains, and the
/// same write succeeds once the handle is closed.
#[cfg(windows)]
#[test]
fn windows_sharing_violation_fails_cleanly() {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_SHARE_READ: u32 = 1;
    let t = Tmp::new("share");
    let out = t.root().join("out.stl");
    fs::write(&out, OLD).unwrap();
    let s = t.set();
    let held = fs::OpenOptions::new().read(true).share_mode(FILE_SHARE_READ).open(&out).unwrap();
    let e = write_all(target(&s, "out.stl"), WriteOptions::overwrite(true), b"abc").unwrap_err();
    assert_eq!(e.code(), "io");
    assert!(temps(&t.root()).is_empty());
    drop(held);
    assert_eq!(fs::read(&out).unwrap(), OLD);
    assert_receipt(&write_all(target(&s, "out.stl"), WriteOptions::overwrite(true), b"abc").unwrap(), "abc");
    assert_eq!(fs::read(&out).unwrap(), b"abc");
}

/// A reader holding the target open with std's default share mode (which on Windows
/// includes `FILE_SHARE_DELETE`) does not block replacing it on POSIX; on Windows the
/// outcome depends on the file system's rename semantics, and is either a clean replace or
/// a clean `io` failure.
#[test]
fn reader_holding_the_target_open() {
    let t = Tmp::new("reader");
    let out = t.root().join("out.stl");
    fs::write(&out, OLD).unwrap();
    let s = t.set();
    let held = fs::File::open(&out).unwrap();
    let r = write_all(target(&s, "out.stl"), WriteOptions::overwrite(true), b"abc");
    drop(held);
    if cfg!(windows) {
        match r {
            Ok(rec) => {
                assert_receipt(&rec, "abc");
                assert_eq!(fs::read(&out).unwrap(), b"abc");
            }
            Err(e) => {
                assert_eq!(e.code(), "io");
                assert_eq!(fs::read(&out).unwrap(), OLD);
            }
        }
    } else {
        assert_receipt(&r.unwrap(), "abc");
        assert_eq!(fs::read(&out).unwrap(), b"abc");
    }
    assert!(temps(&t.root()).is_empty());
}
