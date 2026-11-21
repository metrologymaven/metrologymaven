//! The registry build step (spec node `CORE.REG.MAN`): collects the command declarations of
//! every crate that declares commands, and exports them as the one canonical Registry
//! Manifest through `fid_registry::manifest::export_with`, which validates every declaration
//! with `CORE.REG.VAL` first. There is no path around that validation.
//!
//! `fid-registry` cannot depend on the crates that declare commands (they depend on it), so
//! the collection lives here, above them. [`SOURCES`] lists every crate on main that exposes
//! command definitions, sorted by crate name; a crate that adds commands adds one entry. A
//! crate whose `command_defs()` fails is `declaration_build_failed` naming that crate; a
//! command ID or tool name declared twice, in one crate or across crates, is VAL's
//! `duplicate_command` / `tool_name_collision` inside `declarations_invalid`.
//!
//! The quantity catalogue is an input: the registry defines no kind or unit
//! (REQ-CORE.REG.DEF-003). A crate that needs kinds beyond `GEO.FRAME.UNITS` (`length`,
//! `angle`) publishes them as a quantity-catalogue fragment through one function next to its
//! declarations ([`QUANTITY_SOURCES`]). [`merge_catalogues`] adds the fragments to
//! `Catalogue::geo_frame()` in that fixed crate order, add-only, through
//! `Catalogue::from_json`, and refuses with a typed error a fragment that redefines a GEO kind
//! (`geo_kind_redefined`), a kind two crates define differently (`quantity_kind_conflict`)
//! and anything the catalogue checks refuse (`catalogue_invalid`). [`workspace_catalogue`] is
//! the merged catalogue whose digest the workspace manifest carries.
//!
//! Settings (`CORE.STO.SET`) are collected the same way: [`SETTINGS_SOURCES`] lists every
//! crate that declares settings with `fid_settings!`, sorted by crate name, and
//! [`workspace_settings`] checks them all into one `SettingsRegistry` (against the merged
//! catalogue), whose `manifest()` is the workspace `settings-manifest.json`. A refused
//! declaration or a key declared by two crates is `settings_invalid`.

#[cfg(feature = "test-support")]
pub mod test_support;

use fid_canon::CValue;
use fid_registry::catalogue::{is_kind_name, MAX_CATALOGUE_BYTES};
use fid_registry::manifest::{export_with, Export, ManifestError, ManifestInForce};
use fid_registry::{Catalogue, CommandDecl, CommandDef, DefError, QuantityCatalogue};
use fid_store::settings::{SettingSource, SettingsRegistry};
use std::collections::BTreeMap;
use std::fmt;

/// A crate's definition function.
pub type DefsFn = fn() -> Result<Vec<CommandDef>, DefError>;

/// One crate that declares commands.
#[derive(Clone, Copy)]
pub struct Source {
    /// The crate name, for errors.
    pub krate: &'static str,
    /// Its command definitions.
    pub defs: DefsFn,
}

impl fmt::Debug for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Source").field("krate", &self.krate).finish_non_exhaustive()
    }
}

fn dep_defs() -> Result<Vec<CommandDef>, DefError> {
    Ok(vec![fid_dep::command_def()?, fid_dep::set_command_def()?])
}

fn registry_defs() -> Result<Vec<CommandDef>, DefError> {
    Ok(vec![fid_registry::manifest::command_def()?])
}

/// The four settings commands of `CORE.STO.SET`, declared with the registry macros (whose
/// definitions are built at compile time and cannot fail); VAL still checks them at export.
fn store_defs() -> Result<Vec<CommandDef>, DefError> {
    Ok(fid_store::settings::declarations().into_iter().map(|d| d.def).collect())
}

/// The two adoption commands of `AUTO.REC.ADOPT` (`human_only`, request-only on MCP) and the
/// ten lifecycle commands of `AUTO.REC.LIFE` (register, unregister and open_draft W2;
/// release and deprecate W3, `human_only`, request-only on MCP), declared with the registry
/// macros.
fn program_rec_defs() -> Result<Vec<CommandDef>, DefError> {
    let adopt = fid_program_rec::adopt::declarations().into_iter().map(|d| d.def);
    let life = fid_program_rec::life::declarations().into_iter().map(|d| d.def);
    Ok(adopt.chain(life).collect())
}

/// The four branch commands of `CORE.JRN.BR.NAME`, `journal.diff_branch@1` of
/// `CORE.JRN.BR.DIFF` and the three promotion commands of `CORE.JRN.BR.PROMO` (handlers in
/// `fid-journal-cmd`).
fn journal_defs() -> Result<Vec<CommandDef>, DefError> {
    let mut defs = fid_journal::branch::command_defs()?;
    defs.extend(fid_journal::diff::command_defs()?);
    defs.extend(fid_journal::promote::command_defs()?);
    Ok(defs)
}

/// The three model commands of `UNC.MODEL.GRAPH.STORE` (`unc.create_model@1` W2,
/// `unc.validate_model@1` R1, `unc.show_model@1` R0), declared with the registry macros.
fn unc_defs() -> Result<Vec<CommandDef>, DefError> {
    Ok(fid_unc::model_store::declarations().into_iter().map(|d| d.def).collect())
}

/// Every crate on main that declares commands, sorted by crate name. `fid-bus` declares the
/// four request commands of CORE.BUS.POL.PROJ (`request.list_requests@1`,
/// `request.withdraw_request@1`, `request.reject_request@1`, internal `request.file_request@1`).
/// `fid-journal` declares the four branch commands of CORE.JRN.BR.NAME and the branch diff of
/// CORE.JRN.BR.DIFF (handlers in `fid-journal-cmd`). `fid-profile-cmd` declares
/// `sign.set_profile@1` and `sign.show_profile@1` of TRUST.PROFILE.SELECT. `fid-mig` declares
/// `migrate.list_sources@1` of MIG.SRC.REG.
pub const SOURCES: [Source; 15] = [
    Source { krate: "fid-align", defs: fid_align::command_defs },
    Source { krate: "fid-bus", defs: fid_bus::policy::projection::command_defs },
    Source { krate: "fid-dep", defs: dep_defs },
    Source { krate: "fid-geo-cmd", defs: fid_geo_cmd::command_defs },
    Source { krate: "fid-jobs", defs: fid_jobs::command_defs },
    Source { krate: "fid-journal", defs: journal_defs },
    Source { krate: "fid-mig", defs: fid_mig::src::reg::command_defs },
    Source { krate: "fid-ors", defs: fid_ors::command_defs },
    Source { krate: "fid-profile-cmd", defs: fid_profile_cmd::command_defs },
    Source { krate: "fid-program-rec", defs: program_rec_defs },
    Source { krate: "fid-project-cmd", defs: fid_project_cmd::command_defs },
    Source { krate: "fid-registry", defs: registry_defs },
    Source { krate: "fid-report", defs: fid_report::doc::command_defs },
    Source { krate: "fid-store", defs: store_defs },
    Source { krate: "fid-unc", defs: unc_defs },
];

/// A crate's settings function, generated by `fid_store::fid_settings!`.
pub type SettingsFn = fn() -> Vec<SettingSource>;

/// One crate that declares settings.
#[derive(Clone, Copy)]
pub struct SettingsSource {
    /// The crate name, for errors.
    pub krate: &'static str,
    /// Its setting declarations.
    pub settings: SettingsFn,
}

impl fmt::Debug for SettingsSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SettingsSource").field("krate", &self.krate).finish_non_exhaustive()
    }
}

/// Every crate on main that declares settings, sorted by crate name: `fid-bus` declares the
/// approved file roots `bus.fs_roots` (CORE.BUS.POL.FS); `fid-catalog` the results catalog
/// location `stats.catalog_path` (RPT.CATALOG.STORE.SCHEMA); `fid-journal` the Activity Log
/// rotation settings `journal.activity_log_*` (CORE.JRN.STREAM.ALOG).
pub const SETTINGS_SOURCES: [SettingsSource; 3] = [
    SettingsSource { krate: "fid-bus", settings: fid_bus::policy::fs::fs_settings },
    SettingsSource { krate: "fid-catalog", settings: fid_catalog::catalog_settings },
    SettingsSource { krate: "fid-journal", settings: fid_journal::stream::activity_settings },
];

/// A crate's quantity-catalogue fragment function.
pub type KindsFn = fn() -> &'static str;

/// One crate that publishes quantity kinds beyond `GEO.FRAME.UNITS`.
#[derive(Clone, Copy)]
pub struct QuantitySource {
    /// The crate name, for errors.
    pub krate: &'static str,
    /// Its fragment document: `{"format": "fid-quantity-kinds/1", "kinds": [...]}`, the kinds
    /// in the `CORE.REG.DEF.SCHEMA` catalogue form, sorted strictly by name.
    pub kinds: KindsFn,
}

impl fmt::Debug for QuantitySource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("QuantitySource").field("krate", &self.krate).finish_non_exhaustive()
    }
}

/// The `format` of a quantity-catalogue fragment.
pub const KINDS_FORMAT: &str = "fid-quantity-kinds/1";

/// Every crate on main that publishes quantity kinds, sorted by crate name; the merge order.
pub const QUANTITY_SOURCES: [QuantitySource; 1] =
    [QuantitySource { krate: "fid-jobs", kinds: fid_jobs::quantity_catalogue }];

/// Why the build failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildError {
    /// A crate's definition function failed; its DEF code (`declaration_build_failed`).
    Declaration { krate: &'static str, code: &'static str },
    /// The export (validation included) failed.
    Manifest(ManifestError),
    /// A fragment names a `GEO.FRAME.UNITS` kind (`geo_kind_redefined`); `kind` comes from
    /// the GEO table, never from input.
    GeoKindRedefined { krate: &'static str, kind: String },
    /// Two crates define one kind differently (`quantity_kind_conflict`). `kind` has passed
    /// the kind-name check (`^[a-z][a-z0-9_]{0,31}$`).
    KindConflict { kind: String, first: &'static str, second: &'static str },
    /// A fragment is malformed, or the catalogue with its kinds added is refused
    /// (`catalogue_invalid`); `catalogue_code` is `bad_catalogue` or the `CatalogueError` code.
    Catalogue { krate: &'static str, catalogue_code: &'static str },
    /// The settings declarations are refused (`settings_invalid`): the first refusal, sorted
    /// by key; `key` only when it is well formed, `settings_code` the `CORE.STO.SET` code.
    Settings { key: Option<String>, settings_code: &'static str },
}

impl BuildError {
    /// Stable code: `declaration_build_failed`, `geo_kind_redefined`,
    /// `quantity_kind_conflict`, `catalogue_invalid`, `settings_invalid` or the manifest
    /// error's code.
    pub fn code(&self) -> &'static str {
        match self {
            BuildError::Declaration { .. } => "declaration_build_failed",
            BuildError::Manifest(e) => e.code(),
            BuildError::GeoKindRedefined { .. } => "geo_kind_redefined",
            BuildError::KindConflict { .. } => "quantity_kind_conflict",
            BuildError::Catalogue { .. } => "catalogue_invalid",
            BuildError::Settings { .. } => "settings_invalid",
        }
    }

    /// JSON report: the manifest error's form, or `{"code", "crate", ...}` with
    /// `def_code`, `kind`, `crates` or `catalogue_code`.
    pub fn to_value(&self) -> CValue {
        let s = |x: &str| CValue::Str(x.to_string());
        let mut o = BTreeMap::new();
        o.insert("code".to_string(), s(self.code()));
        match self {
            BuildError::Manifest(e) => return e.to_value(),
            BuildError::Declaration { krate, code } => {
                o.insert("crate".to_string(), s(krate));
                o.insert("def_code".to_string(), s(code));
            }
            BuildError::GeoKindRedefined { krate, kind } => {
                o.insert("crate".to_string(), s(krate));
                o.insert("kind".to_string(), s(kind));
            }
            BuildError::KindConflict { kind, first, second } => {
                o.insert("crates".to_string(), CValue::Arr(vec![s(first), s(second)]));
                o.insert("kind".to_string(), s(kind));
            }
            BuildError::Catalogue { krate, catalogue_code } => {
                o.insert("catalogue_code".to_string(), s(catalogue_code));
                o.insert("crate".to_string(), s(krate));
            }
            BuildError::Settings { key, settings_code } => {
                if let Some(k) = key {
                    o.insert("key".to_string(), s(k));
                }
                o.insert("settings_code".to_string(), s(settings_code));
            }
        }
        CValue::Obj(o)
    }
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BuildError::Declaration { krate, code } => write!(f, "declaration_build_failed: {krate}: {code}"),
            BuildError::Manifest(e) => e.fmt(f),
            BuildError::GeoKindRedefined { krate, kind } => write!(f, "geo_kind_redefined: {krate}: {kind}"),
            BuildError::KindConflict { kind, first, second } => {
                write!(f, "quantity_kind_conflict: {kind}: {first}, {second}")
            }
            BuildError::Catalogue { krate, catalogue_code } => {
                write!(f, "catalogue_invalid: {krate}: {catalogue_code}")
            }
            BuildError::Settings { key, settings_code } => match key {
                Some(k) => write!(f, "settings_invalid: {k}: {settings_code}"),
                None => write!(f, "settings_invalid: {settings_code}"),
            },
        }
    }
}

impl std::error::Error for BuildError {}

/// The declarations of `sources`, in source order.
pub fn declarations(sources: &[Source]) -> Result<Vec<CommandDecl>, BuildError> {
    let mut out = Vec::new();
    for s in sources {
        let defs = (s.defs)().map_err(|e| BuildError::Declaration { krate: s.krate, code: e.code() })?;
        out.extend(defs.iter().map(CommandDecl::from_def));
    }
    Ok(out)
}

/// The kinds of one fragment, as (name, kind object), in document order. The fragment is
/// exactly `{"format", "kinds"}` with `format` [`KINDS_FORMAT`], at most 1 MiB and 5 levels
/// deep, no duplicate key; each kind an object whose `kind` passes the kind-name check, names
/// strictly ascending (byte order). The kind objects themselves are checked by
/// `Catalogue::from_json` once added.
fn fragment_kinds(src: &QuantitySource) -> Result<Vec<(String, CValue)>, BuildError> {
    let bad = |catalogue_code: &'static str| BuildError::Catalogue { krate: src.krate, catalogue_code };
    let bytes = (src.kinds)().as_bytes();
    if bytes.len() > MAX_CATALOGUE_BYTES {
        return Err(bad("catalogue_too_large"));
    }
    let v = fid_canon::parse_with_limits(bytes, MAX_CATALOGUE_BYTES, 5).map_err(|e| bad(e.code()))?;
    let CValue::Obj(root) = v else { return Err(bad("bad_catalogue")) };
    let format_ok = matches!(root.get("format"), Some(CValue::Str(f)) if f == KINDS_FORMAT);
    let Some(CValue::Arr(kinds)) = root.get("kinds") else { return Err(bad("bad_catalogue")) };
    if root.len() != 2 || !format_ok {
        return Err(bad("bad_catalogue"));
    }
    let mut out: Vec<(String, CValue)> = Vec::with_capacity(kinds.len());
    for k in kinds {
        let name = match k {
            CValue::Obj(o) => match o.get("kind") {
                Some(CValue::Str(n)) if is_kind_name(n) => n.clone(),
                _ => return Err(bad("bad_catalogue")),
            },
            _ => return Err(bad("bad_catalogue")),
        };
        if out.last().is_some_and(|(prev, _)| *prev >= name) {
            return Err(bad("bad_catalogue"));
        }
        out.push((name, k.clone()));
    }
    Ok(out)
}

/// `Catalogue::geo_frame()` with the kinds of `sources` added, in source order, add-only.
///
/// A fragment kind named like a GEO kind is `geo_kind_redefined` (even when identical); a kind
/// an earlier crate defined with another value is `quantity_kind_conflict` (an identical
/// definition is accepted once); after each crate the whole catalogue is re-read with
/// `Catalogue::from_json`, so its shape rules, the GEO check and symbol uniqueness across
/// kinds apply, and a refusal is `catalogue_invalid` naming that crate. The result does not
/// depend on the order of non-conflicting sources: the catalogue is sorted by kind name.
pub fn merge_catalogues(sources: &[QuantitySource]) -> Result<Catalogue, BuildError> {
    let geo = Catalogue::geo_frame()
        .map_err(|e| BuildError::Catalogue { krate: "fid-geo-frame", catalogue_code: e.code() })?;
    let geo_names: Vec<String> = geo.kind_names().into_iter().map(str::to_string).collect();
    let CValue::Obj(mut doc) = geo.to_value() else {
        return Err(BuildError::Catalogue { krate: "fid-geo-frame", catalogue_code: "bad_catalogue" });
    };
    let mut kinds: BTreeMap<String, (CValue, &'static str)> = BTreeMap::new();
    if let Some(CValue::Arr(base)) = doc.get("kinds") {
        for k in base {
            if let CValue::Obj(o) = k {
                if let Some(CValue::Str(n)) = o.get("kind") {
                    kinds.insert(n.clone(), (k.clone(), "fid-geo-frame"));
                }
            }
        }
    }
    let mut merged = geo;
    for src in sources {
        for (name, value) in fragment_kinds(src)? {
            if let Some(g) = geo_names.iter().find(|g| **g == name) {
                return Err(BuildError::GeoKindRedefined { krate: src.krate, kind: g.clone() });
            }
            match kinds.get(&name) {
                Some((have, _)) if *have == value => {}
                Some(&(_, first)) => return Err(BuildError::KindConflict { kind: name, first, second: src.krate }),
                None => {
                    kinds.insert(name, (value, src.krate));
                }
            }
        }
        doc.insert("kinds".to_string(), CValue::Arr(kinds.values().map(|(v, _)| v.clone()).collect()));
        let bad = |catalogue_code: &'static str| BuildError::Catalogue { krate: src.krate, catalogue_code };
        let bytes = fid_canon::to_canonical_bytes(&CValue::Obj(doc.clone())).map_err(|e| bad(e.code()))?;
        merged = Catalogue::from_json(&bytes).map_err(|e| bad(e.code()))?;
    }
    Ok(merged)
}

/// [`merge_catalogues`] over [`QUANTITY_SOURCES`]: the catalogue the workspace manifest is
/// validated against and whose digest it carries.
pub fn workspace_catalogue() -> Result<Catalogue, BuildError> {
    merge_catalogues(&QUANTITY_SOURCES)
}

/// Collects, validates and exports `sources` against `catalogue`.
pub fn build(sources: &[Source], catalogue: &Catalogue) -> Result<Export, BuildError> {
    export_with(&declarations(sources)?, catalogue).map_err(BuildError::Manifest)
}

/// [`build`] over [`SOURCES`].
pub fn build_workspace(catalogue: &Catalogue) -> Result<Export, BuildError> {
    build(&SOURCES, catalogue)
}

/// The manifest in force for `registry.show_manifest@1`, built from [`SOURCES`].
pub fn manifest_in_force(catalogue: Catalogue) -> Result<ManifestInForce, BuildError> {
    ManifestInForce::new(&declarations(&SOURCES)?, catalogue).map_err(BuildError::Manifest)
}

/// The settings of `sources`, checked together against `catalogue` (`CORE.STO.SET` REQ-001):
/// every declaration, and no key declared twice across crates.
pub fn settings(sources: &[SettingsSource], catalogue: &dyn QuantityCatalogue) -> Result<SettingsRegistry, BuildError> {
    let all: Vec<SettingSource> = sources.iter().flat_map(|s| (s.settings)()).collect();
    SettingsRegistry::build_with(&all, catalogue).map_err(|errors| {
        let first = errors.into_iter().next();
        BuildError::Settings {
            key: first.as_ref().and_then(|e| e.key.clone()),
            settings_code: first.map(|e| e.code).unwrap_or("settings_invalid"),
        }
    })
}

/// [`settings`] over [`SETTINGS_SOURCES`]: the workspace settings registry, whose
/// `manifest()` is `settings-manifest.json`.
pub fn workspace_settings(catalogue: &Catalogue) -> Result<SettingsRegistry, BuildError> {
    settings(&SETTINGS_SOURCES, catalogue)
}
