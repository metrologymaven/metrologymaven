//! Every command definition declared in the workspace, for tests (feature `test-support`).
//!
//! [`all_defs`] is [`crate::SOURCES`] plus the crates that declare commands but are not in
//! `SOURCES` yet: the egress command of `fid-bus`, RDL, GDT (record, status, datum, DRF, diameter and radius checks), the
//! package reader, the signature check of `fid-sign-core` (TRUST.SIGN.TROOT), report binding, and the two export and two import commands of `fid-io-reg`
//! (IO.REG.ENV.EXPORT, IO.REG.ENV.IMPORT; built over a registry with one format in each
//! direction, so they have a parameter schema). Their commands are not in `SOURCES` because
//! they are built over the format registry, which the build step does not have. Shared by this crate's
//! bus-validation test and by `fid-mcp`'s registry walk (AUTO.MCP.PROJ), so both see the same
//! commands. Never enabled outside `[dev-dependencies]`.

use crate::SOURCES;
use fid_canon::CValue;
use fid_io_reg::env::export::command::{export_file_def, format_def};
use fid_io_reg::env::import::command::{format_def as import_format_def, import_file_def};
use fid_io_reg::{Direction, FormatDescriptor, FormatId, Origin, Phase, Registry, VersionRange};
use fid_registry::CommandDef;
use std::collections::BTreeMap;

/// The export format the export commands are built over.
pub const TEST_EXPORT_FORMAT: &str = "fid-test-points";

fn err(what: &str, e: impl std::fmt::Debug) -> String {
    format!("{what}: {e:?}")
}

/// A registry with one export format, so the export commands have a parameter schema.
pub fn export_registry() -> Result<Registry, String> {
    let mut r = Registry::new();
    r.register(test_format(Direction::Export)?).map_err(|e| err("register", e))?;
    Ok(r)
}

/// [`export_registry`] plus the same format for import, so the import commands have a
/// parameter schema too.
pub fn io_registry() -> Result<Registry, String> {
    let mut r = export_registry()?;
    r.register(test_format(Direction::Import)?).map_err(|e| err("register", e))?;
    Ok(r)
}

/// The test format [`TEST_EXPORT_FORMAT`] in one direction.
fn test_format(direction: Direction) -> Result<FormatDescriptor, String> {
    FormatDescriptor::new(
        FormatId::new(TEST_EXPORT_FORMAT).map_err(|e| err("format id", e))?,
        VersionRange::new("1", "1").map_err(|e| err("version range", e))?,
        Vec::new(),
        vec!["ftp".to_string()],
        CValue::Obj(BTreeMap::new()),
        Phase::new(0).ok_or_else(|| "phase 0 is not valid".to_string())?,
        Origin::FirstParty,
        direction,
    )
    .map_err(|e| err("format descriptor", e))
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
            fid_gdt::drf::build::build_command_def().map_err(|e| err("fid-gdt build_drf", e))?,
            fid_gdt::drf::build::get_command_def().map_err(|e| err("fid-gdt get_drf", e))?,
            fid_gdt::dim::radial::diameter_command_def().map_err(|e| err("fid-gdt eval_diameter", e))?,
            fid_gdt::dim::radial::radius_command_def().map_err(|e| err("fid-gdt eval_radius", e))?,
        ],
    );
    extra("fid-pkg", fid_pkg::reader::declarations().into_iter().map(|d| d.def).collect());
    extra("fid-sign-core", fid_sign_core::troot::declarations().into_iter().map(|d| d.def).collect());
    extra("fid-verify", fid_verify::command::command_defs());
    extra("fid-report (bind)", fid_report::bind::declarations().into_iter().map(|d| d.def).collect());
    let reg = io_registry()?;
    let format = FormatId::new(TEST_EXPORT_FORMAT).map_err(|e| err("format id", e))?;
    extra(
        "fid-io-reg",
        vec![
            export_file_def(&reg).map_err(|e| err("export_file", e))?,
            format_def(&reg, &format, &[]).map_err(|e| err("format_def", e))?,
            import_file_def(&reg).map_err(|e| err("import_file", e))?,
            import_format_def(&reg, &format, &[]).map_err(|e| err("import format_def", e))?,
        ],
    );
    Ok(defs)
}
