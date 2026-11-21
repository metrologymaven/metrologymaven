//! Every command definition in the workspace compiles for the bus's parameter validation
//! (CORE.BUS.PIPE.VALID): its parameter schema is accepted by `fid_bus::validate::Compiled`,
//! so no registered command refuses every call with `validation/schema`. Asked for in the
//! re-review of #341, after a too-strict pattern rule refused real command patterns. Covers
//! every crate in `SOURCES` and the crates that declare commands but are not in `SOURCES`
//! yet (GDT, RDL, the package reader, export, import, egress). The expectation (zero compile errors)
//! is the spec's rule, not a recorded run.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::validate::Compiled;
use fid_registry::CommandDef;
use fid_registry_build::test_support;

/// Every declared command of the workspace (`fid_registry_build::test_support::all_defs`),
/// including the two import commands of `fid-io-reg` (IO.REG.ENV.IMPORT).
fn all_defs() -> Vec<(String, CommandDef)> {
    test_support::all_defs().unwrap()
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
