//! KRN.SOLV.CONIC.PATCH AC-2 (REQ-002, REQ-003): the patched crate's source has no
//! exponential, power or PSD cone code, no clock, thread or printing, and no hash maps,
//! outside comments. Also checks what the KNP-1 syntax scan needs to classify the crate at
//! all (ASCII identifiers, patch 0008) and a few KNP-1 names directly.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

/// AC-2's list, searched in code (comments and string literals removed).
const AC2_FORBIDDEN: [&str; 7] = ["exp_cone", "pow_cone", "psd", "std::time", "std::thread", "println!", "HashMap"];
/// Further KNP-1 names that must not appear in code either.
const KNP1_FORBIDDEN: [&str; 9] =
    ["unsafe", "mul_add", "powi", "powf", "HashSet", "std::fs", "std::env", "std::net", "thread_local"];

fn crate_src() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|e| e == "rs") {
            out.push(p);
        }
    }
}

/// The code of a Rust source file: comments replaced by a space, string and char literal
/// contents removed (the quotes are kept), so that only code tokens remain.
fn code_only(src: &str) -> String {
    let c: Vec<char> = src.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let at = |i: usize, s: &str| s.chars().enumerate().all(|(k, ch)| c.get(i + k) == Some(&ch));
    while i < c.len() {
        if at(i, "//") {
            while i < c.len() && c[i] != '\n' {
                i += 1;
            }
            out.push(' ');
        } else if at(i, "/*") {
            let mut depth = 0;
            while i < c.len() {
                if at(i, "/*") {
                    depth += 1;
                    i += 2;
                } else if at(i, "*/") {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
            out.push(' ');
        } else if c[i] == 'r'
            && (i == 0 || !(c[i - 1].is_alphanumeric() || c[i - 1] == '_'))
            && (c.get(i + 1) == Some(&'"') || c.get(i + 1) == Some(&'#'))
        {
            let mut j = i + 1;
            let mut hashes = 0;
            while c.get(j) == Some(&'#') {
                hashes += 1;
                j += 1;
            }
            if c.get(j) != Some(&'"') {
                out.push(c[i]);
                i += 1;
                continue;
            }
            j += 1;
            loop {
                assert!(j < c.len(), "unterminated raw string");
                if c[j] == '"' && (0..hashes).all(|k| c.get(j + 1 + k) == Some(&'#')) {
                    j += 1 + hashes;
                    break;
                }
                j += 1;
            }
            out.push_str("\"\"");
            i = j;
        } else if c[i] == '"' {
            let mut j = i + 1;
            while c[j] != '"' {
                j += if c[j] == '\\' { 2 } else { 1 };
            }
            out.push_str("\"\"");
            i = j + 1;
        } else if c[i] == '\'' && c.get(i + 1) == Some(&'\\') {
            let mut j = i + 2;
            while c[j] != '\'' {
                j += 1;
            }
            out.push_str("''");
            i = j + 1;
        } else if c[i] == '\'' && c.get(i + 2) == Some(&'\'') {
            out.push_str("''");
            i += 3;
        } else {
            out.push(c[i]);
            i += 1;
        }
    }
    out
}

fn files_and_code() -> Vec<(String, String)> {
    let mut files = Vec::new();
    rust_files(&crate_src(), &mut files);
    assert!(files.len() > 20, "crate source not found");
    files
        .into_iter()
        .map(|p| {
            let rel = p.strip_prefix(crate_src()).unwrap().to_string_lossy().replace('\\', "/");
            (rel, code_only(&std::fs::read_to_string(&p).unwrap()))
        })
        .collect()
}

#[test]
fn code_only_strips_comments_and_literals() {
    let src = "let a = 1; // HashMap\n/* psd /* nested */ println! */ let s = \"std::time\"; let r = r#\"exp_cone\"#; let c = '\\''; f::<'a>(x)";
    let code = code_only(src);
    for w in ["HashMap", "psd", "println", "std::time", "exp_cone"] {
        assert!(!code.contains(w), "{w} survived in {code:?}");
    }
    assert!(code.contains("let a = 1;") && code.contains("f::<'a>(x)"), "{code:?}");
}

#[test]
fn ac2_no_forbidden_symbols_outside_comments() {
    let mut hits = Vec::new();
    for (file, code) in files_and_code() {
        for w in AC2_FORBIDDEN.iter().chain(KNP1_FORBIDDEN.iter()) {
            if code.contains(w) {
                hits.push(format!("{file}: {w}"));
            }
        }
    }
    assert!(hits.is_empty(), "forbidden symbols in fid-clarabel code: {hits:?}");
}

#[test]
fn identifiers_are_ascii() {
    let mut hits = Vec::new();
    for (file, code) in files_and_code() {
        if let Some(ch) = code.chars().find(|c| !c.is_ascii()) {
            hits.push(format!("{file}: {ch:?}"));
        }
    }
    assert!(hits.is_empty(), "non-ASCII characters in code: {hits:?}");
}
