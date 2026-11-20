//! Every command definition in the workspace compiles for the bus's parameter validation
//! (CORE.BUS.PIPE.VALID): its parameter schema is accepted by `fid_bus::validate::Compiled`,
//! so no registered command refuses every call with `validation/schema`. Asked for in the
//! re-review of #341, after a too-strict pattern rule refused real command patterns. Covers
//! every crate in `SOURCES` and the crates that declare commands but are not in `SOURCES`
//! yet (GDT, RDL, the package reader, export, egress). The expectation (zero compile errors)
//! is the spec's rule, not a recorded run.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::validate::Compiled;
use fid_canon::CValue;
use fid_io_reg::env::export::command::{export_file_def, format_def};
use fid_io_reg::{Direction, FormatDescriptor, FormatId, Origin, Phase, Registry, VersionRange};
use fid_registry::CommandDef;
use fid_registry_build::SOURCES;
use std::collections::BTreeMap;

/// A registry with one export format, so the export commands have a parameter schema.
fn export_registry() -> Registry {
    let mut r = Registry::new();
    let d = FormatDescriptor::new(
        FormatId::new("fid-test-points").unwrap(),
        VersionRange::new("1", "1").unwrap(),
        Vec::new(),
        vec!["ftp".to_string()],
        CValue::Obj(BTreeMap::new()),
        Phase::new(0).unwrap(),
        Origin::FirstParty,
        Direction::Export,
    )
    .unwrap();
    r.register(d).unwrap();
    r
}

fn all_defs() -> Vec<(String, CommandDef)> {
    let mut defs: Vec<(String, CommandDef)> = Vec::new();
    for s in SOURCES.iter() {
        for d in (s.defs)().unwrap_or_else(|e| panic!("{}: {e}", s.krate)) {
            defs.push((s.krate.to_string(), d));
        }
    }
    let mut extra = |krate: &str, ds: Vec<CommandDef>| defs.extend(ds.into_iter().map(|d| (krate.to_string(), d)));
    extra("fid-bus (egress)", fid_bus::policy::egress::command_defs().unwrap());
    extra("fid-rdl", fid_rdl::command_defs().unwrap());
    extra(
        "fid-gdt",
        vec![
            fid_gdt::chars::record::command_def().unwrap(),
            fid_gdt::dmt_flow::status::command_def().unwrap(),
            fid_gdt::drf::datum::command_def().unwrap(),
        ],
    );
    extra("fid-pkg", fid_pkg::reader::declarations().into_iter().map(|d| d.def).collect());
    extra("fid-report (bind)", fid_report::bind::declarations().into_iter().map(|d| d.def).collect());
    let reg = export_registry();
    extra(
        "fid-io-reg",
        vec![
            export_file_def(&reg).unwrap(),
            format_def(&reg, &FormatId::new("fid-test-points").unwrap(), &[]).unwrap(),
        ],
    );
    defs
}

#[test]
fn every_registered_params_schema_compiles_for_the_bus() {
    let defs = all_defs();
    let failures: Vec<String> = defs
        .iter()
        .filter_map(|(krate, d)| {
            Compiled::compile(&d.params_schema).err().map(|e| format!("{krate} {}@{}: {e}", d.id, d.major))
        })
        .collect();
    assert!(failures.is_empty(), "params schemas the bus cannot compile:\n{}", failures.join("\n"));
    assert!(defs.len() >= 30, "expected the workspace's commands, found {}", defs.len());
}
