//! Acceptance criteria of spec node CORE.BUS.POL.FS (AC-1 .. AC-4, REQ-001 .. REQ-006).
//!
//! AC-1 runs the hand-labelled `fixtures/core/bus/pol/fs/cases.csv` against a tree this file
//! builds in a temporary folder (links and junctions cannot be stored portably in git). Inputs
//! in the CSV use `%XX` for bytes that a CSV cannot hold readably (UTF-8, `\`, NUL) and
//! `{tree}` for the tree's absolute path.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::error::{BusError, Code};
use fid_bus::policy::fs::{
    check_raw_paths, fs_settings, path_like_params, resolve, FileHandle, Roots, MAX_REL_BYTES, ROOTS_KEY,
};
use fid_bus::principal::{
    Channel, ChannelKind, ConfirmMethod, HumanPresence, Identity, JournalAppend, OsUser, PresenceOutcome, Principal,
    PrincipalId, PrincipalKind, PrincipalRef, SessionId, SessionKey, SessionTable, UpgradeClock, UpgradeContext,
    UpgradeGrant, UpgradeRequest,
};
use fid_canon::{parse, to_canonical_bytes, CValue, Digest};
use fid_journal::{EntryDraft, EntryTime, Manifests};
use fid_registry::CommandDef;
use fid_store::settings::{encode_record, FileStore, ProjectValues, Scope, SettingsRegistry, Sources};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

// ------------------------------------------------------------------ principals

struct TestChannel {
    kind: ChannelKind,
    identity: Identity,
    session: SessionKey,
    grant: Option<UpgradeGrant>,
}

impl Channel for TestChannel {
    fn kind(&self) -> ChannelKind {
        self.kind
    }
    fn authenticated_identity(&self) -> Option<Identity> {
        Some(self.identity.clone())
    }
    fn session(&self) -> &SessionKey {
        &self.session
    }
    fn upgrade_grant(&self) -> Option<&UpgradeGrant> {
        self.grant.as_ref()
    }
}

fn channel(table: &mut SessionTable, kind: ChannelKind, subject: &str) -> TestChannel {
    let mut identity = Identity::new(PrincipalId::new(subject).unwrap());
    identity.os_user = Some(OsUser::new(subject).unwrap());
    if matches!(kind, ChannelKind::ServerSession | ChannelKind::Internal) {
        identity.acting_for =
            Some(PrincipalRef { kind: PrincipalKind::Human, id: PrincipalId::new("trigger").unwrap() });
    }
    let session = table.open_session(SessionId::new(&format!("{}-{subject}", kind.as_str())).unwrap()).unwrap();
    TestChannel { kind, identity, session, grant: None }
}

fn principal_as(table: &mut SessionTable, kind: PrincipalKind) -> Principal {
    let ch = match kind {
        PrincipalKind::Human => ChannelKind::Gui,
        PrincipalKind::Agent => ChannelKind::Mcp,
        PrincipalKind::Script => ChannelKind::Cli,
        PrincipalKind::Service => ChannelKind::Service,
        PrincipalKind::Device => ChannelKind::Device,
        PrincipalKind::System => ChannelKind::Internal,
    };
    let c = channel(table, ch, "alice");
    let p = table.derive_principal(&c).unwrap();
    assert_eq!(p.kind(), kind);
    p
}

struct Confirms(OsUser);

impl HumanPresence for Confirms {
    fn confirm(&self, _req: &UpgradeRequest, _deadline: Duration) -> PresenceOutcome {
        PresenceOutcome::Confirmed { os_user: self.0.clone(), method: ConfirmMethod::OsPresence }
    }
}

struct StepClock(u64);

impl UpgradeClock for StepClock {
    fn now(&mut self) -> EntryTime {
        self.0 += 1_000_000;
        EntryTime { utc: "2026-11-19T10:00:00Z".into(), mono_ns: self.0, sync: "ntp".into() }
    }
}

struct NoJournal;

impl JournalAppend for NoJournal {
    fn append(&mut self, _draft: &EntryDraft) -> Result<(), BusError> {
        Ok(())
    }
}

// ------------------------------------------------------------------ commands

fn def(id: &str, class: &str, fs_effect: &str, param: &str, exts: &[&str]) -> CommandDef {
    let exts: Vec<String> = exts.iter().map(|e| format!("\"{e}\"")).collect();
    let json = format!(
        r#"{{"id":"{id}","major":1,"title":"Test command","description":"File handle test command.",
"params_schema":{{"type":"object","properties":{{"{param}":{{"type":"string","path_like":true,"x-extensions":[{}]}},
"note":{{"type":"string"}}}}}},"result_schema":{{"type":"object"}},"side_effect":"{class}",
"mcp_projection":"exposed","determinism":"D0","idempotency":"none","fs_effect":"{fs_effect}","undo":"head_move",
"preconditions":[],"effects":["TestDone"],"algorithms":[],"exposure":["cli","mcp"],
"lifecycle":{{"state":"active","since":"2026-01-01"}},"web":"available","human_only":false}}"#,
        exts.join(",")
    );
    CommandDef::from_value(parse(json.as_bytes()).unwrap()).unwrap()
}

fn read_def() -> CommandDef {
    def("test.import_part", "R1", "read_root", "source", &["stl", "STEP", "gz"])
}

fn write_def() -> CommandDef {
    def("test.export_table", "W1", "write_root", "target", &["csv", "stl"])
}

fn none_def() -> CommandDef {
    def("test.inspect_name", "R0", "none", "file", &["stl"])
}

// ------------------------------------------------------------------ tree

static NEXT: AtomicU64 = AtomicU64::new(0);

/// A temporary tree; removed on drop. `path` is canonical (no 8.3 names, no `/var` alias), and
/// on Windows without the `\\?\` prefix so `{tree}/...` raw paths parse as normal paths.
struct Tree {
    path: PathBuf,
    dirlinks: bool,
    filelinks: bool,
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn plain(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy().to_string();
    match s.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC") => PathBuf::from(rest),
        _ => p,
    }
}

fn file(p: &Path, text: &str) {
    std::fs::write(p, text).unwrap();
}

/// A directory link: a symbolic link on Unix (`target` relative or absolute as given), a
/// junction on Windows (always to the absolute `abs`).
fn dir_link(link: &Path, target: &str, abs: &Path) -> bool {
    #[cfg(unix)]
    {
        let _ = abs;
        std::os::unix::fs::symlink(target, link).is_ok()
    }
    #[cfg(windows)]
    {
        let _ = target;
        std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(abs)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

fn file_link(link: &Path, target: &str) -> bool {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).is_ok()
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_file(target, link).is_ok()
    }
}

impl Tree {
    fn new() -> Tree {
        let name = format!("fid-bus-fs-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed));
        let raw = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&raw);
        std::fs::create_dir_all(&raw).unwrap();
        let path = plain(std::fs::canonicalize(&raw).unwrap());
        let mut t = Tree { path, dirlinks: true, filelinks: true };
        t.build();
        t
    }

    fn p(&self, rel: &str) -> PathBuf {
        rel.split('/').fold(self.path.clone(), |p, s| p.join(s))
    }

    fn build(&mut self) {
        for d in ["outside", "imports/sub", "imports/dir.stl", "exports/sub", "shop"] {
            std::fs::create_dir_all(self.p(d)).unwrap();
        }
        file(&self.p("outside/secret.stl"), "outside stl");
        file(&self.p("outside/secret.txt"), "outside txt");
        for f in ["part.stl", "PART2.STL", "sub/deep.stl", "notes.txt", "archive.tar.gz", "part.stl.exe"] {
            file(&self.p(&format!("imports/{f}")), f);
        }
        for f in ["part.exe.stl", "noext", ".stl", "caf\u{e9}.stl"] {
            file(&self.p(&format!("imports/{f}")), f);
        }
        file(&self.p("exports/existing.csv"), "original");
        file(&self.p("shop/fixture.step"), "step");
        file(&self.p("shop/fixture.stl"), "stl");

        let mut d = true;
        d &= dir_link(&self.p("imports/link_out"), "../outside", &self.p("outside"));
        d &= dir_link(&self.p("imports/link_in"), "sub", &self.p("imports/sub"));
        d &= dir_link(&self.p("imports/link_up"), "..", &self.path);
        d &= dir_link(&self.p("exports/link_out"), "../outside", &self.p("outside"));
        d &= dir_link(&self.p("exports/link_sub"), "sub", &self.p("exports/sub"));
        self.dirlinks = d;

        let abs_out = self.p("outside/secret.stl").to_string_lossy().to_string();
        let mut f = true;
        f &= file_link(&self.p("imports/file_out.stl"), &format!("..{SEP}outside{SEP}secret.stl"));
        f &= file_link(&self.p("imports/file_in.stl"), "part.stl");
        f &= file_link(&self.p("imports/file_ext.stl"), "notes.txt");
        f &= file_link(&self.p("imports/loop_a.stl"), "loop_b.stl");
        f &= file_link(&self.p("imports/loop_b.stl"), "loop_a.stl");
        f &= file_link(&self.p("imports/file_abs_out.stl"), &abs_out);
        f &= file_link(&self.p("exports/dangling.csv"), "missing_target.csv");
        f &= file_link(&self.p("exports/link_existing.csv"), "existing.csv");
        self.filelinks = f;
        // Unix CI must exercise every link case.
        if cfg!(unix) {
            assert!(self.dirlinks && self.filelinks, "could not create links on Unix");
        }
        #[cfg(windows)]
        {
            assert!(self.dirlinks, "could not create junctions on Windows");
            // In CI the symlink rows must run, never skip silently.
            if std::env::var_os("CI").is_some() {
                assert!(self.filelinks, "CI: could not create file symlinks on Windows");
            }
        }
    }

    fn roots_value(&self) -> CValue {
        let root = |id: &str, kind: &str, dir: &str, exts: Option<&[&str]>| {
            let mut o = BTreeMap::new();
            o.insert("id".to_string(), CValue::Str(id.into()));
            o.insert("kind".to_string(), CValue::Str(kind.into()));
            o.insert("path".to_string(), CValue::Str(self.p(dir).to_string_lossy().to_string()));
            if let Some(e) = exts {
                o.insert("extensions".to_string(), CValue::Arr(e.iter().map(|x| CValue::Str((*x).into())).collect()));
            }
            CValue::Obj(o)
        };
        CValue::Arr(vec![
            root("imports", "import", "imports", None),
            root("exports", "export", "exports", None),
            root("shop", "import", "shop", Some(&["STEP"])),
        ])
    }

    fn roots(&self) -> Roots {
        Roots::from_value(&self.roots_value()).unwrap()
    }
}

#[cfg(windows)]
const SEP: char = '\\';
#[cfg(not(windows))]
const SEP: char = '/';

// ------------------------------------------------------------------ AC-1

fn decode(field: &str, tree: &Path) -> String {
    let b = field.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = std::str::from_utf8(&b[i + 1..i + 3]).unwrap();
            out.push(u8::from_str_radix(hex, 16).unwrap());
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap().replace("{tree}", &tree.to_string_lossy())
}

fn outcome(r: &Result<fid_bus::policy::fs::ResolvedPath, BusError>) -> String {
    match r {
        Ok(_) => "ok".to_string(),
        Err(e) => format!("{}/{}", e.code(), e.reason()),
    }
}

/// AC-1: every handle of `cases.csv` resolves to its hand-labelled outcome on this platform;
/// a resolved read also opens and yields the file's bytes.
#[test]
fn ac1_cases_match_fixture() {
    let tree = Tree::new();
    let roots = tree.roots();
    let mut table = SessionTable::new();
    let text = std::fs::read_to_string(repo().join("fixtures/core/bus/pol/fs/cases.csv")).unwrap();
    let mut lines = text.lines();
    assert_eq!(lines.next(), Some("id,principal,effect,input,expected,needs"));
    let (mut run, mut skipped, mut ids) = (0, 0, std::collections::BTreeSet::new());
    for line in lines.filter(|l| !l.is_empty()) {
        let f: Vec<&str> = line.split(',').collect();
        assert_eq!(f.len(), 6, "{line}");
        assert!(ids.insert(f[0].to_string()), "duplicate id {}", f[0]);
        let needs_ok = match f[5] {
            "-" => true,
            "dirlink" => tree.dirlinks,
            "filelink" => tree.filelinks,
            other => panic!("needs {other:?}"),
        };
        if !needs_ok {
            eprintln!("skipped {} (link kind not creatable here)", f[0]);
            skipped += 1;
            continue;
        }
        let kind = match f[1] {
            "agent" => PrincipalKind::Agent,
            "script" => PrincipalKind::Script,
            "human" => PrincipalKind::Human,
            other => panic!("principal {other:?}"),
        };
        let (def, param) = match f[2] {
            "read" => (read_def(), "source"),
            "write" => (write_def(), "target"),
            "none" => (none_def(), "file"),
            other => panic!("effect {other:?}"),
        };
        let principal = principal_as(&mut table, kind);
        let input = decode(f[3], &tree.path);
        let r = resolve(&principal, &def, param, &input, &roots);
        assert_eq!(outcome(&r), f[4], "case {} input {input:?}", f[0]);
        if let (Ok(p), "read") = (&r, f[2]) {
            let mut s = String::new();
            p.open_read().unwrap().read_to_string(&mut s).unwrap();
            assert!(!s.is_empty(), "case {}", f[0]);
        }
        run += 1;
    }
    assert!(run + skipped >= 40);
    assert!(run >= 100 || cfg!(windows), "ran {run}");
    // Nothing was written by resolution.
    assert_eq!(std::fs::read_to_string(tree.p("exports/existing.csv")).unwrap(), "original");
    assert!(!tree.p("exports/new.csv").exists());
}

// ------------------------------------------------------------------ AC-2, REQ-001

fn assert_reason<T: std::fmt::Debug>(r: Result<T, BusError>, code: Code, reason: &str) {
    let e = r.unwrap_err();
    assert_eq!((e.code(), e.reason()), (code, reason), "{e}");
}

/// AC-2: an MCP agent and a CLI session that is not upgraded get `raw_path_refused` for
/// `C:\data\part.stl` and `/home/u/part.stl`; after the CLI session is upgraded to human, a raw
/// path is accepted.
#[test]
fn ac2_raw_paths_by_principal() {
    let tree = Tree::new();
    let roots = tree.roots();
    let def = read_def();
    let mut table = SessionTable::new();
    let agent = principal_as(&mut table, PrincipalKind::Agent);
    let mut cli = channel(&mut table, ChannelKind::Cli, "bob");
    let script = table.derive_principal(&cli).unwrap();
    assert_eq!(script.kind(), PrincipalKind::Script);
    let existing = tree.p("imports/part.stl").to_string_lossy().to_string();
    for p in [&agent, &script] {
        for raw in [r"C:\data\part.stl", "/home/u/part.stl", existing.as_str()] {
            assert_reason(resolve(p, &def, "source", raw, &roots), Code::Policy, "raw_path_refused");
            let mut params = BTreeMap::new();
            params.insert("source".to_string(), CValue::Str(raw.to_string()));
            assert_reason(check_raw_paths(p, &def, &CValue::Obj(params)), Code::Policy, "raw_path_refused");
        }
    }
    // Every non-human kind is refused (fail closed beyond agent and script).
    for kind in [PrincipalKind::Service, PrincipalKind::Device, PrincipalKind::System] {
        let p = principal_as(&mut table, kind);
        assert_reason(resolve(&p, &def, "source", &existing, &roots), Code::Policy, "raw_path_refused");
    }

    let pending = table.prepare_upgrade(&cli).unwrap();
    let confirmed = pending.confirm(&Confirms(OsUser::new("bob").unwrap()), &mut StepClock(1_000)).unwrap();
    let ctx = UpgradeContext {
        stream: "record".into(),
        branch: "main".into(),
        manifests: Manifests { registry: Digest([0x11; 32]), kernel: Digest([0x22; 32]) },
    };
    cli.grant = Some(table.commit_upgrade(confirmed, &mut NoJournal, &ctx).unwrap());
    let human = table.derive_principal(&cli).unwrap();
    assert_eq!(human.kind(), PrincipalKind::Human);
    let mut s = String::new();
    resolve(&human, &def, "source", &existing, &roots).unwrap().open_read().unwrap().read_to_string(&mut s).unwrap();
    assert_eq!(s, "part.stl");
    for raw in [r"C:\data\part.stl", "/home/u/part.stl"] {
        let e = resolve(&human, &def, "source", raw, &roots).unwrap_err();
        assert_ne!(e.reason(), "raw_path_refused", "{raw}");
        let mut params = BTreeMap::new();
        params.insert("source".to_string(), CValue::Str(raw.to_string()));
        check_raw_paths(&human, &def, &CValue::Obj(params)).unwrap();
    }
    // A GUI human too.
    let gui = principal_as(&mut table, PrincipalKind::Human);
    resolve(&gui, &def, "source", &existing, &roots).unwrap();
}

/// REQ-001: the whole-call check looks at `path_like` parameters only, and refuses a
/// non-string value.
#[test]
fn req001_check_raw_paths_scans_path_like_params() {
    let def = read_def();
    assert_eq!(path_like_params(&def), vec!["source".to_string()]);
    let mut table = SessionTable::new();
    let agent = principal_as(&mut table, PrincipalKind::Agent);
    let mut params = BTreeMap::new();
    params.insert("note".to_string(), CValue::Str("/etc/passwd".into()));
    params.insert("source".to_string(), CValue::Str("root:imports/part.stl".into()));
    check_raw_paths(&agent, &def, &CValue::Obj(params.clone())).unwrap();
    params.insert("source".to_string(), CValue::Arr(vec![CValue::Str("/etc/passwd".into())]));
    assert_reason(check_raw_paths(&agent, &def, &CValue::Obj(params)), Code::Policy, "bad_handle");
    // A parameter that is not path_like is a caller bug.
    let roots = Roots::default();
    assert_reason(resolve(&agent, &def, "note", "root:imports/a.stl", &roots), Code::Internal, "invariant_violated");
}

// ------------------------------------------------------------------ AC-3, REQ-005

/// AC-3: an export handle to an existing file under a `write_root` command gives
/// `would_overwrite` and nothing is written; a file that appears between resolution and the
/// create is never overwritten either (exclusive create).
#[test]
fn ac3_never_overwrites() {
    let tree = Tree::new();
    let roots = tree.roots();
    let def = write_def();
    let mut table = SessionTable::new();
    let agent = principal_as(&mut table, PrincipalKind::Agent);
    assert_reason(
        resolve(&agent, &def, "target", "root:exports/existing.csv", &roots),
        Code::Policy,
        "would_overwrite",
    );
    assert_eq!(std::fs::read_to_string(tree.p("exports/existing.csv")).unwrap(), "original");

    // Race: resolved while absent, created by someone else before the handler's create.
    let r = resolve(&agent, &def, "target", "root:exports/race.csv", &roots).unwrap();
    file(&tree.p("exports/race.csv"), "theirs");
    assert_reason(r.create_new(), Code::Policy, "would_overwrite");
    assert_eq!(std::fs::read_to_string(tree.p("exports/race.csv")).unwrap(), "theirs");

    // The normal path writes through the held root folder.
    let r = resolve(&agent, &def, "target", "root:exports/sub/out.csv", &roots).unwrap();
    assert_eq!(r.root().map(|r| r.as_str()), Some("exports"));
    r.create_new().unwrap().write_all(b"a,b\n").unwrap();
    assert_eq!(std::fs::read_to_string(tree.p("exports/sub/out.csv")).unwrap(), "a,b\n");
    // Read and write openers are not interchangeable.
    assert_reason(r.open_read(), Code::Internal, "invariant_violated");
}

/// REQ-005 on case-insensitive file systems (Windows, default macOS volumes): a target that
/// differs from an existing file only in case is an overwrite.
#[cfg(any(windows, target_os = "macos"))]
#[test]
fn req005_case_variant_is_an_overwrite() {
    let tree = Tree::new();
    let mut table = SessionTable::new();
    let agent = principal_as(&mut table, PrincipalKind::Agent);
    let r = resolve(&agent, &write_def(), "target", "root:exports/EXISTING.CSV", &tree.roots());
    assert_reason(r, Code::Policy, "would_overwrite");
}

// ------------------------------------------------------------------ AC-4, REQ-006

/// AC-4: `root:imports/../../etc/passwd` gives `outside_root`, before any file-system access
/// (the root does not even exist here).
#[test]
fn ac4_parent_segments_are_outside_root() {
    let mut table = SessionTable::new();
    let agent = principal_as(&mut table, PrincipalKind::Agent);
    let roots =
        Roots::from_value(&parse(br#"[{"id":"imports","kind":"import","path":"/nonexistent-fid-root"}]"#).unwrap());
    // On Windows `/nonexistent` is not absolute; use an empty set there, the outcome is the same.
    let roots = roots.unwrap_or_default();
    for h in ["root:imports/../../etc/passwd", "root:imports/../../etc/passwd.stl"] {
        assert_reason(resolve(&agent, &read_def(), "source", h, &roots), Code::Policy, "outside_root");
        assert_reason(FileHandle::parse(h), Code::Policy, "outside_root");
    }
}

/// REQ-006: the relative part may be 1024 bytes, not 1025; absolute parts, NUL, drive letters
/// and stream markers are `bad_handle`.
#[test]
fn req006_lexical_limits() {
    let seg = "a".repeat(200);
    let mut rel = String::new();
    while rel.len() + seg.len() + 1 < MAX_REL_BYTES - 10 {
        rel.push_str(&seg);
        rel.push('/');
    }
    let pad = MAX_REL_BYTES - rel.len() - 4;
    rel.push_str(&"b".repeat(pad));
    rel.push_str(".stl");
    assert_eq!(rel.len(), MAX_REL_BYTES);
    let h = FileHandle::parse(&format!("root:imports/{rel}")).unwrap();
    assert_eq!(h.rel.file_name().len(), pad + 4);
    assert_eq!(h.to_string(), format!("root:imports/{rel}"));
    let long = format!("root:imports/b{rel}");
    assert_reason(FileHandle::parse(&long), Code::Policy, "bad_handle");
    for bad in ["root:imports//x.stl", "root:imports/a\0.stl", "root:imports/C:/x.stl", "root:imports/a.stl:s"] {
        assert_reason(FileHandle::parse(bad), Code::Policy, "bad_handle");
    }
}

// ------------------------------------------------------------------ roots and settings

/// The roots setting: admin, machine scope only, valid against CORE.STO.SET; the resolver
/// reads it through the settings layer and refuses a malformed configuration as a whole.
#[test]
fn roots_come_from_the_admin_setting() {
    let registry = SettingsRegistry::build(&fs_settings()).unwrap();
    let decl = registry.get(ROOTS_KEY).unwrap();
    assert!(decl.admin());
    assert_eq!(decl.scopes().collect::<Vec<_>>(), vec![Scope::Machine]);

    let tree = Tree::new();
    let user = FileStore::new(Scope::User, tree.p("cfg/user/settings.json")).unwrap();
    let machine_path = tree.p("cfg/machine/settings.json");
    let machine = FileStore::new(Scope::Machine, machine_path.clone()).unwrap();
    let sources = Sources { project: None, user: &user, machine: &machine };
    // Default: no roots, so every handle is `bad_root`.
    let (roots, warnings) = Roots::from_settings(&sources).unwrap();
    assert!(roots.is_empty() && warnings.is_empty());

    let mut values = BTreeMap::new();
    values.insert(ROOTS_KEY.to_string(), tree.roots_value());
    let mut doc = BTreeMap::new();
    doc.insert("format".to_string(), CValue::Str("fid-settings/1".into()));
    doc.insert("values".to_string(), CValue::Obj(values));
    std::fs::create_dir_all(machine_path.parent().unwrap()).unwrap();
    std::fs::write(&machine_path, to_canonical_bytes(&CValue::Obj(doc)).unwrap()).unwrap();
    let (roots, warnings) = Roots::from_settings(&sources).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(roots.iter().map(|r| r.id().as_str()).collect::<Vec<_>>(), vec!["exports", "imports", "shop"]);
    let mut table = SessionTable::new();
    let agent = principal_as(&mut table, PrincipalKind::Agent);
    resolve(&agent, &read_def(), "source", "root:imports/part.stl", &roots).unwrap();
    assert_reason(
        resolve(&agent, &read_def(), "source", "root:imports/part.stl", &Roots::default()),
        Code::Policy,
        "bad_root",
    );
}

struct OneProjectRow(Vec<u8>);

impl ProjectValues for OneProjectRow {
    fn working_branch(&self) -> &str {
        "main"
    }
    fn record(&self, key: &str) -> Result<Option<Vec<u8>>, String> {
        Ok((key == ROOTS_KEY).then(|| self.0.clone()))
    }
}

fn settings_file(path: &Path, value: CValue) {
    let mut values = BTreeMap::new();
    values.insert(ROOTS_KEY.to_string(), value);
    let mut doc = BTreeMap::new();
    doc.insert("format".to_string(), CValue::Str("fid-settings/1".into()));
    doc.insert("values".to_string(), CValue::Obj(values));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, to_canonical_bytes(&CValue::Obj(doc)).unwrap()).unwrap();
}

/// Roots come only from the admin, machine-scope setting: a `bus.fs_roots` value stored at user
/// or project scope is ignored with a warning, and no root comes from it.
#[test]
fn roots_at_user_or_project_scope_are_ignored() {
    let tree = Tree::new();
    let user_path = tree.p("cfg/user/settings.json");
    settings_file(&user_path, tree.roots_value());
    let user = FileStore::new(Scope::User, user_path).unwrap();
    let machine = FileStore::new(Scope::Machine, tree.p("cfg/machine/settings.json")).unwrap();
    let project = OneProjectRow(encode_record(Some(&tree.roots_value())).unwrap());
    let sources = Sources { project: Some(&project), user: &user, machine: &machine };
    let (roots, warnings) = Roots::from_settings(&sources).unwrap();
    assert!(roots.is_empty(), "{roots:?}");
    let scopes: Vec<Scope> = warnings.iter().map(|w| w.scope).collect();
    assert!(scopes.contains(&Scope::User) && scopes.contains(&Scope::Project), "{warnings:?}");
}

#[test]
fn malformed_roots_are_refused_whole() {
    let abs = if cfg!(windows) { r"C:\\r" } else { "/r" };
    let ok = format!(r#"[{{"id":"a","kind":"import","path":"{abs}"}}]"#);
    assert_eq!(Roots::from_value(&parse(ok.as_bytes()).unwrap()).unwrap().len(), 1);
    for bad in [
        format!(r#"[{{"id":"a","kind":"import","path":"{abs}"}},{{"id":"a","kind":"export","path":"{abs}"}}]"#),
        format!(r#"[{{"id":"A","kind":"import","path":"{abs}"}}]"#),
        format!(r#"[{{"id":"a","kind":"both","path":"{abs}"}}]"#),
        r#"[{"id":"a","kind":"import","path":"relative/dir"}]"#.to_string(),
        format!(r#"[{{"id":"a","kind":"import","path":"{abs}","extra":1}}]"#),
        format!(r#"[{{"id":"a","kind":"import","path":"{abs}","extensions":[]}}]"#),
        format!(r#"[{{"id":"a","kind":"import","path":"{abs}","extensions":["stl","STL"]}}]"#),
        format!(r#"[{{"id":"a","kind":"import","path":"{abs}","extensions":[".stl"]}}]"#),
        r#"{"id":"a"}"#.to_string(),
    ] {
        assert!(Roots::from_value(&parse(bad.as_bytes()).unwrap()).is_err(), "{bad}");
    }
}

// ------------------------------------------------------------------ links at open (TOCTOU)

/// A folder checked at resolution and swapped for a link to outside before the open: the open
/// through the held root folder fails with `outside_root` and nothing outside is read or
/// written.
#[test]
fn swapped_folder_is_caught_at_open() {
    let tree = Tree::new();
    if !tree.dirlinks {
        return;
    }
    let roots = tree.roots();
    let mut table = SessionTable::new();
    let agent = principal_as(&mut table, PrincipalKind::Agent);

    std::fs::create_dir_all(tree.p("imports/swap")).unwrap();
    file(&tree.p("imports/swap/secret.stl"), "inside");
    std::fs::create_dir_all(tree.p("exports/swap")).unwrap();
    let read = resolve(&agent, &read_def(), "source", "root:imports/swap/secret.stl", &roots).unwrap();
    let write = resolve(&agent, &write_def(), "target", "root:exports/swap/new.stl", &roots).unwrap();

    std::fs::remove_dir_all(tree.p("imports/swap")).unwrap();
    assert!(dir_link(&tree.p("imports/swap"), "../outside", &tree.p("outside")));
    std::fs::remove_dir_all(tree.p("exports/swap")).unwrap();
    assert!(dir_link(&tree.p("exports/swap"), "../outside", &tree.p("outside")));

    assert_reason(read.open_read(), Code::Policy, "outside_root");
    assert_reason(write.create_new(), Code::Policy, "outside_root");
    assert!(!tree.p("outside/new.stl").exists());
    // And resolving again refuses at resolution.
    assert_reason(
        resolve(&agent, &read_def(), "source", "root:imports/swap/secret.stl", &roots),
        Code::Policy,
        "outside_root",
    );
}

/// A file checked at resolution and replaced by a link before the open is refused.
#[test]
fn swapped_file_is_caught_at_open() {
    let tree = Tree::new();
    if !tree.filelinks {
        return;
    }
    let mut table = SessionTable::new();
    let agent = principal_as(&mut table, PrincipalKind::Agent);
    file(&tree.p("imports/victim.stl"), "inside");
    let read = resolve(&agent, &read_def(), "source", "root:imports/victim.stl", &tree.roots()).unwrap();
    std::fs::remove_file(tree.p("imports/victim.stl")).unwrap();
    let target = tree.p("outside/secret.stl").to_string_lossy().to_string();
    assert!(file_link(&tree.p("imports/victim.stl"), &target));
    assert_reason(read.open_read(), Code::Policy, "outside_root");
}

/// A hard link inside the root to a file outside it shares that file's data; a file with more
/// than one link is refused at resolution and at open (Unix `st_nlink`, Windows
/// `nNumberOfLinks`).
#[test]
fn hard_links_are_refused() {
    let tree = Tree::new();
    let mut table = SessionTable::new();
    let agent = principal_as(&mut table, PrincipalKind::Agent);
    std::fs::hard_link(tree.p("outside/secret.stl"), tree.p("imports/hard.stl")).unwrap();
    let r = resolve(&agent, &read_def(), "source", "root:imports/hard.stl", &tree.roots());
    assert_reason(r, Code::Policy, "outside_root");
    // Linked after resolution: refused at open.
    file(&tree.p("imports/later.stl"), "inside");
    let r = resolve(&agent, &read_def(), "source", "root:imports/later.stl", &tree.roots()).unwrap();
    std::fs::hard_link(tree.p("imports/later.stl"), tree.p("outside/later.stl")).unwrap();
    assert_reason(r.open_read(), Code::Policy, "outside_root");
}

/// The final component swapped after resolution for a link to another file inside the root
/// with another extension: the open does not follow it, so the checked name stays
/// authoritative for the extension rule.
#[test]
fn final_component_swapped_for_inside_link_is_refused() {
    let tree = Tree::new();
    if !tree.filelinks {
        return;
    }
    let mut table = SessionTable::new();
    let agent = principal_as(&mut table, PrincipalKind::Agent);
    file(&tree.p("imports/swapext.stl"), "stl");
    let read = resolve(&agent, &read_def(), "source", "root:imports/swapext.stl", &tree.roots()).unwrap();
    std::fs::remove_file(tree.p("imports/swapext.stl")).unwrap();
    assert!(file_link(&tree.p("imports/swapext.stl"), "notes.txt"));
    assert_reason(read.open_read(), Code::Policy, "outside_root");
}

/// Nested `path_like` declarations cannot be checked, so the command is refused.
#[test]
fn nested_path_like_is_refused() {
    let json = r#"{"id":"test.nested","major":1,"title":"Test command","description":"Nested file parameter.",
"params_schema":{"type":"object","properties":{"opts":{"type":"object","properties":{"file":{"type":"string","path_like":true}}}}},
"result_schema":{"type":"object"},"side_effect":"R1","mcp_projection":"exposed","determinism":"D0","idempotency":"none",
"fs_effect":"read_root","undo":"head_move","preconditions":[],"effects":["TestDone"],"algorithms":[],"exposure":["cli"],
"lifecycle":{"state":"active","since":"2026-01-01"},"web":"available","human_only":false}"#;
    let def = CommandDef::from_value(parse(json.as_bytes()).unwrap()).unwrap();
    let mut table = SessionTable::new();
    let agent = principal_as(&mut table, PrincipalKind::Agent);
    let mut inner = BTreeMap::new();
    inner.insert("file".to_string(), CValue::Str("/etc/passwd".into()));
    let mut params = BTreeMap::new();
    params.insert("opts".to_string(), CValue::Obj(inner));
    assert_reason(check_raw_paths(&agent, &def, &CValue::Obj(params)), Code::Internal, "invariant_violated");
}

/// Unix: a FIFO is not a regular file; it is refused without blocking.
#[cfg(unix)]
#[test]
fn fifo_is_refused_on_unix() {
    let tree = Tree::new();
    let fifo = tree.p("imports/pipe.stl");
    let made = std::process::Command::new("mkfifo").arg(&fifo).status().map(|s| s.success()).unwrap_or(false);
    if !made {
        return;
    }
    let mut table = SessionTable::new();
    let agent = principal_as(&mut table, PrincipalKind::Agent);
    let r = resolve(&agent, &read_def(), "source", "root:imports/pipe.stl", &tree.roots());
    assert_reason(r, Code::Policy, "bad_handle");
}

/// Windows: a junction to outside is refused (also in AC-1); the long-path verbatim form of a
/// human's raw path is accepted for a human only.
#[cfg(windows)]
#[test]
fn windows_junction_and_verbatim_paths() {
    let tree = Tree::new();
    let roots = tree.roots();
    let mut table = SessionTable::new();
    let agent = principal_as(&mut table, PrincipalKind::Agent);
    let human = principal_as(&mut table, PrincipalKind::Human);
    assert_reason(
        resolve(&agent, &read_def(), "source", "root:imports/link_out/secret.stl", &roots),
        Code::Policy,
        "outside_root",
    );
    let verbatim = format!(r"\\?\{}", tree.p("imports").join("part.stl").display());
    assert_reason(resolve(&agent, &read_def(), "source", &verbatim, &roots), Code::Policy, "raw_path_refused");
    resolve(&human, &read_def(), "source", &verbatim, &roots).unwrap().open_read().unwrap();
}
