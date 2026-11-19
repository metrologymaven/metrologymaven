//! Spec node CORE.STO.SET: project-scope values in `project.db` (`settings_current`). A change
//! entry built by `settings.set_value@1` is staged in the same transaction as its journal entry
//! row and then read back through `ProjectTable`, as the project journal will do.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_registry::{Catalogue, Surface};
use fid_store::settings::{
    decode_record, list_values, reset_value, set_value, ActivityLog, Caller, ChangeEntry, FileStore, JournalTarget,
    ListValuesParams, PrincipalKind, ProjectScope, ProjectTable, ProjectValues, ResetValueParams, Scope,
    SetValueParams, SettingValue, SettingWarning, SettingsCtx, SettingsJournal, SettingsRegistry, Source,
};
use fid_store::{OpenMode, ProjectLocation, Store, Value};
use sha2::{Digest as _, Sha256};
use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

const KEY: &str = "probe.approach_distance";

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        static N: AtomicU32 = AtomicU32::new(0);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos();
        let name = format!("fid-settbl-{tag}-{}-{nanos}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst));
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

/// Commits each project-scope entry like the Record Journal will: an `entries` row and the
/// `settings_current` row in one transaction.
struct DbJournal<'s> {
    store: &'s Store,
    seq: RefCell<u32>,
}

impl SettingsJournal for DbJournal<'_> {
    fn commit(&self, target: &JournalTarget, entry: &ChangeEntry) -> Result<(), String> {
        let JournalTarget::Project { branch } = target else { return Err("project entries only".into()) };
        let n = {
            let mut s = self.seq.borrow_mut();
            *s += 1;
            *s
        };
        let hex: String = Sha256::digest(format!("entry {n}").as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
        let hash = format!("sha256:{hex}");
        let mut txn = self.store.transaction();
        txn.insert(
            "entries",
            &[
                ("hash", Value::from(hash.as_str())),
                ("stream", Value::from("s")),
                ("branch", Value::from(branch.as_str())),
                ("kind", Value::from(entry.command())),
                ("header", Value::Blob(b"header".to_vec())),
            ],
        );
        fid_store::settings::stage_project_value(&mut txn, entry, &hash).map_err(|e| e.to_string())?;
        txn.commit().map(|_| ()).map_err(|e| e.code().to_string())
    }
}

struct NoLog;

impl ActivityLog for NoLog {
    fn warn(&self, w: &SettingWarning) {
        panic!("unexpected warning {w}");
    }
}

fn registry() -> SettingsRegistry {
    let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/core/sto/set/decls.json")).unwrap();
    SettingsRegistry::from_manifest(&bytes, &Catalogue::geo_frame().unwrap()).unwrap()
}

#[test]
fn project_values_round_trip_through_settings_current() {
    let t = TempDir::new("rt");
    let root = t.0.join("p");
    let (_loc, lock) = ProjectLocation::create(&root).unwrap();
    drop(lock);
    let store = Store::open(&root, OpenMode::Write).unwrap();
    let table = ProjectTable::new(&store);
    assert_eq!(table.record(KEY).unwrap(), None);

    let reg = registry();
    let user = FileStore::new(Scope::User, t.0.join("u").join("settings.json")).unwrap();
    let machine = FileStore::new(Scope::Machine, t.0.join("m").join("settings.json")).unwrap();
    let journal = DbJournal { store: &store, seq: RefCell::new(0) };
    let ctx = SettingsCtx {
        registry: &reg,
        caller: Caller { kind: PrincipalKind::Human, surface: Surface::Gui },
        project: Some(ProjectScope { values: &table, branch: "main".into(), writable: true }),
        user: &user,
        machine: &machine,
        journal: &journal,
        log: &NoLog,
    };
    set_value(&ctx, SetValueParams { key: KEY.into(), scope: Scope::Project, value: SettingValue(CValue::Num(0.2)) })
        .unwrap();
    let rec = table.record(KEY).unwrap().unwrap();
    assert_eq!(rec, br#"{"v":0.2}"#);
    assert_eq!(decode_record(&rec), Ok(Some(CValue::Num(0.2))));
    let rows = list_values(&ctx, ListValuesParams { prefix: Some("probe.".into()), scope: None }).unwrap();
    assert_eq!((rows[0].value.0.clone(), rows[0].source_scope), (CValue::Num(0.2), Source::Project));

    // The row names the entry that wrote it.
    let (n, branch): (i64, String) = store
        .read(|c| {
            c.query_row(
                "SELECT count(*), e.branch FROM settings_current s JOIN entries e ON e.hash = s.entry_hash",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
        })
        .unwrap();
    assert_eq!((n, branch.as_str()), (1, "main"));

    // A reset leaves an empty record: no project value.
    let r = reset_value(&ctx, ResetValueParams { key: KEY.into(), scope: Scope::Project }).unwrap();
    assert_eq!(r.previous.map(|p| p.0), Some(CValue::Num(0.2)));
    assert_eq!(table.record(KEY).unwrap().unwrap(), b"{}");
    let rows = list_values(&ctx, ListValuesParams { prefix: Some("probe.".into()), scope: None }).unwrap();
    assert_eq!((rows[0].value.0.clone(), rows[0].source_scope), (CValue::Num(2.0), Source::Default));
    assert!(!user.path().exists());
}

#[test]
fn records_decode_strictly() {
    assert_eq!(decode_record(b"{}"), Ok(None));
    assert_eq!(decode_record(br#"{"v":[1]}"#), Ok(Some(CValue::Arr(vec![CValue::Num(1.0)]))));
    for bad in [&b"0.2"[..], br#"{"v":1,"w":2}"#, br#"{"x":1}"#, br#"{"v":1,"v":2}"#, b"", b"\xff"] {
        assert_eq!(decode_record(bad), Err(()), "{bad:?}");
    }
}
