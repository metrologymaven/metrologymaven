//! Acceptance criteria of spec node CORE.STO.LAYOUT (AC-1 .. AC-7) and its requirements.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_store::layout::{
    format_text, host_name, is_network_fs_magic, is_network_fs_name, is_network_windows_path, is_store_name, is_uuid,
    lock_text, parse_format, parse_lock_info, parse_storage_writers, path_chars, user_cache_dir, windows_verbatim,
    CREATED_ENTRIES, MAX_PATH_CHARS,
};
use fid_store::{Area, HexName, LockInfo, OpenMode, ProjectLocation, StoreError, StoreWarning};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/core/sto/layout");
const WORKSPACE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const CHILD_ENV: &str = "FID_STORE_LAYOUT_CHILD_PROJECT";
const READY: &str = "FID_STORE_CHILD_READY";

/// A unique directory under the system temp dir, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        static N: AtomicU32 = AtomicU32::new(0);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos();
        let name = format!("fid-store-{tag}-{}-{nanos}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst));
        let p = std::path::absolute(std::env::temp_dir().join(name)).unwrap();
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn hex(s: &str) -> HexName {
    HexName::new(s).unwrap()
}

fn new_project(tag: &str) -> (TempDir, PathBuf) {
    let t = TempDir::new(tag);
    let root = t.0.join("p");
    let (_loc, lock) = ProjectLocation::create(&root).unwrap();
    drop(lock);
    (t, root)
}

// ---------------------------------------------------------------- child process holding a lock

/// Helper run in a child process by the lock tests: opens the project for writing, reports
/// ready, then holds the lock until stdin closes.
#[test]
#[ignore = "helper process for the lock tests"]
fn child_holder() {
    let Some(p) = std::env::var_os(CHILD_ENV) else { return };
    let (_loc, lock) = ProjectLocation::open(Path::new(&p), OpenMode::Write).unwrap();
    println!("{READY}");
    std::io::stdout().flush().unwrap();
    let mut s = String::new();
    let _ = std::io::stdin().read_line(&mut s);
    drop(lock);
}

struct Holder {
    child: Child,
    out: BufReader<ChildStdout>,
}

impl Holder {
    fn spawn(root: &Path) -> Holder {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["child_holder", "--exact", "--ignored", "--nocapture", "--test-threads=1"])
            .env(CHILD_ENV, root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut out = BufReader::new(child.stdout.take().unwrap());
        loop {
            let mut line = String::new();
            assert!(out.read_line(&mut line).unwrap() > 0, "child exited before taking the lock");
            if line.contains(READY) {
                break;
            }
        }
        Holder { child, out }
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Lets the child release the lock and exit normally.
    fn finish(mut self) {
        drop(self.child.stdin.take());
        let mut rest = String::new();
        let _ = std::io::Read::read_to_string(&mut self.out, &mut rest);
        assert!(self.child.wait().unwrap().success(), "child failed: {rest}");
    }

    /// Kills the child while it holds the lock and reaps it.
    ///
    /// The child's stdin stays open until the child has been reaped. `Child::wait` would
    /// otherwise close it first, and on macOS SIGKILL does not stop every thread of a
    /// multi-threaded process at once: the helper thread could see end-of-file and run its
    /// clean release (which clears the lock record) before the kill takes effect.
    fn kill(mut self) -> std::process::ExitStatus {
        let stdin = self.child.stdin.take();
        self.child.kill().unwrap();
        let status = self.child.wait().unwrap();
        drop(stdin);
        status
    }
}

// ---------------------------------------------------------------- AC-1

#[test]
fn ac1_create_makes_exactly_the_layout() {
    let t = TempDir::new("ac1");
    let root = t.0.join("p");
    let (loc, lock) = ProjectLocation::create(&root).unwrap();
    assert_eq!(loc.list().unwrap(), CREATED_ENTRIES.to_vec());
    let mut on_disk: Vec<String> =
        std::fs::read_dir(&root).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    on_disk.sort();
    assert_eq!(on_disk, CREATED_ENTRIES.to_vec());
    let format = std::fs::read_to_string(root.join("format")).unwrap();
    assert_eq!(format.lines().next(), Some("fid-project-layout@1"));
    assert!(is_uuid(loc.uuid()));
    assert_eq!(loc.layout_version(), 1);
    for d in ["blobs", "kernels", "tmp"] {
        assert!(root.join(d).is_dir());
    }
    assert_eq!(std::fs::metadata(root.join("project.db")).unwrap().len(), 0);
    assert!(lock.warnings().is_empty());
    assert_eq!(lock.info().pid, std::process::id());
    drop(lock);
    // A clean release clears the holder record.
    assert_eq!(std::fs::read(root.join("lock")).unwrap(), b"");
    // Reopen reads the same UUID.
    let (again, _) = ProjectLocation::open(&root, OpenMode::Read).unwrap();
    assert_eq!(again.uuid(), loc.uuid());
}

#[test]
fn create_needs_an_empty_folder_and_open_needs_a_project() {
    let t = TempDir::new("empty");
    std::fs::write(t.0.join("note.txt"), b"x").unwrap();
    assert!(matches!(ProjectLocation::create(&t.0), Err(StoreError::NotEmpty)));
    assert!(matches!(ProjectLocation::open(&t.0, OpenMode::Read), Err(StoreError::NotAProject(_))));
    let (_t2, root) = new_project("again");
    assert!(matches!(ProjectLocation::create(&root), Err(StoreError::NotEmpty)));
    // Missing area.
    std::fs::remove_dir(root.join("kernels")).unwrap();
    assert!(matches!(ProjectLocation::open(&root, OpenMode::Write), Err(StoreError::NotAProject(_))));
}

// ---------------------------------------------------------------- REQ-002 cache location

#[test]
fn req002_cache_dir_is_per_user_and_outside_the_project() {
    let (_t, root) = new_project("cache");
    let (loc, _) = ProjectLocation::open(&root, OpenMode::Read).unwrap();
    if let Some(c) = loc.cache_dir() {
        assert!(c.ends_with(Path::new("fiducial").join("cache").join(loc.uuid())));
        assert!(!c.starts_with(loc.root()));
    }
    fn env(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<std::ffi::OsString> {
        move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| std::ffi::OsString::from(*v))
    }
    assert_eq!(user_cache_dir("linux", env(&[("HOME", "/home/u")])), Some(Path::new("/home/u").join(".cache")));
    assert_eq!(
        user_cache_dir("linux", env(&[("HOME", "/home/u"), ("XDG_CACHE_HOME", "/xdg")])),
        Some(PathBuf::from("/xdg"))
    );
    assert_eq!(
        user_cache_dir("linux", env(&[("HOME", "/home/u"), ("XDG_CACHE_HOME", "rel")])),
        Some(Path::new("/home/u").join(".cache"))
    );
    assert_eq!(
        user_cache_dir("macos", env(&[("HOME", "/Users/u")])),
        Some(Path::new("/Users/u").join("Library").join("Caches"))
    );
    assert!(user_cache_dir("windows", env(&[("LOCALAPPDATA", r"C:\Users\u\AppData\Local")])).is_some());
    assert_eq!(user_cache_dir("linux", env(&[])), None);
}

// ---------------------------------------------------------------- AC-2 (REQ-003, REQ-004)

#[test]
fn ac2_second_process_gets_locked_with_holder_pid_and_host() {
    let (_t, root) = new_project("ac2");
    let holder = Holder::spawn(&root);
    match ProjectLocation::open(&root, OpenMode::Write) {
        Err(StoreError::Locked(Some(info))) => {
            assert_eq!(info.pid, holder.pid());
            assert_eq!(info.host, host_name());
            assert!(info.started > 1_600_000_000);
        }
        other => panic!("expected locked with holder, got {other:?}"),
    }
    // Read-only opens are not blocked.
    assert!(ProjectLocation::open(&root, OpenMode::Read).unwrap().1.is_none());
    holder.finish();
    // After a clean release the next writer gets the lock without warnings.
    let (_loc, lock) = ProjectLocation::open(&root, OpenMode::Write).unwrap();
    assert!(lock.unwrap().warnings().is_empty());
}

#[test]
fn ac2_second_writer_in_the_same_process_is_locked_too() {
    let (_t, root) = new_project("ac2b");
    let (_a, lock) = ProjectLocation::open(&root, OpenMode::Write).unwrap();
    let err = ProjectLocation::open(&root, OpenMode::Write).unwrap_err();
    assert_eq!(err.code(), "locked");
    match err {
        StoreError::Locked(Some(i)) => assert_eq!(i, *lock.unwrap().info()),
        other => panic!("{other:?}"),
    }
}

// ---------------------------------------------------------------- AC-3 (REQ-005)

fn fixture_lock(name: &str) -> String {
    std::fs::read_to_string(Path::new(FIXTURES).join("stale_lock").join(name)).unwrap()
}

#[test]
fn ac3_stale_lock_on_this_host_is_taken_with_one_warning() {
    let (_t, root) = new_project("ac3");
    let text = fixture_lock("lock").replace("@THIS_HOST@", &host_name());
    std::fs::write(root.join("lock"), text).unwrap();
    let (_loc, lock) = ProjectLocation::open(&root, OpenMode::Write).unwrap();
    let lock = lock.unwrap();
    assert_eq!(
        lock.warnings(),
        &[StoreWarning::StaleLockTaken(LockInfo { pid: 4_294_967_294, host: host_name(), started: 1_700_000_000 })]
    );
    assert_eq!(lock.warnings()[0].code(), "stale_lock_taken");
    // The record now names this process.
    let now = parse_lock_info(&std::fs::read(root.join("lock")).unwrap()).unwrap();
    assert_eq!(now.pid, std::process::id());
}

#[test]
fn ac3_lock_from_another_host_is_never_taken() {
    let (_t, root) = new_project("ac3b");
    let text = fixture_lock("lock_other_host");
    std::fs::write(root.join("lock"), &text).unwrap();
    match ProjectLocation::open(&root, OpenMode::Write) {
        Err(StoreError::Locked(Some(i))) => {
            assert_eq!(i.host, "other-host.invalid");
            assert_eq!(i.pid, 4_294_967_294);
        }
        other => panic!("expected locked, got {other:?}"),
    }
    assert_eq!(std::fs::read_to_string(root.join("lock")).unwrap(), text);
}

#[test]
fn ac3_garbled_lock_record_fails_closed() {
    let (_t, root) = new_project("ac3c");
    std::fs::write(root.join("lock"), b"fid-lock@1\npid=12\n").unwrap();
    let err = ProjectLocation::open(&root, OpenMode::Write).unwrap_err();
    assert_eq!(err.code(), "lock_file_invalid");
}

#[test]
fn ac3_killed_holder_lock_is_taken_over() {
    let (_t, root) = new_project("ac3d");
    let holder = Holder::spawn(&root);
    let pid = holder.pid();
    // The live holder's record names it.
    let held = parse_lock_info(&std::fs::read(root.join("lock")).unwrap()).unwrap();
    assert_eq!((held.pid, held.host.as_str()), (pid, host_name().as_str()));
    let status = holder.kill();
    // The holder died from the kill and did not release the lock itself.
    assert!(!status.success(), "holder exited normally: {status:?}");
    #[cfg(unix)]
    assert_eq!(std::os::unix::process::ExitStatusExt::signal(&status), Some(9), "{status:?}");
    assert_eq!(std::fs::read(root.join("lock")).unwrap(), lock_text(&held).into_bytes(), "record changed by the kill");
    // The holder is reaped, so the OS has released its lock; poll briefly with a deadline in
    // case a platform finishes closing the handle a little after the wait returns.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let result = loop {
        let r = ProjectLocation::open(&root, OpenMode::Write);
        if !matches!(r, Err(StoreError::Locked(_))) || std::time::Instant::now() >= deadline {
            break r;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let lock = result.unwrap().1.unwrap();
    match lock.warnings() {
        [StoreWarning::StaleLockTaken(prev)] => assert_eq!(*prev, held),
        other => panic!("expected one stale_lock_taken, got {other:?}"),
    }
    // The record now names this process.
    assert_eq!(parse_lock_info(&std::fs::read(root.join("lock")).unwrap()).unwrap().pid, std::process::id());
}

// ---------------------------------------------------------------- AC-4 (REQ-006)

/// An absolute path of exactly `total` characters below `base`.
fn path_of_len(base: &Path, total: usize) -> PathBuf {
    let mut s = base.to_str().unwrap().trim_end_matches(std::path::MAIN_SEPARATOR).to_string();
    let mut rem = total - s.chars().count();
    assert!(rem >= 2);
    while rem > 0 {
        let take = if rem > 101 {
            if rem - 101 >= 2 {
                100
            } else {
                98
            }
        } else {
            rem - 1
        };
        s.push(std::path::MAIN_SEPARATOR);
        s.push_str(&"d".repeat(take));
        rem -= take + 1;
    }
    PathBuf::from(s)
}

#[test]
fn ac4_project_at_a_1024_character_path() {
    let t = TempDir::new("ac4");
    let root = path_of_len(&t.0, MAX_PATH_CHARS);
    assert_eq!(root.to_str().unwrap().chars().count(), 1024);
    let digest = "ab".repeat(32);
    {
        let (loc, lock) = ProjectLocation::create(&root).unwrap();
        lock.write_file(Area::Blobs, &[hex("ab")], &hex(&digest), b"long path blob").unwrap();
        lock.write_file(Area::Tmp, &[], &hex("0123"), b"tmp").unwrap();
        assert_eq!(loc.read_file(Area::Blobs, &[hex("ab")], &hex(&digest), 1024).unwrap(), b"long path blob");
    }
    let (loc, lock) = ProjectLocation::open(&root, OpenMode::Write).unwrap();
    let lock = lock.unwrap();
    assert!(lock.warnings().is_empty());
    assert_eq!(loc.read_file(Area::Blobs, &[hex("ab")], &hex(&digest), 1024).unwrap(), b"long path blob");
    lock.write_file(Area::Kernels, &[], &hex("ff"), b"kernel").unwrap();
    let listing = loc.list().unwrap();
    assert!(listing.contains(&format!("blobs/ab/{digest}")));
    assert!(listing.contains(&"kernels/ff".to_string()));
    drop(lock);
    let (ro, none) = ProjectLocation::open(&root, OpenMode::Read).unwrap();
    assert!(none.is_none());
    assert_eq!(ro.read_file(Area::Tmp, &[], &hex("0123"), 16).unwrap(), b"tmp");
}

#[test]
fn ac4_paths_over_1024_characters_are_refused() {
    let t = TempDir::new("ac4b");
    let root = path_of_len(&t.0, MAX_PATH_CHARS + 1);
    let err = ProjectLocation::create(&root).unwrap_err();
    assert!(matches!(err, StoreError::PathTooLong { chars: 1025 }), "{err:?}");
    assert_eq!(err.code(), "path_too_long");
}

#[test]
fn windows_verbatim_and_char_counting() {
    assert_eq!(windows_verbatim(r"C:\a\b"), r"\\?\C:\a\b");
    assert_eq!(windows_verbatim(r"\\srv\share\x"), r"\\?\UNC\srv\share\x");
    assert_eq!(windows_verbatim(r"\\?\C:\a"), r"\\?\C:\a");
    assert_eq!(path_chars(r"\\?\C:\abc"), 6);
    assert_eq!(path_chars(r"\\?\UNC\srv\s"), 7);
    assert_eq!(path_chars("/é/x"), 4);
}

// ---------------------------------------------------------------- AC-5 (REQ-007)

#[test]
fn ac5_session_100_creates_only_safe_names() {
    let dir = Path::new(FIXTURES).join("session_100");
    let session = std::fs::read_to_string(dir.join("session.txt")).unwrap();
    let expected: Vec<String> =
        std::fs::read_to_string(dir.join("expected_listing.txt")).unwrap().lines().map(String::from).collect();
    let t = TempDir::new("ac5");
    let root = t.0.join("p");
    let (loc, lock) = ProjectLocation::create(&root).unwrap();
    let mut lock = Some(lock);
    let mut written = Vec::new();
    let mut puts = 0;
    for line in session.lines().filter(|l| !l.starts_with('#') && !l.is_empty()) {
        let f: Vec<&str> = line.split(' ').collect();
        match f.as_slice() {
            ["reopen"] => {
                drop(lock.take());
                let (_l, next) = ProjectLocation::open(&root, OpenMode::Write).unwrap();
                let next = next.unwrap();
                assert!(next.warnings().is_empty());
                lock = Some(next);
            }
            ["put", area, sub, name, content] => {
                let area = match *area {
                    "blobs" => Area::Blobs,
                    "kernels" => Area::Kernels,
                    "tmp" => Area::Tmp,
                    other => panic!("area {other}"),
                };
                let subs: Vec<HexName> = if *sub == "-" { vec![] } else { vec![hex(sub)] };
                let bytes: Vec<u8> = (0..content.len())
                    .step_by(2)
                    .map(|i| u8::from_str_radix(&content[i..i + 2], 16).unwrap())
                    .collect();
                lock.as_ref().unwrap().write_file(area, &subs, &hex(name), &bytes).unwrap();
                written.push((area, subs, hex(name), bytes));
                puts += 1;
            }
            other => panic!("bad session line {other:?}"),
        }
    }
    assert_eq!(puts, 100);
    drop(lock);
    let listing = loc.list().unwrap();
    assert_eq!(listing, expected);
    let mut folded = std::collections::BTreeSet::new();
    for path in &listing {
        for part in path.split('/') {
            assert!(
                !part.is_empty() && part.bytes().all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-'))
            );
            assert!(is_store_name(part), "{part}");
        }
        // All names are ASCII (checked above), so NFC is the identity; fold case.
        assert!(folded.insert(path.to_ascii_lowercase()), "{path} collides after case folding");
    }
    for (area, subs, name, bytes) in &written {
        assert_eq!(&loc.read_file(*area, subs, name, 1024).unwrap(), bytes);
    }
}

#[test]
fn req007_user_text_never_becomes_a_name() {
    for bad in ["", "a", "AB", "ab/cd", "..", "../ab", "ab.cd", "\u{e9}a", "Ab", "lock", "g0", "a".repeat(129).as_str()]
    {
        let err = HexName::new(bad).unwrap_err();
        assert_eq!(err.code(), "invalid_name", "{bad:?}");
    }
    assert!(HexName::new(&"0f".repeat(32)).is_ok());
    assert!(is_store_name("project.db-wal"));
    assert!(!is_store_name("FORMAT"));
    assert!(!is_store_name("Project.db"));
}

// ---------------------------------------------------------------- AC-6 (REQ-009)

#[test]
fn ac6_newer_layout_refuses_write_open() {
    let (_t, root) = new_project("ac6");
    std::fs::write(root.join("format"), b"fid-project-layout@99\n").unwrap();
    let err = ProjectLocation::open(&root, OpenMode::Write).unwrap_err();
    assert!(matches!(err, StoreError::LayoutTooNew { found: 99, supported: 1 }), "{err:?}");
    assert_eq!(err.code(), "layout_too_new");
    // The lock was not touched.
    assert_eq!(std::fs::read(root.join("lock")).unwrap(), b"");
}

#[test]
fn format_parser_is_strict() {
    let u = "0123abcd-0000-4000-8000-00000000abcd";
    assert_eq!(parse_format(format_text(1, u).as_bytes()).unwrap(), (1, u.to_string()));
    assert_eq!(parse_format(format!("fid-project-layout@1\r\nuuid={u}\r\n").as_bytes()).unwrap().0, 1);
    for bad in [
        String::new(),
        "fid-project-layout@0\nuuid=x\n".to_string(),
        "fid-project-layout@+1\n".to_string(),
        "fid-project-layout@1\n".to_string(),
        format!("fid-project-layout@1\nuuid={}\n", u.to_uppercase()),
        "fid-project-layout@1\nuuid=../../../etc/passwd-0000-0000-00000\n".to_string(),
        format!("fid-project-layout@1\nuuid={u}\nextra\n"),
        "x".repeat(5000),
    ] {
        assert_eq!(parse_format(bad.as_bytes()).unwrap_err().code(), "not_a_project", "{bad:?}");
    }
    assert_eq!(parse_format(b"fid-project-layout@4294967296\n").unwrap_err().code(), "not_a_project");
    assert_eq!(parse_format(b"fid-project-layout@2\nanything").unwrap_err().code(), "layout_too_new");
}

#[test]
fn lock_record_round_trip_and_hostile_input() {
    let info = LockInfo { pid: 42, host: "build-01.example".into(), started: 1_700_000_123 };
    assert_eq!(parse_lock_info(lock_text(&info).as_bytes()).unwrap(), info);
    for bad in [
        &b""[..],
        b"fid-lock@2\npid=1\nhost=h\nstarted=1\n",
        b"fid-lock@1\npid=-1\nhost=h\nstarted=1\n",
        b"fid-lock@1\npid=4294967296\nhost=h\nstarted=1\n",
        b"fid-lock@1\npid=1\npid=2\nhost=h\nstarted=1\n",
        b"fid-lock@1\npid=1\nhost=\nstarted=1\n",
        b"fid-lock@1\npid=1\nhost=h\x07\nstarted=1\n",
        b"fid-lock@1\npid=1\nhost=h\nstarted=1\nfoo=bar\n",
        b"fid-lock@1\npid=1\nhost=h\n",
        b"\xff\xfe",
    ] {
        assert_eq!(parse_lock_info(bad).unwrap_err().code(), "lock_file_invalid", "{bad:?}");
    }
}

// ---------------------------------------------------------------- REQ-008 network file systems

#[test]
fn req008_network_fs_classification() {
    assert!(is_network_fs_magic(0x6969));
    assert!(is_network_fs_magic(0xFF53_4D42));
    assert!(is_network_fs_magic(0xFFFF_FFFF_FF53_4D42));
    assert!(!is_network_fs_magic(0xEF53)); // ext4
    assert!(!is_network_fs_magic(0x0102_1994)); // tmpfs
    assert!(is_network_fs_name("smbfs"));
    assert!(is_network_fs_name("nfs"));
    assert!(!is_network_fs_name("apfs"));
    assert!(is_network_windows_path(r"\\?\UNC\srv\share\p"));
    assert!(is_network_windows_path(r"\\srv\share\p"));
    assert!(!is_network_windows_path(r"\\?\C:\Users\p"));
    // The CI temp directories are local: create and write open succeed (covered by AC-1/AC-4).
}

// ---------------------------------------------------------------- AC-7 (REQ-010)

#[test]
fn ac7_storage_writers_file_parses_and_names_workspace_crates() {
    let text = std::fs::read_to_string(Path::new(WORKSPACE).join("storage-writers.toml")).unwrap();
    let writers = parse_storage_writers(&text).unwrap();
    assert_eq!(writers, vec!["fid-store".to_string()]);
    let layers = std::fs::read_to_string(Path::new(WORKSPACE).join("layers.toml")).unwrap();
    for w in &writers {
        assert!(layers.contains(&format!("\"{w}\" = ")), "{w} is not a workspace crate in layers.toml");
        assert!(Path::new(WORKSPACE).join("crates").join(w).join("Cargo.toml").is_file());
    }
    for bad in [
        "",
        "[storage_writers]\n",
        "[storage_writers]\nallowed = [\"a\"]\nallowed = [\"b\"]\n",
        "[other]\nallowed = [\"a\"]\n",
        "[storage_writers]\nallowed = [a]\n",
        "[storage_writers]\nallowed = [\"a\",\n\"b\"]\n",
        "[storage_writers]\nallowed = [\"../x\"]\n",
    ] {
        assert!(parse_storage_writers(bad).is_err(), "{bad:?}");
    }
}

// ---------------------------------------------------------------- links inside a project

#[cfg(unix)]
#[test]
fn symlinked_area_is_refused() {
    let (t, root) = new_project("link");
    let outside = t.0.join("outside");
    std::fs::create_dir(&outside).unwrap();
    // A writer that already holds the lock must not follow a link swapped in later.
    let (_loc, lock) = ProjectLocation::open(&root, OpenMode::Write).unwrap();
    let lock = lock.unwrap();
    std::fs::remove_dir(root.join("blobs")).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("blobs")).unwrap();
    let err = lock.write_file(Area::Blobs, &[], &hex("ab"), b"x").unwrap_err();
    assert_eq!(err.code(), "symlink_refused", "{err:?}");
    drop(lock);
    let err = ProjectLocation::open(&root, OpenMode::Write).unwrap_err();
    assert_eq!(err.code(), "symlink_refused", "{err:?}");
    assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn symlinked_lock_file_is_refused() {
    let (t, root) = new_project("linklock");
    let target = t.0.join("elsewhere");
    std::fs::write(&target, b"").unwrap();
    std::fs::remove_file(root.join("lock")).unwrap();
    std::os::unix::fs::symlink(&target, root.join("lock")).unwrap();
    let err = ProjectLocation::open(&root, OpenMode::Write).unwrap_err();
    assert_eq!(err.code(), "symlink_refused", "{err:?}");
}

#[cfg(unix)]
#[test]
fn symlink_in_the_path_to_the_root_is_followed() {
    let (t, root) = new_project("linkroot");
    let alias = t.0.join("alias");
    std::os::unix::fs::symlink(&root, &alias).unwrap();
    let (_loc, lock) = ProjectLocation::open(&alias, OpenMode::Write).unwrap();
    assert!(lock.unwrap().warnings().is_empty());
}

// ---------------------------------------------------------------- manual recovery

#[test]
fn force_unlock_needs_the_exact_holder_and_a_free_os_lock() {
    let (_t, root) = new_project("force");
    let text = fixture_lock("lock_other_host");
    std::fs::write(root.join("lock"), &text).unwrap();
    let holder = match ProjectLocation::open(&root, OpenMode::Write) {
        Err(StoreError::Locked(Some(i))) => i,
        other => panic!("expected locked, got {other:?}"),
    };
    let wrong = LockInfo { pid: 7, ..holder.clone() };
    let err = ProjectLocation::force_unlock(&root, Some(&wrong)).unwrap_err();
    assert_eq!(err.code(), "lock_mismatch");
    assert_eq!(ProjectLocation::force_unlock(&root, None).unwrap_err().code(), "lock_mismatch");
    assert_eq!(std::fs::read_to_string(root.join("lock")).unwrap(), text);
    ProjectLocation::force_unlock(&root, Some(&holder)).unwrap();
    let (_loc, lock) = ProjectLocation::open(&root, OpenMode::Write).unwrap();
    let lock = lock.unwrap();
    assert!(lock.warnings().is_empty());
    // While a process holds the OS lock, force_unlock refuses.
    let mine = lock.info().clone();
    let err = ProjectLocation::force_unlock(&root, Some(&mine)).unwrap_err();
    assert_eq!(err.code(), "locked");
}

#[test]
fn force_unlock_clears_a_garbled_record_only_when_confirmed_as_garbled() {
    let (_t, root) = new_project("force2");
    std::fs::write(root.join("lock"), b"fid-lock@1\npid=12\n").unwrap();
    assert_eq!(ProjectLocation::open(&root, OpenMode::Write).unwrap_err().code(), "lock_file_invalid");
    ProjectLocation::force_unlock(&root, None).unwrap();
    assert!(ProjectLocation::open(&root, OpenMode::Write).unwrap().1.is_some());
}

#[test]
fn force_unlock_only_touches_projects() {
    let t = TempDir::new("force3");
    std::fs::write(t.0.join("lock"), fixture_lock("lock_other_host")).unwrap();
    let err = ProjectLocation::force_unlock(&t.0, None).unwrap_err();
    assert_eq!(err.code(), "not_a_project");
    assert_eq!(std::fs::read_to_string(t.0.join("lock")).unwrap(), fixture_lock("lock_other_host"));
}

// ---------------------------------------------------------------- lock file replaced while held

#[cfg(unix)]
#[test]
fn lock_file_renamed_over_while_held_stops_writes() {
    let (t, root) = new_project("lost");
    let (_loc, lock) = ProjectLocation::open(&root, OpenMode::Write).unwrap();
    let lock = lock.unwrap();
    lock.verify().unwrap();
    lock.write_file(Area::Tmp, &[], &hex("01"), b"before").unwrap();
    let fresh = t.0.join("fresh");
    std::fs::write(&fresh, b"").unwrap();
    std::fs::rename(&fresh, root.join("lock")).unwrap();
    assert_eq!(lock.verify().unwrap_err().code(), "lock_lost");
    let err = lock.write_file(Area::Tmp, &[], &hex("02"), b"after").unwrap_err();
    assert_eq!(err.code(), "lock_lost");
    // A second writer can now lock the new file; the first one has stopped writing.
    let (_b, second) = ProjectLocation::open(&root, OpenMode::Write).unwrap();
    assert!(second.is_some());
    assert!(!root.join("tmp").join("02").exists());
}

#[cfg(windows)]
#[test]
fn held_lock_file_cannot_be_replaced_or_deleted() {
    let (t, root) = new_project("lostw");
    let (_loc, lock) = ProjectLocation::open(&root, OpenMode::Write).unwrap();
    let lock = lock.unwrap();
    let fresh = t.0.join("fresh");
    std::fs::write(&fresh, b"").unwrap();
    assert!(std::fs::rename(&fresh, root.join("lock")).is_err());
    assert!(std::fs::remove_file(root.join("lock")).is_err());
    lock.verify().unwrap();
    lock.write_file(Area::Tmp, &[], &hex("01"), b"ok").unwrap();
}

// ---------------------------------------------------------------- special files

#[cfg(unix)]
fn mkfifo(p: &Path) {
    let ok = Command::new("mkfifo").arg(p).status().unwrap().success();
    assert!(ok, "mkfifo failed");
}

#[cfg(unix)]
#[test]
fn fifo_in_the_project_is_refused_without_hanging() {
    let (_t, root) = new_project("fifo");
    let (loc, lock) = ProjectLocation::open(&root, OpenMode::Write).unwrap();
    let lock = lock.unwrap();
    mkfifo(&root.join("tmp").join("abcd"));
    let err = loc.read_file(Area::Tmp, &[], &hex("abcd"), 16).unwrap_err();
    assert_eq!(err.code(), "not_regular_file", "{err:?}");
    let err = lock.write_file(Area::Tmp, &[], &hex("abcd"), b"x").unwrap_err();
    assert_eq!(err.code(), "not_regular_file", "{err:?}");
    drop(lock);
    std::fs::remove_file(root.join("lock")).unwrap();
    mkfifo(&root.join("lock"));
    let err = ProjectLocation::open(&root, OpenMode::Write).unwrap_err();
    assert_eq!(err.code(), "not_regular_file", "{err:?}");
}

// ---------------------------------------------------------------- Windows junctions

#[cfg(windows)]
fn junction(link: &Path, target: &Path) {
    let ok = Command::new("cmd")
        .arg("/c")
        .arg("mklink")
        .arg("/J")
        .arg(link)
        .arg(target)
        .stdout(Stdio::null())
        .status()
        .unwrap()
        .success();
    assert!(ok, "mklink /J failed");
}

#[cfg(windows)]
#[test]
fn junctioned_area_is_refused() {
    let (t, root) = new_project("junction");
    let outside = t.0.join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("ab"), b"outside").unwrap();
    let (loc, lock) = ProjectLocation::open(&root, OpenMode::Write).unwrap();
    let lock = lock.unwrap();
    std::fs::remove_dir(root.join("blobs")).unwrap();
    junction(&root.join("blobs"), &outside);
    let err = lock.write_file(Area::Blobs, &[], &hex("cd"), b"x").unwrap_err();
    assert_eq!(err.code(), "symlink_refused", "{err:?}");
    let err = loc.read_file(Area::Blobs, &[], &hex("ab"), 16).unwrap_err();
    assert_eq!(err.code(), "symlink_refused", "{err:?}");
    drop(lock);
    drop(loc);
    let err = ProjectLocation::open(&root, OpenMode::Write).unwrap_err();
    assert_eq!(err.code(), "symlink_refused", "{err:?}");
    let names: Vec<_> = std::fs::read_dir(&outside).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(names, vec![std::ffi::OsString::from("ab")]);
    std::fs::remove_dir(root.join("blobs")).unwrap();
}

#[cfg(windows)]
#[test]
fn junction_in_the_path_to_the_root_is_followed() {
    let (t, root) = new_project("junctionroot");
    let alias = t.0.join("alias");
    junction(&alias, &root);
    let (_loc, lock) = ProjectLocation::open(&alias, OpenMode::Write).unwrap();
    assert!(lock.unwrap().warnings().is_empty());
}

// TEMPORARY stress loop (removed before merge): same flow as AC-3 killed holder, 40x, while
// another test spawns and finishes holders concurrently.
#[test]
fn tmp_stress_killed_holder() {
    for _ in 0..40 {
        let (_t, root) = new_project("stress");
        let holder = Holder::spawn(&root);
        let pid = holder.pid();
        let status = holder.kill();
        assert!(!status.success());
        let (_loc, lock) = ProjectLocation::open(&root, OpenMode::Write).unwrap();
        let lock = lock.unwrap();
        assert!(matches!(lock.warnings(), [StoreWarning::StaleLockTaken(p)] if p.pid == pid), "{:?}", lock.warnings());
    }
}

#[test]
fn tmp_stress_finishing_holders() {
    for _ in 0..40 {
        let (_t, root) = new_project("stress2");
        Holder::spawn(&root).finish();
    }
}
