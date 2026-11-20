//! The host matrix (`xart-matrix.toml`, REQ-001, REQ-005).
//!
//! A strict TOML subset, read fail-closed: UTF-8, LF only (CR, tab or BOM refused), at most
//! 64 KiB. `#` comment lines and blank lines are ignored. The first other line is exactly
//! `schema = "1"`; then one block per host, each exactly
//!
//! ```text
//! [[host]]
//! name = "<label>"          # [a-z0-9._-], 1 to 64 bytes
//! runner = "<runner label>" # [A-Za-z0-9._-], 1 to 64 bytes (the CI runs-on label)
//! optional = true|false     # an optional host may be `not_run` without failing the gate
//! ```
//!
//! in that order. Hosts are sorted by name in byte order with no duplicates (names are
//! lowercase, so this also refuses names that differ only in case), 1 to 64 hosts.

use crate::{is_label, XartError};

/// Largest matrix file read.
pub const MAX_MATRIX_BYTES: usize = 64 * 1024;
/// Most hosts in a matrix.
pub const MAX_HOSTS: usize = 64;

/// One host of the matrix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Host {
    pub name: String,
    pub runner: String,
    pub optional: bool,
}

/// The parsed matrix, hosts sorted by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Matrix {
    pub hosts: Vec<Host>,
}

impl Matrix {
    /// The host named `name`.
    pub fn host(&self, name: &str) -> Option<&Host> {
        self.hosts.iter().find(|h| h.name == name)
    }
}

fn quoted<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let v = line.strip_prefix(key)?.strip_prefix(" = \"")?.strip_suffix('"')?;
    Some(v)
}

fn is_runner(s: &str) -> bool {
    (1..=64).contains(&s.len()) && s.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
}

/// Parses a matrix file.
pub fn parse_matrix(bytes: &[u8]) -> Result<Matrix, XartError> {
    let bad = |line: usize, reason: &'static str| XartError::Matrix { line, reason };
    if bytes.len() > MAX_MATRIX_BYTES {
        return Err(bad(0, "too_large"));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| bad(0, "not_utf8"))?;
    if text.contains(['\r', '\t', '\u{feff}']) {
        return Err(bad(0, "cr_tab_or_bom"));
    }
    let lines: Vec<(usize, &str)> = text
        .lines()
        .enumerate()
        .map(|(i, l)| (i + 1, l))
        .filter(|(_, l)| !l.is_empty() && !l.starts_with('#'))
        .collect();
    let mut it = lines.into_iter();
    match it.next() {
        Some((_, "schema = \"1\"")) => {}
        Some((n, _)) => return Err(bad(n, "expected_schema_1")),
        None => return Err(bad(0, "empty")),
    }
    let mut hosts: Vec<Host> = Vec::new();
    while let Some((n, header)) = it.next() {
        if header != "[[host]]" {
            return Err(bad(n, "expected_host_table"));
        }
        let (nn, l) = it.next().ok_or_else(|| bad(n, "truncated_host"))?;
        let name = quoted(l, "name").filter(|v| is_label(v)).ok_or_else(|| bad(nn, "bad_name"))?;
        let (nr, l) = it.next().ok_or_else(|| bad(n, "truncated_host"))?;
        let runner = quoted(l, "runner").filter(|v| is_runner(v)).ok_or_else(|| bad(nr, "bad_runner"))?;
        let (no, l) = it.next().ok_or_else(|| bad(n, "truncated_host"))?;
        let optional = match l {
            "optional = true" => true,
            "optional = false" => false,
            _ => return Err(bad(no, "bad_optional")),
        };
        if hosts.last().is_some_and(|h| h.name.as_str() >= name) {
            return Err(bad(nn, "hosts_not_sorted_or_duplicate"));
        }
        if hosts.len() == MAX_HOSTS {
            return Err(bad(n, "too_many_hosts"));
        }
        hosts.push(Host { name: name.to_string(), runner: runner.to_string(), optional });
    }
    if hosts.is_empty() {
        return Err(bad(0, "no_hosts"));
    }
    Ok(Matrix { hosts })
}
