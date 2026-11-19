//! Typed errors of the template format: `report.template_invalid` with the RFC 6901 pointer
//! of the first offending element and a fixed reason word, and
//! `report.template_schema_unsupported` with the version found.

use crate::doc::must_escape;
use fid_canon::CValue;
use std::collections::BTreeMap;
use std::fmt;

/// Longest pointer an error holds, in bytes of UTF-8; a longer one is cut at a character
/// boundary. Pointers are built from input keys, so they are untrusted text.
pub const MAX_ERROR_POINTER_BYTES: usize = 1024;

/// Errors of [`super::parse_template`], [`super::migrate`] and [`super::serialise`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplateError {
    /// `report.template_invalid`: a budget, the syntax or the schema check failed at
    /// `pointer` (RFC 6901, `""` is the whole document) for `reason`.
    Invalid { pointer: String, reason: &'static str },
    /// `report.template_schema_unsupported`: no migration leads from `version` to the
    /// current schema version.
    SchemaUnsupported { version: u32 },
}

impl TemplateError {
    /// An invalid-template error; `pointer` is cut to [`MAX_ERROR_POINTER_BYTES`].
    pub(crate) fn invalid(pointer: &str, reason: &'static str) -> TemplateError {
        let mut cut = pointer.len().min(MAX_ERROR_POINTER_BYTES);
        while !pointer.is_char_boundary(cut) {
            cut -= 1;
        }
        TemplateError::Invalid { pointer: pointer.get(..cut).unwrap_or("").to_string(), reason }
    }

    /// Stable machine-readable error code.
    pub fn code(&self) -> &'static str {
        match self {
            TemplateError::Invalid { .. } => "report.template_invalid",
            TemplateError::SchemaUnsupported { .. } => "report.template_schema_unsupported",
        }
    }

    /// The pointer of an invalid-template error. Untrusted text: it holds input keys.
    pub fn pointer(&self) -> Option<&str> {
        match self {
            TemplateError::Invalid { pointer, .. } => Some(pointer),
            TemplateError::SchemaUnsupported { .. } => None,
        }
    }

    /// The fixed reason word of an invalid-template error.
    pub fn reason(&self) -> Option<&'static str> {
        match self {
            TemplateError::Invalid { reason, .. } => Some(reason),
            TemplateError::SchemaUnsupported { .. } => None,
        }
    }

    /// The `--json` / MCP error object. The pointer is marked untrusted:
    /// `{"code", "pointer": {"text", "untrusted": true}, "reason"}` or
    /// `{"code", "schema_version": "<u32>"}`.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("code".to_string(), CValue::Str(self.code().to_string()));
        match self {
            TemplateError::Invalid { pointer, reason } => {
                let mut p = BTreeMap::new();
                p.insert("text".to_string(), CValue::Str(pointer.clone()));
                p.insert("untrusted".to_string(), CValue::Bool(true));
                o.insert("pointer".to_string(), CValue::Obj(p));
                o.insert("reason".to_string(), CValue::Str((*reason).to_string()));
            }
            TemplateError::SchemaUnsupported { version } => {
                o.insert("schema_version".to_string(), CValue::Str(version.to_string()));
            }
        }
        CValue::Obj(o)
    }
}

/// The pointer with `\` doubled and every character [`must_escape`] flags written as
/// `\u{XXXX}`, so hostile keys cannot reach a terminal raw.
fn escaped(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c == '\\' {
            out.push_str("\\\\");
        } else if must_escape(c) {
            out.push_str(&format!("\\u{{{:04X}}}", u32::from(c)));
        } else {
            out.push(c);
        }
    }
    out
}

impl fmt::Display for TemplateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TemplateError::Invalid { pointer, reason } => {
                write!(f, "{}: {reason} at \"{}\"", self.code(), escaped(pointer))
            }
            TemplateError::SchemaUnsupported { version } => write!(f, "{}: schema_version {version}", self.code()),
        }
    }
}

impl std::error::Error for TemplateError {}

/// `base` extended by one RFC 6901 reference token (`~` as `~0`, `/` as `~1`).
pub(crate) fn child(base: &str, token: &str) -> String {
    let mut out = String::with_capacity(base.len() + token.len() + 1);
    out.push_str(base);
    push_token(&mut out, token);
    out
}

/// Appends `/` and the escaped token.
pub(crate) fn push_token(out: &mut String, token: &str) {
    out.push('/');
    for c in token.chars() {
        match c {
            '~' => out.push_str("~0"),
            '/' => out.push_str("~1"),
            c => out.push(c),
        }
    }
}
