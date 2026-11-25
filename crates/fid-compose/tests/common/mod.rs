//! Shared helpers of the `fid-compose` integration tests: temporary projects, roots, session
//! hosts with and without a fake presence provider, one call through the tracker, the smallest
//! parameter document a schema accepts, and the Record Journal of a project as headers.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::error::{BusError, Detail, ValidationReason};
use fid_bus::policy::fs::Roots;
use fid_bus::port::SessionHost;
use fid_bus::principal::{HumanPresence, OsUser, PresenceOutcome, UpgradeRequest};
use fid_bus::unit::Normalised;
use fid_bus::validate::{Compiled, Hooks, RefLookup};
use fid_bus_api::PortKind;
use fid_canon::CValue;
use fid_compose::{process_os_user, Composition, HostTransport, InProcessHost, SystemUpgradeClock};
use fid_journal::{list_store, ConfirmMethod, ListQuery};
use fid_store::{OpenMode, Store};
use fid_surface::jobs::{CountingKeySource, ManualClock, Reason, Tracker};
use fid_surface::Envelope;
use fid_types::CommandId;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

static NEXT: AtomicU64 = AtomicU64::new(0);

/// A fresh, empty temporary folder (canonical, without a Windows verbatim prefix).
pub fn temp_dir(name: &str) -> PathBuf {
    let raw = std::env::temp_dir().join(format!(
        "fid-compose-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&raw);
    std::fs::create_dir_all(&raw).unwrap();
    let p = std::fs::canonicalize(&raw).unwrap();
    #[cfg(windows)]
    let p = PathBuf::from(p.to_string_lossy().trim_start_matches(r"\\?\"));
    p
}

/// A project created by `CORE.STO.LAYOUT` (through the store, which also writes the empty
/// schema) at `<base>/p`, closed again. Returns its path.
pub fn new_project(base: &Path) -> PathBuf {
    let dir = base.join("p");
    drop(Store::create(&dir).unwrap());
    dir
}

/// One approved root `id` of `kind` (`import` or `export`) at `path`.
pub fn roots(id: &str, kind: &str, path: &Path) -> Roots {
    let mut o = BTreeMap::new();
    o.insert("id".to_string(), CValue::Str(id.into()));
    o.insert("kind".to_string(), CValue::Str(kind.into()));
    o.insert("path".to_string(), CValue::Str(path.to_string_lossy().to_string()));
    Roots::from_value(&CValue::Arr(vec![CValue::Obj(o)])).unwrap()
}

/// A presence provider that confirms the session's OS user (test code only).
pub struct Confirms;

impl HumanPresence for Confirms {
    fn confirm(&self, req: &UpgradeRequest, _deadline: Duration) -> PresenceOutcome {
        PresenceOutcome::Confirmed { os_user: req.os_user.clone(), method: ConfirmMethod::OsPresence }
    }
}

pub fn os_user() -> OsUser {
    process_os_user().unwrap()
}

/// A session host with no presence provider, as the shipped composition builds it.
pub fn plain_host() -> Arc<SessionHost> {
    Arc::new(SessionHost::new(os_user(), None, Box::new(SystemUpgradeClock)).unwrap())
}

/// A session host whose fake provider confirms the session's OS user.
pub fn confirming_host() -> Arc<SessionHost> {
    Arc::new(SessionHost::new(os_user(), Some(Arc::new(Confirms)), Box::new(SystemUpgradeClock)).unwrap())
}

pub fn host(session: Arc<SessionHost>, kind: PortKind, roots: Roots) -> InProcessHost {
    InProcessHost::with_host(session, kind, roots, Composition::workspace().unwrap()).unwrap()
}

pub type Track = Tracker<HostTransport, ManualClock, CountingKeySource>;

pub fn tracker(transport: HostTransport) -> Track {
    Tracker::new(transport, ManualClock::new(), CountingKeySource::new(0x0190_1234_5678))
}

/// Sends `command` with `params` through the tracker and returns its final envelope.
pub fn send(t: &mut Track, command: &str, params: CValue) -> Envelope {
    let id = t.submit(CommandId::parse(command).unwrap(), params, Duration::from_secs(60)).unwrap();
    t.pump(&mut ());
    let report = t.take(id).unwrap();
    assert_eq!(report.reason, Reason::Final, "{command}");
    report.envelope
}

/// `{"<k>": <v>, ...}`.
pub fn obj(pairs: &[(&str, CValue)]) -> CValue {
    CValue::Obj(pairs.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect())
}

/// A detail of an envelope error.
pub fn detail(env: &Envelope, key: &str) -> Option<String> {
    match env.error.as_ref()?.details.as_ref()? {
        CValue::Obj(o) => match o.get(key) {
            Some(CValue::Str(s)) => Some(s.clone()),
            _ => None,
        },
        _ => None,
    }
}

/// The headers of the Record Journal of the project in `dir`, in order.
pub fn journal_headers(dir: &Path) -> Vec<CValue> {
    let store = Store::open(dir, OpenMode::Read).unwrap();
    list_store(&store, "record", &ListQuery::default()).unwrap().headers.into_iter().map(|h| h.header).collect()
}

/// `v[a][b]...` as text.
pub fn text_at(v: &CValue, path: &[&str]) -> Option<String> {
    let mut cur = v;
    for k in path {
        let CValue::Obj(o) = cur else { return None };
        cur = o.get(*k)?;
    }
    match cur {
        CValue::Str(s) => Some(s.clone()),
        _ => None,
    }
}

/// The contents of the project's `lock` file (empty when no writer holds it).
pub fn lock_record(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("lock")).unwrap_or_default()
}

// ------------------------------------------------------------------ smallest parameter documents

struct NoHooks;

impl Hooks for NoHooks {
    fn literal(&self, _: &str, _: &CValue) -> Result<Normalised, BusError> {
        Err(BusError::Validation(ValidationReason::UnitMismatch, Detail::new("no literals")))
    }
    fn entity(&self, _: &str) -> RefLookup {
        RefLookup::Missing
    }
}

/// Whether `value` conforms to `schema` (no literals, no entities).
pub fn conforms(schema: &CValue, value: &CValue) -> bool {
    Compiled::compile(schema).is_ok_and(|c| c.evaluate(value, &NoHooks).is_ok())
}

const STRINGS: [&str; 14] = [
    "a",
    "0190a3b2-0000-7000-8000-000000000001",
    "sha256:0000000000000000000000000000000000000000000000000000000000000000",
    "main",
    "2026-01-01T00:00:00Z",
    "root:none/a.json",
    "demo.thing@1",
    "demo.thing",
    "a.b",
    "A",
    "0",
    "1.0.0",
    "x1",
    "ab",
];

/// The smallest value `schema` accepts, as far as these simple rules find one: `const`, the
/// first `enum` value, the first `oneOf`/`anyOf` branch, an object of its required properties,
/// `minItems` items, the minimum of a number, `false`, and the first of a few candidate
/// strings that conforms.
pub fn smallest(schema: &CValue) -> CValue {
    let CValue::Obj(s) = schema else { return CValue::Obj(BTreeMap::new()) };
    if let Some(c) = s.get("const") {
        return c.clone();
    }
    if let Some(CValue::Arr(e)) = s.get("enum") {
        if let Some(v) = e.first() {
            return v.clone();
        }
    }
    if let Some(CValue::Arr(alts)) = s.get("oneOf").or_else(|| s.get("anyOf")) {
        if let Some(a) = alts.first() {
            return smallest(a);
        }
    }
    let ty = match s.get("type") {
        Some(CValue::Str(t)) => t.clone(),
        Some(CValue::Arr(ts)) => match ts.first() {
            Some(CValue::Str(t)) => t.clone(),
            _ => "object".to_string(),
        },
        _ if s.contains_key("properties") => "object".to_string(),
        _ => "null".to_string(),
    };
    match ty.as_str() {
        "object" => {
            let mut out = BTreeMap::new();
            let props = match s.get("properties") {
                Some(CValue::Obj(p)) => p.clone(),
                _ => BTreeMap::new(),
            };
            if let Some(CValue::Arr(req)) = s.get("required") {
                for r in req {
                    if let CValue::Str(k) = r {
                        let v = props.get(k).map_or(CValue::Null, smallest);
                        out.insert(k.clone(), v);
                    }
                }
            }
            CValue::Obj(out)
        }
        "array" => {
            let n = match s.get("minItems") {
                Some(CValue::Num(n)) => *n as usize,
                _ => 0,
            };
            let item = s.get("items").map_or(CValue::Null, smallest);
            CValue::Arr(vec![item; n])
        }
        "string" => {
            if s.get("path_like") == Some(&CValue::Bool(true)) {
                return CValue::Str("root:none/a.json".into());
            }
            for c in STRINGS {
                let v = CValue::Str(c.into());
                if conforms(schema, &v) {
                    return v;
                }
            }
            let n = match s.get("minLength") {
                Some(CValue::Num(n)) => *n as usize,
                _ => 1,
            };
            CValue::Str("a".repeat(n.max(1)))
        }
        "integer" | "number" => match (s.get("minimum"), s.get("exclusiveMinimum")) {
            (Some(CValue::Num(m)), _) => CValue::Num(*m),
            (_, Some(CValue::Num(m))) => CValue::Num(m.floor() + 1.0),
            _ => CValue::Num(0.0),
        },
        "boolean" => CValue::Bool(false),
        _ => CValue::Null,
    }
}
