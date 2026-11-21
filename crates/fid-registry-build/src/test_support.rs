//! Every command definition declared in the workspace, for tests (feature `test-support`).
//!
//! [`all_defs`] is [`crate::SOURCES`] plus the crates that declare commands but are not in
//! `SOURCES` yet: the egress command of `fid-bus`, RDL, GDT (record, status, datum), the
//! package reader, report binding and the two export and two import commands of `fid-io-reg`
//! (built over a registry with one export and one import format, so they have a parameter
//! schema). Shared by this crate's
//! bus-validation test and by `fid-mcp`'s registry walk (AUTO.MCP.PROJ), so both see the same
//! commands. Never enabled outside `[dev-dependencies]`.

use crate::SOURCES;
use fid_canon::CValue;
use fid_io_reg::env::export::command::{export_file_def, format_def};
use fid_io_reg::env::import::command as import;
use fid_io_reg::{Direction, FormatDescriptor, FormatId, Origin, Phase, Registry, VersionRange};
use fid_registry::CommandDef;
use std::collections::BTreeMap;

/// The export format the export commands are built over.
pub const TEST_EXPORT_FORMAT: &str = "fid-test-points";

fn err(what: &str, e: impl std::fmt::Debug) -> String {
    format!("{what}: {e:?}")
}

/// A registry with one export and one import format, so the export and import commands have
/// a parameter schema.
pub fn export_registry() -> Result<Registry, String> {
    let mut r = Registry::new();
    let d = FormatDescriptor::new(
        FormatId::new(TEST_EXPORT_FORMAT).map_err(|e| err("format id", e))?,
        VersionRange::new("1", "1").map_err(|e| err("version range", e))?,
        Vec::new(),
        vec!["ftp".to_string()],
        CValue::Obj(BTreeMap::new()),
        Phase::new(0).ok_or_else(|| "phase 0 is not valid".to_string())?,
        Origin::FirstParty,
        Direction::Export,
    )
    .map_err(|e| err("format descriptor", e))?;
    r.register(d).map_err(|e| err("register", e))?;
    let i = FormatDescriptor::new(
        FormatId::new(TEST_EXPORT_FORMAT).map_err(|e| err("format id", e))?,
        VersionRange::new("1", "1").map_err(|e| err("version range", e))?,
        Vec::new(),
        vec!["ftp".to_string()],
        CValue::Obj(BTreeMap::new()),
        Phase::new(0).ok_or_else(|| "phase 0 is not valid".to_string())?,
        Origin::FirstParty,
        Direction::Import,
    )
    .map_err(|e| err("format descriptor", e))?;
    r.register(i).map_err(|e| err("register", e))?;
    Ok(r)
}

/// Every declared command of the workspace, with the crate that declares it.
pub fn all_defs() -> Result<Vec<(String, CommandDef)>, String> {
    let mut defs: Vec<(String, CommandDef)> = Vec::new();
    for s in SOURCES.iter() {
        for d in (s.defs)().map_err(|e| err(s.krate, e))? {
            defs.push((s.krate.to_string(), d));
        }
    }
    let mut extra = |krate: &str, ds: Vec<CommandDef>| defs.extend(ds.into_iter().map(|d| (krate.to_string(), d)));
    extra("fid-bus (egress)", fid_bus::policy::egress::command_defs().map_err(|e| err("fid-bus (egress)", e))?);
    extra("fid-rdl", fid_rdl::command_defs().map_err(|e| err("fid-rdl", e))?);
    extra(
        "fid-gdt",
        vec![
            fid_gdt::chars::record::command_def().map_err(|e| err("fid-gdt record", e))?,
            fid_gdt::dmt_flow::status::command_def().map_err(|e| err("fid-gdt status", e))?,
            fid_gdt::drf::datum::command_def().map_err(|e| err("fid-gdt datum", e))?,
        ],
    );
    extra("fid-pkg", fid_pkg::reader::declarations().into_iter().map(|d| d.def).collect());
    extra("fid-report (bind)", fid_report::bind::declarations().into_iter().map(|d| d.def).collect());
    let reg = export_registry()?;
    let format = FormatId::new(TEST_EXPORT_FORMAT).map_err(|e| err("format id", e))?;
    extra(
        "fid-io-reg",
        vec![
            export_file_def(&reg).map_err(|e| err("export_file", e))?,
            format_def(&reg, &format, &[]).map_err(|e| err("format_def", e))?,
            import::import_file_def(&reg).map_err(|e| err("import_file", e))?,
            import::format_def(&reg, &format, &[]).map_err(|e| err("import format_def", e))?,
        ],
    );
    Ok(defs)
}
