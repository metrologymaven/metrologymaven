//! Acceptance criteria of spec node AUTO.SURF.ROOTS (AC-1 .. AC-6) and the proof that the
//! surface gate and the bus guard CORE.BUS.POL.FS give the same verdict on the bus's
//! hand-labelled corpus `fixtures/core/bus/pol/fs/cases.csv`.
//!
//! Principals are never written by hand: each test derives them from a channel through the
//! bus's `SessionTable`, once per channel kind (GUI, CLI, Python, control, MCP, server MCP,
//! server session, server service, service, device, internal), so every surface entry point
//! that exists is covered. Handle resolution is the bus's (`fid_bus::policy::fs::resolve`):
//! the tests run the surface gate first and hand only what it admits to the bus resolver, as
//! a surface does.
#![cfg(not(target_family = "wasm"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::error::BusError;
use fid_bus::policy::fs::{check_raw_paths, resolve, FileHandle, ResolvedPath, Roots};
use fid_bus::principal::{
    Channel, ChannelKind, ConfirmMethod, HumanPresence, Identity, JournalAppend, OsUser, PresenceOutcome, Principal,
    PrincipalId, PrincipalKind as BusKind, PrincipalRef, SessionId, SessionKey, SessionTable, UpgradeClock,
    UpgradeContext, UpgradeGrant, UpgradeRequest,
};
use fid_canon::{digest, parse, to_canonical_bytes, CValue, Digest};
use fid_journal::{EntryDraft, EntryTime, Manifests};
use fid_registry::CommandDef;
use fid_surface::roots::{
    admit, dispatch_gated, gate_raw_paths, gate_root_config, is_root_config, visible_commands, PrincipalKind,
    RootError, ERROR_MAP,
};
use fid_surface::{project, Projection, SurfaceProjection};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
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

static NEXT: AtomicU64 = AtomicU64::new(0);

fn channel(table: &mut SessionTable, kind: ChannelKind, subject: &str) -> TestChannel {
    let mut identity = Identity::new(PrincipalId::new(subject).unwrap());
    identity.os_user = Some(OsUser::new(subject).unwrap());
    if matches!(kind, ChannelKind::ServerSession | ChannelKind::Internal) {
        identity.acting_for = Some(PrincipalRef { kind: BusKind::Human, id: PrincipalId::new("trigger").unwrap() });
    }
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let session = table.open_session(SessionId::new(&format!("{}-{subject}-{n}", kind.as_str())).unwrap()).unwrap();
    TestChannel { kind, identity, session, grant: None }
}

/// One derived principal per channel kind: every surface entry point.
fn principals(table: &mut SessionTable) -> Vec<(ChannelKind, Principal)> {
    ChannelKind::ALL
        .iter()
        .map(|k| {
            let c = channel(table, *k, "alice");
            (*k, table.derive_principal(&c).unwrap())
        })
        .collect()
}

/// The surface's view of a derived principal's kind. Exhaustive, so a new bus kind fails to
/// compile here.
fn kind_of(p: &Principal) -> PrincipalKind {
    match p.kind() {
        BusKind::Human => PrincipalKind::Human,
        BusKind::Agent => PrincipalKind::Agent,
        BusKind::Script => PrincipalKind::Script,
        BusKind::Service => PrincipalKind::Service,
        BusKind::Device => PrincipalKind::Device,
        BusKind::System => PrincipalKind::System,
    }
}

fn principal_as(table: &mut SessionTable, kind: BusKind) -> Principal {
    principals(table).into_iter().map(|(_, p)| p).find(|p| p.kind() == kind).unwrap()
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

/// A CLI or Python session upgraded to `human` by a confirmed person (CORE.BUS.PRIN).
fn upgraded(table: &mut SessionTable, kind: ChannelKind) -> Principal {
    let mut c = channel(table, kind, "bob");
    assert_eq!(table.derive_principal(&c).unwrap().kind(), BusKind::Script);
    let pending = table.prepare_upgrade(&c).unwrap();
    let confirmed = pending.confirm(&Confirms(OsUser::new("bob").unwrap()), &mut StepClock(1_000)).unwrap();
    let ctx = UpgradeContext {
        stream: "record".into(),
        branch: "main".into(),
        manifests: Manifests { registry: Digest([0x11; 32]), kernel: Digest([0x22; 32]) },
    };
    c.grant = Some(table.commit_upgrade(confirmed, &mut NoJournal, &ctx).unwrap());
    let p = table.derive_principal(&c).unwrap();
    assert_eq!(p.kind(), BusKind::Human);
    p
}

// ------------------------------------------------------------------ commands

/// The same definition JSON as the bus's own tests, as a registry `CommandDef` (for the bus)
/// and as a manifest entry (for the surface projection).
fn def_value(id: &str, class: &str, fs_effect: &str, params: &str) -> CValue {
    let json = format!(
        r#"{{"id":"{id}","major":1,"title":"Test command","description":"File handle test command.",
"params_schema":{{"type":"object","properties":{params}}},"result_schema":{{"type":"object"}},"side_effect":"{class}",
"mcp_projection":"exposed","determinism":"D0","idempotency":"none","fs_effect":"{fs_effect}","undo":"head_move",
"preconditions":[],"effects":["TestDone"],"algorithms":[],"exposure":["cli","mcp"],
"lifecycle":{{"state":"active","since":"2026-01-01"}},"web":"available","human_only":false}}"#
    );
    parse(json.as_bytes()).unwrap()
}

fn file_params(param: &str, exts: &[&str]) -> String {
    let exts: Vec<String> = exts.iter().map(|e| format!("\"{e}\"")).collect();
    format!(
        r#"{{"{param}":{{"type":"string","path_like":true,"x-extensions":[{}]}},"note":{{"type":"string"}}}}"#,
        exts.join(",")
    )
}

/// Projects definitions through a real canonical manifest.
fn projection_of(defs: &[CValue]) -> Projection {
    let commands = defs
        .iter()
        .map(|d| {
            let CValue::Obj(mut o) = d.clone() else { panic!("definition must be an object") };
            o.insert("definition_digest".into(), CValue::Str(digest(d).unwrap().to_string()));
            CValue::Obj(o)
        })
        .collect();
    let mut m = BTreeMap::new();
    m.insert("format".to_string(), CValue::Num(1.0));
    m.insert("quantity_catalogue_digest".to_string(), CValue::Str(format!("sha256:{}", "ab".repeat(32))));
    m.insert("commands".to_string(), CValue::Arr(commands));
    project(&to_canonical_bytes(&CValue::Obj(m)).unwrap()).unwrap()
}

/// One command as both the bus and the surface see it.
struct Cmd {
    def: CommandDef,
    proj: SurfaceProjection,
    param: &'static str,
}

fn cmd(id: &str, class: &str, fs_effect: &str, param: &'static str, exts: &[&str]) -> Cmd {
    let v = def_value(id, class, fs_effect, &file_params(param, exts));
    let def = CommandDef::from_value(v.clone()).unwrap();
    let proj = projection_of(&[v]).commands.into_iter().next().unwrap();
    Cmd { def, proj, param }
}

fn read_cmd() -> Cmd {
    cmd("test.import_part", "R1", "read_root", "source", &["stl", "STEP", "gz"])
}

fn write_cmd() -> Cmd {
    cmd("test.export_table", "W1", "write_root", "target", &["csv", "stl"])
}

fn none_cmd() -> Cmd {
    cmd("test.inspect_name", "R0", "none", "file", &["stl"])
}

fn params(name: &str, value: CValue) -> CValue {
    let mut o = BTreeMap::new();
    o.insert(name.to_string(), value);
    CValue::Obj(o)
}

fn verdict_of<T>(r: &Result<T, BusError>) -> String {
    match r {
        Ok(_) => "ok".to_string(),
        Err(e) => format!("{}/{}", e.code(), e.reason()),
    }
}

fn surface_verdict(r: &Result<(), RootError>) -> String {
    match r {
        Ok(()) => "ok".to_string(),
        Err(e) => format!("{}/{}", e.bus_code(), e.bus_reason()),
    }
}

/// What a surface does: its gate first, and only what passes goes to the bus resolver.
fn through_surface(p: &Principal, c: &Cmd, value: &str, roots: &Roots) -> (String, Option<ResolvedPath>) {
    let gate = admit(kind_of(p), &c.proj, &params(c.param, CValue::Str(value.to_string())));
    if gate.is_err() {
        return (surface_verdict(&gate), None);
    }
    let r = resolve(p, &c.def, c.param, value, roots);
    (verdict_of(&r), r.ok())
}

// ------------------------------------------------------------------ trees

fn plain(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy().to_string();
    match s.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC") => PathBuf::from(rest),
        _ => p,
    }
}

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

#[cfg(windows)]
const SEP: char = '\\';
#[cfg(not(windows))]
const SEP: char = '/';

/// A temporary folder, removed on drop; `path` is canonical without the `\\?\` prefix.
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

impl Tree {
    fn empty() -> Tree {
        let name = format!("fid-surface-roots-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed));
        let raw = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&raw);
        std::fs::create_dir_all(&raw).unwrap();
        Tree { path: plain(std::fs::canonicalize(&raw).unwrap()), dirlinks: true, filelinks: true }
    }

    fn p(&self, rel: &str) -> PathBuf {
        rel.split('/').fold(self.path.clone(), |p, s| p.join(s))
    }

    fn file(&self, rel: &str, text: &str) {
        std::fs::write(self.p(rel), text).unwrap();
    }

    fn require_links(&self) {
        if cfg!(unix) {
            assert!(self.dirlinks && self.filelinks, "could not create links on Unix");
        }
        if cfg!(windows) {
            assert!(self.dirlinks, "could not create junctions on Windows");
            if std::env::var_os("CI").is_some() {
                assert!(self.filelinks, "CI: could not create file symlinks on Windows");
            }
        }
    }

    fn roots(&self, list: &[(&str, &str, &str, Option<&[&str]>)]) -> Roots {
        let items = list
            .iter()
            .map(|(id, kind, dir, exts)| {
                let mut o = BTreeMap::new();
                o.insert("id".to_string(), CValue::Str((*id).into()));
                o.insert("kind".to_string(), CValue::Str((*kind).into()));
                o.insert("path".to_string(), CValue::Str(self.p(dir).to_string_lossy().to_string()));
                if let Some(e) = exts {
                    o.insert("extensions".into(), CValue::Arr(e.iter().map(|x| CValue::Str((*x).into())).collect()));
                }
                CValue::Obj(o)
            })
            .collect();
        Roots::from_value(&CValue::Arr(items)).unwrap()
    }

    /// The tree of CORE.BUS.POL.FS's corpus.
    fn corpus() -> (Tree, Roots) {
        let mut t = Tree::empty();
        for d in ["outside", "imports/sub", "imports/dir.stl", "exports/sub", "shop"] {
            std::fs::create_dir_all(t.p(d)).unwrap();
        }
        t.file("outside/secret.stl", "outside stl");
        t.file("outside/secret.txt", "outside txt");
        for f in ["part.stl", "PART2.STL", "sub/deep.stl", "notes.txt", "archive.tar.gz", "part.stl.exe"] {
            t.file(&format!("imports/{f}"), f);
        }
        for f in ["part.exe.stl", "noext", ".stl", "caf\u{e9}.stl"] {
            t.file(&format!("imports/{f}"), f);
        }
        t.file("exports/existing.csv", "original");
        t.file("shop/fixture.step", "step");
        t.file("shop/fixture.stl", "stl");
        let mut d = true;
        d &= dir_link(&t.p("imports/link_out"), "../outside", &t.p("outside"));
        d &= dir_link(&t.p("imports/link_in"), "sub", &t.p("imports/sub"));
        d &= dir_link(&t.p("imports/link_up"), "..", &t.path);
        d &= dir_link(&t.p("exports/link_out"), "../outside", &t.p("outside"));
        d &= dir_link(&t.p("exports/link_sub"), "sub", &t.p("exports/sub"));
        t.dirlinks = d;
        let abs_out = t.p("outside/secret.stl").to_string_lossy().to_string();
        let mut f = true;
        f &= file_link(&t.p("imports/file_out.stl"), &format!("..{SEP}outside{SEP}secret.stl"));
        f &= file_link(&t.p("imports/file_in.stl"), "part.stl");
        f &= file_link(&t.p("imports/file_ext.stl"), "notes.txt");
        f &= file_link(&t.p("imports/loop_a.stl"), "loop_b.stl");
        f &= file_link(&t.p("imports/loop_b.stl"), "loop_a.stl");
        f &= file_link(&t.p("imports/file_abs_out.stl"), &abs_out);
        f &= file_link(&t.p("exports/dangling.csv"), "missing_target.csv");
        f &= file_link(&t.p("exports/link_existing.csv"), "existing.csv");
        t.filelinks = f;
        t.require_links();
        let roots = t.roots(&[
            ("imports", "import", "imports", None),
            ("exports", "export", "exports", None),
            ("shop", "import", "shop", Some(&["STEP"])),
        ]);
        (t, roots)
    }

    /// The leaf's tree `fixtures/roots/tree/`: read root `in`, write root `out`, built at run
    /// time because links and junctions cannot be stored portably in git.
    fn leaf() -> (Tree, Roots) {
        let mut t = Tree::empty();
        for d in ["in/parts", "out", "swap-outside"] {
            std::fs::create_dir_all(t.p(d)).unwrap();
        }
        t.file("in/parts/a.stl", "solid a");
        t.file("secret.txt", "secret");
        t.file("swap-outside/a.stl", "outside");
        // `in/link-out` points to the parent folder of `in`, which holds `secret.txt`.
        t.dirlinks = dir_link(&t.p("in/link-out"), "..", &t.path);
        t.require_links();
        let roots = t.roots(&[("in", "import", "in", None), ("out", "export", "out", None)]);
        (t, roots)
    }
}

fn decode(field: &str, tree: &Path) -> String {
    let b = field.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            out.push(u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap(), 16).unwrap());
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap().replace("{tree}", &tree.to_string_lossy())
}

// ------------------------------------------------------------------ same verdict as the bus

/// Every row of CORE.BUS.POL.FS's 109-case corpus, for the principal of every channel kind:
/// the surface's raw-path verdict equals the bus's `check_raw_paths`; the surface gate followed
/// by the bus resolver gives exactly the bus resolver's verdict alone; and for the row's own
/// principal that verdict is the hand-labelled one.
#[test]
fn corpus_same_verdict_as_the_bus() {
    let (tree, roots) = Tree::corpus();
    let mut table = SessionTable::new();
    let everyone = principals(&mut table);
    let (read, write, none) = (read_cmd(), write_cmd(), none_cmd());
    let text = std::fs::read_to_string(repo().join("fixtures/core/bus/pol/fs/cases.csv")).unwrap();
    let mut lines = text.lines();
    assert_eq!(lines.next(), Some("id,principal,effect,input,expected,needs"));
    let (mut rows, mut skipped, mut refused_at_surface) = (0, 0, 0);
    for line in lines.filter(|l| !l.is_empty()) {
        let f: Vec<&str> = line.split(',').collect();
        assert_eq!(f.len(), 6, "{line}");
        let needs_ok = match f[5] {
            "-" => true,
            "dirlink" => tree.dirlinks,
            "filelink" => tree.filelinks,
            other => panic!("needs {other:?}"),
        };
        if !needs_ok {
            skipped += 1;
            continue;
        }
        let c = match f[2] {
            "read" => &read,
            "write" => &write,
            "none" => &none,
            other => panic!("effect {other:?}"),
        };
        let input = decode(f[3], &tree.path);
        let value = params(c.param, CValue::Str(input.clone()));
        for (ch, p) in &everyone {
            let k = kind_of(p);
            assert_eq!(k.as_str(), p.kind().as_str(), "{ch:?}");
            let surface = surface_verdict(&gate_raw_paths(k, &c.proj, &value));
            let bus = verdict_of(&check_raw_paths(p, &c.def, &value));
            assert_eq!(surface, bus, "row {} via {ch:?}: raw-path verdicts differ", f[0]);
            let (combined, _) = through_surface(p, c, &input, &roots);
            let alone = verdict_of(&resolve(p, &c.def, c.param, &input, &roots));
            assert_eq!(combined, alone, "row {} via {ch:?}: the surface gate changed the outcome", f[0]);
            if surface != "ok" {
                refused_at_surface += 1;
            }
            if p.kind().as_str() == f[1] {
                assert_eq!(combined, f[4], "row {} via {ch:?} input {input:?}", f[0]);
            }
        }
        rows += 1;
    }
    assert_eq!(rows + skipped, 109);
    assert!(rows >= 100 || cfg!(windows), "ran {rows}");
    assert!(refused_at_surface > 0);
    // Nothing was written by any of it.
    assert_eq!(std::fs::read_to_string(tree.p("exports/existing.csv")).unwrap(), "original");
    assert!(!tree.p("exports/new.csv").exists());
}

/// Non-string values, several file parameters, other parameters and a nested `path_like`
/// declaration: the surface and the bus agree, in the same order.
#[test]
fn edge_values_same_verdict_as_the_bus() {
    let two = def_value(
        "test.merge_parts",
        "R1",
        "read_root",
        r#"{"a":{"type":"string","path_like":true},"b":{"type":"array","path_like":true},"note":{"type":"string"}}"#,
    );
    let nested = def_value(
        "test.nested",
        "R1",
        "read_root",
        r#"{"opts":{"type":"object","properties":{"file":{"type":"string","path_like":true}}}}"#,
    );
    let defs = [CommandDef::from_value(two.clone()).unwrap(), CommandDef::from_value(nested.clone()).unwrap()];
    let proj = projection_of(&[two, nested]);
    assert!(!proj.commands[0].nested_path_like && proj.commands[1].nested_path_like);
    let cases: Vec<CValue> = vec![
        parse(br#"{"a":"/etc/passwd","b":["/x"]}"#).unwrap(),
        parse(br#"{"a":"root:imports/a.stl","b":["/x"]}"#).unwrap(),
        parse(br#"{"a":"root:imports/a.stl","b":"root:imports/b.stl","note":"/etc/passwd"}"#).unwrap(),
        parse(br#"{"a":5}"#).unwrap(),
        parse(br#"{"opts":{"file":"/etc/passwd"}}"#).unwrap(),
        parse(br#"{}"#).unwrap(),
        parse(br#"["/etc/passwd"]"#).unwrap(),
        parse(br#""/etc/passwd""#).unwrap(),
    ];
    let mut table = SessionTable::new();
    for (ch, p) in principals(&mut table) {
        for (def, sp) in defs.iter().zip(&proj.commands) {
            for v in &cases {
                let surface = surface_verdict(&gate_raw_paths(kind_of(&p), sp, v));
                let bus = verdict_of(&check_raw_paths(&p, def, v));
                assert_eq!(surface, bus, "{ch:?} {} {v:?}", def.id);
            }
        }
    }
}

/// The leaf's error names map onto codes and reasons that exist in the bus catalog, and the
/// surface's principal kinds are the bus's.
#[test]
fn errors_and_kinds_match_the_bus() {
    let catalog = fid_bus::current_catalog();
    for (leaf, code, reason) in ERROR_MAP {
        assert!(catalog.get(*code).is_some_and(|rs| rs.contains(*reason)), "{leaf} -> {code}/{reason}");
    }
    let mut table = SessionTable::new();
    let derived: BTreeSet<&str> = principals(&mut table).iter().map(|(_, p)| kind_of(p).as_str()).collect();
    let surface: BTreeSet<&str> = PrincipalKind::ALL.iter().map(|k| k.as_str()).collect();
    assert_eq!(derived, surface);
}

// ------------------------------------------------------------------ AC-1 .. AC-6

fn leaf_read_cmd() -> Cmd {
    cmd("test.read_any", "R1", "read_root", "source", &["stl", "txt"])
}

/// AC-1: `root:in/parts/a.stl` passes every surface's gate and resolves to the canonical path
/// inside `in`.
#[test]
fn ac1_handle_resolves_inside_its_root() {
    let (tree, roots) = Tree::leaf();
    let c = leaf_read_cmd();
    let want = std::fs::canonicalize(tree.p("in/parts/a.stl")).unwrap();
    let mut table = SessionTable::new();
    for (ch, p) in principals(&mut table) {
        let (v, r) = through_surface(&p, &c, "root:in/parts/a.stl", &roots);
        assert_eq!(v, "ok", "{ch:?}");
        let r = r.unwrap();
        assert_eq!(r.root().map(|x| x.as_str()), Some("in"));
        let got = std::fs::canonicalize(roots.iter().find(|x| x.id().as_str() == "in").unwrap().path())
            .unwrap()
            .join(r.rel_path());
        assert_eq!(got, want, "{ch:?}");
        let mut s = String::new();
        r.open_read().unwrap().read_to_string(&mut s).unwrap();
        assert_eq!(s, "solid a");
    }
}

/// AC-2: `in/link-out` is a symlink (Unix) or junction (Windows) to the parent folder;
/// `root:in/link-out/secret.txt` fails with `root_escape` (bus `policy/outside_root`).
#[test]
fn ac2_link_escape_is_refused() {
    let (tree, roots) = Tree::leaf();
    let c = leaf_read_cmd();
    let mut table = SessionTable::new();
    for (ch, p) in principals(&mut table) {
        let (v, r) = through_surface(&p, &c, "root:in/link-out/secret.txt", &roots);
        assert_eq!(v, "policy/outside_root", "{ch:?}");
        assert!(r.is_none());
    }
    assert_eq!(std::fs::read_to_string(tree.p("secret.txt")).unwrap(), "secret");
}

/// AC-3: the five malformed handles fail the shared handle grammar before any file-system
/// access (the roots do not even exist here). Four are `bad_handle`; `root:in/../x` is
/// `outside_root` (`root_escape`), the bus grammar's verdict for `..`, kept so the two gates
/// cannot disagree.
#[test]
fn ac3_malformed_handles() {
    let c = leaf_read_cmd();
    let roots = Roots::default();
    let cases = [
        ("root:in/../x", "policy/outside_root"),
        ("root:in/C:/x", "policy/bad_handle"),
        ("root:in//server/share", "policy/bad_handle"),
        ("root:in/a.txt:stream", "policy/bad_handle"),
        ("root:in/a\0.stl", "policy/bad_handle"),
    ];
    let mut table = SessionTable::new();
    for (h, want) in cases {
        assert_eq!(verdict_of(&FileHandle::parse(h)), want, "{h:?}");
        for (ch, p) in principals(&mut table) {
            assert_eq!(through_surface(&p, &c, h, &roots).0, want, "{h:?} via {ch:?}");
        }
    }
}

/// AC-4: `C:\data\a.stl` in a `path_like` parameter from an agent (and from every other
/// principal but `human`) gives `raw_path_refused` and the fake bus receives nothing. A field
/// in the request that claims a principal changes nothing; the kind is the derived one.
#[test]
fn ac4_raw_path_refused_before_the_bus() {
    let c = leaf_read_cmd();
    let mut sent = Vec::new();
    let mut table = SessionTable::new();
    let call = |p: &Principal, value: &CValue, sent: &mut Vec<CValue>| {
        dispatch_gated(kind_of(p), &c.proj, value, |v| sent.push(v.clone())).map_err(|e| e.code())
    };
    let raw = r"C:\data\a.stl";
    let mut req = BTreeMap::new();
    req.insert("source".to_string(), CValue::Str(raw.into()));
    req.insert("principal".to_string(), CValue::Str("human".into()));
    let req = CValue::Obj(req);
    for (ch, p) in principals(&mut table) {
        let r = call(&p, &req, &mut sent);
        if p.kind() == BusKind::Human {
            assert_eq!(r, Ok(()), "{ch:?}");
        } else {
            assert_eq!(r, Err("raw_path_refused"), "{ch:?}");
            assert_eq!(sent.len(), 0, "{ch:?}: the bus received a refused call");
        }
    }
    assert_eq!(sent.len(), 1, "only the GUI human's call is sent");
    // The refusal as a surface reports it.
    let agent = principal_as(&mut table, BusKind::Agent);
    let e = admit(kind_of(&agent), &c.proj, &params("source", CValue::Str(raw.into()))).unwrap_err();
    assert_eq!((e.bus_code(), e.bus_reason()), ("policy", "raw_path_refused"));
    let env = e.to_envelope(c.proj.command_id.as_str());
    assert_eq!(env.error.as_ref().map(|b| b.code.as_str()), Some("raw_path_refused"));
    env.validate().unwrap();
    // A CLI or Python session upgraded to human may pass a raw path.
    for kind in [ChannelKind::Cli, ChannelKind::Python] {
        let human = upgraded(&mut table, kind);
        assert_eq!(call(&human, &req, &mut sent), Ok(()), "{kind:?}");
    }
    assert_eq!(sent.len(), 3);
}

fn read_json(rel: &str) -> CValue {
    parse(&std::fs::read(repo().join(rel)).unwrap()).unwrap()
}

fn strings(v: Option<&CValue>) -> Vec<String> {
    match v {
        Some(CValue::Arr(items)) => items
            .iter()
            .map(|i| match i {
                CValue::Str(s) => s.clone(),
                other => panic!("{other:?}"),
            })
            .collect(),
        other => panic!("{other:?}"),
    }
}

/// AC-5: in `fixtures/manifest/roots-config.json`, `settings.set_roots@1` (A0, roots effect) is
/// root config, and the projection filter yields it for `human` only, on every channel; the
/// expected sets come from the independent Python reference.
#[test]
fn ac5_root_config_hidden_from_automation() {
    let proj = project(&std::fs::read(repo().join("fixtures/manifest/roots-config.json")).unwrap()).unwrap();
    let want = read_json("fixtures/projection/roots-config.visible.json");
    let CValue::Obj(want) = want else { panic!("expected an object") };
    let root_config: Vec<String> =
        proj.commands.iter().filter(|c| is_root_config(c)).map(|c| c.command_id.as_str().to_string()).collect();
    assert_eq!(root_config, strings(want.get("root_config")));
    assert!(root_config.contains(&"settings.set_roots@1".to_string()));
    let Some(CValue::Obj(visible)) = want.get("visible") else { panic!("visible") };
    let set_roots = proj.commands.iter().find(|c| c.command_id.as_str() == "settings.set_roots@1").unwrap();
    let mut table = SessionTable::new();
    for (ch, p) in principals(&mut table) {
        let k = kind_of(&p);
        let got: Vec<String> = visible_commands(k, &proj).iter().map(|c| c.command_id.as_str().to_string()).collect();
        assert_eq!(got, strings(visible.get(k.as_str())), "{ch:?}");
        let mut sent = 0;
        let r = dispatch_gated(k, set_roots, &parse(br#"{"roots":[]}"#).unwrap(), |_| sent += 1);
        if k == PrincipalKind::Human {
            assert!(r.is_ok() && sent == 1, "{ch:?}");
        } else {
            let e = r.unwrap_err();
            assert_eq!((e.code(), e.bus_code(), e.bus_reason()), ("root_config_hidden", "policy", "human_only"));
            assert_eq!(sent, 0, "{ch:?}");
            assert!(gate_root_config(k, set_roots).is_err());
        }
    }
}

/// AC-6 (race): a folder resolved inside the root and swapped for a link to outside before
/// the open; the checked open fails with `root_escape` (`outside_root`) and nothing outside is
/// read.
#[test]
fn ac6_swap_race_caught_at_open() {
    let (tree, roots) = Tree::leaf();
    let c = leaf_read_cmd();
    let mut table = SessionTable::new();
    let agent = principal_as(&mut table, BusKind::Agent);
    let (v, r) = through_surface(&agent, &c, "root:in/parts/a.stl", &roots);
    assert_eq!(v, "ok");
    let r = r.unwrap();
    std::fs::remove_dir_all(tree.p("in/parts")).unwrap();
    assert!(dir_link(&tree.p("in/parts"), "../swap-outside", &tree.p("swap-outside")));
    assert_eq!(verdict_of(&r.open_read()), "policy/outside_root");
    assert_eq!(through_surface(&agent, &c, "root:in/parts/a.stl", &roots).0, "policy/outside_root");
}
