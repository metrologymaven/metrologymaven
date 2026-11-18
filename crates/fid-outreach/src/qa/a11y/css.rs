//! Bounded CSS reader: comments, style rules, nested at-rule blocks and declarations, plus
//! colour literal detection, `var()` resolution against token values and WCAG contrast.

use super::html::LineIndex;
use super::vocab::NAMED_COLOURS;
use std::collections::BTreeMap;

/// Deepest accepted block nesting in a stylesheet.
pub const MAX_DEPTH: usize = 32;
/// Deepest `var()` / alias chain followed during resolution.
const MAX_VAR_DEPTH: usize = 8;

/// One declaration. Property names are lowercased except custom properties.
#[derive(Debug, Clone)]
pub struct Decl {
    pub name: String,
    pub value: String,
    pub line: u32,
}

/// A style rule (or a declaration-only at-rule such as `@font-face`).
#[derive(Debug, Clone)]
pub struct StyleRule {
    pub selector: String,
    /// Line of the selector (or at-rule prelude).
    pub line: u32,
    pub decls: Vec<Decl>,
    /// `Some("dark")` / `Some("light")` inside a `prefers-color-scheme` media block.
    pub scheme: Option<String>,
    /// False for at-rule blocks (`@font-face`, `@keyframes` frames, ...): no contrast pair.
    pub contrast: bool,
}

/// Replaces comments with spaces, keeping newlines so that lines stay correct.
fn strip_comments(src: &str) -> Result<String, String> {
    let b = src.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    let mut quote: Option<u8> = None;
    while let Some(&c) = b.get(i) {
        if let Some(q) = quote {
            out.push(c);
            if c == b'\\' {
                if let Some(&d) = b.get(i + 1) {
                    out.push(d);
                    i += 2;
                    continue;
                }
            } else if c == q || c == b'\n' {
                quote = None;
            }
            i += 1;
            continue;
        }
        if c == b'"' || c == b'\'' {
            quote = Some(c);
            out.push(c);
            i += 1;
            continue;
        }
        if c == b'/' && b.get(i + 1) == Some(&b'*') {
            let end = b
                .get(i + 2..)
                .and_then(|h| h.windows(2).position(|w| w == b"*/"))
                .map_or(b.len(), |p| i + 2 + p + 2);
            for d in b.get(i..end).unwrap_or(&[]) {
                out.push(if *d == b'\n' { b'\n' } else { b' ' });
            }
            i = end;
            continue;
        }
        out.push(c);
        i += 1;
    }
    String::from_utf8(out).map_err(|_| "invalid UTF-8".to_string())
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
    lines: LineIndex,
    base: u32,
    out: Vec<StyleRule>,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    fn line(&self, off: usize) -> u32 {
        self.base.saturating_add(self.lines.line_of(off).saturating_sub(1))
    }

    fn slice(&self, from: usize, to: usize) -> String {
        self.b.get(from..to).and_then(|x| std::str::from_utf8(x).ok()).unwrap_or("").to_string()
    }

    fn skip_ws(&mut self) {
        while self.peek().is_some_and(|c| c.is_ascii_whitespace()) {
            self.i += 1;
        }
    }

    /// Advances to the first byte in `stops` outside strings, parentheses and brackets.
    fn scan_until(&mut self, stops: &[u8]) -> usize {
        let mut depth = 0usize;
        while let Some(c) = self.peek() {
            if depth == 0 && stops.contains(&c) {
                return self.i;
            }
            match c {
                b'"' | b'\'' => {
                    self.i += 1;
                    while let Some(d) = self.peek() {
                        self.i += 1;
                        if d == b'\\' {
                            self.i += 1;
                        } else if d == c || d == b'\n' {
                            break;
                        }
                    }
                    continue;
                }
                b'(' | b'[' => depth += 1,
                b')' | b']' => depth = depth.saturating_sub(1),
                _ => {}
            }
            self.i += 1;
        }
        self.b.len()
    }

    fn block(&mut self, depth: usize, scheme: Option<&str>, contrast: bool, nested: bool) -> Result<(), String> {
        if depth > MAX_DEPTH {
            return Err(format!("block nesting deeper than {MAX_DEPTH} at line {}", self.line(self.i)));
        }
        loop {
            self.skip_ws();
            let Some(c) = self.peek() else { return Ok(()) };
            if c == b'}' {
                self.i += 1;
                if nested {
                    return Ok(());
                }
                continue;
            }
            let start = self.i;
            if c == b'@' {
                let end = self.scan_until(b"{;}");
                let prelude = self.slice(start, end).trim().to_string();
                match self.peek() {
                    Some(b'{') => {
                        self.i += 1;
                        let kw: String = prelude
                            .chars()
                            .skip(1)
                            .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '-')
                            .collect::<String>()
                            .to_ascii_lowercase();
                        let pl = prelude.to_ascii_lowercase();
                        match kw.as_str() {
                            "media" => {
                                let sch = if pl.contains("prefers-color-scheme") {
                                    if pl.contains("dark") {
                                        Some("dark")
                                    } else if pl.contains("light") {
                                        Some("light")
                                    } else {
                                        scheme
                                    }
                                } else {
                                    scheme
                                };
                                self.block(depth + 1, sch, contrast, true)?;
                            }
                            "supports" | "layer" | "container" | "document" | "scope" | "starting-style" => {
                                self.block(depth + 1, scheme, contrast, true)?;
                            }
                            "keyframes" | "-webkit-keyframes" => self.block(depth + 1, scheme, false, true)?,
                            _ => {
                                let decls = self.decl_block(depth + 1)?;
                                self.out.push(StyleRule {
                                    selector: prelude,
                                    line: self.line(start),
                                    decls,
                                    scheme: scheme.map(str::to_string),
                                    contrast: false,
                                });
                            }
                        }
                    }
                    Some(b';') => self.i += 1,
                    _ => {}
                }
                continue;
            }
            let end = self.scan_until(b"{}");
            if self.peek() == Some(b'{') {
                self.i += 1;
                let selector = self.slice(start, end).trim().to_string();
                let decls = self.decl_block(depth + 1)?;
                let line = self.line(start);
                self.out.push(StyleRule { selector, line, decls, scheme: scheme.map(str::to_string), contrast });
            }
        }
    }

    fn skip_block(&mut self, depth: usize) -> Result<(), String> {
        if depth > MAX_DEPTH {
            return Err(format!("block nesting deeper than {MAX_DEPTH} at line {}", self.line(self.i)));
        }
        loop {
            self.scan_until(b"{}");
            match self.peek() {
                None => return Ok(()),
                Some(b'{') => {
                    self.i += 1;
                    self.skip_block(depth + 1)?;
                }
                Some(_) => {
                    self.i += 1;
                    return Ok(());
                }
            }
        }
    }

    fn decl_block(&mut self, depth: usize) -> Result<Vec<Decl>, String> {
        let mut decls = Vec::new();
        loop {
            self.skip_ws();
            match self.peek() {
                None => return Ok(decls),
                Some(b'}') => {
                    self.i += 1;
                    return Ok(decls);
                }
                Some(b';') => {
                    self.i += 1;
                    continue;
                }
                Some(_) => {}
            }
            let start = self.i;
            let end = self.scan_until(b";{}");
            if self.peek() == Some(b'{') {
                // Nested rule (CSS nesting): not evaluated.
                self.i += 1;
                self.skip_block(depth + 1)?;
                continue;
            }
            if let Some(d) = make_decl(&self.slice(start, end), self.line(start)) {
                decls.push(d);
            }
        }
    }
}

fn make_decl(text: &str, line: u32) -> Option<Decl> {
    let (n, v) = text.split_once(':')?;
    let n = n.trim();
    if n.is_empty() {
        return None;
    }
    let name = if n.starts_with("--") { n.to_string() } else { n.to_ascii_lowercase() };
    Some(Decl { name, value: v.trim().to_string(), line })
}

/// Parses a stylesheet whose first line is line `base_line` of its file.
pub fn parse_sheet(src: &str, base_line: u32) -> Result<Vec<StyleRule>, String> {
    let clean = strip_comments(src)?;
    let b = clean.as_bytes();
    let mut p = Parser { b, i: 0, lines: LineIndex::new(b), base: base_line, out: Vec::new() };
    p.block(0, None, true, false)?;
    Ok(p.out)
}

/// Parses the declarations of a `style` attribute (all on the element's line).
pub fn parse_inline(src: &str, line: u32) -> Vec<Decl> {
    let Ok(clean) = strip_comments(src) else { return Vec::new() };
    let b = clean.as_bytes();
    let mut p = Parser { b, i: 0, lines: LineIndex::new(b), base: line, out: Vec::new() };
    let mut decls = Vec::new();
    loop {
        p.skip_ws();
        match p.peek() {
            None => break,
            Some(b';' | b'}' | b'{') => {
                p.i += 1;
                continue;
            }
            Some(_) => {}
        }
        let start = p.i;
        let end = p.scan_until(b";");
        if let Some(d) = make_decl(&p.slice(start, end), line) {
            decls.push(d);
        }
    }
    decls
}

const COLOUR_FUNCS: &[&str] =
    &["rgb", "rgba", "hsl", "hsla", "hwb", "lab", "lch", "oklab", "oklch", "color", "color-mix", "light-dark"];

/// Properties whose identifiers may be named colours.
fn colour_property(prop: &str) -> bool {
    prop.starts_with("--")
        || prop.starts_with("border")
        || prop.starts_with("outline")
        || prop.starts_with("background")
        || prop.starts_with("text-decoration")
        || prop.starts_with("column-rule")
        || matches!(
            prop,
            "color"
                | "fill"
                | "stroke"
                | "box-shadow"
                | "text-shadow"
                | "caret-color"
                | "accent-color"
                | "stop-color"
                | "flood-color"
                | "lighting-color"
                | "scrollbar-color"
                | "text-emphasis-color"
                | "text-emphasis"
        )
}

fn ident_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'-' || c == b'_' || c >= 0x80
}

/// Index just past the parenthesis that closes the one opened before `from`.
fn close_paren(b: &[u8], from: usize) -> usize {
    let mut depth = 1usize;
    let mut i = from;
    while let Some(&c) = b.get(i) {
        match c {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    b.len()
}

/// Literal colours in a declaration value: hex colours and colour functions anywhere, named
/// colours in colour-bearing properties. `url()` contents and strings are skipped.
pub fn literal_colours(prop: &str, value: &str) -> Vec<String> {
    let b = value.as_bytes();
    let named_ok = colour_property(prop);
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(&c) = b.get(i) {
        if c == b'"' || c == b'\'' {
            i += 1;
            while let Some(&d) = b.get(i) {
                i += 1;
                if d == b'\\' {
                    i += 1;
                } else if d == c {
                    break;
                }
            }
        } else if c == b'#' {
            let s = i + 1;
            let mut j = s;
            while b.get(j).is_some_and(|d| ident_byte(*d)) {
                j += 1;
            }
            let hex = value.get(s..j).unwrap_or("");
            if matches!(hex.len(), 3 | 4 | 6 | 8) && hex.bytes().all(|d| d.is_ascii_hexdigit()) {
                out.push(format!("#{hex}"));
            }
            i = j.max(i + 1);
        } else if c.is_ascii_digit() || c == b'.' {
            while b.get(i).is_some_and(|d| d.is_ascii_alphanumeric() || *d == b'.' || *d == b'%') {
                i += 1;
            }
        } else if ident_byte(c) {
            let s = i;
            while b.get(i).is_some_and(|d| ident_byte(*d)) {
                i += 1;
            }
            let word = value.get(s..i).unwrap_or("").to_ascii_lowercase();
            if b.get(i) == Some(&b'(') {
                if word == "url" {
                    i = close_paren(b, i + 1);
                } else if COLOUR_FUNCS.contains(&word.as_str()) {
                    let end = close_paren(b, i + 1);
                    out.push(value.get(s..end).unwrap_or("").to_string());
                    i = end;
                } else {
                    i += 1;
                }
            } else if named_ok && NAMED_COLOURS.contains(&word.as_str()) {
                out.push(word);
            }
        } else {
            i += 1;
        }
    }
    out
}

/// An sRGB colour with channels and alpha in 0..=1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

/// Opaque white, the user-agent canvas assumed when no page background is set.
pub const WHITE: Rgba = Rgba { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };

fn hex_val(s: &str) -> Option<f64> {
    u8::from_str_radix(s, 16).ok().map(|v| f64::from(v) / 255.0)
}

/// Parses `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, `rgb()` and `rgba()` (comma or space
/// syntax, numbers or percentages). Other forms are not colour values for the gate.
pub fn parse_colour(s: &str) -> Option<Rgba> {
    let s = s.trim();
    if let Some(h) = s.strip_prefix('#') {
        if !h.bytes().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let pair = |i: usize| h.get(i..i + 2).and_then(hex_val);
        let single = |i: usize| h.get(i..i + 1).and_then(|d| hex_val(&format!("{d}{d}")));
        return match h.len() {
            3 => Some(Rgba { r: single(0)?, g: single(1)?, b: single(2)?, a: 1.0 }),
            4 => Some(Rgba { r: single(0)?, g: single(1)?, b: single(2)?, a: single(3)? }),
            6 => Some(Rgba { r: pair(0)?, g: pair(2)?, b: pair(4)?, a: 1.0 }),
            8 => Some(Rgba { r: pair(0)?, g: pair(2)?, b: pair(4)?, a: pair(6)? }),
            _ => None,
        };
    }
    let lower = s.to_ascii_lowercase();
    let inner = lower.strip_prefix("rgba(").or_else(|| lower.strip_prefix("rgb("))?.strip_suffix(')')?;
    let parts: Vec<&str> = inner.split([',', ' ', '/']).filter(|p| !p.is_empty()).collect();
    if parts.len() != 3 && parts.len() != 4 {
        return None;
    }
    let chan = |p: &str| -> Option<f64> {
        let v = match p.strip_suffix('%') {
            Some(q) => q.parse::<f64>().ok()? / 100.0,
            None => p.parse::<f64>().ok()? / 255.0,
        };
        v.is_finite().then_some(v.clamp(0.0, 1.0))
    };
    let alpha = |p: &str| -> Option<f64> {
        let v = match p.strip_suffix('%') {
            Some(q) => q.parse::<f64>().ok()? / 100.0,
            None => p.parse::<f64>().ok()?,
        };
        v.is_finite().then_some(v.clamp(0.0, 1.0))
    };
    let a = match parts.get(3).copied() {
        Some(p) => alpha(p)?,
        None => 1.0,
    };
    Some(Rgba { r: chan(parts.first().copied()?)?, g: chan(parts.get(1).copied()?)?, b: chan(parts.get(2).copied()?)?, a })
}

/// Splits at the first comma outside parentheses.
fn split_top_comma(s: &str) -> (&str, Option<&str>) {
    let mut depth = 0usize;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => return (s.get(..i).unwrap_or(s), s.get(i + 1..)),
            _ => {}
        }
    }
    (s, None)
}

/// Splits a value into top-level components (whitespace and commas outside parentheses).
pub fn components(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0usize;
    for c in s.chars() {
        match c {
            '(' => {
                depth += 1;
                cur.push(c);
            }
            ')' => {
                depth = depth.saturating_sub(1);
                cur.push(c);
            }
            ',' | ' ' | '\t' | '\n' | '\r' if depth == 0 => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            _ => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Colour scope for one theme: token values keyed by custom property name, and stylesheet
/// aliases (`--a: var(--b)`) declared on `:root` or `html`.
pub struct Scope<'a> {
    pub tokens: &'a BTreeMap<String, Rgba>,
    pub aliases: &'a BTreeMap<String, String>,
}

impl Scope<'_> {
    /// Resolves a value that is a `var()` reference to a token colour, following aliases and
    /// one fallback per level. Literal colours are not resolved (rule A13 reports them).
    pub fn resolve(&self, value: &str) -> Option<Rgba> {
        self.resolve_at(value, 0)
    }

    fn resolve_at(&self, value: &str, depth: usize) -> Option<Rgba> {
        if depth > MAX_VAR_DEPTH {
            return None;
        }
        let v = value.trim();
        let v = v.strip_suffix("!important").unwrap_or(v).trim();
        let inner = v.strip_prefix("var(")?.strip_suffix(')')?;
        let (name, fallback) = split_top_comma(inner);
        let name = name.trim();
        if let Some(c) = self.tokens.get(name) {
            return Some(*c);
        }
        if let Some(a) = self.aliases.get(name) {
            if let Some(c) = self.resolve_at(a, depth + 1) {
                return Some(c);
            }
        }
        fallback.and_then(|f| self.resolve_at(f, depth + 1))
    }

    /// Background colour set by a rule (`background-color`, or the one colour component of
    /// `background`); the last applicable declaration wins.
    pub fn background(&self, decls: &[Decl]) -> Option<Rgba> {
        let mut bg = None;
        for d in decls {
            match d.name.as_str() {
                "background-color" => bg = self.resolve(&d.value),
                "background" => bg = components(&d.value).iter().filter_map(|c| self.resolve(c)).next_back(),
                _ => {}
            }
        }
        bg
    }

    /// Text colour set by a rule (last `color` declaration).
    pub fn foreground(&self, decls: &[Decl]) -> Option<Rgba> {
        decls.iter().filter(|d| d.name == "color").map(|d| self.resolve(&d.value)).next_back().flatten()
    }
}

/// True when a rule's own font declarations make its text large (at least 24 px, or at
/// least 18.66 px and bold). Sizes in units other than px count as normal text.
pub fn large_text(decls: &[Decl]) -> bool {
    let mut size: Option<f64> = None;
    let mut bold = false;
    let px = |s: &str| s.split('/').next().and_then(|p| p.strip_suffix("px")).and_then(|p| p.parse::<f64>().ok());
    let weight = |s: &str| -> Option<bool> {
        match s.to_ascii_lowercase().as_str() {
            "bold" | "bolder" => Some(true),
            "normal" | "lighter" => Some(false),
            w => w.parse::<u32>().ok().filter(|n| (1..=1000).contains(n)).map(|n| n >= 700),
        }
    };
    for d in decls {
        match d.name.as_str() {
            "font-size" => size = px(d.value.trim()),
            "font-weight" => bold = weight(d.value.trim()).unwrap_or(false),
            "font" => {
                size = None;
                bold = false;
                for c in components(&d.value) {
                    if size.is_none() {
                        if let Some(v) = px(c.as_str()) {
                            size = Some(v);
                            continue;
                        }
                    }
                    if size.is_none() {
                        if let Some(w) = weight(c.as_str()) {
                            bold = w;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    match size {
        Some(s) => s >= 24.0 || (bold && s >= 18.66),
        None => false,
    }
}

/// `top` composited over `bottom` (source-over).
pub fn over(top: Rgba, bottom: Rgba) -> Rgba {
    let a = top.a + bottom.a * (1.0 - top.a);
    if a <= 0.0 {
        return Rgba { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };
    }
    let mix = |t: f64, u: f64| (t * top.a + u * bottom.a * (1.0 - top.a)) / a;
    Rgba { r: mix(top.r, bottom.r), g: mix(top.g, bottom.g), b: mix(top.b, bottom.b), a }
}

fn linear(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Relative luminance of an opaque sRGB colour (WCAG 2.2 definition).
pub fn luminance(c: Rgba) -> f64 {
    0.2126 * linear(c.r) + 0.7152 * linear(c.g) + 0.0722 * linear(c.b)
}

/// Contrast ratio of two opaque colours (WCAG 2.2 definition), at least 1.
pub fn contrast(a: Rgba, b: Rgba) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// `x` rounded half-up to two decimals with fixed formatting.
pub fn round2(x: f64) -> String {
    let v = (x * 100.0 + 0.5).floor();
    let v = if v.is_finite() && v >= 0.0 { v as u64 } else { 0 };
    format!("{}.{:02}", v / 100, v % 100)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn rules_media_and_lines() {
        let css = "/* c\n */a{color:var(--x)}\n@media (prefers-color-scheme: dark){\n b{color:red;background:#fff}}";
        let rules = parse_sheet(css, 1).unwrap();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].selector, "a");
        assert_eq!(rules[0].decls[0].line, 2);
        assert_eq!(rules[1].scheme.as_deref(), Some("dark"));
        assert_eq!(rules[1].decls[1].line, 4);
    }

    #[test]
    fn literals() {
        assert_eq!(literal_colours("color", "#777"), vec!["#777".to_string()]);
        assert_eq!(literal_colours("border", "1px solid red"), vec!["red".to_string()]);
        assert!(literal_colours("font-family", "Tomato, serif").is_empty());
        assert!(literal_colours("background", "url(#a1b2c3) var(--x)").is_empty());
        assert_eq!(literal_colours("color", "var(--x, rgb(1 2 3))"), vec!["rgb(1 2 3)".to_string()]);
    }

    #[test]
    fn contrast_known_values() {
        let w = parse_colour("#fff").unwrap();
        let k = parse_colour("#000000").unwrap();
        assert_eq!(round2(contrast(w, k)), "21.00");
        assert_eq!(round2(contrast(w, w)), "1.00");
    }
}
