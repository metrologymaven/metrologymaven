//! Token-level, fail-closed check of the places in Rust source that can make the compiler read
//! a file or the environment (REQ-KRN.MAN.MANI.IMPL-001: no compiled byte escapes the digest).
//!
//! The scan is a small lexer, not a parser, and it deliberately does not follow macro
//! semantics. It skips line and nested block comments, string, byte-string, C-string and
//! raw-string literals and char literals. Every remaining occurrence of the identifiers
//! `include`, `include_str`, `include_bytes`, `env`, `option_env` and `path` must have one exact
//! allowed shape (see [`references`]); any other occurrence is refused, whatever it means. So
//! aliases (`use core::include_bytes as ib`), includes inside `macro_rules!` bodies, `path = ..`
//! passed to a macro or `cfg_attr`, and `env!` of anything but Cargo's package variables are
//! all errors by construction. An unterminated literal or comment is an error too. The caller
//! checks each returned literal path against the crate's hashed files.

/// What a reference reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RefKind {
    /// `include!`: the file is Rust source and is scanned in turn.
    Code,
    /// `include_str!` or `include_bytes!`: the file is data.
    Data,
    /// `#[path = ".."]`: a module file.
    ModPath,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    Ident(String),
    /// A string literal: `Some(text)` for a plain `".."` without escapes or a raw `r".."`,
    /// `None` for any other string literal.
    Str(Option<String>),
    Punct(u8),
    Other,
}

const LEX: &str = "source_lex";

fn ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c >= 0x80
}

fn ident_cont(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80
}

/// `/* .. */` with nesting, starting at `i`; returns the index after it.
fn block_comment(b: &[u8], mut i: usize) -> Result<usize, &'static str> {
    let mut depth = 0usize;
    while i < b.len() {
        if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
            depth += 1;
            i += 2;
        } else if b[i] == b'*' && b.get(i + 1) == Some(&b'/') {
            depth -= 1;
            i += 2;
            if depth == 0 {
                return Ok(i);
            }
        } else {
            i += 1;
        }
    }
    Err(LEX)
}

/// A `".."` literal whose opening quote is at `i`: (text if it has no escapes, index after).
fn string(src: &str, i: usize) -> Result<(Option<String>, usize), &'static str> {
    let b = src.as_bytes();
    let mut j = i + 1;
    let mut escaped = false;
    loop {
        match b.get(j) {
            None => return Err(LEX),
            Some(b'\\') => {
                escaped = true;
                j += 2;
            }
            Some(b'"') => break,
            Some(_) => j += 1,
        }
    }
    let text = if escaped { None } else { src.get(i + 1..j).map(str::to_string) };
    Ok((text, j + 1))
}

/// Whether `#`* then `"` follows at `i` (a raw string after its `r`, `br` or `cr` prefix).
fn raw_follows(b: &[u8], mut i: usize) -> bool {
    while b.get(i) == Some(&b'#') {
        i += 1;
    }
    b.get(i) == Some(&b'"')
}

/// A raw string whose `#`s start at `i` (after the prefix): (text, index after).
fn raw_string(src: &str, i: usize) -> Result<(String, usize), &'static str> {
    let b = src.as_bytes();
    let mut j = i;
    while b.get(j) == Some(&b'#') {
        j += 1;
    }
    let hashes = j - i;
    let start = j + 1;
    let mut k = start;
    while k < b.len() {
        if b[k] == b'"' && b.len() >= k + 1 + hashes && b[k + 1..k + 1 + hashes].iter().all(|c| *c == b'#') {
            let text = src.get(start..k).ok_or(LEX)?.to_string();
            return Ok((text, k + 1 + hashes));
        }
        k += 1;
    }
    Err(LEX)
}

/// A char literal or a lifetime whose quote is at `i`; returns the index after the literal,
/// or after the quote alone for a lifetime.
fn quote(b: &[u8], i: usize) -> Result<usize, &'static str> {
    match b.get(i + 1) {
        None => Err(LEX),
        Some(b'\\') => {
            // Skip the backslash and the escaped character (which may itself be a quote).
            let mut j = i + 3;
            while j < b.len() && b[j] != b'\'' {
                j += 1;
            }
            if j < b.len() {
                Ok(j + 1)
            } else {
                Err(LEX)
            }
        }
        Some(&c) => {
            let len = match c {
                0x00..=0x7f => 1,
                0xc0..=0xdf => 2,
                0xe0..=0xef => 3,
                _ => 4,
            };
            if b.get(i + 1 + len) == Some(&b'\'') {
                Ok(i + 2 + len)
            } else {
                Ok(i + 1)
            }
        }
    }
}

fn lex(src: &str) -> Result<Vec<Tok>, &'static str> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_whitespace() {
            i += 1;
        } else if c == b'/' && b.get(i + 1) == Some(&b'/') {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
        } else if c == b'/' && b.get(i + 1) == Some(&b'*') {
            i = block_comment(b, i)?;
        } else if c == b'"' {
            let (text, next) = string(src, i)?;
            out.push(Tok::Str(text));
            i = next;
        } else if c == b'\'' {
            i = quote(b, i)?;
            out.push(Tok::Other);
        } else if c.is_ascii_digit() {
            while i < b.len() && ident_cont(b[i]) {
                i += 1;
            }
            out.push(Tok::Other);
        } else if ident_start(c) {
            let start = i;
            while i < b.len() && ident_cont(b[i]) {
                i += 1;
            }
            let word = src.get(start..i).ok_or(LEX)?;
            match (word, b.get(i)) {
                ("r" | "br" | "cr", Some(b'"' | b'#')) if raw_follows(b, i) => {
                    let (text, next) = raw_string(src, i)?;
                    out.push(Tok::Str(if word == "r" { Some(text) } else { None }));
                    i = next;
                }
                ("b" | "c", Some(b'"')) => {
                    let (_, next) = string(src, i)?;
                    out.push(Tok::Str(None));
                    i = next;
                }
                ("b", Some(b'\'')) => {
                    i = quote(b, i)?;
                    out.push(Tok::Other);
                }
                _ => out.push(Tok::Ident(word.to_string())),
            }
        } else {
            out.push(Tok::Punct(c));
            i += 1;
        }
    }
    Ok(out)
}

/// Index of the delimiter closing the group opened at `open` (any of `(`, `[`, `{`, counted
/// together), or the last token when the group is unbalanced (fail closed: the rest counts as
/// inside).
fn group_end(t: &[Tok], open: usize) -> usize {
    let mut depth = 0usize;
    for (j, tok) in t.iter().enumerate().skip(open) {
        match tok {
            Tok::Punct(b'(' | b'[' | b'{') => depth += 1,
            Tok::Punct(b')' | b']' | b'}') => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return j;
                }
            }
            _ => {}
        }
    }
    t.len().saturating_sub(1)
}

/// Marks every token of every `macro_rules!` definition (from the `macro_rules` identifier to
/// the end of its body).
fn macro_rules_mask(t: &[Tok]) -> Vec<bool> {
    let mut mask = vec![false; t.len()];
    let mut k = 0;
    while k < t.len() {
        if t[k] == Tok::Ident("macro_rules".to_string()) {
            // `macro_rules ! name { .. }`; anything malformed marks to the end of the file.
            let mut open = k + 1;
            while open < t.len() && !matches!(t[open], Tok::Punct(b'(' | b'[' | b'{')) {
                open += 1;
            }
            let end = if open < t.len() { group_end(t, open) } else { t.len().saturating_sub(1) };
            for m in mask.iter_mut().take(end + 1).skip(k) {
                *m = true;
            }
            k = end;
        }
        k += 1;
    }
    mask
}

/// The identifiers that can make the compiler read a file or the environment.
const RESTRICTED: [&str; 6] = ["include", "include_str", "include_bytes", "env", "option_env", "path"];

/// Whether `name` is a variable `env!`/`option_env!` may read: Cargo sets these from the
/// crate's `Cargo.toml`, which is hashed.
fn env_allowed(name: &str) -> bool {
    name == "CARGO_CRATE_NAME" || (name.starts_with("CARGO_PKG_") && name.len() > "CARGO_PKG_".len())
}

/// `name!("lit")` starting at `k` (the identifier): the literal, if the shape is exact.
fn call_literal(t: &[Tok], k: usize) -> Option<&str> {
    match (t.get(k + 1), t.get(k + 2), t.get(k + 3), t.get(k + 4)) {
        (Some(Tok::Punct(b'!')), Some(Tok::Punct(b'(')), Some(Tok::Str(Some(lit))), Some(Tok::Punct(b')'))) => {
            Some(lit.as_str())
        }
        _ => None,
    }
}

/// `#[path = "lit"]` around `k` (the `path` identifier), directly on a `mod name;` item (other
/// outer attributes and a visibility may come between): the literal, if the shape is exact.
fn mod_path_literal(t: &[Tok], k: usize) -> Option<&str> {
    if k < 2 || t[k - 2] != Tok::Punct(b'#') || t[k - 1] != Tok::Punct(b'[') {
        return None;
    }
    let lit = match (t.get(k + 1), t.get(k + 2), t.get(k + 3)) {
        (Some(Tok::Punct(b'=')), Some(Tok::Str(Some(lit))), Some(Tok::Punct(b']'))) => lit.as_str(),
        _ => return None,
    };
    let mut j = k + 4;
    // Further outer attributes on the same item.
    while t.get(j) == Some(&Tok::Punct(b'#')) && t.get(j + 1) == Some(&Tok::Punct(b'[')) {
        j = group_end(t, j + 1) + 1;
    }
    // Visibility: `pub` or `pub(..)`.
    if t.get(j) == Some(&Tok::Ident("pub".to_string())) {
        j += 1;
        if t.get(j) == Some(&Tok::Punct(b'(')) {
            j = group_end(t, j) + 1;
        }
    }
    match (t.get(j), t.get(j + 1), t.get(j + 2)) {
        (Some(Tok::Ident(m)), Some(Tok::Ident(_)), Some(Tok::Punct(b';'))) if m == "mod" => Some(lit),
        _ => None,
    }
}

/// Every file reference in `src`, in source order. Token-level and fail closed: each
/// occurrence of a restricted identifier (`include`, `include_str`, `include_bytes`, `env`,
/// `option_env`, `path`) outside strings and comments must have one exact allowed shape, else
/// the static reason code is returned. Allowed:
///
/// - `include!("lit")`, `include_str!("lit")`, `include_bytes!("lit")`, not inside a
///   `macro_rules!` definition (an alias such as `use core::include_bytes as ib` is the
///   identifier in another shape, so it is refused);
/// - `#[path = "lit"]` directly on a `mod name;` item, not inside `macro_rules!` (so neither
///   `cfg_attr(.., path = ..)` nor `path = ..` passed to a macro);
/// - `env!("lit")` / `option_env!("lit")` with `lit` `CARGO_CRATE_NAME` or `CARGO_PKG_*`.
pub(crate) fn references(src: &str) -> Result<Vec<(RefKind, String)>, &'static str> {
    let t = lex(src)?;
    let in_macro = macro_rules_mask(&t);
    let mut refs = Vec::new();
    for (k, tok) in t.iter().enumerate() {
        let Tok::Ident(word) = tok else { continue };
        if !RESTRICTED.contains(&word.as_str()) {
            continue;
        }
        match word.as_str() {
            "include" | "include_str" | "include_bytes" => {
                if in_macro[k] {
                    return Err("include_in_macro_rules");
                }
                let lit = call_literal(&t, k).ok_or("include_not_literal")?;
                let kind = if word == "include" { RefKind::Code } else { RefKind::Data };
                refs.push((kind, lit.to_string()));
            }
            "env" | "option_env" => match call_literal(&t, k) {
                Some(lit) if env_allowed(lit) => {}
                _ => return Err("env_not_allowed"),
            },
            _ => {
                if in_macro[k] {
                    return Err("path_in_macro_rules");
                }
                let lit = mod_path_literal(&t, k).ok_or("path_not_mod_attribute")?;
                refs.push((RefKind::ModPath, lit.to_string()));
            }
        }
    }
    Ok(refs)
}

#[cfg(test)]
mod tests {
    use super::{references, RefKind};

    #[test]
    fn finds_the_allowed_shapes() {
        let src = "#[path = \"x/y.rs\"] mod y;\nconst A: &[u8] = include_bytes!(\"t.bin\");\n\
                   include!(\"gen.rs\");\nconst S: &str = core::include_str!(r#\"s.txt\"#);\n\
                   #[path = \"v.rs\"]\n#[allow(dead_code)]\npub(crate) mod v;\n\
                   const N: &str = env!(\"CARGO_PKG_NAME\");\nconst C: Option<&str> = option_env!(\"CARGO_CRATE_NAME\");\n";
        assert_eq!(
            references(src),
            Ok(vec![
                (RefKind::ModPath, "x/y.rs".to_string()),
                (RefKind::Data, "t.bin".to_string()),
                (RefKind::Code, "gen.rs".to_string()),
                (RefKind::Data, "s.txt".to_string()),
                (RefKind::ModPath, "v.rs".to_string()),
            ])
        );
    }

    #[test]
    fn ignores_comments_strings_and_chars() {
        let src = "// include!(\"a\") env!(\"HOME\") path\n/* /* include_str!(\"b\") */ */\n\
                   let s = \"include!(\\\"c\\\") path env\";\n\
                   let r = r##\"#[path = \"d\"]\"##; let q = '\"'; let e = b'\\''; fn f<'a>(x: &'a u8) {}\n";
        assert_eq!(references(src), Ok(vec![]));
    }

    #[test]
    fn every_other_shape_is_refused() {
        let cases: [(&str, &str); 17] = [
            ("include_str!(concat!(env!(\"OUT_DIR\"), \"/x\"))", "include_not_literal"),
            ("include!(\"a\\\\b.rs\")", "include_not_literal"),
            ("include!(b\"a.rs\")", "include_not_literal"),
            ("include!(\"a.rs\",)", "include_not_literal"),
            ("include_bytes![\"a.bin\"]", "include_not_literal"),
            ("use core::include_bytes as ib;", "include_not_literal"),
            ("macro_rules! m { ($p:expr) => { include_bytes!($p) } }", "include_in_macro_rules"),
            ("macro_rules! m { () => { include_bytes!(\"t.bin\") } }", "include_in_macro_rules"),
            ("macro_rules! m { () => { #[path = \"x.rs\"] mod x; } }", "path_in_macro_rules"),
            ("#[path = concat!(\"a\")] mod a;", "path_not_mod_attribute"),
            ("#[cfg_attr(unix, path = \"u.rs\")] mod u;", "path_not_mod_attribute"),
            ("p!(path = \"../x.rs\");", "path_not_mod_attribute"),
            ("#![path = \"x.rs\"]", "path_not_mod_attribute"),
            ("#[path = \"x.rs\"] fn f() {}", "path_not_mod_attribute"),
            ("let path = 1;", "path_not_mod_attribute"),
            ("const H: &str = env!(\"HOME\");", "env_not_allowed"),
            ("const M: &str = env!(\"CARGO_MANIFEST_DIR\");", "env_not_allowed"),
        ];
        for (src, reason) in cases {
            assert_eq!(references(src), Err(reason), "{src}");
        }
        assert_eq!(references("const O: Option<&str> = option_env!(\"OUT_DIR\");"), Err("env_not_allowed"));
        assert_eq!(references("fn include() {}"), Err("include_not_literal"));
        assert_eq!(references("let s = \"unterminated"), Err("source_lex"));
        assert_eq!(references("/* /* */"), Err("source_lex"));
    }
}
