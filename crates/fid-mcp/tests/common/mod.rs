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

/// Whether `v` conforms to `schema`, for the keywords the MCP emitter writes: `type` (a name
/// or a list), `properties`, `required` and `additionalProperties: false`. `{}` takes anything.
pub fn conforms(schema: &CValue, v: &CValue) -> bool {
    let CValue::Obj(s) = schema else { return false };
    let type_ok = |t: &str| match (t, v) {
        ("object", CValue::Obj(_))
        | ("array", CValue::Arr(_))
        | ("string", CValue::Str(_))
        | ("boolean", CValue::Bool(_))
        | ("null", CValue::Null) => true,
        ("number", CValue::Num(_) | CValue::I64(_) | CValue::U64(_)) => true,
        ("integer", CValue::Num(n)) => n.fract() == 0.0,
        ("integer", CValue::I64(_) | CValue::U64(_)) => true,
        _ => false,
    };
    match s.get("type") {
        None => {}
        Some(CValue::Str(t)) if type_ok(t) => {}
        Some(CValue::Arr(ts)) if ts.iter().any(|t| matches!(t, CValue::Str(t) if type_ok(t))) => {}
        Some(_) => return false,
    }
    let CValue::Obj(o) = v else { return true };
    if let Some(CValue::Arr(req)) = s.get("required") {
        if !req.iter().all(|r| matches!(r, CValue::Str(r) if o.contains_key(r))) {
            return false;
        }
    }
    let props = match s.get("properties") {
        Some(CValue::Obj(p)) => p.clone(),
        _ => BTreeMap::new(),
    };
    for (k, x) in o {
        match props.get(k) {
            Some(ps) if !conforms(ps, x) => return false,
            Some(_) => {}
            None if s.get("additionalProperties") == Some(&CValue::Bool(false)) => return false,
            None => {}
        }
    }
    true
}
