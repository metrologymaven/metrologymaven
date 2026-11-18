//! Acceptance criteria of spec node IO.REG.ROOTS (AC-1 .. AC-7) and the requirements
//! behind them. Every fixture is built per test in a fresh temporary folder; expected
//! outcomes are hand-labelled from the spec's rules. Symbolic-link tests run where the
//! platform lets the test create links; junction tests run on Windows.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_io_reg::{IoError, Principal, RelPath, RootAccess, RootHandle, RootId, RootSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

const PART: &[u8] = b"solid part\nendsolid part\n";

/// A temporary folder removed on drop. `raw` is the path as created, `canon` its
/// canonical form.
struct Tmp {
    raw: PathBuf,
    canon: PathBuf,
}

impl Tmp {
    fn new(name: &str) -> Tmp {
        let raw = std::env::temp_dir().join(format!("fid-io-roots-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&raw);
        fs::create_dir_all(&raw).unwrap();
        let canon = fs::canonicalize(&raw).unwrap();
        Tmp { raw, canon }
    }

    /// The root folder `R` with `R/a/part.stl`.
    fn with_part(name: &str) -> Tmp {
        let t = Tmp::new(name);
        fs::create_dir_all(t.root().join("a")).unwrap();
        fs::write(t.root().join("a").join("part.stl"), PART).unwrap();
        t
    }

    fn root(&self) -> PathBuf {
        self.raw.join("R")
    }

    /// The canonical root path in plain (non-verbatim) form, as a link target.
    fn root_plain(&self) -> PathBuf {
        plain(&self.canon.join("R"))
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.raw);
    }
}

/// Strips the Windows verbatim prefix `\\?\` from a drive path.
fn plain(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC") => PathBuf::from(rest),
        _ => p.to_path_buf(),
    }
}

fn set(t: &Tmp, id: &str, access: RootAccess) -> RootSet {
    let mut s = RootSet::new();
    s.insert(RootId::new(id).unwrap(), access, t.root());
    s
}

fn read_all(s: &RootSet, who: Principal, input: &str) -> Result<Vec<u8>, IoError> {
    let mut f = s.resolve_import(who, input)?;
    let mut buf = Vec::new();
    f.file.read_to_end(&mut buf).unwrap();
    Ok(buf)
}

fn code(r: Result<Vec<u8>, IoError>) -> &'static str {
    match r {
        Ok(_) => "ok",
        Err(e) => e.code(),
    }
}

/// Creates a directory symbolic link; false if the platform refuses (Windows without the
/// symlink privilege).
fn symlink_dir(target: &Path, link: &Path) -> bool {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).is_ok()
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_dir(target, link).is_ok()
    }
}

/// Creates a file symbolic link; false if the platform refuses.
fn symlink_file(target: &Path, link: &Path) -> bool {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).is_ok()
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_file(target, link).is_ok()
    }
}

/// Creates a Windows junction (no privilege needed).
#[cfg(windows)]
fn junction(target: &Path, link: &Path) {
    let st =
        std::process::Command::new("cmd").arg("/C").arg("mklink").arg("/J").arg(link).arg(target).status().unwrap();
    assert!(st.success(), "mklink /J {} {}", link.display(), target.display());
}

// ------------------------------------------------------------------ AC-1

#[test]
fn ac1_agent_resolves_handle_to_readable_file() {
    let t = Tmp::with_part("ac1");
    let s = set(&t, "imp", RootAccess::Import);
    for who in [Principal::Agent, Principal::Script, Principal::Human] {
        assert_eq!(read_all(&s, who, "root://imp/a/part.stl").unwrap(), PART);
    }
    // Backslash separates too; `.` components are dropped.
    assert_eq!(read_all(&s, Principal::Agent, r"root://imp/a\part.stl").unwrap(), PART);
    assert_eq!(read_all(&s, Principal::Agent, "root://imp/./a/./part.stl").unwrap(), PART);
    let f = s.resolve_import(Principal::Agent, "root://imp/a/part.stl").unwrap();
    assert_eq!(f.root.as_ref().map(RootId::as_str), Some("imp"));
    assert_eq!(f.path, Path::new("a").join("part.stl"));
    assert!(f.warnings.is_empty());
    // Missing files and folders are plain I/O errors, not escapes.
    assert_eq!(code(read_all(&s, Principal::Agent, "root://imp/a/none.stl")), "io");
    assert_eq!(code(read_all(&s, Principal::Agent, "root://imp/a")), "io");
    assert_eq!(code(read_all(&s, Principal::Agent, "root://imp")), "io");
}

// ------------------------------------------------------------------ AC-2

#[test]
fn ac2_agent_and_script_raw_paths_are_forbidden() {
    let t = Tmp::with_part("ac2");
    let s = set(&t, "imp", RootAccess::ImportExport);
    let raw_part = t.root().join("a").join("part.stl");
    let raws = [
        r"C:\data\part.stl".to_string(),
        "/data/part.stl".to_string(),
        "a/part.stl".to_string(),
        "ROOT://imp/a/part.stl".to_string(),
        raw_part.to_string_lossy().into_owned(),
    ];
    for who in [Principal::Agent, Principal::Script] {
        for raw in &raws {
            assert_eq!(s.resolve_import(who, raw).unwrap_err(), IoError::RawPathForbidden, "{raw}");
            assert_eq!(s.resolve_export(who, raw).unwrap_err(), IoError::RawPathForbidden, "{raw}");
        }
    }
    assert_eq!(s.fs_calls(), 0);
}

// ------------------------------------------------------------------ AC-3

#[test]
fn ac3_lexical_escapes_fail_before_any_file_system_call() {
    let t = Tmp::with_part("ac3");
    let s = set(&t, "imp", RootAccess::ImportExport);
    let bad = [
        "root://imp/../secret.txt",
        "root://imp/C:/x",
        "root://imp//server/share/x",
        r"root://imp/\\server\share\x",
        r"root://imp/\\?\C:\x",
        "root://imp/a/../../secret.txt",
        "root://imp/a/..",
        r"root://imp/a\..\..\x",
        "root://imp/a//part.stl",
        "root://imp/a/part.stl:stream",
        "root://imp/c:",
        "root://imp/a/NUL",
        "root://imp/con.txt",
        "root://imp/a/COM1",
        "root://imp/.. ",
        "root://imp/...",
        "root://imp/a/part.stl.",
        "root://imp/a\u{0}b",
        "root://imp/a/",
    ];
    for h in bad {
        assert_eq!(RootHandle::parse(h).unwrap_err(), IoError::PathOutsideRoot, "{h:?}");
        for who in [Principal::Agent, Principal::Script, Principal::Human] {
            assert_eq!(s.resolve_import(who, h).unwrap_err(), IoError::PathOutsideRoot, "{h:?}");
            assert_eq!(s.resolve_export(who, h).unwrap_err(), IoError::PathOutsideRoot, "{h:?}");
        }
    }
    // The resolver's own record: not one file-system call was made.
    assert_eq!(s.fs_calls(), 0);

    // An unconfigured root path proves the same: the lexical error wins over any lookup.
    let mut gone = RootSet::new();
    gone.insert(RootId::new("imp").unwrap(), RootAccess::Import, t.raw.join("does-not-exist"));
    assert_eq!(gone.resolve_import(Principal::Agent, "root://imp/../x").unwrap_err(), IoError::PathOutsideRoot);
    assert_eq!(gone.fs_calls(), 0);
    assert_eq!(gone.resolve_import(Principal::Agent, "root://imp/x").unwrap_err().code(), "io");
    assert!(gone.fs_calls() > 0);
}

#[test]
fn handle_parsing() {
    let h = RootHandle::parse(r"root://imp/a\b/./c.stl").unwrap();
    assert_eq!(h.root.as_str(), "imp");
    assert_eq!(h.rel.components(), ["a", "b", "c.stl"]);
    assert_eq!(h.to_string(), "root://imp/a/b/c.stl");
    assert!(RootHandle::parse("root://imp").unwrap().rel.is_empty());
    assert_eq!(RootHandle::parse("/x").unwrap_err(), IoError::RawPathForbidden);
    assert_eq!(RootHandle::parse("root://bad id/x").unwrap_err(), IoError::RootNotFound);
    assert_eq!(RootHandle::parse("root:///x").unwrap_err(), IoError::RootNotFound);
    assert_eq!(RelPath::parse("").unwrap().components().len(), 0);
    assert_eq!(RelPath::parse("con.d/x").unwrap_err(), IoError::PathOutsideRoot);
    assert_eq!(RelPath::parse("console/x").unwrap().components(), ["console", "x"]);
}

// ------------------------------------------------------------------ AC-4

#[cfg(unix)]
#[test]
fn ac4_posix_symlinks_leaving_the_root_fail() {
    let t = Tmp::with_part("ac4p");
    let r = t.root();
    fs::write(t.raw.join("secret.txt"), b"secret").unwrap();
    assert!(symlink_dir(Path::new("/etc"), &r.join("link")));
    assert!(symlink_dir(Path::new(".."), &r.join("up")));
    assert!(symlink_dir(Path::new("/"), &r.join("slash")));
    assert!(symlink_dir(Path::new("../.."), &r.join("a").join("esc")));
    assert!(symlink_file(&t.canon.join("secret.txt"), &r.join("abs-file")));
    assert!(symlink_file(Path::new("../secret.txt"), &r.join("rel-file")));
    // A link that points inside the root first and then climbs out.
    assert!(symlink_dir(Path::new("a/../.."), &r.join("climb")));
    let s = set(&t, "imp", RootAccess::ImportExport);
    for h in [
        "root://imp/link/hosts",
        "root://imp/up/secret.txt",
        "root://imp/slash/etc/hosts",
        "root://imp/a/esc/secret.txt",
        "root://imp/abs-file",
        "root://imp/rel-file",
        "root://imp/climb/secret.txt",
    ] {
        assert_eq!(code(read_all(&s, Principal::Agent, h)), "path_outside_root", "{h}");
        assert_eq!(s.resolve_export(Principal::Agent, h).unwrap_err(), IoError::PathOutsideRoot, "{h}");
    }
    // An export whose final name is an escaping link is refused too.
    assert_eq!(s.resolve_export(Principal::Agent, "root://imp/link").unwrap_err(), IoError::PathOutsideRoot);
}

#[cfg(windows)]
#[test]
fn ac4_windows_junctions_and_links_leaving_the_root_fail() {
    let t = Tmp::with_part("ac4w");
    let r = t.root();
    fs::write(t.raw.join("secret.txt"), b"secret").unwrap();
    let windir = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    junction(&windir, &r.join("link"));
    junction(&plain(&t.canon), &r.join("up"));
    let s = set(&t, "imp", RootAccess::ImportExport);
    let mut handles =
        vec!["root://imp/link/hosts", "root://imp/link/System32/drivers/etc/hosts", "root://imp/up/secret.txt"];
    // Symbolic links need a privilege the runner may lack; test them when it is held.
    if symlink_dir(Path::new(".."), &r.join("sym-up")) {
        handles.push("root://imp/sym-up/secret.txt");
    }
    if symlink_file(&plain(&t.canon.join("secret.txt")), &r.join("abs-file")) {
        handles.push("root://imp/abs-file");
    }
    for h in handles {
        assert_eq!(code(read_all(&s, Principal::Agent, h)), "path_outside_root", "{h}");
        assert_eq!(s.resolve_export(Principal::Agent, h).unwrap_err(), IoError::PathOutsideRoot, "{h}");
    }
}

// ------------------------------------------------------------------ AC-5

#[test]
fn ac5_links_inside_the_root_resolve() {
    let t = Tmp::with_part("ac5");
    let r = t.root();
    let s = set(&t, "imp", RootAccess::ImportExport);
    let mut handles = Vec::new();
    if symlink_dir(Path::new("a"), &r.join("b")) {
        handles.push("root://imp/b/part.stl");
    }
    if symlink_dir(&t.root_plain().join("a"), &r.join("b-abs")) {
        handles.push("root://imp/b-abs/part.stl");
    }
    if symlink_dir(Path::new("../a"), &r.join("a").join("self")) {
        handles.push("root://imp/a/self/self/part.stl");
    }
    if symlink_file(Path::new("a/part.stl"), &r.join("p.stl")) {
        handles.push("root://imp/p.stl");
    }
    #[cfg(windows)]
    {
        junction(&t.root_plain().join("a"), &r.join("j"));
        handles.push("root://imp/j/part.stl");
    }
    #[cfg(unix)]
    assert_eq!(handles.len(), 4, "POSIX symbolic links must be creatable");
    for h in &handles {
        let f = s.resolve_import(Principal::Agent, h).unwrap();
        assert_eq!(f.path, Path::new("a").join("part.stl"), "{h}");
        assert_eq!(read_all(&s, Principal::Agent, h).unwrap(), PART, "{h}");
        let e = s.resolve_export(Principal::Agent, h).unwrap();
        assert_eq!(e.path, Path::new("a").join("part.stl"), "{h}");
        assert_eq!(e.name, "part.stl");
    }
}

#[test]
fn link_loops_fail_with_io() {
    let t = Tmp::with_part("loop");
    let r = t.root();
    if symlink_dir(Path::new("l2"), &r.join("l1")) && symlink_dir(Path::new("l1"), &r.join("l2")) {
        let s = set(&t, "imp", RootAccess::Import);
        assert_eq!(code(read_all(&s, Principal::Agent, "root://imp/l1/x")), "io");
    }
}

// ------------------------------------------------------------------ AC-6

#[test]
fn ac6_wrong_direction_or_unknown_root_is_not_found() {
    let t = Tmp::with_part("ac6");
    let mut s = RootSet::new();
    s.insert(RootId::new("exp").unwrap(), RootAccess::Export, t.root());
    s.insert(RootId::new("imp").unwrap(), RootAccess::Import, t.root());
    assert_eq!(s.resolve_import(Principal::Agent, "root://exp/x.stl").unwrap_err(), IoError::RootNotFound);
    assert_eq!(s.resolve_export(Principal::Agent, "root://imp/x.stl").unwrap_err(), IoError::RootNotFound);
    assert_eq!(s.resolve_import(Principal::Agent, "root://nope/a/part.stl").unwrap_err(), IoError::RootNotFound);
    assert_eq!(s.fs_calls(), 0);
    let e = s.resolve_export(Principal::Agent, "root://exp/a/x.stl").unwrap();
    assert_eq!(e.name, "x.stl");
    assert_eq!(e.path, Path::new("a").join("x.stl"));
    assert_eq!(e.root.as_ref().map(RootId::as_str), Some("exp"));
    // The returned folder handle is the checked one: the file can be created through it.
    e.dir.write(&e.name, b"out").unwrap();
    assert_eq!(fs::read(t.root().join("a").join("x.stl")).unwrap(), b"out");
    assert_eq!(read_all(&s, Principal::Agent, "root://imp/a/x.stl").unwrap(), b"out");
    // The root itself is not an export target; a missing folder is an I/O error.
    assert_eq!(s.resolve_export(Principal::Agent, "root://exp").unwrap_err().code(), "io");
    assert_eq!(s.resolve_export(Principal::Agent, "root://exp/none/x.stl").unwrap_err().code(), "io");
    assert_eq!(s.resolve_export(Principal::Agent, "root://exp/top.stl").unwrap().name, "top.stl");
}

// ------------------------------------------------------------------ AC-7

#[test]
fn ac7_human_raw_paths_are_allowed() {
    let t = Tmp::with_part("ac7");
    let s = RootSet::new();
    let part = t.root().join("a").join("part.stl");
    let f = s.resolve_import(Principal::Human, &part.to_string_lossy()).unwrap();
    assert!(f.root.is_none());
    assert_eq!(f.path, part);
    assert_eq!(read_all(&s, Principal::Human, &part.to_string_lossy()).unwrap(), PART);
    let out = t.root().join("a").join("out.stl");
    let e = s.resolve_export(Principal::Human, &out.to_string_lossy()).unwrap();
    assert_eq!(e.name, "out.stl");
    assert!(e.root.is_none());
    e.dir.write(&e.name, b"x").unwrap();
    assert_eq!(fs::read(&out).unwrap(), b"x");
}

// ------------------------------------------------------------------ hard links (warn)

#[cfg(unix)]
#[test]
fn hard_links_are_reported_not_refused() {
    let t = Tmp::with_part("hard");
    fs::hard_link(t.root().join("a").join("part.stl"), t.raw.join("outside.stl")).unwrap();
    let s = set(&t, "imp", RootAccess::Import);
    let f = s.resolve_import(Principal::Agent, "root://imp/a/part.stl").unwrap();
    assert_eq!(f.warnings, [fid_io_reg::RootWarning::HardLinked { links: 2 }]);
    assert_eq!(f.warnings[0].code(), "hard_linked");
}
