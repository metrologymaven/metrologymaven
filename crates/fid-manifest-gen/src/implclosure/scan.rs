//! Finds the places in Rust source that can make the compiler read a file by path:
//! `include!`, `include_str!` and `include_bytes!` invocations and `path = ".."` inside an
//! attribute (`#[path = ".."]`, also inside `cfg_attr`). The caller checks each literal path
//! against the hashed file set (REQ-KRN.MAN.MANI.IMPL-001: no compiled byte escapes the digest).
//!
//! The scan is a small lexer, not a parser. It skips line and nested block comments, string,
//! byte-string, C-string and raw-string literals and char literals, so text that only looks
//! like an include is ignored. It fails closed: an include or a `path =` whose argument is not
//! one plain string literal (a macro argument, `concat!`, `env!`, an escaped string) is an
//! error, and so is an unterminated literal or comment.

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

/// Marks the tokens inside each attribute `#[..]` or `#![..]`.
fn attribute_mask(t: &[Tok]) -> Vec<bool> {
    let mut mask = vec![false; t.len()];
    let mut k = 0;
    while k < t.len() {
        if t[k] == Tok::Punct(b'#') {
            let mut open = k + 1;
            if t.get(open) == Some(&Tok::Punct(b'!')) {
                open += 1;
            }
            if t.get(open) == Some(&Tok::Punct(b'[')) {
                let mut depth = 0usize;
                let mut j = open;
                while j < t.len() {
                    match t[j] {
                        Tok::Punct(b'[') => depth += 1,
                        Tok::Punct(b']') => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    mask[j] = true;
                    j += 1;
                }
                k = j;
            }
        }
        k += 1;
    }
    mask
}

/// Every file reference in `src`, in source order. Errors are static reason codes.
pub(crate) fn references(src: &str) -> Result<Vec<(RefKind, String)>, &'static str> {
    let t = lex(src)?;
    let in_attr = attribute_mask(&t);
    let mut refs = Vec::new();
    for (k, tok) in t.iter().enumerate() {
        let Tok::Ident(word) = tok else { continue };
        let kind = match word.as_str() {
            "include" => Some(RefKind::Code),
            "include_str" | "include_bytes" => Some(RefKind::Data),
            _ => None,
        };
        if let Some(kind) = kind {
            if t.get(k + 1) == Some(&Tok::Punct(b'!')) {
                match (t.get(k + 2), t.get(k + 3), t.get(k + 4)) {
                    (
                        Some(Tok::Punct(b'(' | b'[' | b'{')),
                        Some(Tok::Str(Some(path))),
                        Some(Tok::Punct(b')' | b']' | b'}' | b',')),
                    ) => refs.push((kind, path.clone())),
                    _ => return Err("include_not_literal"),
                }
            }
        } else if word == "path" && in_attr[k] && t.get(k + 1) == Some(&Tok::Punct(b'=')) {
            match t.get(k + 2) {
                Some(Tok::Str(Some(path))) => refs.push((RefKind::ModPath, path.clone())),
                _ => return Err("path_attr_not_literal"),
            }
        }
    }
    Ok(refs)
}

#[cfg(test)]
mod tests {
    use super::{references, RefKind};

    #[test]
    fn finds_includes_and_path_attributes() {
        let src = "#[path = \"x/y.rs\"] mod y;\nconst A: &[u8] = include_bytes!(\"t.bin\");\n\
                   include!(\"gen.rs\");\nconst S: &str = core::include_str!(r#\"s.txt\"#);\n\
                   #[cfg_attr(unix, path = \"u.rs\")] mod u;\n";
        assert_eq!(
            references(src),
            Ok(vec![
                (RefKind::ModPath, "x/y.rs".to_string()),
                (RefKind::Data, "t.bin".to_string()),
                (RefKind::Code, "gen.rs".to_string()),
                (RefKind::Data, "s.txt".to_string()),
                (RefKind::ModPath, "u.rs".to_string()),
            ])
        );
    }

    #[test]
    fn ignores_comments_strings_chars_and_plain_identifiers() {
        let src = "// include!(\"a\")\n/* /* include_str!(\"b\") */ */\nlet s = \"include!(\\\"c\\\")\";\n\
                   let r = r##\"#[path = \"d\"]\"##; let q = '\"'; let e = b'\\''; fn f<'a>(x: &'a u8) {}\n\
                   let path = \"not an attribute\"; fn include() {} let include_str = 1;\n";
        assert_eq!(references(src), Ok(vec![]));
    }

    #[test]
    fn fails_closed() {
        assert_eq!(references("include_str!(concat!(env!(\"OUT_DIR\"), \"/x\"))"), Err("include_not_literal"));
        assert_eq!(references("macro_rules! m { ($p:expr) => { include_bytes!($p) } }"), Err("include_not_literal"));
        assert_eq!(references("include!(\"a\\\\b.rs\")"), Err("include_not_literal"));
        assert_eq!(references("include!(b\"a.rs\")"), Err("include_not_literal"));
        assert_eq!(references("#[path = concat!(\"a\")] mod a;"), Err("path_attr_not_literal"));
        assert_eq!(references("let s = \"unterminated"), Err("source_lex"));
        assert_eq!(references("/* /* */"), Err("source_lex"));
    }
}
