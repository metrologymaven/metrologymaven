//! Surface names derived from a registry command ID (CONSTITUTION §3). Spec node
//! `AUTO.SURF.PROJ`, REQ-002 to REQ-004.
//!
//! Every surface takes its names from here, so no emitter derives a name on its own:
//!
//! - CLI: `domain.verb_object@major` gives `["fid", domain, verb-object]`;
//! - MCP: `domain_verb_object`, plus `_vN` when the major N is 2 or more, at most 64 characters
//!   of `[a-z0-9_]`;
//! - Python: module `fiducial.<domain>`, function `<verb_object>` plus `_vN` when N >= 2.
//!
//! The name grammar is that of [`fid_types::CommandName`], narrowed to what every surface can
//! carry: exactly one dot, and each of the two segments starts with a lowercase letter and ends
//! with a lowercase letter or digit (a leading digit is not a Python identifier, a leading
//! underscore would become a leading hyphen, which a CLI reads as a flag). A segment that is a
//! Python keyword is refused because it cannot be a module or function name.

use crate::projection::ProjError;
use fid_types::CommandName;

/// Longest MCP tool name (CONSTITUTION §3).
pub const MCP_NAME_MAX: usize = 64;

/// The CLI program name, the first element of every CLI path.
pub const CLI_PROGRAM: &str = "fid";

/// The Python package every module lives in.
pub const PY_PACKAGE: &str = "fiducial";

/// Python 3 hard keywords that are lowercase (the only ones a command segment could spell).
/// Soft keywords (`match`, `case`, `type`, `_`) are valid identifiers and are allowed.
const PY_KEYWORDS: [&str; 32] = [
    "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif", "else", "except",
    "finally", "for", "from", "global", "if", "import", "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise",
    "return", "try", "while", "with", "yield",
];

/// The names one command major projects to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DerivedNames {
    /// `["fid", domain, verb-object]`.
    pub cli_path: Vec<String>,
    /// MCP tool name.
    pub mcp_name: String,
    /// Python module, `fiducial.<domain>`.
    pub py_module: String,
    /// Python function name.
    pub py_name: String,
}

/// Whether `s` is one surface-safe segment: `[a-z]([a-z0-9_]*[a-z0-9])?`.
fn is_segment(s: &str) -> bool {
    let b = s.as_bytes();
    let (Some(first), Some(last)) = (b.first(), b.last()) else {
        return false;
    };
    first.is_ascii_lowercase()
        && (last.is_ascii_lowercase() || last.is_ascii_digit())
        && b.iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
}

/// Whether `s` matches `^[a-z0-9_]{1,}$` (the length limit is checked separately).
fn is_mcp_chars(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
}

/// `_vN` for N >= 2, empty otherwise.
fn version_suffix(major: u32) -> String {
    if major >= 2 {
        format!("_v{major}")
    } else {
        String::new()
    }
}

/// Splits a command name into `(domain, verb_object)`, refusing anything outside the surface
/// grammar with `bad_command_id`.
pub fn split_name(command: &str, name: &str) -> Result<(String, String), ProjError> {
    let bad = |detail: &str| ProjError::BadCommandId { command: command.to_string(), detail: detail.to_string() };
    if !CommandName::is_valid(name) {
        return Err(bad("not a command name"));
    }
    let mut parts = name.split('.');
    let (Some(domain), Some(verb), None) = (parts.next(), parts.next(), parts.next()) else {
        return Err(bad("expected exactly one dot: domain.verb_object"));
    };
    if !is_segment(domain) {
        return Err(bad("domain must match [a-z]([a-z0-9_]*[a-z0-9])?"));
    }
    if !is_segment(verb) {
        return Err(bad("verb_object must match [a-z]([a-z0-9_]*[a-z0-9])?"));
    }
    Ok((domain.to_string(), verb.to_string()))
}

/// Derives every surface name of `name@major`. `command` is the full ID, used in errors.
pub fn derive(command: &str, name: &str, major: u32) -> Result<DerivedNames, ProjError> {
    let (domain, verb) = split_name(command, name)?;
    for seg in [&domain, &verb] {
        if PY_KEYWORDS.contains(&seg.as_str()) {
            return Err(ProjError::BadNameChars {
                command: command.to_string(),
                name: seg.clone(),
                detail: "a Python keyword cannot be a module or function name".to_string(),
            });
        }
    }
    let suffix = version_suffix(major);
    let mcp_name = format!("{domain}_{verb}{suffix}");
    if !is_mcp_chars(&mcp_name) {
        return Err(ProjError::BadNameChars {
            command: command.to_string(),
            name: mcp_name,
            detail: "MCP tool names use only [a-z0-9_]".to_string(),
        });
    }
    if mcp_name.len() > MCP_NAME_MAX {
        return Err(ProjError::NameTooLong { command: command.to_string(), len: mcp_name.len(), name: mcp_name });
    }
    Ok(DerivedNames {
        cli_path: vec![CLI_PROGRAM.to_string(), domain.clone(), verb.replace('_', "-")],
        mcp_name,
        py_module: format!("{PY_PACKAGE}.{domain}"),
        py_name: format!("{verb}{suffix}"),
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::panic)]
    use super::*;

    fn ok(name: &str, major: u32) -> DerivedNames {
        match derive(&format!("{name}@{major}"), name, major) {
            Ok(d) => d,
            Err(e) => panic!("{name}@{major}: {e}"),
        }
    }

    fn code(name: &str, major: u32) -> &'static str {
        derive(&format!("{name}@{major}"), name, major).map(|_| "ok").unwrap_or_else(|e| e.code())
    }

    #[test]
    fn ac2_run_bestfit() {
        let d = ok("align.run_bestfit", 1);
        assert_eq!(d.cli_path, ["fid", "align", "run-bestfit"]);
        assert_eq!(d.mcp_name, "align_run_bestfit");
        assert_eq!((d.py_module.as_str(), d.py_name.as_str()), ("fiducial.align", "run_bestfit"));
    }

    #[test]
    fn version_suffix_from_major_two() {
        assert_eq!(ok("a.b", 0).mcp_name, "a_b");
        assert_eq!(ok("a.b", 1).py_name, "b");
        assert_eq!(ok("a.b", 2).mcp_name, "a_b_v2");
        assert_eq!(ok("a.b", 2).py_name, "b_v2");
        assert_eq!(ok("a.b", 99_999).mcp_name, "a_b_v99999");
        // The CLI path carries no version: majors are a CLI flag concern (AUTO.CLI.TREE).
        assert_eq!(ok("a.b_c", 3).cli_path, ["fid", "a", "b-c"]);
    }

    #[test]
    fn mcp_length_boundary() {
        // "d_" + 62 letters = 64 characters: accepted; one more is refused.
        let v62 = "v".repeat(62);
        assert_eq!(ok(&format!("d.{v62}"), 1).mcp_name.len(), 64);
        assert_eq!(code(&format!("d.{v62}x"), 1), "name_too_long");
        // The suffix counts.
        let v59 = "v".repeat(59);
        assert_eq!(code(&format!("d.{v59}"), 9), "ok");
        assert_eq!(code(&format!("d.{v59}"), 10), "name_too_long");
    }

    #[test]
    fn grammar_is_refused_not_repaired() {
        for bad in ["", "a", "a.b.c", "A.b", "a..b", "_a.b", "a_.b", "a._b", "a.b_", "1a.b", "a.1b", "a-b.c", "a.b c"] {
            assert_eq!(code(bad, 1), "bad_command_id", "{bad:?}");
        }
        assert_eq!(code("a1.b2", 1), "ok");
        for kw in ["import.x", "x.import", "class.x", "x.lambda", "x.yield"] {
            assert_eq!(code(kw, 1), "bad_name_chars", "{kw:?}");
        }
        for soft in ["x.match", "x.case", "x.type"] {
            assert_eq!(code(soft, 1), "ok", "{soft:?}");
        }
    }
}
