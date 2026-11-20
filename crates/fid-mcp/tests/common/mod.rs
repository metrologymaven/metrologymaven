//! Shared helpers of the fid-mcp integration tests.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, dead_code)]

use fid_canon::{digest, parse, to_canonical_bytes, CValue};
use fid_mcp::McpTool;
use fid_registry::CommandDef;
use fid_surface::{project, ProjError, Projection};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn read(rel: &str) -> Vec<u8> {
    std::fs::read(root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

pub fn json(s: &str) -> CValue {
    parse(s.as_bytes()).unwrap()
}

/// The canonical manifest of `defs` (CORE.REG.MAN shape), as the registry exports it.
pub fn manifest_of(defs: &[CommandDef]) -> Vec<u8> {
    let commands = defs
        .iter()
        .map(|d| {
            let v = d.to_value();
            let CValue::Obj(mut o) = v.clone() else { panic!("definition must be an object") };
            o.insert("definition_digest".into(), CValue::Str(digest(&v).unwrap().to_string()));
            CValue::Obj(o)
        })
        .collect();
    let mut m = BTreeMap::new();
    m.insert("format".to_string(), CValue::Num(1.0));
    m.insert("quantity_catalogue_digest".to_string(), CValue::Str(format!("sha256:{}", "ab".repeat(32))));
    m.insert("commands".to_string(), CValue::Arr(commands));
    to_canonical_bytes(&CValue::Obj(m)).unwrap()
}

/// The projection of one definition.
pub fn projection_of(def: &CommandDef) -> Result<Projection, ProjError> {
    project(&manifest_of(std::slice::from_ref(def)))
}

/// The registry definitions of a manifest file.
pub fn defs_of(manifest: &[u8]) -> Vec<CommandDef> {
    let CValue::Obj(mut m) = parse(manifest).unwrap() else { panic!("manifest must be an object") };
    let Some(CValue::Arr(cmds)) = m.remove("commands") else { panic!("no commands") };
    cmds.into_iter()
        .map(|c| {
            let CValue::Obj(mut o) = c else { panic!("command must be an object") };
            o.remove("definition_digest");
            CommandDef::from_value(CValue::Obj(o)).unwrap()
        })
        .collect()
}

pub fn tool_named<'a>(tools: &'a [McpTool], name: &str) -> &'a McpTool {
    tools.iter().find(|t| t.name == name).unwrap_or_else(|| panic!("no tool {name}"))
}

/// Hooks for [`conforms`]: quantity literals are normalised against the GEO.FRAME.UNITS
/// catalogue as the bus does (`CORE.BUS.UNIT`); no entity exists.
struct Hooks;

impl fid_bus::validate::Hooks for Hooks {
    fn literal(&self, kind: &str, value: &CValue) -> Result<fid_bus::unit::Normalised, fid_bus::error::BusError> {
        fid_bus::unit::normalise_value(value, kind, &fid_registry::Catalogue::geo_frame().unwrap())
    }
    fn entity(&self, _id: &str) -> fid_bus::validate::RefLookup {
        fid_bus::validate::RefLookup::Missing
    }
}

/// Whether `v` conforms to `schema`, checked by the bus's own JSON Schema 2020-12 validator
/// (`fid_bus::validate::Compiled`, CORE.BUS.PIPE.VALID): every keyword the generated schemas
/// use is asserted (`allOf`, `if`/`then`/`else`, `enum`, `const`, `pattern`, `minLength`,
/// `required`, `additionalProperties`, the quantity literals). The schema must compile.
pub fn conforms(schema: &CValue, v: &CValue) -> bool {
    let c = fid_bus::validate::Compiled::compile(schema).unwrap_or_else(|e| panic!("schema does not compile: {e}"));
    c.evaluate(v, &Hooks).is_ok()
}
