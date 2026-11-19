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
//! (REQ-CORE.REG.DEF-003). With the `GEO.FRAME.UNITS` catalogue alone the workspace does not
//! export today, because `fid-jobs` declares `x-quantity: "time"` and a `number` field
//! without `x-quantity`; that is reported as typed VAL errors, never skipped (see the leaf's
//! Open questions).

use fid_canon::CValue;
use fid_registry::manifest::{export_with, Export, ManifestError, ManifestInForce};
use fid_registry::{Catalogue, CommandDecl, CommandDef, DefError};
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

/// Every crate on main that declares commands, sorted by crate name.
pub const SOURCES: [Source; 5] = [
    Source { krate: "fid-align", defs: fid_align::command_defs },
    Source { krate: "fid-dep", defs: dep_defs },
    Source { krate: "fid-jobs", defs: fid_jobs::command_defs },
    Source { krate: "fid-registry", defs: registry_defs },
    Source { krate: "fid-report", defs: fid_report::doc::command_defs },
];

/// Why the build failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildError {
    /// A crate's definition function failed; its DEF code (`declaration_build_failed`).
    Declaration { krate: &'static str, code: &'static str },
    /// The export (validation included) failed.
    Manifest(ManifestError),
}

impl BuildError {
    /// Stable code: `declaration_build_failed` or the manifest error's code.
    pub fn code(&self) -> &'static str {
        match self {
            BuildError::Declaration { .. } => "declaration_build_failed",
            BuildError::Manifest(e) => e.code(),
        }
    }

    /// JSON report: the manifest error's form, or `{"code", "crate", "def_code"}`.
    pub fn to_value(&self) -> CValue {
        match self {
            BuildError::Manifest(e) => e.to_value(),
            BuildError::Declaration { krate, code } => {
                let mut o = BTreeMap::new();
                o.insert("code".to_string(), CValue::Str(self.code().to_string()));
                o.insert("crate".to_string(), CValue::Str((*krate).to_string()));
                o.insert("def_code".to_string(), CValue::Str((*code).to_string()));
                CValue::Obj(o)
            }
        }
    }
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BuildError::Declaration { krate, code } => write!(f, "declaration_build_failed: {krate}: {code}"),
            BuildError::Manifest(e) => e.fmt(f),
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
