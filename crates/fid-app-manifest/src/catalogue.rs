//! The catalogues a manifest is checked against, built in process by an app binary or read
//! from artifact files by the repository check.
//!
//! Files in the `--catalogues` directory (all required unless noted):
//!
//! | File | Format | Owner |
//! |---|---|---|
//! | `registry-manifest.json` | CORE.REG.MAN manifest (output of `registry.show_manifest@1`); each `definition_digest` is checked and each definition parsed | CORE.REG.MAN |
//! | `settings.json` | `fid-settings-manifest/1` (CORE.STO.SET), checked like the in-process registry | CORE.STO.SET |
//! | `panels.json` | `{"format": "fid.panels", "version": 1, "panels": [{"id", ...}]}` | VIZ.SHELL.DOCK.HOST |
//! | `layouts.json` | `{"format": "fid.layouts", "version": 1, "layouts": [{"id", ...}]}` | VIZ.SHELL.DOCK.LAYOUT |
//! | `templates.json` | `{"format": "fid.templates", "version": 1, "templates": [{"id", ...}]}` (provisional) | RPT.MODEL |
//! | `apps.toml` | the ECO.APPS.LIST app list; only the `[[app]]` `id`s are read | ECO.APPS.LIST |
//! | `quantities.json` (optional) | a quantity catalogue for the setting schemas (`Catalogue::from_json`); `GEO.FRAME.UNITS` when absent | CORE.REG |
//!
//! The panel, layout and template exports are read as lists of IDs (fields other than `id`
//! are ignored) until their owners publish schemas. A problem with a file is
//! `MAN-IO <file> <reason>`; all files are read and every problem is reported.

use crate::{code, is_seed_id, is_token, sort_findings, Finding};
use fid_canon::{digest_bytes, parse_with_limits, to_canonical_bytes, CValue};
use fid_registry::{Catalogue, CommandDef, MAX_COMMANDS};
use fid_store::settings::SettingsRegistry;
use fid_types::{CommandId, CommandName, Digest, SideEffect};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::Path;

pub const REGISTRY_FILE: &str = "registry-manifest.json";
pub const SETTINGS_FILE: &str = "settings.json";
pub const PANELS_FILE: &str = "panels.json";
pub const LAYOUTS_FILE: &str = "layouts.json";
pub const TEMPLATES_FILE: &str = "templates.json";
pub const APPS_FILE: &str = "apps.toml";
pub const QUANTITIES_FILE: &str = "quantities.json";

/// Largest panel, layout or template catalogue, or quantity catalogue, in bytes.
const MAX_LIST_BYTES: usize = 4 * 1024 * 1024;
/// Largest `apps.toml` (the ECO.APPS.LIST limit).
const MAX_APPS_BYTES: usize = 64 * 1024;
/// Largest seed file, and most seed files read from one directory.
const MAX_SEED_BYTES: usize = 1024 * 1024;
const MAX_SEED_FILES: usize = 512;
/// Most IDs in one list catalogue.
const MAX_LIST_ITEMS: usize = 100_000;
const LIST_DEPTH: usize = 16;

/// The registry as the manifest sees it: each command's ID and side-effect class.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RegistryView {
    commands: BTreeMap<CommandId, SideEffect>,
    namespaces: BTreeSet<String>,
}

/// A command's namespace: its name before the first dot.
pub(crate) fn namespace_of(id: &CommandId) -> &str {
    id.name().split('.').next().unwrap_or_default()
}

impl RegistryView {
    /// A view of the given commands (an app binary passes its registry in force).
    pub fn from_commands(commands: impl IntoIterator<Item = (CommandId, SideEffect)>) -> RegistryView {
        let commands: BTreeMap<CommandId, SideEffect> = commands.into_iter().collect();
        let namespaces = commands.keys().map(|id| namespace_of(id).to_string()).collect();
        RegistryView { commands, namespaces }
    }

    /// Reads a CORE.REG.MAN manifest (for example `ManifestInForce::bytes`, or the file written
    /// from `registry.show_manifest@1`). Refused: over `fid_registry::MAX_MANIFEST_BYTES`, not
    /// JSON or a duplicate key, other top-level fields than `commands`, `format` (1) and
    /// `quantity_catalogue_digest`, more than `MAX_COMMANDS` commands, a `definition_digest`
    /// that is missing or does not match the definition, a definition `CommandDef` rejects,
    /// and a repeated (ID, major).
    pub fn from_manifest_bytes(bytes: &[u8]) -> Result<RegistryView, &'static str> {
        if bytes.len() > fid_registry::MAX_MANIFEST_BYTES {
            return Err("too_large");
        }
        let v = parse_with_limits(bytes, fid_registry::MAX_MANIFEST_BYTES, fid_canon::MAX_DEPTH).map_err(|_| "json")?;
        let CValue::Obj(top) = v else { return Err("shape") };
        if top.keys().map(String::as_str).collect::<Vec<_>>() != ["commands", "format", "quantity_catalogue_digest"] {
            return Err("shape");
        }
        if top.get("format") != Some(&CValue::Num(f64::from(fid_registry::MANIFEST_FORMAT))) {
            return Err("format");
        }
        let Some(CValue::Arr(items)) = top.get("commands") else { return Err("shape") };
        if items.len() > MAX_COMMANDS {
            return Err("too_many");
        }
        let mut commands = BTreeMap::new();
        for item in items {
            let CValue::Obj(def) = item else { return Err("shape") };
            let mut body = def.clone();
            let stated = match body.remove("definition_digest") {
                Some(CValue::Str(s)) => s,
                _ => return Err("digest"),
            };
            let body = CValue::Obj(body);
            let canonical = to_canonical_bytes(&body).map_err(|_| "digest")?;
            if Digest::from_bytes(digest_bytes(&canonical).0).to_string() != stated {
                return Err("digest");
            }
            let def = CommandDef::from_value(body).map_err(|_| "definition")?;
            let name = CommandName::parse(&def.id).map_err(|_| "definition")?;
            let id = CommandId::new(&name, def.major).map_err(|_| "definition")?;
            if commands.insert(id, def.side_effect).is_some() {
                return Err("duplicate");
            }
        }
        Ok(RegistryView::from_commands(commands))
    }

    /// Every command and its class, sorted by ID.
    pub fn commands(&self) -> &BTreeMap<CommandId, SideEffect> {
        &self.commands
    }

    /// Every namespace that holds at least one command.
    pub fn namespaces(&self) -> &BTreeSet<String> {
        &self.namespaces
    }
}

/// Everything a manifest is validated against.
pub struct Catalogues {
    pub registry: RegistryView,
    pub settings: SettingsRegistry,
    pub panels: BTreeSet<String>,
    pub layouts: BTreeSet<String>,
    pub templates: BTreeSet<String>,
    /// `[[app]]` IDs of `apps.toml`.
    pub apps: BTreeSet<String>,
    /// Seed row IDs; `None` skips the seed check of `known_gaps`.
    pub seeds: Option<BTreeSet<String>>,
}

impl Catalogues {
    /// Reads the catalogue files of `dir` (see the module docs) and, when given, every `*.md`
    /// seed file directly in `seeds`. `Err` lists every unreadable or invalid file, sorted.
    pub fn from_dir(dir: &Path, seeds: Option<&Path>) -> Result<Catalogues, Vec<Finding>> {
        let mut f = Vec::new();
        let mut io = |file: &str, why: &'static str| f.push(Finding::new(code::IO, file, why));

        let registry = match read_bounded(&dir.join(REGISTRY_FILE), fid_registry::MAX_MANIFEST_BYTES) {
            Ok(b) => RegistryView::from_manifest_bytes(&b).map_err(|why| io(REGISTRY_FILE, why)).ok(),
            Err(why) => {
                io(REGISTRY_FILE, why);
                None
            }
        };
        let quantities = match read_optional(&dir.join(QUANTITIES_FILE), MAX_LIST_BYTES) {
            Ok(Some(b)) => Catalogue::from_json(&b).map_err(|_| io(QUANTITIES_FILE, "invalid")).ok(),
            Ok(None) => Catalogue::geo_frame().map_err(|_| io(QUANTITIES_FILE, "builtin")).ok(),
            Err(why) => {
                io(QUANTITIES_FILE, why);
                None
            }
        };
        let settings = match (
            read_bounded(&dir.join(SETTINGS_FILE), fid_store::settings::decl::MAX_MANIFEST_BYTES),
            &quantities,
        ) {
            (Ok(b), Some(q)) => SettingsRegistry::from_manifest(&b, q).map_err(|_| io(SETTINGS_FILE, "invalid")).ok(),
            (Ok(_), None) => None,
            (Err(why), _) => {
                io(SETTINGS_FILE, why);
                None
            }
        };
        let mut list = |file: &'static str, format: &str, key: &str| match read_bounded(&dir.join(file), MAX_LIST_BYTES)
        {
            Ok(b) => read_id_list(&b, format, key).map_err(|why| io(file, why)).ok(),
            Err(why) => {
                io(file, why);
                None
            }
        };
        let panels = list(PANELS_FILE, "fid.panels", "panels");
        let layouts = list(LAYOUTS_FILE, "fid.layouts", "layouts");
        let templates = list(TEMPLATES_FILE, "fid.templates", "templates");
        let apps = match read_bounded(&dir.join(APPS_FILE), MAX_APPS_BYTES) {
            Ok(b) => read_app_ids(&b).map_err(|why| io(APPS_FILE, why)).ok(),
            Err(why) => {
                io(APPS_FILE, why);
                None
            }
        };
        let seeds = match seeds {
            None => Some(None),
            Some(p) => read_seed_dir(p).map(Some).map_err(|why| io("seeds", why)).ok(),
        };
        match (registry, settings, panels, layouts, templates, apps, seeds) {
            (Some(registry), Some(settings), Some(panels), Some(layouts), Some(templates), Some(apps), Some(seeds))
                if f.is_empty() =>
            {
                Ok(Catalogues { registry, settings, panels, layouts, templates, apps, seeds })
            }
            _ => Err(sort_findings(f)),
        }
    }
}

/// Reads a `{"format": <format>, "version": 1, <key>: [{"id": <token>, ...}]}` catalogue and
/// returns its distinct IDs.
pub fn read_id_list(bytes: &[u8], format: &str, key: &str) -> Result<BTreeSet<String>, &'static str> {
    let v = parse_with_limits(bytes, MAX_LIST_BYTES, LIST_DEPTH).map_err(|_| "json")?;
    let CValue::Obj(top) = v else { return Err("shape") };
    let mut expected = vec!["format", "version", key];
    expected.sort_unstable();
    if top.keys().map(String::as_str).collect::<Vec<_>>() != expected {
        return Err("shape");
    }
    if top.get("format") != Some(&CValue::Str(format.to_string())) || top.get("version") != Some(&CValue::Num(1.0)) {
        return Err("format");
    }
    let Some(CValue::Arr(items)) = top.get(key) else { return Err("shape") };
    if items.len() > MAX_LIST_ITEMS {
        return Err("too_many");
    }
    let mut ids = BTreeSet::new();
    for item in items {
        let CValue::Obj(o) = item else { return Err("entry") };
        let Some(CValue::Str(id)) = o.get("id") else { return Err("entry") };
        if !is_token(id) {
            return Err("entry");
        }
        if !ids.insert(id.clone()) {
            return Err("duplicate");
        }
    }
    Ok(ids)
}

/// Reads the `[[app]]` IDs of an ECO.APPS.LIST `apps.toml`. Other fields are that node's.
pub fn read_app_ids(bytes: &[u8]) -> Result<BTreeSet<String>, &'static str> {
    if bytes.len() > MAX_APPS_BYTES {
        return Err("too_large");
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "not_utf8")?;
    let table: toml::Table = text.parse().map_err(|_| "toml")?;
    let Some(toml::Value::Array(rows)) = table.get("app") else { return Err("shape") };
    let mut ids = BTreeSet::new();
    for row in rows {
        let Some(toml::Value::String(id)) = row.get("id") else { return Err("entry") };
        if !is_token(id) {
            return Err("entry");
        }
        if !ids.insert(id.clone()) {
            return Err("duplicate");
        }
    }
    Ok(ids)
}

/// The seed row IDs of a seed file: the first cell of every Markdown table row that is a
/// seed ID (`PW.RPT.01`).
pub fn seed_ids(text: &str) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    for line in text.lines() {
        let Some(rest) = line.trim_start().strip_prefix('|') else { continue };
        let cell = rest.split('|').next().unwrap_or_default().trim();
        if is_seed_id(cell) {
            ids.insert(cell.to_string());
        }
    }
    ids
}

fn read_seed_dir(dir: &Path) -> Result<BTreeSet<String>, &'static str> {
    let entries = std::fs::read_dir(dir).map_err(|_| "unreadable")?;
    let mut names = BTreeSet::new();
    for entry in entries {
        let entry = entry.map_err(|_| "unreadable")?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.ends_with(".md") {
            names.insert(name.to_string());
            if names.len() > MAX_SEED_FILES {
                return Err("too_many");
            }
        }
    }
    let mut ids = BTreeSet::new();
    for name in names {
        let bytes = read_bounded(&dir.join(&name), MAX_SEED_BYTES)?;
        let text = std::str::from_utf8(&bytes).map_err(|_| "not_utf8")?;
        ids.extend(seed_ids(text));
    }
    Ok(ids)
}

/// Reads a regular file of at most `max` bytes.
pub(crate) fn read_bounded(path: &Path, max: usize) -> Result<Vec<u8>, &'static str> {
    let file = std::fs::File::open(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            "missing"
        } else {
            "unreadable"
        }
    })?;
    let meta = file.metadata().map_err(|_| "unreadable")?;
    if !meta.is_file() {
        return Err("not_a_file");
    }
    if meta.len() > max as u64 {
        return Err("too_large");
    }
    let mut buf = Vec::new();
    file.take(max as u64 + 1).read_to_end(&mut buf).map_err(|_| "unreadable")?;
    if buf.len() > max {
        return Err("too_large");
    }
    Ok(buf)
}

/// [`read_bounded`], with `Ok(None)` when the file does not exist.
fn read_optional(path: &Path, max: usize) -> Result<Option<Vec<u8>>, &'static str> {
    match read_bounded(path, max) {
        Ok(b) => Ok(Some(b)),
        Err("missing") => Ok(None),
        Err(why) => Err(why),
    }
}
