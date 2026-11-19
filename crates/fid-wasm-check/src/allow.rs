//! The import allowlist file (`wasm-imports.allow`, REQ-KRN.MAN.WASM-003) and the check.
//!
//! Format, read strictly (anything else is an error naming the line):
//!
//! ```text
//! # comment lines start with '#'; blank lines are allowed
//! <module> <name> -- <justification>
//! ```
//!
//! `<module>` and `<name>` are 1 to 64 bytes of `[a-z0-9_]`, separated by one space; the
//! justification is non-empty printable ASCII with no leading or trailing space. The file is
//! UTF-8 with LF line endings (a CR, a tab or a byte-order mark is refused), at most
//! [`MAX_ALLOW_BYTES`] bytes and [`MAX_ENTRIES`] entries. Two entries for the same
//! `(module, name)` are refused. Entries allow function imports only: an import of any
//! other kind (table, memory, global, tag) is denied whatever its name. Entries for the
//! [`TRAP_MODULE`] module are refused. A function import from [`TRAP_MODULE`] named in
//! [`crate::seal::CLOCK_IMPORTS`] (what [`crate::seal::seal`] produces) passes as
//! `trapped`: the runner makes it trap when called, so it never feeds a value.

use std::fmt;

use crate::module::{Import, ImportKind, Module};
use crate::seal::{is_trapped, TRAP_MODULE};

/// Largest accepted allowlist, in bytes.
pub const MAX_ALLOW_BYTES: usize = 64 * 1024;
/// Most entries.
pub const MAX_ENTRIES: usize = 256;
/// Longest module or name token, in bytes.
pub const MAX_TOKEN_BYTES: usize = 64;

/// One allowed function import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub module: String,
    pub name: String,
    pub justification: String,
}

/// A parsed allowlist, in file order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Allowlist {
    entries: Vec<Entry>,
}

/// Allowlist errors; `line` is 1-based.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AllowError {
    TooLarge,
    NotUtf8,
    TooManyEntries,
    /// A CR, tab, byte-order mark or other character the format does not allow.
    BadCharacter {
        line: usize,
    },
    Syntax {
        line: usize,
    },
    MissingJustification {
        line: usize,
    },
    Duplicate {
        line: usize,
    },
}

impl AllowError {
    /// Stable code.
    pub fn code(&self) -> &'static str {
        match self {
            AllowError::TooLarge => "too_large",
            AllowError::NotUtf8 => "not_utf8",
            AllowError::TooManyEntries => "too_many_entries",
            AllowError::BadCharacter { .. } => "bad_character",
            AllowError::Syntax { .. } => "syntax",
            AllowError::MissingJustification { .. } => "missing_justification",
            AllowError::Duplicate { .. } => "duplicate",
        }
    }
}

impl fmt::Display for AllowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AllowError::BadCharacter { line }
            | AllowError::Syntax { line }
            | AllowError::MissingJustification { line }
            | AllowError::Duplicate { line } => write!(f, "{} at line {line}", self.code()),
            _ => f.write_str(self.code()),
        }
    }
}

impl std::error::Error for AllowError {}

fn is_token(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_TOKEN_BYTES
        && s.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
}

impl Allowlist {
    /// Parses the file bytes.
    pub fn parse(bytes: &[u8]) -> Result<Allowlist, AllowError> {
        if bytes.len() > MAX_ALLOW_BYTES {
            return Err(AllowError::TooLarge);
        }
        let text = std::str::from_utf8(bytes).map_err(|_| AllowError::NotUtf8)?;
        let mut entries: Vec<Entry> = Vec::new();
        for (i, raw) in text.split('\n').enumerate() {
            let line = i + 1;
            if !raw.bytes().all(|c| (0x20..=0x7e).contains(&c)) {
                return Err(AllowError::BadCharacter { line });
            }
            if raw.is_empty() || raw.starts_with('#') {
                continue;
            }
            let (head, justification) = match raw.split_once(" -- ") {
                Some(p) => p,
                None if raw.ends_with(" --") => return Err(AllowError::MissingJustification { line }),
                None => return Err(AllowError::Syntax { line }),
            };
            let (module, name) = head.split_once(' ').ok_or(AllowError::Syntax { line })?;
            if !is_token(module) || !is_token(name) || module == TRAP_MODULE {
                return Err(AllowError::Syntax { line });
            }
            if justification.trim().is_empty() {
                return Err(AllowError::MissingJustification { line });
            }
            if justification.trim() != justification {
                return Err(AllowError::Syntax { line });
            }
            if entries.iter().any(|e| e.module == module && e.name == name) {
                return Err(AllowError::Duplicate { line });
            }
            if entries.len() == MAX_ENTRIES {
                return Err(AllowError::TooManyEntries);
            }
            entries.push(Entry {
                module: module.to_string(),
                name: name.to_string(),
                justification: justification.to_string(),
            });
        }
        Ok(Allowlist { entries })
    }

    /// The entries in file order.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Whether `import` is allowed: a function import with an entry, or a sealed clock import.
    pub fn allows(&self, import: &Import) -> bool {
        is_trapped(&import.module, &import.name, import.kind)
            || (import.kind == ImportKind::Func
                && self.entries.iter().any(|e| e.module == import.module && e.name == import.name))
    }
}

/// Result of checking a module's imports against an allowlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportReport {
    /// Every import in module order, with whether it is allowed.
    pub imports: Vec<(Import, bool)>,
    /// Allowlist entries no import uses (informational).
    pub unused: Vec<Entry>,
}

impl ImportReport {
    /// The denied imports, in module order.
    pub fn denied(&self) -> Vec<&Import> {
        self.imports.iter().filter(|(_, ok)| !ok).map(|(i, _)| i).collect()
    }

    /// True when every import is allowed.
    pub fn passed(&self) -> bool {
        self.imports.iter().all(|(_, ok)| *ok)
    }

    /// The report lines: `allowed <module> <name>`, `trapped <module> <name>` or
    /// `DENIED <module> <name> (<kind>)` per import, `unused <module> <name>` per unused entry, then `imports: N, denied: M`.
    pub fn lines(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .imports
            .iter()
            .map(|(i, ok)| {
                if *ok && is_trapped(&i.module, &i.name, i.kind) {
                    format!("trapped {} {}", i.module, i.name)
                } else if *ok {
                    format!("allowed {} {}", i.module, i.name)
                } else {
                    format!("DENIED {} {} ({})", i.module, i.name, i.kind.as_str())
                }
            })
            .collect();
        out.extend(self.unused.iter().map(|e| format!("unused {} {}", e.module, e.name)));
        out.push(format!("imports: {}, denied: {}", self.imports.len(), self.denied().len()));
        out
    }
}

/// Checks every import of `module` against `allow` (REQ-KRN.MAN.WASM-003).
pub fn check(module: &Module, allow: &Allowlist) -> ImportReport {
    let imports: Vec<(Import, bool)> = module.imports.iter().map(|i| (i.clone(), allow.allows(i))).collect();
    let unused = allow
        .entries()
        .iter()
        .filter(|e| {
            !module.imports.iter().any(|i| i.kind == ImportKind::Func && i.module == e.module && i.name == e.name)
        })
        .cloned()
        .collect();
    ImportReport { imports, unused }
}
