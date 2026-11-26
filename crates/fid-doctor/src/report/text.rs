//! Text view of a doctor report (REQ-PLAT.DOCTOR.REPORT-002 and -006).
//!
//! The renderer walks the report's JSON value tree, so every JSON leaf value appears in the
//! text in its rendered form:
//!
//! - a string as [`quote_text`]: in double quotes, `"` and `\` escaped with a backslash, and
//!   every control, bidi or other format character ([`is_escaped_char`]) written as `\u{hex}`;
//! - a number in the ECMAScript form canonical JSON uses (`fid_canon::format_number`);
//! - `true`, `false` and `null` as in JSON.
//!
//! Layout (two-space indentation, lines end with `\n`):
//!
//! ```text
//! Metrology Maven doctor report
//! schema_version: <n>
//! tool_version: "<s>"
//! phase: "<s>"
//! redacted: <bool>
//!
//! Platform        os, arch (snapshot), tier (assessment), snapshot_schema_version
//! Hardware        cpu, ram, storage, display
//! GPU             gpu
//! Verdicts        outcome, exit_code, assessment_schema_version, profiles
//! Unknowns        one line per unknown item and per profile not (fully) compared, or "none"
//! ```
//!
//! Inside a section a value is written as `<label>: <scalar>`, `<label>: []` / `{}` when empty,
//! `<label>: [a, b]` for an array of scalars, and otherwise `<label>:` followed by its members
//! two spaces deeper: object members by key in canonical (code point) order, array elements
//! labelled `[0]`, `[1]`, ...

use super::ReportError;
use fid_canon::CValue;
use std::collections::BTreeMap;

/// Whether `c` is escaped when rendered: a control character (Unicode category Cc), or a
/// bidi, line/paragraph separator, zero-width or invisible-operator format character
/// (U+061C, U+200B..U+200F, U+2028..U+202E, U+2060..U+2064, U+2066..U+2069, U+FEFF).
pub fn is_escaped_char(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{061C}'
                | '\u{200B}'..='\u{200F}'
                | '\u{2028}'..='\u{202E}'
                | '\u{2060}'..='\u{2064}'
                | '\u{2066}'..='\u{2069}'
                | '\u{FEFF}'
        )
}

/// `s` with every [`is_escaped_char`] character written as `\u{hex}` (lowercase hex, no
/// padding). Nothing else is changed.
pub fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if is_escaped_char(c) {
            out.push_str(&format!("\\u{{{:x}}}", u32::from(c)));
        } else {
            out.push(c);
        }
    }
    out
}

/// `s` in double quotes with `"` and `\` backslash-escaped and every [`is_escaped_char`]
/// character written as `\u{hex}`. The result never holds a control or format character.
pub fn quote_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if is_escaped_char(c) => out.push_str(&format!("\\u{{{:x}}}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn scalar(v: &CValue) -> Result<Option<String>, ReportError> {
    Ok(Some(match v {
        CValue::Null => "null".to_string(),
        CValue::Bool(b) => b.to_string(),
        CValue::Str(s) => quote_text(s),
        CValue::I64(n) => n.to_string(),
        CValue::U64(n) => n.to_string(),
        CValue::F64Bits(x) => format!("0x{:016x}", x.to_bits()),
        CValue::Num(x) => fid_canon::format_number(*x).map_err(|_| ReportError::NonFiniteNumber)?,
        CValue::Arr(_) | CValue::Obj(_) => return Ok(None),
    }))
}

fn pad(indent: usize) -> String {
    " ".repeat(indent)
}

fn walk(out: &mut String, label: &str, v: &CValue, indent: usize) -> Result<(), ReportError> {
    if let Some(s) = scalar(v)? {
        out.push_str(&format!("{}{label}: {s}\n", pad(indent)));
        return Ok(());
    }
    match v {
        CValue::Arr(items) if items.is_empty() => out.push_str(&format!("{}{label}: []\n", pad(indent))),
        CValue::Arr(items) => {
            let mut parts = Vec::with_capacity(items.len());
            for item in items {
                match scalar(item)? {
                    Some(s) => parts.push(s),
                    None => break,
                }
            }
            if parts.len() == items.len() {
                out.push_str(&format!("{}{label}: [{}]\n", pad(indent), parts.join(", ")));
            } else {
                out.push_str(&format!("{}{label}:\n", pad(indent)));
                for (i, item) in items.iter().enumerate() {
                    walk(out, &format!("[{i}]"), item, indent + 2)?;
                }
            }
        }
        CValue::Obj(m) if m.is_empty() => out.push_str(&format!("{}{label}: {{}}\n", pad(indent))),
        CValue::Obj(m) => {
            out.push_str(&format!("{}{label}:\n", pad(indent)));
            for (k, item) in m {
                walk(out, &escape_text(k), item, indent + 2)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn obj(v: &CValue) -> Option<&BTreeMap<String, CValue>> {
    match v {
        CValue::Obj(m) => Some(m),
        _ => None,
    }
}

fn get<'a>(m: Option<&'a BTreeMap<String, CValue>>, key: &str) -> Option<&'a CValue> {
    m.and_then(|m| m.get(key))
}

fn walk_opt(out: &mut String, label: &str, v: Option<&CValue>, indent: usize) -> Result<(), ReportError> {
    match v {
        Some(v) => walk(out, label, v, indent),
        None => Ok(()),
    }
}

fn str_of(v: Option<&CValue>) -> Option<&str> {
    match v {
        Some(CValue::Str(s)) => Some(s.as_str()),
        _ => None,
    }
}

/// Renders the report value tree (`DoctorReport::to_cvalue`).
pub(super) fn render(report: &CValue) -> Result<String, ReportError> {
    let root = obj(report);
    let snapshot = obj(get(root, "snapshot").unwrap_or(&CValue::Null));
    let assessment = obj(get(root, "assessment").unwrap_or(&CValue::Null));
    let mut out = String::new();

    out.push_str("Metrology Maven doctor report\n");
    for key in ["schema_version", "tool_version", "phase", "redacted"] {
        walk_opt(&mut out, key, get(root, key), 0)?;
    }

    out.push_str("\nPlatform\n");
    walk_opt(&mut out, "os", get(snapshot, "os"), 2)?;
    walk_opt(&mut out, "arch", get(snapshot, "arch"), 2)?;
    walk_opt(&mut out, "tier", get(assessment, "tier"), 2)?;
    walk_opt(&mut out, "snapshot_schema_version", get(snapshot, "schema_version"), 2)?;

    out.push_str("\nHardware\n");
    for key in ["cpu", "ram", "storage", "display"] {
        walk_opt(&mut out, key, get(snapshot, key), 2)?;
    }

    out.push_str("\nGPU\n");
    walk_opt(&mut out, "gpu", get(snapshot, "gpu"), 2)?;

    out.push_str("\nVerdicts\n");
    walk_opt(&mut out, "outcome", get(assessment, "outcome"), 2)?;
    walk_opt(&mut out, "exit_code", get(assessment, "exit_code"), 2)?;
    walk_opt(&mut out, "assessment_schema_version", get(assessment, "schema_version"), 2)?;
    walk_opt(&mut out, "profiles", get(assessment, "profiles"), 2)?;

    out.push_str("\nUnknowns\n");
    let mut lines = Vec::new();
    let items = [
        ("os", get(snapshot, "os")),
        ("arch", get(snapshot, "arch")),
        ("tier", get(assessment, "tier")),
        ("cpu", get(snapshot, "cpu")),
        ("ram", get(snapshot, "ram")),
        ("storage", get(snapshot, "storage")),
        ("display", get(snapshot, "display")),
        ("gpu", get(snapshot, "gpu")),
    ];
    for (name, v) in items {
        let m = v.and_then(obj);
        if str_of(get(m, "status")) == Some("unknown") {
            let reason = escape_text(str_of(get(m, "reason")).unwrap_or(""));
            let detail = quote_text(str_of(get(m, "detail")).unwrap_or(""));
            lines.push(format!("  {name}: {reason} {detail}"));
        }
    }
    if let Some(CValue::Arr(profiles)) = get(assessment, "profiles") {
        for p in profiles {
            let m = obj(p);
            let name = quote_text(str_of(get(m, "name")).unwrap_or(""));
            if let Some(reason) = str_of(get(m, "reason")) {
                lines.push(format!("  profile {name}: {}", escape_text(reason)));
            }
            if let Some(CValue::Arr(unknowns)) = get(m, "unknowns") {
                if !unknowns.is_empty() {
                    let fields: Vec<String> = unknowns
                        .iter()
                        .map(|f| match f {
                            CValue::Str(s) => quote_text(s),
                            _ => quote_text(""),
                        })
                        .collect();
                    lines.push(format!("  profile {name}: not compared: {}", fields.join(", ")));
                }
            }
        }
    }
    if lines.is_empty() {
        out.push_str("  none\n");
    } else {
        for l in lines {
            out.push_str(&l);
            out.push('\n');
        }
    }
    Ok(out)
}
