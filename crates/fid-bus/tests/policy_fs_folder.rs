//! Spec node CORE.BUS.POL.FS, folder targets: REQ-007 to REQ-011 and AC-5 to AC-8.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::error::BusError;
use fid_bus::policy::fs::{resolve_folder, Roots};
use fid_bus::principal::{
    Channel, ChannelKind, Identity, OsUser, Principal, PrincipalId, PrincipalKind, PrincipalRef, SessionId, SessionKey,
    SessionTable, UpgradeGrant,
};
use fid_canon::{parse, CValue};
use fid_registry::CommandDef;
use fid_store::{CopyTarget, OpenMode, Store, VerifiedFolder};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

// ------------------------------------------------------------------ principals

struct TestChannel {
    kind: ChannelKind,
    identity: Identity,
    session: SessionKey,
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
        None
    }
}

fn principal(kind: PrincipalKind) -> Principal {
    let mut table = SessionTable::new();
    let ch = match kind {
        PrincipalKind::Human => ChannelKind::Gui,
        PrincipalKind::Agent => ChannelKind::Mcp,
        _ => ChannelKind::Cli,
    };
    let mut identity = Identity::new(PrincipalId::new("alice").unwrap());
    identity.os_user = Some(OsUser::new("alice").unwrap());
    if matches!(ch, ChannelKind::ServerSession | ChannelKind::Internal) {
        identity.acting_for = Some(PrincipalRef { kind: PrincipalKind::Human, id: PrincipalId::new("t").unwrap() });
    }
    let session = table.open_session(SessionId::new(&format!("{}-alice", ch.as_str())).unwrap()).unwrap();
    let c = TestChannel { kind: ch, identity, session };
    let p = table.derive_principal(&c).unwrap();
    assert_eq!(p.kind(), kind);
    p
}

// ------------------------------------------------------------------ commands and tree

fn def(id: &str, class: &str, fs_effect: &str) -> CommandDef {
    let json = format!(
        r#"{{"id":"{id}","major":1,"title":"Test command","description":"Folder handle test command.",
"params_schema":{{"type":"object","properties":{{"target":{{"type":"string","path_like":true}}}}}},
"result_schema":{{"type":"object"}},"side_effect":"{class}","mcp_projection":"exposed","determinism":"D0",
"idempotency":"none","fs_effect":"{fs_effect}","undo":"head_move","preconditions":[],"effects":["TestDone"],
"algorithms":[],"exposure":["cli","mcp"],"lifecycle":{{"state":"active","since":"2026-01-01"}},
"web":"available","human_only":false}}"#
    );
    CommandDef::from_value(parse(json.as_bytes()).unwrap()).unwrap()
}

fn write_def() -> CommandDef {
    def("test.create_project", "W2", "write_root")
}

fn open_def() -> CommandDef {
    def("test.open_project", "R0", "read_root")
}

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Tree(PathBuf);

impl Tree {
    fn new() -> Tree {
        let raw = std::env::temp_dir().join(format!(
            "fid-bus-folder-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&raw);
        std::fs::create_dir_all(&raw).unwrap();
        let p = std::fs::canonicalize(&raw).unwrap();
        #[cfg(windows)]
        let p = PathBuf::from(p.to_string_lossy().trim_start_matches(r"\\?\"));
        for d in ["work", "shared", "outside"] {
            std::fs::create_dir(p.join(d)).unwrap();
        }
        Tree(p)
    }

    fn p(&self, rel: &str) -> PathBuf {
        rel.split('/').fold(self.0.clone(), |p, s| p.join(s))
    }

    fn roots(&self) -> Roots {
        let root = |id: &str, kind: &str, dir: &str| {
            let mut o = BTreeMap::new();
            o.insert("id".to_string(), CValue::Str(id.into()));
            o.insert("kind".to_string(), CValue::Str(kind.into()));
            o.insert("path".to_string(), CValue::Str(self.p(dir).to_string_lossy().to_string()));
            CValue::Obj(o)
        };
        Roots::from_value(&CValue::Arr(vec![root("work", "export", "work"), root("shared", "import", "shared")]))
            .unwrap()
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A folder link (a junction on Windows); false if the OS refused.
fn dir_link(link: &Path, target: &Path) -> bool {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).is_ok()
    }
    #[cfg(windows)]
    {
        match std::process::Command::new("cmd").args(["/C", "mklink", "/J"]).arg(link).arg(target).output() {
            Ok(o) if o.status.success() => true,
            Ok(o) => {
                eprintln!(
                    "mklink /J {} {} failed: {} {}",
                    link.display(),
                    target.display(),
                    String::from_utf8_lossy(&o.stdout),
                    String::from_utf8_lossy(&o.stderr)
                );
                false
            }
            Err(e) => {
                eprintln!("mklink did not run: {e}");
                false
            }
        }
    }
}

fn reason(e: &BusError) -> String {
    format!("{}/{}", e.code(), e.reason())
}

fn project(at: &Path) {
    drop(Store::create(at).unwrap());
}

// ------------------------------------------------------------------ acceptance criteria

/// AC-5: create a folder in an export root; a second create, `..`, the copy prefix and a raw
/// path from an agent are refused.
#[test]
fn ac5_create_new_and_refusals() {
    let t = Tree::new();
    let roots = t.roots();
    let agent = principal(PrincipalKind::Agent);
    let created = resolve_folder(&agent, &write_def(), "target", "root:work/p1", &roots).unwrap().create_new().unwrap();
    assert!(t.p("work/p1").is_dir());
    assert_eq!(created.handle(), "root:work/p1");
    assert_eq!(created.folder_path(), std::fs::canonicalize(t.p("work/p1")).unwrap());
    let e = resolve_folder(&agent, &write_def(), "target", "root:work/p1", &roots).unwrap_err();
    assert_eq!(reason(&e), "policy/would_overwrite");
    let e = resolve_folder(&agent, &write_def(), "target", "root:work/../p1", &roots).unwrap_err();
    assert_eq!(reason(&e), "policy/outside_root");
    let e = resolve_folder(&agent, &write_def(), "target", "root:work/.fid-copy-0a1b", &roots).unwrap_err();
    assert_eq!(reason(&e), "policy/bad_handle");
    let e = resolve_folder(&agent, &write_def(), "target", "root:work/.FID-COPY-x", &roots).unwrap_err();
    assert_eq!(reason(&e), "policy/bad_handle");
    let raw = t.p("work/p2").to_string_lossy().to_string();
    let e = resolve_folder(&agent, &write_def(), "target", &raw, &roots).unwrap_err();
    assert_eq!(reason(&e), "policy/raw_path_refused");
    // A human may pass a raw path.
    let human = principal(PrincipalKind::Human);
    resolve_folder(&human, &write_def(), "target", &raw, &roots).unwrap().create_new().unwrap();
    assert!(t.p("work/p2").is_dir());
    // A folder parameter must not declare extensions; a command without a file effect is bad_root.
    let e = resolve_folder(&agent, &def("test.none", "R0", "none"), "target", "root:work/p3", &roots).unwrap_err();
    assert_eq!(reason(&e), "policy/bad_root");
}

/// AC-6: a link at the last component is `outside_root` wherever it points, also when it is
/// swapped in after resolution; nothing is created or opened.
#[test]
fn ac6_links_at_the_target() {
    let t = Tree::new();
    let roots = t.roots();
    let agent = principal(PrincipalKind::Agent);
    project(&t.p("work/real"));
    let made = dir_link(&t.p("work/in_link"), &t.p("work/real")) & dir_link(&t.p("work/out_link"), &t.p("outside"));
    if !made {
        assert!(cfg!(windows) && std::env::var_os("CI").is_none(), "could not create folder links");
        return;
    }
    for h in ["root:work/in_link", "root:work/out_link"] {
        let e = resolve_folder(&agent, &write_def(), "target", h, &roots).unwrap_err();
        assert_eq!(reason(&e), "policy/outside_root", "{h}");
        let e = resolve_folder(&agent, &open_def(), "target", h, &roots).unwrap_err();
        assert_eq!(reason(&e), "policy/outside_root", "{h}");
    }
    // Swapped in after resolution.
    let target = resolve_folder(&agent, &write_def(), "target", "root:work/late", &roots).unwrap();
    assert!(dir_link(&t.p("work/late"), &t.p("outside")));
    assert_eq!(reason(&target.create_new().unwrap_err()), "policy/outside_root");
    assert_eq!(std::fs::read_dir(t.p("outside")).unwrap().count(), 0);
    let opened = resolve_folder(&agent, &open_def(), "target", "root:work/real", &roots).unwrap();
    std::fs::rename(t.p("work/real"), t.p("work/real_moved")).unwrap();
    assert!(dir_link(&t.p("work/real"), &t.p("work/real_moved")));
    assert_eq!(reason(&opened.open_project().unwrap_err()), "policy/outside_root");
}

/// AC-7: `open_project` needs a valid `format` file; refusals never hold the absolute path.
#[test]
fn ac7_open_project_needs_a_format_file() {
    let t = Tree::new();
    let roots = t.roots();
    let agent = principal(PrincipalKind::Agent);
    project(&t.p("work/ok"));
    let pf = resolve_folder(&agent, &open_def(), "target", "root:work/ok", &roots).unwrap().open_project().unwrap();
    assert!(!pf.is_read_only() && !pf.read_only());
    assert!(!format!("{pf:?}").contains(&*t.0.to_string_lossy()));
    std::fs::create_dir(t.p("work/empty")).unwrap();
    std::fs::create_dir(t.p("work/garbled")).unwrap();
    std::fs::write(t.p("work/garbled/format"), b"not-a-layout\n").unwrap();
    std::fs::create_dir(t.p("work/linked")).unwrap();
    std::fs::write(t.p("outside/format"), b"fid-project-layout@1\n").unwrap();
    let linked = {
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(t.p("outside/format"), t.p("work/linked/format")).is_ok()
        }
        #[cfg(windows)]
        {
            std::os::windows::fs::symlink_file(t.p("outside/format"), t.p("work/linked/format")).is_ok()
        }
    };
    let mut cases = vec!["root:work/empty", "root:work/garbled"];
    if linked {
        cases.push("root:work/linked");
    }
    for h in cases {
        let e = resolve_folder(&agent, &open_def(), "target", h, &roots).unwrap().open_project().unwrap_err();
        assert_eq!(reason(&e), "precondition/false", "{h}");
        assert!(!e.to_string().contains(&*t.0.to_string_lossy()), "{h}: {e}");
    }
    let e = resolve_folder(&agent, &open_def(), "target", "root:work/missing", &roots).unwrap_err();
    assert_eq!(reason(&e), "precondition/false");
}

/// AC-8: a project in an import root opens read-only and nothing in it changes; a write
/// command on that root is `bad_root`.
#[test]
fn ac8_import_root_is_read_only() {
    let t = Tree::new();
    let roots = t.roots();
    let agent = principal(PrincipalKind::Agent);
    project(&t.p("shared/p"));
    let list = |p: &Path| {
        let mut v: Vec<(PathBuf, Vec<u8>, std::time::SystemTime)> = Vec::new();
        let mut stack = vec![p.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap() {
                let e = e.unwrap();
                let m = e.metadata().unwrap();
                let body = if m.is_dir() {
                    stack.push(e.path());
                    Vec::new()
                } else {
                    std::fs::read(e.path()).unwrap()
                };
                v.push((e.path(), body, m.modified().unwrap()));
            }
        }
        v.sort();
        v
    };
    let before = list(&t.p("shared/p"));
    let pf = resolve_folder(&agent, &open_def(), "target", "root:shared/p", &roots).unwrap().open_project().unwrap();
    assert!(pf.is_read_only());
    {
        let store = Store::open_at(&pf, OpenMode::Read).unwrap();
        assert_eq!(store.mode(), OpenMode::Read);
    }
    assert_eq!(Store::open_at(&pf, OpenMode::Write).unwrap_err().code(), "read_only_target");
    assert_eq!(list(&t.p("shared/p")), before);
    let e = resolve_folder(&agent, &write_def(), "target", "root:shared/q", &roots).unwrap_err();
    assert_eq!(reason(&e), "policy/bad_root");
}

/// Save-as targets: `into_copy_target` checks the parent and the name and creates nothing.
#[test]
fn copy_target_creates_nothing() {
    let t = Tree::new();
    let roots = t.roots();
    let agent = principal(PrincipalKind::Agent);
    std::fs::create_dir(t.p("work/sub")).unwrap();
    let c = resolve_folder(&agent, &write_def(), "target", "root:work/sub/p_copy", &roots)
        .unwrap()
        .into_copy_target()
        .unwrap();
    assert_eq!(c.target_name(), "p_copy");
    assert_eq!(c.parent_path(), std::fs::canonicalize(t.p("work/sub")).unwrap());
    assert!(!t.p("work/sub/p_copy").exists());
    let e = resolve_folder(&agent, &write_def(), "target", "root:work/nope/p_copy", &roots).unwrap_err();
    assert_eq!(reason(&e), "precondition/false");
    // The full save-as path: copy a project there.
    project(&t.p("work/src"));
    let src = Store::open(&t.p("work/src"), OpenMode::Write).unwrap();
    let copy = src.copy_to(&c, |_| Ok(()), |rename| rename().map_err(Into::into)).unwrap();
    assert_eq!(copy.location().root(), std::fs::canonicalize(t.p("work/sub/p_copy")).unwrap());
}
