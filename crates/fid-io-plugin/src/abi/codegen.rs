//! Generators, freshness verification, and capability audit for the single
//! format-plugin IDL source (`crates/fid-io-plugin/abi/format-plugin.idl.json`).
//!
//! Mirrors `crates/fid-io-plugin/abi/gen_abi.py` byte-for-byte so both CI
//! (`ci/fixture-checks.txt`) and Rust unit/integration tests verify AC-1 and AC-5.

use crate::abi::AbiError;
use fid_canon::{digest_bytes, parse, to_canonical_bytes, CValue};
use std::collections::BTreeMap;

/// Committed single versioned IDL source.
pub const COMMITTED_IDL: &str = include_str!("../../abi/format-plugin.idl.json");
/// Committed generated WIT component interface.
pub const COMMITTED_WIT: &str = include_str!("../../abi/generated/format-plugin.wit");
/// Committed generated native host length-prefixed message schema.
pub const COMMITTED_MSG_PROTOCOL: &str = include_str!("../../abi/generated/message-protocol.json");

/// Forbidden capability tokens that must never appear in the format-plugin
/// contract interface (REQ-IO.REG.PLUGIN.ABI-006, AC-5).
const FORBIDDEN_CAP_TOKENS: &[&str] = &[
    "wasi:filesystem",
    "wasi:sockets",
    "wasi:clocks",
    "wasi:http",
    "filesystem",
    "socket",
    "sockets",
    "network",
    "clock",
    "clocks",
    "document",
    "journal",
    "store",
    "open_file",
    "read_file",
    "write_file",
];

/// Renders the WIT interface file from raw IDL JSON text.
pub fn render_wit_from_idl(idl_text: &str) -> Result<String, AbiError> {
    let val = parse(idl_text.as_bytes()).map_err(|e| bad_idl(format!("parse error: {e}")))?;
    let canon = to_canonical_bytes(&val).map_err(|e| bad_idl(format!("canonicalization error: {e}")))?;
    let dig = digest_bytes(&canon).to_string();

    let root = obj(&val, "idl")?;
    let pkg = str_field(root, "package")?;
    let ver = obj(field(root, "version")?, "version")?;
    let major = u64_field(ver, "major")?;
    let minor = u64_field(ver, "minor")?;
    let types = arr_field(root, "types")?;
    let exports = arr_field(root, "exports")?;
    let imports = root.get("imports").map(|v| arr(v, "imports")).transpose()?.unwrap_or(&[]);

    let mut lines: Vec<String> = Vec::new();
    lines.push(format!("// Generated from format-plugin.idl.json ({dig}). Do not edit by hand."));
    lines.push(format!("// Spec node IO.REG.PLUGIN.ABI (contract {major}.{minor})."));
    lines.push(format!("package {pkg}@{major}.{minor}.0;"));
    lines.push(String::new());
    lines.push("interface types {".into());

    let mut type_names: Vec<String> = Vec::with_capacity(types.len());
    for td in types {
        let to = obj(td, "type")?;
        let kind = str_field(to, "kind")?;
        let name = str_field(to, "name")?;
        type_names.push(name.to_string());
        match kind {
            "record" => {
                lines.push(format!("    record {name} {{"));
                for f in arr_field(to, "fields")? {
                    let fo = obj(f, "field")?;
                    lines.push(format!("        {}: {},", str_field(fo, "name")?, str_field(fo, "type")?));
                }
                lines.push("    }".into());
            }
            "variant" => {
                lines.push(format!("    variant {name} {{"));
                for c in arr_field(to, "cases")? {
                    let co = obj(c, "case")?;
                    lines.push(format!("        {}({}),", str_field(co, "name")?, str_field(co, "payload")?));
                }
                lines.push("    }".into());
            }
            other => return Err(bad_idl(format!("unsupported type kind: {other}"))),
        }
        lines.push(String::new());
    }
    if lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines.push("}".into());
    lines.push(String::new());
    lines.push("interface plugin {".into());
    lines.push(format!("    use types.{{{}}};", type_names.join(", ")));
    lines.push(String::new());
    for ex in exports {
        let eo = obj(ex, "export")?;
        let mut params = Vec::new();
        for p in arr_field(eo, "params")? {
            let po = obj(p, "param")?;
            params.push(format!("{}: {}", str_field(po, "name")?, str_field(po, "type")?));
        }
        lines.push(format!(
            "    {}: func({}) -> {};",
            str_field(eo, "name")?,
            params.join(", "),
            str_field(eo, "result")?
        ));
    }
    lines.push("}".into());
    lines.push(String::new());
    lines.push("world format-plugin {".into());
    for imp in imports {
        let s = match imp {
            CValue::Str(s) => s.as_str(),
            _ => return Err(bad_idl("import entry must be string")),
        };
        lines.push(format!("    import {s};"));
    }
    lines.push("    export types;".into());
    lines.push("    export plugin;".into());
    lines.push("}".into());

    let mut out = lines.join("\n");
    out.push('\n');
    Ok(out)
}

/// Renders the native length-prefixed message protocol JSON from raw IDL JSON text.
pub fn render_message_protocol_from_idl(idl_text: &str) -> Result<String, AbiError> {
    let val = parse(idl_text.as_bytes()).map_err(|e| bad_idl(format!("parse error: {e}")))?;
    let canon = to_canonical_bytes(&val).map_err(|e| bad_idl(format!("canonicalization error: {e}")))?;
    let dig = digest_bytes(&canon).to_string();

    let root = obj(&val, "idl")?;
    let pkg = str_field(root, "package")?;
    let ver = obj(field(root, "version")?, "version")?;
    let major = u64_field(ver, "major")?;
    let minor = u64_field(ver, "minor")?;
    let framing = field(root, "framing")?.clone();
    let types = field(root, "types")?.clone();
    let exports_val = arr_field(root, "exports")?;
    let mut export_names = Vec::with_capacity(exports_val.len());
    for ex in exports_val {
        let eo = obj(ex, "export")?;
        export_names.push(CValue::Str(str_field(eo, "name")?.to_string()));
    }
    let imports = root.get("imports").cloned().unwrap_or_else(|| CValue::Arr(Vec::new()));

    let messages = CValue::Arr(vec![
        msg_def(1, "DescriptorRequest", "host_to_plugin", None),
        msg_def(2, "DescriptorResponse", "plugin_to_host", Some("plugin-descriptor")),
        msg_def(3, "ReadRequest", "host_to_plugin", Some("read-request")),
        msg_def(4, "ReadRecord", "plugin_to_host", Some("plugin-record")),
        msg_def(5, "ReadEnd", "plugin_to_host", None),
        msg_def(6, "WriteRequest", "host_to_plugin", Some("write-request")),
        msg_def(7, "WriteResponse", "plugin_to_host", Some("list<u8>")),
        msg_def(8, "ErrorResponse", "plugin_to_host", Some("plugin-error")),
    ]);

    let mut contract_version = BTreeMap::new();
    contract_version.insert("major".into(), num_u64(major));
    contract_version.insert("minor".into(), num_u64(minor));

    let mut doc = BTreeMap::new();
    doc.insert("contract_version".into(), CValue::Obj(contract_version));
    doc.insert("exports".into(), CValue::Arr(export_names));
    doc.insert("format".into(), CValue::Str("format-plugin-messages@1".into()));
    doc.insert("framing".into(), framing);
    doc.insert("imports".into(), imports);
    doc.insert("messages".into(), messages);
    doc.insert("package".into(), CValue::Str(pkg.to_string()));
    doc.insert("source_idl_sha256".into(), CValue::Str(dig));
    doc.insert("types".into(), types);

    let mut out = String::new();
    write_pretty_json(&CValue::Obj(doc), 0, &mut out)?;
    out.push('\n');
    Ok(out)
}

/// Checks that `wit_text` and `msg_text` match what `idl_text` generates
/// byte-for-byte, failing with `abi_generated_out_of_date` on any drift (AC-1).
pub fn check_generated_freshness(idl_text: &str, wit_text: &str, msg_text: &str) -> Result<(), AbiError> {
    let want_wit = render_wit_from_idl(idl_text)?;
    if wit_text != want_wit {
        return Err(AbiError::GeneratedOutOfDate {
            artifact: "format-plugin.wit",
            detail: "committed WIT file differs from generated output".into(),
        });
    }
    let want_msg = render_message_protocol_from_idl(idl_text)?;
    if msg_text != want_msg {
        return Err(AbiError::GeneratedOutOfDate {
            artifact: "message-protocol.json",
            detail: "committed message protocol schema differs from generated output".into(),
        });
    }
    Ok(())
}

/// Audits the IDL source, WIT interface, and message schema for forbidden
/// file, socket/network, clock, or document capabilities (REQ-006, AC-5).
pub fn audit_capabilities(idl_text: &str, wit_text: &str, msg_text: &str) -> Result<(), AbiError> {
    let idl_val = parse(idl_text.as_bytes()).map_err(|e| bad_idl(format!("parse error: {e}")))?;
    let idl_root = obj(&idl_val, "idl")?;
    if let Some(imports) = idl_root.get("imports") {
        let items = arr(imports, "imports")?;
        if !items.is_empty() {
            return Err(AbiError::CapabilityViolation {
                detail: format!("IDL declares {} forbidden import(s)", items.len()),
            });
        }
    }

    for (idx, line) in wit_text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") {
            continue;
        }
        if trimmed.starts_with("import ") || trimmed == "import" {
            return Err(AbiError::CapabilityViolation {
                detail: format!("WIT line {} declares forbidden import: {trimmed}", idx + 1),
            });
        }
        if let Some(tok) = find_forbidden_token(trimmed) {
            return Err(AbiError::CapabilityViolation {
                detail: format!("WIT line {} mentions forbidden capability token '{tok}'", idx + 1),
            });
        }
    }

    let msg_val = parse(msg_text.as_bytes()).map_err(|e| bad_idl(format!("message schema parse error: {e}")))?;
    let msg_root = obj(&msg_val, "message_protocol")?;
    if let Some(imports) = msg_root.get("imports") {
        let items = arr(imports, "imports")?;
        if !items.is_empty() {
            return Err(AbiError::CapabilityViolation {
                detail: format!("message schema declares {} forbidden import(s)", items.len()),
            });
        }
    }

    for ex in arr_field(idl_root, "exports")? {
        let eo = obj(ex, "export")?;
        let name = str_field(eo, "name")?;
        if let Some(tok) = find_forbidden_token(name) {
            return Err(AbiError::CapabilityViolation {
                detail: format!("export '{name}' mentions forbidden capability token '{tok}'"),
            });
        }
    }

    Ok(())
}

fn find_forbidden_token(line: &str) -> Option<&'static str> {
    let lower = line.to_ascii_lowercase();
    for tok in FORBIDDEN_CAP_TOKENS {
        let bytes = lower.as_bytes();
        let tbytes = tok.as_bytes();
        if tbytes.len() > bytes.len() {
            continue;
        }
        let mut start = 0;
        while let Some(pos) = lower.get(start..).and_then(|s| s.find(tok)) {
            let abs = start + pos;
            let end = abs + tbytes.len();
            let left_ok = abs == 0 || !is_word_byte(bytes.get(abs.saturating_sub(1)).copied().unwrap_or(b' '));
            let right_ok = end == bytes.len() || !is_word_byte(bytes.get(end).copied().unwrap_or(b' '));
            if left_ok && right_ok {
                return Some(*tok);
            }
            start = abs + 1;
        }
    }
    None
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn bad_idl(detail: impl Into<String>) -> AbiError {
    AbiError::DescriptorInvalid { field: "idl".into(), detail: detail.into() }
}

fn obj<'a>(v: &'a CValue, name: &str) -> Result<&'a BTreeMap<String, CValue>, AbiError> {
    match v {
        CValue::Obj(m) => Ok(m),
        _ => Err(bad_idl(format!("{name} must be an object"))),
    }
}

fn arr<'a>(v: &'a CValue, name: &str) -> Result<&'a [CValue], AbiError> {
    match v {
        CValue::Arr(a) => Ok(a),
        _ => Err(bad_idl(format!("{name} must be an array"))),
    }
}

fn field<'a>(m: &'a BTreeMap<String, CValue>, key: &str) -> Result<&'a CValue, AbiError> {
    m.get(key).ok_or_else(|| bad_idl(format!("missing field '{key}'")))
}

fn str_field<'a>(m: &'a BTreeMap<String, CValue>, key: &str) -> Result<&'a str, AbiError> {
    match field(m, key)? {
        CValue::Str(s) => Ok(s.as_str()),
        _ => Err(bad_idl(format!("field '{key}' must be a string"))),
    }
}

fn arr_field<'a>(m: &'a BTreeMap<String, CValue>, key: &str) -> Result<&'a [CValue], AbiError> {
    arr(field(m, key)?, key)
}

fn u64_field(m: &BTreeMap<String, CValue>, key: &str) -> Result<u64, AbiError> {
    match field(m, key)? {
        CValue::U64(n) => Ok(*n),
        CValue::Num(f) if f.is_finite() && *f >= 0.0 && f.fract() == 0.0 =>
        {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            Ok(*f as u64)
        }
        _ => Err(bad_idl(format!("field '{key}' must be an unsigned integer"))),
    }
}

fn num_u64(n: u64) -> CValue {
    #[allow(clippy::cast_precision_loss)]
    CValue::Num(n as f64)
}

fn msg_def(tag: u64, name: &str, direction: &str, payload: Option<&str>) -> CValue {
    let mut m = BTreeMap::new();
    m.insert("direction".into(), CValue::Str(direction.into()));
    m.insert("name".into(), CValue::Str(name.into()));
    m.insert("payload".into(), payload.map_or(CValue::Null, |p| CValue::Str(p.into())));
    m.insert("tag".into(), num_u64(tag));
    CValue::Obj(m)
}

fn write_pretty_json(val: &CValue, indent: usize, out: &mut String) -> Result<(), AbiError> {
    match val {
        CValue::Null => out.push_str("null"),
        CValue::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        CValue::Str(s) => write_json_string(s, out),
        CValue::I64(n) => out.push_str(&n.to_string()),
        CValue::U64(n) => out.push_str(&n.to_string()),
        CValue::Num(f) => {
            if !f.is_finite() {
                return Err(bad_idl("non-finite number in IDL"));
            }
            if f.fract() == 0.0 && *f >= -9_007_199_254_740_991.0 && *f <= 9_007_199_254_740_991.0 {
                #[allow(clippy::cast_possible_truncation)]
                out.push_str(&(*f as i64).to_string());
            } else {
                out.push_str(&f.to_string());
            }
        }
        CValue::F64Bits(f) => {
            write_json_string(&format!("0x{:016x}", f.to_bits()), out);
        }
        CValue::Arr(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return Ok(());
            }
            out.push_str("[\n");
            for (idx, item) in items.iter().enumerate() {
                push_indent(indent + 2, out);
                write_pretty_json(item, indent + 2, out)?;
                if idx + 1 < items.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            push_indent(indent, out);
            out.push(']');
        }
        CValue::Obj(map) => {
            if map.is_empty() {
                out.push_str("{}");
                return Ok(());
            }
            out.push_str("{\n");
            for (idx, (k, v)) in map.iter().enumerate() {
                push_indent(indent + 2, out);
                write_json_string(k, out);
                out.push_str(": ");
                write_pretty_json(v, indent + 2, out)?;
                if idx + 1 < map.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            push_indent(indent, out);
            out.push('}');
        }
    }
    Ok(())
}

fn push_indent(spaces: usize, out: &mut String) {
    for _ in 0..spaces {
        out.push(' ');
    }
}

fn write_json_string(s: &str, out: &mut String) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\x08' => out.push_str("\\b"),
            '\x0c' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                use std::fmt::Write as _;
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}
