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
//! The scan runs in three passes over each CI file (see `keyword_gate` in the parent module):
//! raw lines, logical lines from `ci_spans` (shell `\` continuations joined without a space),
//! and the whole file with every whitespace character removed ([`compact`], [`scan_compact`]),
//! so a keyword split across lines or spaced out (`target - cpu`) is still found.
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

/// A character of a whole file with its 0-based line and character column.
pub type Located = (char, usize, usize);

/// The whole-file, whitespace-free view used by the third gate pass: the lines' scanned parts
/// (text before any opt-out marker) are joined, a backslash that ends a line is dropped together
/// with the line break (shell continuation), every whitespace character is removed, and the rest
/// is lowercased with `_` mapped to `-`. Each kept character remembers where it came from.
pub fn compact(lines: &[&str]) -> Vec<Located> {
    let mut out = Vec::new();
    for (li, line) in lines.iter().enumerate() {
        let (scanned, _) = split_marker(line);
        let body = scanned.strip_suffix('\\').unwrap_or(scanned);
        for (ci, c) in body.chars().enumerate() {
            if c.is_whitespace() {
                continue;
            }
            let n = match c.to_ascii_lowercase() {
                '_' => '-',
                o => o,
            };
            out.push((n, li, ci));
        }
    }
    out
}

/// Keyword hits in a [`compact`] view as `(first index, last index)` into it. Keywords and
/// selectors are matched with their spaces removed (`rustup run` → `rustuprun`, `cargo +` →
/// `cargo+`); `--config` is a hit when [`config_value_is_risky`] says so.
pub fn scan_compact(chars: &[Located]) -> Vec<(usize, usize)> {
    let text: String = chars.iter().map(|c| c.0).collect();
    // Byte offset -> index into `chars`.
    let mut idx_of = vec![0usize; text.len() + 1];
    let mut b = 0;
    for (i, (c, _, _)) in chars.iter().enumerate() {
        for slot in idx_of.iter_mut().skip(b).take(c.len_utf8()) {
            *slot = i;
        }
        b += c.len_utf8();
    }
    let at = |byte: usize| idx_of.get(byte).copied().unwrap_or(0);
    let mut hits = Vec::new();
    for k in KEYWORDS.iter().chain(SELECTORS.iter()) {
        let k: String = k.chars().filter(|c| !c.is_whitespace()).collect();
        let mut from = 0;
        while let Some(p) = text.get(from..).and_then(|s| s.find(&k)) {
            let s = from + p;
            hits.push((at(s), at(s + k.len() - 1)));
            from = s + k.len();
        }
    }
    let mut from = 0;
    while let Some(p) = text.get(from..).and_then(|s| s.find("--config")) {
        let s = from + p;
        let after = text.get(s + "--config".len()..).unwrap_or("");
        if config_value_is_risky(after) {
            hits.push((at(s), at(s + "--config".len() - 1)));
        }
        from = s + "--config".len();
    }
    hits.sort_unstable();
    hits.dedup();
    hits
}
