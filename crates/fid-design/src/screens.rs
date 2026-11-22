//! `fid-design screens-check --root <design dir> --journeys <dir> [--ga <app>] [--json]` and
//! `fid-design screens-gate --spec <spec dir> --root <design dir> [--json]` (spec node
//! `UX.SCREENS.INV`, REQ-UX.SCREENS.INV-001 .. -006). Tooling, nearest class R0; D0
//! (sorted walk, `BTreeMap`/`BTreeSet` only, sorted findings, RFC 8785 SHA-256 via `fid-canon`,
//! no clock, no network).
//!
//! # Formats
//!
//! - `<root>/screens/inventory.json` (`screen-inventory@1`):
//!   `{ "format": "screen-inventory@1", "screens": [{ "id", "app", "title", "role_layout",
//!   "phase", "traces": [{ "map", "step" }], "keyboard_path", "wireframe", "mockup" }] }`.
//! - `<root>/screens/approvals/*.json` (`screen-approval@1`):
//!   `{ "format": "screen-approval@1", "pr", "screen", "artefact", "journey_digests": { "<map>":
//!   "sha256:..." }, "approver": { "name", "principal": "human" }, "date": "YYYY-MM-DD" }`.
//! - `<journeys>/*.json` (`journey-map@1`): FLOW persona journey maps with `map_id` and
//!   `steps: [{ "n": <positive int>, .. }]`; hashed via RFC 8785 SHA-256 (`fid_canon::digest`).
//!
//! # Findings (exit 1)
//!
//! - `SCREEN-SCHEMA`: malformed inventory, approval record or journey map.
//! - `SCREEN-ID-SYNTAX`: bad `SCR.<APP>.<NAME>`, `WF.<APP>.<NAME>@<rev>` or `MK.<APP>.<NAME>@<rev>`.
//! - `SCREEN-UNTRACED <screen>`: inventory entry has no journey trace.
//! - `SCREEN-TRACE-UNKNOWN <screen>`: trace names a map or step absent from `--journeys`.
//! - `SCREEN-APPROVAL-MISSING <screen> <artefact>`: wireframe or mockup has no approval record.
//! - `SCREEN-APPROVAL-STALE <screen> <artefact>`: stored `journey_digests` differ from current
//!   RFC 8785 SHA-256 digests of the traced journey maps.
//! - `SCREEN-GA-MOCKUP-MISSING <screen>`: under `--ga <app>`, primary screen has `mockup == "none"`.
//! - `SCREEN-GATE-UNAPPROVED <node> <artefact>`: leaf cites a WF or MK ID with no approval record.
//! - `SCREEN-GATE-UNCITED <node> <screen>`: claimed leaf names a screen ID in `## Design`
//!   without citing a wireframe `WF.<APP>.<NAME>@<rev>` of it.
//!
//! Exit codes: 0 no findings, 1 findings, 2 unreadable input or bad usage.

use crate::finding::{findings_text, findings_value, read_text, Finding};
use crate::review::guard::{path_defect, valid_date, value_defect};
use crate::review::{self, walk, Kind};
use fid_canon::CValue;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Report format IDs.
pub const CHECK_REPORT_FORMAT: &str = "fid-design-screens-check@1";
pub const GATE_REPORT_FORMAT: &str = "fid-design-screens-gate@1";
/// File format IDs.
pub const INVENTORY_FORMAT: &str = "screen-inventory@1";
pub const APPROVAL_FORMAT: &str = "screen-approval@1";
pub const JOURNEY_FORMAT: &str = "journey-map@1";

/// Relative paths below `--root`.
pub const INVENTORY_FILE: &str = "screens/inventory.json";
pub const APPROVALS_DIR: &str = "screens/approvals";

/// Finding codes (UX.SCREENS.INV Design).
pub const SCHEMA: &str = "SCREEN-SCHEMA";
pub const ID_SYNTAX: &str = "SCREEN-ID-SYNTAX";
pub const UNTRACED: &str = "SCREEN-UNTRACED";
pub const TRACE_UNKNOWN: &str = "SCREEN-TRACE-UNKNOWN";
pub const APPROVAL_MISSING: &str = "SCREEN-APPROVAL-MISSING";
pub const APPROVAL_STALE: &str = "SCREEN-APPROVAL-STALE";
pub const GATE_UNAPPROVED: &str = "SCREEN-GATE-UNAPPROVED";
pub const GATE_UNCITED: &str = "SCREEN-GATE-UNCITED";
pub const GA_MOCKUP_MISSING: &str = "SCREEN-GA-MOCKUP-MISSING";

/// Largest JSON or NODE.md file read (4 MiB).
pub const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
/// Deepest JSON nesting accepted.
pub const MAX_JSON_DEPTH: usize = 32;
/// Most inventory entries, traces or approvals.
pub const MAX_ITEMS: usize = 10_000;

/// Valid `<APP>` tokens and their canonical lowercase `app` names.
pub const APP_PAIRS: [(&str, &str); 8] = [
    ("INSP", "inspect"),
    ("KIOSK", "kiosk"),
    ("NET", "network"),
    ("PROBE", "probe"),
    ("RPT", "report"),
    ("SETUP", "setup"),
    ("SHELL", "shell"),
    ("STN", "station"),
];

const PHASES: [&str; 6] = ["P0", "P1a", "P1b", "P2", "P3", "later"];
const TOP_KEYS: [&str; 2] = ["format", "screens"];
const SCREEN_KEYS: [&str; 9] =
    ["app", "id", "keyboard_path", "mockup", "phase", "role_layout", "title", "traces", "wireframe"];
const TRACE_KEYS: [&str; 2] = ["map", "step"];
const APPROVAL_KEYS: [&str; 7] = ["approver", "artefact", "date", "format", "journey_digests", "pr", "screen"];
const APPROVER_KEYS: [&str; 2] = ["name", "principal"];

/// Returns the canonical lowercase app name for an `<APP>` segment (`RPT` -> `report`).
pub fn app_for_code(code: &str) -> Option<&'static str> {
    APP_PAIRS.iter().find_map(|(c, a)| (*c == code).then_some(*a))
}

/// Whether `app` is one of the eight canonical app names accepted by `--ga <app>`.
pub fn valid_app_name(app: &str) -> bool {
    APP_PAIRS.iter().any(|(_, a)| *a == app)
}

fn valid_screen_name(s: &str) -> bool {
    let b = s.as_bytes();
    (2..=12).contains(&b.len())
        && b[0].is_ascii_uppercase()
        && b.iter().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

/// Parses `SCR.<APP>.<NAME>` into `(app_code, name)`.
pub fn parse_screen_id(s: &str) -> Option<(&str, &str)> {
    let rest = s.strip_prefix("SCR.")?;
    let (app, name) = rest.split_once('.')?;
    (!name.contains('.') && app_for_code(app).is_some() && valid_screen_name(name)).then_some((app, name))
}

/// Parses `WF.<APP>.<NAME>@<rev>` or `MK.<APP>.<NAME>@<rev>` into `(kind, app_code, name, rev)`.
pub fn parse_artefact_id(s: &str) -> Option<(&str, &str, &str, u64)> {
    let (kind, rest) = if let Some(r) = s.strip_prefix("WF.") { ("WF", r) } else { ("MK", s.strip_prefix("MK.")?) };
    let (body, rev_str) = rest.split_once('@')?;
    let (app, name) = body.split_once('.')?;
    if name.contains('.') || app_for_code(app).is_none() || !valid_screen_name(name) {
        return None;
    }
    if rev_str.is_empty() || rev_str.starts_with('0') || !rev_str.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let rev: u64 = rev_str.parse().ok()?;
    (rev >= 1).then_some((kind, app, name, rev))
}

fn json_u64(v: Option<&CValue>) -> Option<u64> {
    match v {
        Some(CValue::Num(n)) if n.is_finite() && n.fract() == 0.0 && *n >= 1.0 && *n <= 9_007_199_254_740_991.0 => {
            Some(*n as u64)
        }
        _ => None,
    }
}

fn text_ok(v: Option<&CValue>) -> Option<String> {
    match v {
        Some(CValue::Str(s)) if !s.trim().is_empty() && s.len() <= 1024 && s == s.trim() => Some(s.clone()),
        _ => None,
    }
}

#[derive(Debug, Clone)]
struct JourneyInfo {
    digest: String,
    steps: BTreeSet<u64>,
}

fn load_journeys(dir: &Path, f: &mut Vec<Finding>) -> Result<BTreeMap<String, JourneyInfo>, String> {
    let md = std::fs::symlink_metadata(dir).map_err(|e| format!("cannot read --journeys {}: {e}", dir.display()))?;
    if md.file_type().is_symlink() || !md.is_dir() {
        return Err(format!("--journeys {} must be a plain directory", dir.display()));
    }
    let mut wf = Vec::new();
    let entries = walk::walk(dir, "", &mut wf)?;
    f.extend(wf.into_iter().map(|w| Finding::new(SCHEMA, w.subject, w.detail)));

    let mut out = BTreeMap::new();
    for e in entries {
        if e.is_dir || !e.rel.ends_with(".json") {
            continue;
        }
        let text = read_text(&e.path, MAX_FILE_BYTES)?;
        let val = match fid_canon::parse_with_limits(text.as_bytes(), MAX_FILE_BYTES as usize, MAX_JSON_DEPTH) {
            Ok(v) => v,
            Err(err) => {
                f.push(Finding::new(SCHEMA, &e.rel, format!("journey map is not valid JSON: {}", err.code())));
                continue;
            }
        };
        if let Some(d) = value_defect(&val) {
            f.push(Finding::new(SCHEMA, &e.rel, format!("journey map text refused at {d}")));
            continue;
        }
        let CValue::Obj(m) = &val else {
            f.push(Finding::new(SCHEMA, &e.rel, "journey map must be a JSON object"));
            continue;
        };
        if m.get("format") != Some(&CValue::Str(JOURNEY_FORMAT.to_string())) {
            continue;
        }
        let Some(map_id) = text_ok(m.get("map_id")) else {
            f.push(Finding::new(SCHEMA, &e.rel, "journey map missing string `map_id`"));
            continue;
        };
        let Some(CValue::Arr(steps_arr)) = m.get("steps") else {
            f.push(Finding::new(SCHEMA, &e.rel, "journey map missing `steps` array"));
            continue;
        };
        if steps_arr.len() > MAX_ITEMS {
            f.push(Finding::new(SCHEMA, &e.rel, format!("journey map exceeds {MAX_ITEMS} steps")));
            continue;
        }
        let mut steps = BTreeSet::new();
        for s in steps_arr {
            if let CValue::Obj(sm) = s {
                if let Some(n) = json_u64(sm.get("n")) {
                    steps.insert(n);
                }
            }
        }
        let digest = fid_canon::digest(&val)
            .map_err(|err| format!("cannot digest journey map {}: {}", e.rel, err.code()))?
            .to_string();
        match out.entry(map_id) {
            std::collections::btree_map::Entry::Occupied(occ) => {
                f.push(Finding::new(SCHEMA, &e.rel, format!("duplicate journey `map_id` `{}`", occ.key())));
            }
            std::collections::btree_map::Entry::Vacant(vac) => {
                vac.insert(JourneyInfo { digest, steps });
            }
        }
    }
    Ok(out)
}

#[derive(Debug, Clone)]
struct ApprovalRecord {
    screen: String,
    artefact: String,
    journey_digests: BTreeMap<String, String>,
}

fn load_approvals(root: &Path, f: &mut Vec<Finding>) -> Result<Vec<ApprovalRecord>, String> {
    match review::kind(root, APPROVALS_DIR)? {
        Kind::Missing => return Ok(Vec::new()),
        Kind::Dir => {}
        _ => {
            f.push(Finding::new(SCHEMA, APPROVALS_DIR, "approvals path is not a directory"));
            return Ok(Vec::new());
        }
    }
    let mut wf = Vec::new();
    let entries = walk::walk(&root.join(APPROVALS_DIR), APPROVALS_DIR, &mut wf)?;
    f.extend(wf.into_iter().map(|w| Finding::new(SCHEMA, w.subject, w.detail)));

    let mut out = Vec::new();
    for e in entries {
        if e.is_dir || !e.rel.ends_with(".json") || e.rel.ends_with(".origin.json") {
            continue;
        }
        if out.len() >= MAX_ITEMS {
            f.push(Finding::new(SCHEMA, APPROVALS_DIR, format!("exceeds {MAX_ITEMS} approval records")));
            break;
        }
        let text = read_text(&e.path, MAX_FILE_BYTES)?;
        let val = match fid_canon::parse_with_limits(text.as_bytes(), MAX_FILE_BYTES as usize, MAX_JSON_DEPTH) {
            Ok(v) => v,
            Err(err) => {
                f.push(Finding::new(SCHEMA, &e.rel, format!("approval record is not valid JSON: {}", err.code())));
                continue;
            }
        };
        if let Some(d) = value_defect(&val) {
            f.push(Finding::new(SCHEMA, &e.rel, format!("approval record text refused at {d}")));
            continue;
        }
        let CValue::Obj(m) = &val else {
            f.push(Finding::new(SCHEMA, &e.rel, "approval record must be a JSON object"));
            continue;
        };
        let keys: Vec<&str> = m.keys().map(String::as_str).collect();
        let mut ok = true;
        if keys != APPROVAL_KEYS {
            f.push(Finding::new(
                SCHEMA,
                &e.rel,
                format!("approval record must have exactly the keys {}", APPROVAL_KEYS.join(", ")),
            ));
            ok = false;
        }
        if m.get("format") != Some(&CValue::Str(APPROVAL_FORMAT.to_string())) {
            f.push(Finding::new(SCHEMA, &e.rel, format!("`format` must be \"{APPROVAL_FORMAT}\"")));
            ok = false;
        }
        if json_u64(m.get("pr")).is_none() {
            f.push(Finding::new(SCHEMA, &e.rel, "`pr` must be a positive integer"));
            ok = false;
        }
        let screen = match m.get("screen") {
            Some(CValue::Str(s)) if parse_screen_id(s).is_some() => Some(s.clone()),
            _ => {
                f.push(Finding::new(ID_SYNTAX, &e.rel, "`screen` must match SCR.<APP>.<NAME>"));
                ok = false;
                None
            }
        };
        let artefact = match m.get("artefact") {
            Some(CValue::Str(a)) if parse_artefact_id(a).is_some() => Some(a.clone()),
            _ => {
                f.push(Finding::new(ID_SYNTAX, &e.rel, "`artefact` must match WF|MK.<APP>.<NAME>@<rev>"));
                ok = false;
                None
            }
        };
        if let (Some(scr), Some(art)) = (&screen, &artefact) {
            if let (Some((sa, sn)), Some((_, aa, an, _))) = (parse_screen_id(scr), parse_artefact_id(art)) {
                if sa != aa || sn != an {
                    f.push(Finding::new(SCHEMA, &e.rel, format!("`artefact` `{art}` does not match `screen` `{scr}`")));
                    ok = false;
                }
            }
        }
        match m.get("approver") {
            Some(CValue::Obj(ap)) => {
                let akeys: Vec<&str> = ap.keys().map(String::as_str).collect();
                if akeys != APPROVER_KEYS
                    || text_ok(ap.get("name")).is_none()
                    || ap.get("principal") != Some(&CValue::Str("human".to_string()))
                {
                    f.push(Finding::new(SCHEMA, &e.rel, "`approver` must be {name, principal: \"human\"}"));
                    ok = false;
                }
            }
            _ => {
                f.push(Finding::new(SCHEMA, &e.rel, "`approver` must be an object"));
                ok = false;
            }
        }
        match m.get("date") {
            Some(CValue::Str(d)) if valid_date(d) => {}
            _ => {
                f.push(Finding::new(SCHEMA, &e.rel, "`date` must be YYYY-MM-DD"));
                ok = false;
            }
        }
        let mut journey_digests = BTreeMap::new();
        match m.get("journey_digests") {
            Some(CValue::Obj(jd)) if !jd.is_empty() => {
                for (map_id, dv) in jd {
                    match dv {
                        CValue::Str(d) if fid_types::Digest::is_valid(d) => {
                            journey_digests.insert(map_id.clone(), d.clone());
                        }
                        _ => {
                            f.push(Finding::new(
                                SCHEMA,
                                &e.rel,
                                format!("`journey_digests.{map_id}` must be `sha256:<64 lowercase hex>`"),
                            ));
                            ok = false;
                        }
                    }
                }
            }
            _ => {
                f.push(Finding::new(SCHEMA, &e.rel, "`journey_digests` must be a non-empty object of map -> sha256"));
                ok = false;
            }
        }
        if ok {
            if let (Some(screen), Some(artefact)) = (screen, artefact) {
                out.push(ApprovalRecord { screen, artefact, journey_digests });
            }
        }
    }
    Ok(out)
}

#[derive(Debug, Clone)]
struct ScreenEntry {
    id: String,
    app: String,
    traces: Vec<(String, u64)>,
    wireframe: Option<String>,
    mockup: Option<String>,
}

fn parse_inventory(text: &str, f: &mut Vec<Finding>) -> Vec<ScreenEntry> {
    let val = match fid_canon::parse_with_limits(text.as_bytes(), MAX_FILE_BYTES as usize, MAX_JSON_DEPTH) {
        Ok(v) => v,
        Err(e) => {
            f.push(Finding::new(SCHEMA, INVENTORY_FILE, format!("not valid JSON: {}", e.code())));
            return Vec::new();
        }
    };
    if let Some(d) = value_defect(&val) {
        f.push(Finding::new(SCHEMA, INVENTORY_FILE, format!("text refused at {d}")));
        return Vec::new();
    }
    let CValue::Obj(m) = &val else {
        f.push(Finding::new(SCHEMA, INVENTORY_FILE, format!("{INVENTORY_FORMAT} must be a JSON object")));
        return Vec::new();
    };
    let keys: Vec<&str> = m.keys().map(String::as_str).collect();
    if keys != TOP_KEYS {
        f.push(Finding::new(
            SCHEMA,
            INVENTORY_FILE,
            format!("{INVENTORY_FORMAT} must have exactly the keys {}", TOP_KEYS.join(", ")),
        ));
    }
    if m.get("format") != Some(&CValue::Str(INVENTORY_FORMAT.to_string())) {
        f.push(Finding::new(SCHEMA, INVENTORY_FILE, format!("`format` must be \"{INVENTORY_FORMAT}\"")));
    }
    let Some(CValue::Arr(arr)) = m.get("screens") else {
        f.push(Finding::new(SCHEMA, INVENTORY_FILE, "`screens` must be an array"));
        return Vec::new();
    };
    if arr.is_empty() || arr.len() > MAX_ITEMS {
        f.push(Finding::new(SCHEMA, INVENTORY_FILE, format!("`screens` must hold 1 to {MAX_ITEMS} entries")));
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut seen_ids = BTreeSet::new();
    for (i, item) in arr.iter().enumerate() {
        let fallback = format!("screens[{i}]");
        let CValue::Obj(e) = item else {
            f.push(Finding::new(SCHEMA, fallback, "screen entry must be a JSON object"));
            continue;
        };
        let raw_id = match e.get("id") {
            Some(CValue::Str(s)) => Some(s.as_str()),
            _ => None,
        };
        let subj = raw_id.unwrap_or(fallback.as_str());
        let ekeys: Vec<&str> = e.keys().map(String::as_str).collect();
        if ekeys != SCREEN_KEYS {
            f.push(Finding::new(
                SCHEMA,
                subj,
                format!("screen entry must have exactly the keys {}", SCREEN_KEYS.join(", ")),
            ));
        }
        let parsed_id = match raw_id {
            Some(s) => match parse_screen_id(s) {
                Some(pair) => {
                    if !seen_ids.insert(s.to_string()) {
                        f.push(Finding::new(SCHEMA, s, "screen id is listed twice"));
                    }
                    Some((s.to_string(), pair.0.to_string(), pair.1.to_string()))
                }
                None => {
                    f.push(Finding::new(
                        ID_SYNTAX,
                        s,
                        "screen ID must match `SCR.<APP>.<NAME>` (APP in SHELL|SETUP|INSP|PROBE|NET|RPT|KIOSK|STN; NAME 2..12 uppercase)",
                    ));
                    None
                }
            },
            None => {
                f.push(Finding::new(SCHEMA, subj, "`id` must be a string"));
                None
            }
        };
        let app = match text_ok(e.get("app")) {
            Some(a) if valid_app_name(&a) => {
                if let Some((_, ref code, _)) = parsed_id {
                    if app_for_code(code) != Some(a.as_str()) {
                        f.push(Finding::new(
                            SCHEMA,
                            subj,
                            format!("`app` `{a}` does not match screen ID app code `{code}`"),
                        ));
                    }
                }
                a
            }
            _ => {
                f.push(Finding::new(SCHEMA, subj, "`app` must be a canonical app name"));
                String::new()
            }
        };
        for field in ["title", "role_layout", "keyboard_path"] {
            if text_ok(e.get(field)).is_none() {
                f.push(Finding::new(SCHEMA, subj, format!("`{field}` must be non-blank text")));
            }
        }
        match e.get("phase") {
            Some(CValue::Str(p)) if PHASES.contains(&p.as_str()) => {}
            _ => f.push(Finding::new(SCHEMA, subj, "`phase` must be one of P0, P1a, P1b, P2, P3, later")),
        }

        let mut traces = Vec::new();
        match e.get("traces") {
            Some(CValue::Arr(ts)) if ts.is_empty() => {
                f.push(Finding::new(UNTRACED, subj, "screen entry has no journey trace"));
            }
            Some(CValue::Arr(ts)) => {
                for tr in ts {
                    let CValue::Obj(tm) = tr else {
                        f.push(Finding::new(SCHEMA, subj, "trace entry must be {map, step}"));
                        continue;
                    };
                    let tkeys: Vec<&str> = tm.keys().map(String::as_str).collect();
                    if tkeys != TRACE_KEYS {
                        f.push(Finding::new(SCHEMA, subj, "trace entry must have exactly keys `map`, `step`"));
                    }
                    match (text_ok(tm.get("map")), json_u64(tm.get("step"))) {
                        (Some(map), Some(step)) => traces.push((map, step)),
                        _ => f.push(Finding::new(
                            SCHEMA,
                            subj,
                            "trace must have string `map` and positive integer `step`",
                        )),
                    }
                }
            }
            _ => f.push(Finding::new(SCHEMA, subj, "`traces` must be an array of {map, step}")),
        }

        let parse_ref = |field: &str, want_kind: &str, f: &mut Vec<Finding>| -> Option<String> {
            match e.get(field) {
                Some(CValue::Str(s)) if s == "none" => None,
                Some(CValue::Str(s)) => match parse_artefact_id(s) {
                    Some((k, aa, an, _)) if k == want_kind => {
                        if let Some((_, ref sa, ref sn)) = parsed_id {
                            if aa != sa || an != sn {
                                f.push(Finding::new(
                                    ID_SYNTAX,
                                    subj,
                                    format!("`{field}` `{s}` does not match screen `{subj}`"),
                                ));
                            }
                        }
                        Some(s.clone())
                    }
                    _ => {
                        f.push(Finding::new(
                            ID_SYNTAX,
                            subj,
                            format!("`{field}` must be `none` or `{want_kind}.<APP>.<NAME>@<rev>`"),
                        ));
                        None
                    }
                },
                _ => {
                    f.push(Finding::new(SCHEMA, subj, format!("`{field}` must be a string")));
                    None
                }
            }
        };
        let wireframe = parse_ref("wireframe", "WF", f);
        let mockup = parse_ref("mockup", "MK", f);

        if let Some((id, _, _)) = parsed_id {
            out.push(ScreenEntry { id, app, traces, wireframe, mockup });
        }
    }
    out
}

/// Result of `fid-design screens-check`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckReport {
    pub screens: usize,
    pub journeys: usize,
    pub approvals: usize,
    pub findings: Vec<Finding>,
}

/// Runs `fid-design screens-check`. `Err` is an unreadable input or bad argument (exit 2).
pub fn check_screens(root: &Path, journeys_dir: &Path, ga_app: Option<&str>) -> Result<CheckReport, String> {
    let root_md = std::fs::symlink_metadata(root).map_err(|e| format!("cannot read --root {}: {e}", root.display()))?;
    if root_md.file_type().is_symlink() || !root_md.is_dir() {
        return Err(format!("--root {} must be a plain directory", root.display()));
    }
    if let Some(app) = ga_app {
        if !valid_app_name(app) {
            return Err(format!("unknown --ga app `{app}`"));
        }
    }
    let inv_text = match review::kind(root, INVENTORY_FILE)? {
        Kind::File => read_text(&root.join(INVENTORY_FILE), MAX_FILE_BYTES)?,
        Kind::Missing => {
            return Err(format!("cannot read {}: file does not exist", root.join(INVENTORY_FILE).display()))
        }
        Kind::Dir | Kind::Other => {
            return Err(format!("{} is not a regular file", root.join(INVENTORY_FILE).display()))
        }
    };

    let mut f = Vec::new();
    let journeys = load_journeys(journeys_dir, &mut f)?;
    let approvals = load_approvals(root, &mut f)?;
    let screens = parse_inventory(&inv_text, &mut f);

    for scr in &screens {
        let mut traced_maps = BTreeSet::new();
        for (map, step) in &scr.traces {
            match journeys.get(map) {
                None => f.push(Finding::new(
                    TRACE_UNKNOWN,
                    &scr.id,
                    format!("trace references unknown journey map `{map}`"),
                )),
                Some(j) => {
                    traced_maps.insert(map.clone());
                    if !j.steps.contains(step) {
                        f.push(Finding::new(
                            TRACE_UNKNOWN,
                            &scr.id,
                            format!("journey map `{map}` has no step `{step}`"),
                        ));
                    }
                }
            }
        }

        for art in scr.wireframe.iter().chain(scr.mockup.iter()) {
            let matching: Vec<&ApprovalRecord> =
                approvals.iter().filter(|a| a.screen == scr.id && a.artefact == *art).collect();
            if matching.is_empty() {
                f.push(Finding::new(
                    APPROVAL_MISSING,
                    format!("{} {art}", scr.id),
                    "no approval record found in screens/approvals/",
                ));
            } else if !traced_maps.is_empty() {
                let any_fresh = matching.iter().any(|rec| {
                    traced_maps
                        .iter()
                        .all(|m| journeys.get(m).is_some_and(|j| rec.journey_digests.get(m) == Some(&j.digest)))
                });
                if !any_fresh {
                    f.push(Finding::new(
                        APPROVAL_STALE,
                        format!("{} {art}", scr.id),
                        "journey map digest changed since approval; re-review required",
                    ));
                }
            }
        }

        if let Some(app) = ga_app {
            if scr.app == app && scr.mockup.is_none() {
                f.push(Finding::new(
                    GA_MOCKUP_MISSING,
                    &scr.id,
                    format!("primary screen `{}` of app `{app}` has no approved mockup", scr.id),
                ));
            }
        }
    }

    f.sort();
    f.dedup();
    Ok(CheckReport { screens: screens.len(), journeys: journeys.len(), approvals: approvals.len(), findings: f })
}

impl CheckReport {
    pub fn exit_code(&self) -> u8 {
        u8::from(!self.findings.is_empty())
    }

    pub fn to_cvalue(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("format".to_string(), CValue::Str(CHECK_REPORT_FORMAT.to_string()));
        m.insert("screens".to_string(), CValue::Num(self.screens as f64));
        m.insert("journeys".to_string(), CValue::Num(self.journeys as f64));
        m.insert("approvals".to_string(), CValue::Num(self.approvals as f64));
        m.insert("findings".to_string(), findings_value(&self.findings));
        m.insert("ok".to_string(), CValue::Bool(self.findings.is_empty()));
        CValue::Obj(m)
    }

    /// RFC 8785 canonical JSON.
    pub fn to_json(&self) -> Result<Vec<u8>, fid_canon::CanonError> {
        fid_canon::to_canonical_bytes(&self.to_cvalue())
    }

    pub fn to_text(&self) -> String {
        let mut s = format!(
            "fid-design screens-check: {} screen(s), {} journey map(s), {} approval(s)\n",
            self.screens, self.journeys, self.approvals
        );
        findings_text(&mut s, &self.findings);
        s
    }
}

struct NodeMeta {
    id: String,
    is_leaf: bool,
    is_claimed: bool,
    cited_artefacts: BTreeSet<String>,
    named_screens: BTreeSet<String>,
}

fn extract_tokens(s: &str, artefacts: &mut BTreeSet<String>, screens: &mut BTreeSet<String>) {
    for raw in s.split(|c: char| !c.is_ascii_alphanumeric() && !matches!(c, '.' | '@')) {
        let tok = raw.trim_matches('.');
        if parse_artefact_id(tok).is_some() {
            artefacts.insert(tok.to_string());
        } else if parse_screen_id(tok).is_some() {
            screens.insert(tok.to_string());
        }
    }
}

fn parse_node_md(text: &str) -> Option<NodeMeta> {
    let mut lines = text.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    let mut id = String::new();
    let mut is_leaf = false;
    let mut is_claimed = false;
    let mut cited_artefacts = BTreeSet::new();
    let mut named_screens = BTreeSet::new();

    for line in &mut lines {
        let t = line.trim();
        if t == "---" {
            break;
        }
        if let Some(rest) = t.strip_prefix("id:") {
            id = rest.trim().to_string();
        } else if let Some(rest) = t.strip_prefix("status:") {
            is_leaf = rest.trim() == "leaf";
        } else if let Some(rest) = t.strip_prefix("claimed_by:") {
            let v = rest.trim();
            is_claimed = !matches!(v, "" | "null" | "~" | "\"\"" | "''" | "[]");
        } else if let Some(rest) = t.strip_prefix("wireframes:") {
            let mut dummy = BTreeSet::new();
            extract_tokens(rest, &mut cited_artefacts, &mut dummy);
        }
    }
    if id.is_empty() {
        return None;
    }

    let mut in_design = false;
    for line in lines {
        let t = line.trim();
        if t.starts_with("## ") {
            in_design = t == "## Design";
            continue;
        }
        if in_design {
            extract_tokens(line, &mut cited_artefacts, &mut named_screens);
        }
    }
    Some(NodeMeta { id, is_leaf, is_claimed, cited_artefacts, named_screens })
}

/// Result of `fid-design screens-gate`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateReport {
    pub leaves_scanned: usize,
    pub approved_artefacts: usize,
    pub findings: Vec<Finding>,
}

/// Runs `fid-design screens-gate` over `<spec_dir>` against approvals under `<root>`.
/// `Err` is an unreadable directory or missing inventory (exit 2).
pub fn check_gate(spec_dir: &Path, root: &Path) -> Result<GateReport, String> {
    let smd =
        std::fs::symlink_metadata(spec_dir).map_err(|e| format!("cannot read --spec {}: {e}", spec_dir.display()))?;
    if smd.file_type().is_symlink() || !smd.is_dir() {
        return Err(format!("--spec {} must be a plain directory", spec_dir.display()));
    }
    let rmd = std::fs::symlink_metadata(root).map_err(|e| format!("cannot read --root {}: {e}", root.display()))?;
    if rmd.file_type().is_symlink() || !rmd.is_dir() {
        return Err(format!("--root {} must be a plain directory", root.display()));
    }
    if review::kind(root, INVENTORY_FILE)? != Kind::File {
        return Err(format!("cannot read {}: file does not exist", root.join(INVENTORY_FILE).display()));
    }

    let mut f = Vec::new();
    let approvals = load_approvals(root, &mut f)?;
    let approved: BTreeSet<String> = approvals.into_iter().map(|a| a.artefact).collect();

    let mut wf = Vec::new();
    let entries = walk::walk(spec_dir, "", &mut wf)?;
    f.extend(wf.into_iter().map(|w| Finding::new(SCHEMA, w.subject, w.detail)));

    let mut leaves_scanned = 0usize;
    for e in entries {
        if e.is_dir || !(e.rel == "NODE.md" || e.rel.ends_with("/NODE.md")) {
            continue;
        }
        if let Some(d) = path_defect(&e.rel) {
            f.push(Finding::new(SCHEMA, &e.rel, format!("path refused: {d}")));
            continue;
        }
        let text = read_text(&e.path, MAX_FILE_BYTES)?;
        let Some(meta) = parse_node_md(&text) else {
            continue;
        };
        if !meta.is_leaf {
            continue;
        }
        leaves_scanned += 1;

        for art in &meta.cited_artefacts {
            if !approved.contains(art) {
                f.push(Finding::new(
                    GATE_UNAPPROVED,
                    format!("{} {art}", meta.id),
                    format!("{}: cited artefact `{art}` has no approval record", e.rel),
                ));
            }
        }
        if meta.is_claimed {
            for scr in &meta.named_screens {
                if let Some((sa, sn)) = parse_screen_id(scr) {
                    let has_wf = meta.cited_artefacts.iter().any(|art| {
                        parse_artefact_id(art).is_some_and(|(k, aa, an, _)| k == "WF" && aa == sa && an == sn)
                    });
                    if !has_wf {
                        f.push(Finding::new(
                            GATE_UNCITED,
                            format!("{} {scr}", meta.id),
                            format!(
                                "{}: claimed leaf names `{scr}` in ## Design without citing `WF.{sa}.{sn}@<rev>`",
                                e.rel
                            ),
                        ));
                    }
                }
            }
        }
    }

    f.sort();
    f.dedup();
    Ok(GateReport { leaves_scanned, approved_artefacts: approved.len(), findings: f })
}

impl GateReport {
    pub fn exit_code(&self) -> u8 {
        u8::from(!self.findings.is_empty())
    }

    pub fn to_cvalue(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("format".to_string(), CValue::Str(GATE_REPORT_FORMAT.to_string()));
        m.insert("leaves_scanned".to_string(), CValue::Num(self.leaves_scanned as f64));
        m.insert("approved_artefacts".to_string(), CValue::Num(self.approved_artefacts as f64));
        m.insert("findings".to_string(), findings_value(&self.findings));
        m.insert("ok".to_string(), CValue::Bool(self.findings.is_empty()));
        CValue::Obj(m)
    }

    /// RFC 8785 canonical JSON.
    pub fn to_json(&self) -> Result<Vec<u8>, fid_canon::CanonError> {
        fid_canon::to_canonical_bytes(&self.to_cvalue())
    }

    pub fn to_text(&self) -> String {
        let mut s = format!(
            "fid-design screens-gate: {} leaf/leaves scanned, {} approved artefact(s)\n",
            self.leaves_scanned, self.approved_artefacts
        );
        findings_text(&mut s, &self.findings);
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_parsers() {
        assert_eq!(parse_screen_id("SCR.RPT.FAIFORM"), Some(("RPT", "FAIFORM")));
        assert_eq!(parse_screen_id("SCR.SHELL.START"), Some(("SHELL", "START")));
        assert_eq!(parse_screen_id("SCR.rpt.form"), None);
        assert_eq!(parse_screen_id("SCR.BOGUS.FORM"), None);
        assert_eq!(parse_screen_id("SCR.RPT.A"), None);
        assert_eq!(parse_screen_id("SCR.RPT.ABCDEFGHIJKLM"), None);

        assert_eq!(parse_artefact_id("WF.RPT.FAIFORM@2"), Some(("WF", "RPT", "FAIFORM", 2)));
        assert_eq!(parse_artefact_id("MK.INSP.ALIGN@1"), Some(("MK", "INSP", "ALIGN", 1)));
        assert_eq!(parse_artefact_id("WF.RPT.FAIFORM@0"), None);
        assert_eq!(parse_artefact_id("WF.RPT.FAIFORM@01"), None);
        assert_eq!(parse_artefact_id("XX.RPT.FAIFORM@1"), None);
    }
}
