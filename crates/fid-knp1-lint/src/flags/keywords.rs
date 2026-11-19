//! Fail-closed substring gate for CI files and free-form Cargo config values (spec node
//! `KRN.NUM.LINT.FLAGS`, maintainer decision of the third review).
//!
//! Shell and YAML grammar have too many ways to build a flag string (`:=`, `${VAR:-…}`,
//! `printf`, `$'…'`, values on the next line, lists, table and file forms of `--config`) for a
//! parser to converge, so the gate is a plain substring scan: after lowercasing, `_` → `-` and
//! collapsing whitespace runs to one space, ANY occurrence of a [`KEYWORDS`] entry, of a `+`
//! toolchain selector ([`SELECTORS`]) or of a `--config` that points at a `profile`, `build` or
//! `target` key or at a file is a finding. The structured parser in the parent module only
//! refines the rule and column; a line it does not explain is still reported as
//! `KNP1-FLAG-KEYWORD`.
//!
//! A line opts out only with an explicit same-line marker `knp1-flags: allow <RULE> <reason>`
//! (for example `# knp1-flags: allow KNP1-FLAG-KEYWORD deny warnings only`). The marker
//! suppresses that one rule on that one line and is itself reported as an informational
//! `KNP1-FLAG-OPTOUT` note. Text after the marker is not scanned.

/// Substrings that make a line a finding (after normalisation).
pub const KEYWORDS: [&str; 13] = [
    "target-cpu",
    "target-feature",
    "codegen-units",
    "llvm-args",
    "fp-contract",
    "-ctarget",
    "rustflags",
    "rustdocflags",
    "cargo-encoded-rustflags",
    "rustup run",
    "rustup override",
    "rustup default",
    "rustup-toolchain",
];

/// `+<toolchain>` selectors (after normalisation).
pub const SELECTORS: [&str; 4] = ["cargo +", "rustc +", "cargo.exe +", "rustc.exe +"];

/// The opt-out marker.
pub const MARKER: &str = "knp1-flags: allow ";

/// Lowercases, maps `_` to `-` and collapses whitespace runs; returns the normalised text and,
/// for each of its bytes, the 0-based character index in `line` it came from.
fn normalise(line: &str) -> (String, Vec<usize>) {
    let mut out = String::new();
    let mut map = Vec::new();
    let mut prev_space = false;
    for (ci, c) in line.chars().enumerate() {
        let n = if c.is_whitespace() {
            if prev_space {
                continue;
            }
            prev_space = true;
            ' '
        } else {
            prev_space = false;
            match c.to_ascii_lowercase() {
                '_' => '-',
                o => o,
            }
        };
        let before = out.len();
        out.push(n);
        map.extend(std::iter::repeat_n(ci, out.len() - before));
    }
    (out, map)
}

/// True when the value after a `--config` points at a profile, build or target key, at a file,
/// or at nothing visible (a variable): anything but a plain `other.key=value`.
fn config_value_is_risky(after: &str) -> bool {
    let v = after.trim_start_matches([' ', '=', '"', '\'']);
    let v: String = v.chars().take_while(|c| !c.is_whitespace() && !matches!(c, '"' | '\'')).collect();
    v.is_empty()
        || v.starts_with("profile")
        || v.starts_with("build")
        || v.starts_with("target")
        || !v.contains('=')
        || v.starts_with('$')
}

/// The part of `line` that is scanned (before an opt-out marker) and the marker, if any:
/// `(rule, reason)`. A marker without a rule or a reason is not a marker.
pub fn split_marker(line: &str) -> (&str, Option<(String, String)>) {
    let Some(i) = line.find(MARKER) else { return (line, None) };
    let rest = line.get(i + MARKER.len()..).unwrap_or("");
    let mut words = rest.split_whitespace();
    let rule = words.next().unwrap_or("").to_string();
    let reason = words.collect::<Vec<_>>().join(" ");
    if rule.is_empty() || reason.is_empty() {
        return (line, None);
    }
    (line.get(..i).unwrap_or(""), Some((rule, reason)))
}

/// Every keyword hit in `text` as `(0-based character column, what)`, sorted by column.
pub fn scan(text: &str) -> Vec<(usize, String)> {
    let (n, map) = normalise(text);
    let mut hits = Vec::new();
    let col = |b: usize| map.get(b).copied().unwrap_or(0);
    for k in KEYWORDS.iter().chain(SELECTORS.iter()) {
        let mut from = 0;
        while let Some(p) = n.get(from..).and_then(|s| s.find(k)) {
            hits.push((col(from + p), (*k).to_string()));
            from += p + k.len();
        }
    }
    let mut from = 0;
    while let Some(p) = n.get(from..).and_then(|s| s.find("--config")) {
        let at = from + p;
        let after = n.get(at + "--config".len()..).unwrap_or("");
        if config_value_is_risky(after) {
            hits.push((col(at), "--config".to_string()));
        }
        from = at + "--config".len();
    }
    hits.sort();
    hits.dedup();
    hits
}
