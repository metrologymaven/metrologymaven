//! Token files (`tokens/**/*.tokens.json`) in the pinned DTCG draft syntax with the project
//! constraints of spec node UX.SYSTEM.PKG:
//!
//! - a token is an object with `$value`; any other object is a group; the token name is the
//!   group path joined with `.`;
//! - `$type` may sit on the token or be inherited from the nearest group;
//! - aliases only as a whole string `{name}` (in `$value` or inside a composite value); no
//!   other string may contain `{` or `}` (no expressions);
//! - `color`: `#rrggbb` or `#rrggbbaa` (lowercase hex); `dimension`: a decimal number with
//!   unit `px` (logical pixels) or `pt` (print); `duration`: a non-negative decimal number
//!   with unit `ms`.

use crate::{is_valid_name, Code, Finding};
use fid_canon::CValue;
use std::collections::BTreeMap;

/// DTCG keys allowed on a token object.
const TOKEN_KEYS: [&str; 5] = ["$value", "$type", "$description", "$extensions", "$deprecated"];
/// DTCG keys allowed on a group object.
const GROUP_KEYS: [&str; 4] = ["$type", "$description", "$extensions", "$deprecated"];

/// One token as written in a token file.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub name: String,
    /// Declared or inherited `$type`.
    pub ty: Option<String>,
    /// The raw `$value`.
    pub value: CValue,
    /// Package-relative path of the file that defines it.
    pub file: String,
    /// Aliases nested inside a composite value (not the whole-value alias).
    pub nested_refs: Vec<String>,
}

impl Token {
    /// The alias target when the whole `$value` is `{name}`.
    pub fn alias(&self) -> Option<&str> {
        match &self.value {
            CValue::Str(s) => parse_alias(s),
            _ => None,
        }
    }

    /// Canonical JSON of `$type` and `$value`, used to decide "value-changed".
    pub fn fingerprint(&self) -> Option<Vec<u8>> {
        let mut m = BTreeMap::new();
        m.insert("type".to_string(), self.ty.clone().map_or(CValue::Null, CValue::Str));
        m.insert("value".to_string(), self.value.clone());
        fid_canon::to_canonical_bytes(&CValue::Obj(m)).ok()
    }
}

/// `{name}` with no further braces; returns the inner text.
pub fn parse_alias(s: &str) -> Option<&str> {
    let inner = s.strip_prefix('{')?.strip_suffix('}')?;
    if inner.contains(['{', '}']) {
        return None;
    }
    Some(inner)
}

/// Walk state: path segments, the object, the inherited `$type`.
type Frame<'a> = (Vec<String>, &'a BTreeMap<String, CValue>, Option<String>);

/// Walks one parsed token file and appends its tokens and findings.
pub fn collect(file: &str, root: &CValue, out: &mut Vec<Token>, findings: &mut Vec<Finding>) {
    let CValue::Obj(top) = root else {
        findings.push(Finding::new(Code::Schema, file, "a token file must be a JSON object of groups"));
        return;
    };
    if top.contains_key("$value") {
        findings.push(Finding::new(Code::Schema, file, "the top level of a token file is a group, not a token"));
        return;
    }
    let mut stack: Vec<Frame<'_>> = vec![(Vec::new(), top, None)];
    while let Some((path, obj, inherited)) = stack.pop() {
        let name = path.join(".");
        let subject = if name.is_empty() { file.to_string() } else { name.clone() };
        let own_type = match obj.get("$type") {
            None => None,
            Some(CValue::Str(t)) => Some(t.clone()),
            Some(_) => {
                findings.push(Finding::new(Code::Schema, &subject, format!("{file}: $type must be a string")));
                None
            }
        };
        let ty = own_type.or(inherited);
        if let Some(value) = obj.get("$value") {
            for k in obj.keys() {
                if !TOKEN_KEYS.contains(&k.as_str()) {
                    findings.push(Finding::new(Code::Schema, &subject, format!("{file}: unexpected key {k:?} in a token")));
                }
            }
            let mut nested = Vec::new();
            check_value(file, &name, ty.as_deref(), value, &mut nested, findings);
            out.push(Token { name, ty, value: value.clone(), file: file.to_string(), nested_refs: nested });
            continue;
        }
        // A group: children in reverse so the stack pops them in key order.
        for (k, child) in obj.iter().rev() {
            if k.starts_with('$') {
                if !GROUP_KEYS.contains(&k.as_str()) {
                    findings.push(Finding::new(Code::Schema, &subject, format!("{file}: unexpected key {k:?} in a group")));
                }
                continue;
            }
            let mut p = path.clone();
            p.push(k.clone());
            if k.is_empty() || k.contains('.') {
                findings.push(Finding::new(Code::NameSyntax, p.join("."), format!("{file}: bad name segment {k:?}")));
                continue;
            }
            match child {
                CValue::Obj(m) => stack.push((p, m, ty.clone())),
                _ => findings.push(Finding::new(
                    Code::Schema,
                    p.join("."),
                    format!("{file}: member is neither a token nor a group"),
                )),
            }
        }
    }
}

fn check_value(
    file: &str,
    name: &str,
    ty: Option<&str>,
    value: &CValue,
    nested: &mut Vec<String>,
    findings: &mut Vec<Finding>,
) {
    let mut fail = |msg: String| findings.push(Finding::new(Code::Schema, name, format!("{file}: {msg}")));
    if let CValue::Str(s) = value {
        if s.contains(['{', '}']) {
            match parse_alias(s) {
                Some(target) if is_valid_name(target) => {}
                _ => fail(format!("{s:?} is not a whole `{{name}}` alias (expressions are not allowed)")),
            }
            return;
        }
    }
    let Some(ty) = ty else {
        fail("literal value without a $type".to_string());
        return;
    };
    let text = match value {
        CValue::Str(s) => Some(s.as_str()),
        _ => None,
    };
    match ty {
        "color" => {
            if !text.is_some_and(is_hex_colour) {
                fail("color must be \"#rrggbb\" or \"#rrggbbaa\" in lowercase hex".to_string());
            }
        }
        "dimension" => {
            let ok = text.is_some_and(|t| {
                t.strip_suffix("px").or_else(|| t.strip_suffix("pt")).is_some_and(|n| is_decimal(n, true))
            });
            if !ok {
                fail("dimension must be a decimal number with unit px or pt, e.g. \"8px\"".to_string());
            }
        }
        "duration" => {
            if !text.is_some_and(|t| t.strip_suffix("ms").is_some_and(|n| is_decimal(n, false))) {
                fail("duration must be a non-negative decimal number with unit ms, e.g. \"120ms\"".to_string());
            }
        }
        _ => {
            // Composite or other DTCG types: only the alias and no-expression rules apply.
            let mut stack = vec![value];
            while let Some(v) = stack.pop() {
                match v {
                    CValue::Str(s) if s.contains(['{', '}']) => match parse_alias(s) {
                        Some(t) if is_valid_name(t) => nested.push(t.to_string()),
                        _ => fail(format!("{s:?} is not a whole `{{name}}` alias (expressions are not allowed)")),
                    },
                    CValue::Arr(a) => stack.extend(a.iter()),
                    CValue::Obj(m) => stack.extend(m.values()),
                    _ => {}
                }
            }
        }
    }
}

fn is_hex_colour(s: &str) -> bool {
    let Some(h) = s.strip_prefix('#') else {
        return false;
    };
    (h.len() == 6 || h.len() == 8) && h.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// `-?(0|[1-9][0-9]*)(\.[0-9]+)?`; the sign only when `signed`.
fn is_decimal(s: &str, signed: bool) -> bool {
    let s = if signed { s.strip_prefix('-').unwrap_or(s) } else { s };
    let (int, frac) = match s.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (s, None),
    };
    let digits = |t: &str| !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit());
    let frac_ok = match frac {
        Some(f) => digits(f),
        None => true,
    };
    digits(int) && (int == "0" || !int.starts_with('0')) && frac_ok
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(json: &str) -> (Vec<Token>, Vec<Finding>) {
        let v = fid_canon::parse(json.as_bytes()).unwrap_or(CValue::Null);
        let (mut t, mut f) = (Vec::new(), Vec::new());
        collect("tokens/t.tokens.json", &v, &mut t, &mut f);
        (t, f)
    }

    #[test]
    fn groups_inherit_type_and_names_join() {
        let (t, f) = run(r##"{"a":{"$type":"color","b":{"$value":"#00ff00"},"c":{"$value":"{a.b}"}},"d":{"$type":"duration","$value":"0ms"}}"##);
        assert!(f.is_empty(), "{f:?}");
        let names: Vec<_> = t.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, ["a.b", "a.c", "d"]);
        assert_eq!(t[0].ty.as_deref(), Some("color"));
        assert_eq!(t[1].alias(), Some("a.b"));
    }

    #[test]
    fn constraint_violations_are_schema_findings() {
        let bad = [
            r##"{"a":{"$type":"color","$value":"#ABCDEF"}}"##,
            r##"{"a":{"$type":"color","$value":"#abc"}}"##,
            r##"{"a":{"$type":"dimension","$value":"1rem"}}"##,
            r##"{"a":{"$type":"dimension","$value":"01px"}}"##,
            r##"{"a":{"$type":"duration","$value":"-1ms"}}"##,
            r##"{"a":{"$type":"duration","$value":"1s"}}"##,
            r##"{"a":{"$type":"dimension","$value":"{b} * 2"}}"##,
            r##"{"a":{"$value":"#000000"}}"##,
            r##"{"a":{"$type":"shadow","$value":{"color":"calc({b})"}}}"##,
            r##"{"a":{"$value":"#000000","$type":"color","extra":1}}"##,
            r##"{"a":3}"##,
            r##"[]"##,
        ];
        for b in bad {
            let (_, f) = run(b);
            assert!(f.iter().any(|x| x.code == Code::Schema), "{b}: {f:?}");
        }
        let ok = [
            r##"{"a":{"$type":"dimension","$value":"-0.5pt"}}"##,
            r##"{"a":{"$type":"color","$value":"#0a0b0c80"}}"##,
            r##"{"a":{"$type":"fontWeight","$value":600}}"##,
        ];
        for o in ok {
            let (_, f) = run(o);
            assert!(f.is_empty(), "{o}: {f:?}");
        }
    }

    #[test]
    fn nested_aliases_are_recorded() {
        let (t, f) = run(r##"{"s":{"$type":"shadow","$value":{"color":"{c.x}","blur":"2px"}}}"##);
        assert!(f.is_empty(), "{f:?}");
        assert_eq!(t[0].nested_refs, vec!["c.x".to_string()]);
    }
}
