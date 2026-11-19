//! GPU readback scan of non-test Rust source (REQ-VIZ.RENDER.FRAME.SHLINT-002).
//!
//! A small tokeniser drops comments and literals (so `map_async` in a string or comment does
//! not count), items under an outer `#[cfg(test)]` and scopes under an inner `#![cfg(test)]`
//! are removed, and then every `map_async` token is checked:
//!
//! - `<recv>.map_async(<mode>, ..)` or `<recv>.slice(..).map_async(<mode>, ..)`: when the first
//!   argument names `Write` and not `Read` it is an upload and is ignored; otherwise it is a
//!   readback, and the last identifier of `<recv>` must be a name the manifest classifies as a
//!   colour, depth or ID target ([`crate::Rule::Readback`] otherwise).
//! - any other shape (a receiver that is a call, an index or `self`, a path call such as
//!   `BufferSlice::map_async(..)`, a function named `map_async`) is
//!   [`crate::Rule::UnresolvedReadback`]: the lint fails closed when it cannot tell what is read.
//!
//! Only the exact attribute `cfg(test)` marks test code; `cfg(any(test, ..))` and friends are
//! scanned like normal code.

use crate::manifest::{ReadbackClass, ShaderManifest};
use crate::{Finding, Rule, ShlintError, MANIFEST_PATH};

/// A token: identifiers (and numbers) keep their text, literals are opaque, the rest is one
/// ASCII punctuation byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tok {
    Ident(String),
    /// A plain `"..."` string literal; the text between the quotes, escapes not decoded.
    Str(String),
    /// Any other literal (numbers, characters, byte, C and raw strings).
    Lit,
    Punct(u8),
}

/// A token with its 1-based line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub tok: Tok,
    pub line: u32,
}

/// Tokenises Rust source. Errors are (line, reason).
pub fn lex(src: &str) -> Result<Vec<Token>, (u32, String)> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut line: u32 = 1;
    let mut i = 0usize;
    while let Some(&c) = b.get(i) {
        let start_line = line;
        match c {
            b'\n' => {
                line = line.saturating_add(1);
                i += 1;
            }
            b' ' | b'\t' | b'\r' | 0x0b | 0x0c => i += 1,
            b'/' if b.get(i + 1) == Some(&b'/') => {
                while b.get(i).is_some_and(|&x| x != b'\n') {
                    i += 1;
                }
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                i = skip_block_comment(b, i, &mut line)?;
            }
            b'"' => {
                let end = skip_string(b, i + 1, &mut line, start_line)?;
                let text = src
                    .get(i + 1..end.saturating_sub(1))
                    .ok_or_else(|| (start_line, "string literal is not on a character boundary".to_string()))?;
                out.push(Token { tok: Tok::Str(text.to_string()), line: start_line });
                i = end;
            }
            b'\'' => {
                i = lex_quote(b, i, &mut out, start_line)?;
            }
            _ if c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80 => {
                if let Some(next) = literal_with_prefix(b, i, &mut line, start_line)? {
                    i = next;
                    out.push(Token { tok: Tok::Lit, line: start_line });
                    continue;
                }
                let s = i;
                while b.get(i).is_some_and(|&x| x.is_ascii_alphanumeric() || x == b'_' || x >= 0x80) {
                    i += 1;
                }
                let text = src
                    .get(s..i)
                    .ok_or_else(|| (start_line, "identifier is not on a character boundary".to_string()))?;
                let tok = if c.is_ascii_digit() { Tok::Lit } else { Tok::Ident(text.to_string()) };
                out.push(Token { tok, line: start_line });
            }
            _ => {
                out.push(Token { tok: Tok::Punct(c), line: start_line });
                i += 1;
            }
        }
    }
    Ok(out)
}

/// Skips a (nested) block comment starting at `i` (the `/`); returns the index after it.
fn skip_block_comment(b: &[u8], mut i: usize, line: &mut u32) -> Result<usize, (u32, String)> {
    let start = *line;
    let mut depth = 0usize;
    loop {
        match (b.get(i), b.get(i + 1)) {
            (Some(b'/'), Some(b'*')) => {
                depth += 1;
                i += 2;
            }
            (Some(b'*'), Some(b'/')) => {
                depth = depth.saturating_sub(1);
                i += 2;
                if depth == 0 {
                    return Ok(i);
                }
            }
            (Some(b'\n'), _) => {
                *line = line.saturating_add(1);
                i += 1;
            }
            (Some(_), _) => i += 1,
            (None, _) => return Err((start, "unterminated block comment".to_string())),
        }
    }
}

/// Skips the body of a `"..."` literal whose first content byte is at `i`; returns the index
/// after the closing quote.
fn skip_string(b: &[u8], mut i: usize, line: &mut u32, start: u32) -> Result<usize, (u32, String)> {
    loop {
        match b.get(i) {
            Some(b'"') => return Ok(i + 1),
            Some(b'\\') => {
                if b.get(i + 1) == Some(&b'\n') {
                    *line = line.saturating_add(1);
                }
                i += 2;
            }
            Some(b'\n') => {
                *line = line.saturating_add(1);
                i += 1;
            }
            Some(_) => i += 1,
            None => return Err((start, "unterminated string literal".to_string())),
        }
    }
}

/// Skips a raw string body: `i` is at the first `#` or `"` after the `r`.
fn skip_raw_string(b: &[u8], mut i: usize, line: &mut u32, start: u32) -> Result<usize, (u32, String)> {
    let mut hashes = 0usize;
    while b.get(i) == Some(&b'#') {
        hashes += 1;
        i += 1;
    }
    if b.get(i) != Some(&b'"') {
        return Err((start, "malformed raw string literal".to_string()));
    }
    i += 1;
    loop {
        match b.get(i) {
            Some(b'"') => {
                let end = i + 1 + hashes;
                if b.get(i + 1..end).is_some_and(|h| h.iter().all(|&x| x == b'#')) {
                    return Ok(end);
                }
                i += 1;
            }
            Some(b'\n') => {
                *line = line.saturating_add(1);
                i += 1;
            }
            Some(_) => i += 1,
            None => return Err((start, "unterminated raw string literal".to_string())),
        }
    }
}

/// String-like literals that start with a letter: `r"…"`, `r#"…"#`, `b"…"`, `br"…"`, `c"…"`,
/// `cr"…"`, `b'…'`. Returns the index after the literal, or `None` when `i` starts an identifier
/// (including a raw identifier such as `r#type`).
fn literal_with_prefix(b: &[u8], i: usize, line: &mut u32, start: u32) -> Result<Option<usize>, (u32, String)> {
    let mut p = i;
    let prefixed = matches!(b.get(p), Some(b'b' | b'c'));
    if prefixed {
        p += 1;
    }
    if b.get(p) == Some(&b'r') {
        let mut q = p + 1;
        while b.get(q) == Some(&b'#') {
            q += 1;
        }
        if b.get(q) == Some(&b'"') {
            return skip_raw_string(b, p + 1, line, start).map(Some);
        }
        return Ok(None);
    }
    if prefixed && b.get(p) == Some(&b'"') {
        return skip_string(b, p + 1, line, start).map(Some);
    }
    if b.get(i) == Some(&b'b') && b.get(p) == Some(&b'\'') {
        let mut scratch = Vec::new();
        return lex_quote(b, p, &mut scratch, start).map(Some);
    }
    Ok(None)
}

/// At a `'`: a character literal (pushed as [`Tok::Lit`]) or a lifetime or label (pushed as
/// punctuation; the name that follows is lexed as an identifier). Returns the next index.
fn lex_quote(b: &[u8], i: usize, out: &mut Vec<Token>, line: u32) -> Result<usize, (u32, String)> {
    match b.get(i + 1) {
        None | Some(b'\n') => Err((line, "stray `'`".to_string())),
        Some(b'\\') => {
            // Escape: the closing quote comes after at least one escaped byte.
            let mut j = i + 3;
            while let Some(&x) = b.get(j) {
                if x == b'\'' {
                    out.push(Token { tok: Tok::Lit, line });
                    return Ok(j + 1);
                }
                if x == b'\n' || j > i + 12 {
                    break;
                }
                j += 1;
            }
            Err((line, "malformed character literal".to_string()))
        }
        Some(&c) => {
            let len = utf8_len(c);
            if b.get(i + 1 + len) == Some(&b'\'') {
                out.push(Token { tok: Tok::Lit, line });
                Ok(i + 2 + len)
            } else {
                out.push(Token { tok: Tok::Punct(b'\''), line });
                Ok(i + 1)
            }
        }
    }
}

fn utf8_len(first: u8) -> usize {
    match first {
        0xf0..=0xff => 4,
        0xe0..=0xef => 3,
        0xc0..=0xdf => 2,
        _ => 1,
    }
}

fn is_punct(t: Option<&Token>, c: u8) -> bool {
    matches!(t, Some(Token { tok: Tok::Punct(p), .. }) if *p == c)
}

fn is_ident(t: Option<&Token>, s: &str) -> bool {
    matches!(t, Some(Token { tok: Tok::Ident(x), .. }) if x == s)
}

fn opener(c: u8) -> Option<u8> {
    match c {
        b')' => Some(b'('),
        b']' => Some(b'['),
        b'}' => Some(b'{'),
        _ => None,
    }
}

/// For a balanced token list: `partner[k]` is the index of the bracket matching bracket `k`.
fn match_brackets(toks: &[Token]) -> Result<Vec<Option<usize>>, (u32, String)> {
    let mut partner = vec![None; toks.len()];
    let mut stack: Vec<(usize, u8)> = Vec::new();
    for (k, t) in toks.iter().enumerate() {
        if let Tok::Punct(c) = t.tok {
            if matches!(c, b'(' | b'[' | b'{') {
                stack.push((k, c));
            } else if let Some(open) = opener(c) {
                match stack.pop() {
                    Some((o, oc)) if oc == open => {
                        partner[k] = Some(o);
                        partner[o] = Some(k);
                    }
                    _ => return Err((t.line, format!("unbalanced `{}`", c as char))),
                }
            }
        }
    }
    if let Some((o, _)) = stack.last() {
        let line = toks.get(*o).map_or(0, |t| t.line);
        return Err((line, "unclosed bracket".to_string()));
    }
    Ok(partner)
}

/// Keywords after which a `,` does not end a `#[cfg(test)]` item (the item has a body or a `;`).
const ITEM_KEYWORDS: [&str; 17] = [
    "fn",
    "let",
    "mod",
    "impl",
    "struct",
    "enum",
    "trait",
    "type",
    "use",
    "const",
    "static",
    "pub",
    "async",
    "unsafe",
    "extern",
    "union",
    "macro_rules",
];

/// Removes items under an outer `#[cfg(test)]` and scopes under an inner `#![cfg(test)]`.
pub fn strip_cfg_test(toks: &[Token]) -> Result<Vec<Token>, (u32, String)> {
    let partner = match_brackets(toks)?;
    let mut out = Vec::with_capacity(toks.len());
    let mut k = 0usize;
    while k < toks.len() {
        if let Some(after) = cfg_test_attr(toks, k, false) {
            k = skip_item(toks, &partner, skip_attrs(toks, &partner, after));
            continue;
        }
        if let Some(after) = cfg_test_attr(toks, k, true) {
            k = skip_scope(toks, &partner, after);
            continue;
        }
        if let Some(t) = toks.get(k) {
            out.push(t.clone());
        }
        k += 1;
    }
    Ok(out)
}

/// Matches `#[cfg(test)]` (or `#![cfg(test)]` when `inner`) at `k`; returns the index after it.
fn cfg_test_attr(toks: &[Token], k: usize, inner: bool) -> Option<usize> {
    if !is_punct(toks.get(k), b'#') {
        return None;
    }
    let mut p = k + 1;
    if inner {
        if !is_punct(toks.get(p), b'!') {
            return None;
        }
        p += 1;
    }
    let ok = is_punct(toks.get(p), b'[')
        && is_ident(toks.get(p + 1), "cfg")
        && is_punct(toks.get(p + 2), b'(')
        && is_ident(toks.get(p + 3), "test")
        && is_punct(toks.get(p + 4), b')')
        && is_punct(toks.get(p + 5), b']');
    ok.then_some(p + 6)
}

/// Skips further outer attributes `#[...]` starting at `k`.
fn skip_attrs(toks: &[Token], partner: &[Option<usize>], mut k: usize) -> usize {
    while is_punct(toks.get(k), b'#') && is_punct(toks.get(k + 1), b'[') {
        match partner.get(k + 1).copied().flatten() {
            Some(close) => k = close + 1,
            None => break,
        }
    }
    k
}

/// Skips one item, statement, field or arm starting at `k`; returns the index after it.
fn skip_item(toks: &[Token], partner: &[Option<usize>], start: usize) -> usize {
    let keyword_item =
        matches!(toks.get(start), Some(Token { tok: Tok::Ident(s), .. }) if ITEM_KEYWORDS.contains(&s.as_str()));
    let mut k = start;
    while let Some(t) = toks.get(k) {
        match t.tok {
            Tok::Punct(b'(' | b'[') => match partner.get(k).copied().flatten() {
                Some(close) => k = close + 1,
                None => return toks.len(),
            },
            Tok::Punct(b'{') => {
                return match partner.get(k).copied().flatten() {
                    // A brace group ends the item, except a struct-literal-like `= S { .. };`
                    // where a `;` follows directly.
                    Some(close) if is_punct(toks.get(close + 1), b';') => close + 2,
                    Some(close) => close + 1,
                    None => toks.len(),
                };
            }
            Tok::Punct(b';') => return k + 1,
            Tok::Punct(b',') if !keyword_item => return k + 1,
            // The end of the enclosing scope: the item ends here; the bracket is kept.
            Tok::Punct(b')' | b']' | b'}') => return k,
            _ => k += 1,
        }
    }
    k
}

/// Skips to the end of the current scope (the closing bracket is kept) or of the file.
fn skip_scope(toks: &[Token], partner: &[Option<usize>], mut k: usize) -> usize {
    while let Some(t) = toks.get(k) {
        match t.tok {
            Tok::Punct(b'(' | b'[' | b'{') => match partner.get(k).copied().flatten() {
                Some(close) => k = close + 1,
                None => return toks.len(),
            },
            Tok::Punct(b')' | b']' | b'}') => return k,
            _ => k += 1,
        }
    }
    k
}

/// wgpu helpers that read a buffer back without a `map_async` call this scan can attribute
/// (`wgpu::util::DownloadBuffer::read_buffer`).
pub const READBACK_HELPERS: [&str; 2] = ["DownloadBuffer", "read_buffer"];

/// Macros that embed a file's text or bytes; only a listed `.wgsl` module may be embedded.
const INCLUDE_MACROS: [&str; 3] = ["include_str", "include_bytes", "include_wgsl"];

/// Macros that embed SPIR-V, which never goes through the WGSL manifest.
const SPIRV_MACROS: [&str; 2] = ["include_spirv", "include_spirv_raw"];

/// Rust keywords that cannot name a mapped buffer.
const KEYWORDS: [&str; 12] =
    ["self", "Self", "super", "crate", "await", "match", "if", "else", "return", "move", "in", "as"];

/// Scans one non-test Rust file (path relative to the crate directory) for readbacks.
pub fn scan(rel: &str, src: &str, manifest: &ShaderManifest) -> Result<Vec<Finding>, ShlintError> {
    let parse_err = |(line, reason): (u32, String)| ShlintError::RustParse { path: rel.to_string(), line, reason };
    let toks = lex(src).map_err(parse_err)?;
    let toks = strip_cfg_test(&toks).map_err(parse_err)?;
    let partner = match_brackets(&toks).map_err(parse_err)?;
    let mut findings = Vec::new();
    for (k, t) in toks.iter().enumerate() {
        if let Tok::Ident(s) = &t.tok {
            if READBACK_HELPERS.contains(&s.as_str()) {
                findings.push(Finding {
                    location: format!("{rel}:{}", t.line),
                    rule: Rule::UnresolvedReadback,
                    message: format!("uses wgpu readback helper `{s}`; cannot tell which target is read"),
                });
                continue;
            }
        }
        if !matches!(&t.tok, Tok::Ident(s) if s == "map_async") {
            continue;
        }
        let location = format!("{rel}:{}", t.line);
        let unresolved = |why: &str| Finding {
            location: location.clone(),
            rule: Rule::UnresolvedReadback,
            message: format!("map_async {why}; cannot tell which target is read"),
        };
        let open = k + 1;
        let close = if is_punct(toks.get(open), b'(') { partner.get(open).copied().flatten() } else { None };
        let (Some(close), true) = (close, k > 0 && is_punct(toks.get(k - 1), b'.')) else {
            findings.push(unresolved("is not called as a method"));
            continue;
        };
        if is_write_mapping(&toks, &partner, open + 1, close) {
            continue;
        }
        match receiver(&toks, &partner, k.saturating_sub(2), k >= 2) {
            None => findings.push(unresolved("is called on an expression that is not a named buffer")),
            Some(name) => match manifest.readback_class(&name) {
                ReadbackClass::Target => {}
                ReadbackClass::Input => findings.push(Finding {
                    location: location.clone(),
                    rule: Rule::Readback,
                    message: format!(
                        "maps `{name}` for reading, which {MANIFEST_PATH} classifies as input, not as a colour, depth or ID target"
                    ),
                }),
                ReadbackClass::Unknown => findings.push(Finding {
                    location: location.clone(),
                    rule: Rule::Readback,
                    message: format!(
                        "maps `{name}` for reading, which {MANIFEST_PATH} does not declare as a colour, depth or ID target"
                    ),
                }),
            },
        }
    }
    Ok(findings)
}

/// True when the first argument (tokens `from..close` up to the first top-level `,`) names
/// `Write` and not `Read`: an upload mapping. Anything else counts as a read (fail closed).
fn is_write_mapping(toks: &[Token], partner: &[Option<usize>], from: usize, close: usize) -> bool {
    let mut write = false;
    let mut read = false;
    let mut k = from;
    while k < close {
        match toks.get(k).map(|t| &t.tok) {
            Some(Tok::Punct(b',')) => break,
            Some(Tok::Punct(b'(' | b'[' | b'{')) => match partner.get(k).copied().flatten() {
                Some(c) if c < close => {
                    // Look inside nested groups too.
                    for t in toks.get(k..c).unwrap_or(&[]) {
                        match &t.tok {
                            Tok::Ident(s) if s == "Write" => write = true,
                            Tok::Ident(s) if s == "Read" => read = true,
                            _ => {}
                        }
                    }
                    k = c + 1;
                }
                _ => return false,
            },
            Some(Tok::Ident(s)) => {
                if s == "Write" {
                    write = true;
                } else if s == "Read" {
                    read = true;
                }
                k += 1;
            }
            Some(_) => k += 1,
            None => return false,
        }
    }
    write && !read
}

/// The name of the buffer or texture a `.map_async` call maps: `j` is the token before the `.`.
/// `x.slice(..)` is looked through; any other call, index or keyword gives `None`.
fn receiver(toks: &[Token], partner: &[Option<usize>], mut j: usize, valid: bool) -> Option<String> {
    if !valid {
        return None;
    }
    loop {
        match &toks.get(j)?.tok {
            Tok::Ident(name) if !KEYWORDS.contains(&name.as_str()) => return Some(name.clone()),
            Tok::Punct(b')') => {
                let open = partner.get(j).copied().flatten()?;
                if open >= 3 && is_ident(toks.get(open - 1), "slice") && is_punct(toks.get(open - 2), b'.') {
                    j = open - 3;
                } else {
                    return None;
                }
            }
            _ => return None,
        }
    }
}

/// Shader text that bypasses the manifest (REQ-001, fail closed), in non-test code:
///
/// - `include_str!`, `include_bytes!` or `include_wgsl!` whose argument is not one plain string
///   literal, names a file that is not `.wgsl`, or resolves (relative to the source file) to a
///   `.wgsl` path that the manifest does not list;
/// - `include_spirv!` and `include_spirv_raw!`;
/// - `ShaderSource::<variant>` other than `Wgsl`;
/// - `ShaderSource::Wgsl(..)` whose argument holds a string literal outside an include macro,
///   or no include macro at all (a variable or computed text cannot be tied to a module).
pub fn scan_shader_sources(rel: &str, src: &str, manifest: &ShaderManifest) -> Result<Vec<Finding>, ShlintError> {
    let parse_err = |(line, reason): (u32, String)| ShlintError::RustParse { path: rel.to_string(), line, reason };
    let toks = lex(src).map_err(parse_err)?;
    let toks = strip_cfg_test(&toks).map_err(parse_err)?;
    let partner = match_brackets(&toks).map_err(parse_err)?;
    let mut findings = Vec::new();
    let mut push = |line: u32, message: String| {
        findings.push(Finding { location: format!("{rel}:{line}"), rule: Rule::UnmanifestedShader, message });
    };
    for (k, t) in toks.iter().enumerate() {
        let Tok::Ident(name) = &t.tok else {
            continue;
        };
        let is_macro = is_punct(toks.get(k + 1), b'!');
        if is_macro && SPIRV_MACROS.contains(&name.as_str()) {
            push(t.line, format!("{name}! embeds SPIR-V, which bypasses {MANIFEST_PATH}"));
        } else if is_macro && INCLUDE_MACROS.contains(&name.as_str()) {
            if let Some(m) = check_include(rel, name, &toks, k, manifest) {
                push(t.line, m);
            }
        } else if name == "ShaderSource" && is_punct(toks.get(k + 1), b':') && is_punct(toks.get(k + 2), b':') {
            let Some(Token { tok: Tok::Ident(variant), .. }) = toks.get(k + 3) else {
                continue;
            };
            if variant != "Wgsl" {
                push(t.line, format!("ShaderSource::{variant} bypasses {MANIFEST_PATH}"));
                continue;
            }
            let open = k + 4;
            let close = if is_punct(toks.get(open), b'(') { partner.get(open).copied().flatten() } else { None };
            let Some(close) = close else {
                push(t.line, "ShaderSource::Wgsl is not called with an argument".to_string());
                continue;
            };
            if let Some(m) = check_wgsl_arg(&toks, open + 1, close) {
                push(t.line, m);
            }
        }
    }
    Ok(findings)
}

/// The string literal argument of the macro at `k` (`name ! ( "..." )`), if it is exactly one.
fn macro_string_arg(toks: &[Token], k: usize) -> Option<&str> {
    let delimited = is_punct(toks.get(k + 2), b'(') && is_punct(toks.get(k + 4), b')');
    match toks.get(k + 3) {
        Some(Token { tok: Tok::Str(s), .. }) if delimited => Some(s.as_str()),
        _ => None,
    }
}

fn check_include(rel: &str, name: &str, toks: &[Token], k: usize, manifest: &ShaderManifest) -> Option<String> {
    let Some(arg) = macro_string_arg(toks, k) else {
        return Some(format!("{name}! argument is not one plain string literal; cannot tie it to a listed module"));
    };
    if arg.contains('\\') || !arg.ends_with(".wgsl") {
        return Some(format!("{name}! of `{arg}`, which is not a .wgsl module listed in {MANIFEST_PATH}"));
    }
    match resolve_relative(rel, arg) {
        Some(path) if manifest.module(&path).is_some() => None,
        Some(path) => Some(format!("{name}! of `{arg}` ({path}), which is not listed in {MANIFEST_PATH}")),
        None => Some(format!("{name}! of `{arg}` points outside the crate")),
    }
}

/// `arg` resolved against the folder of `rel`, normalised; `None` when it leaves the crate.
pub fn resolve_relative(rel: &str, arg: &str) -> Option<String> {
    let mut parts: Vec<&str> = rel.split('/').collect();
    parts.pop();
    if arg.starts_with('/') {
        return None;
    }
    for seg in arg.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
}

/// A `ShaderSource::Wgsl(..)` argument is accepted only when it contains an include macro and
/// every string literal in it is that macro's argument.
fn check_wgsl_arg(toks: &[Token], from: usize, close: usize) -> Option<String> {
    let mut has_include = false;
    for k in from..close {
        match toks.get(k).map(|t| &t.tok) {
            Some(Tok::Ident(n)) if INCLUDE_MACROS.contains(&n.as_str()) && is_punct(toks.get(k + 1), b'!') => {
                has_include = true;
            }
            Some(Tok::Str(_)) => {
                let in_macro = k >= 3
                    && is_punct(toks.get(k - 1), b'(')
                    && is_punct(toks.get(k - 2), b'!')
                    && matches!(toks.get(k - 3), Some(Token { tok: Tok::Ident(n), .. }) if INCLUDE_MACROS.contains(&n.as_str()));
                if !in_macro {
                    return Some("ShaderSource::Wgsl holds inline WGSL text, which bypasses the manifest".to_string());
                }
            }
            _ => {}
        }
    }
    if has_include {
        None
    } else {
        Some("ShaderSource::Wgsl argument is not an include of a listed .wgsl module".to_string())
    }
}
