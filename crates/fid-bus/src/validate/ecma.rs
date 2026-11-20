//! Bounded ECMA-262 regular expressions for `pattern`, `patternProperties` and
//! `format: "regex"`.
//!
//! JSON Schema 2020-12 patterns are ECMA-262 regular expressions. This module accepts an
//! allowlisted subset, translates it to the syntax of the `regex` crate with ECMA-262
//! meanings spelled out, and compiles it with size and nesting limits. The `regex` crate
//! matches with finite automata in time linear in the input (no backtracking), so no
//! pattern can take catastrophic time. Everything outside the subset is refused when the
//! schema is compiled, so it can never be silently read with another meaning:
//!
//! - accepted: literals; `.`; `^`, `$`; groups `( )`, `(?: )`, `(?<name> )`; `|`;
//!   quantifiers `*`, `+`, `?`, `{n}`, `{n,}`, `{n,m}`, each optionally lazy (`?`);
//!   classes `[...]`, `[^...]` with ranges; escapes `\d \D \w \W \s \S` (the upper-case
//!   ones outside classes only), `\b` inside a class (backspace), `\t \n \v \f \r`, `\0` (not before a digit), `\xHH`,
//!   `\uHHHH` (not a surrogate) and `\` before one of ``^$\.*+?()[]{}|/-``;
//! - ECMA-262 meanings: `\d` is `[0-9]`, `\w` is `[0-9A-Za-z_]` (ASCII, as in ECMA-262
//!   without the `i` flag), `\s` is ECMA-262 WhiteSpace and LineTerminator, `.` is any
//!   character except the four line terminators, `$` is the end of the input (no `m`
//!   flag), and the match is a search anywhere in the string (an unanchored `test`);
//! - refused: lookaround, backreferences, `\b \B` outside classes, `\p{..}`, `\c`, `\k`, inline flags,
//!   `[]` and `[^]`, a lone `{`, `}` or `]`, any other escape, and patterns over
//!   [`MAX_PATTERN_BYTES`] bytes or beyond the compiled-size and nesting limits.

use regex::{Regex, RegexBuilder};

/// Longest accepted pattern, in bytes.
pub const MAX_PATTERN_BYTES: usize = 1024;
/// Compiled program size limit of one pattern (bytes, `regex` crate accounting).
pub const REGEX_SIZE_LIMIT: usize = 1 << 20;
/// Lazy DFA cache limit of one pattern, in bytes.
pub const REGEX_DFA_LIMIT: usize = 1 << 20;
/// Nesting limit of one pattern (groups, repetitions, classes).
pub const REGEX_NEST_LIMIT: u32 = 32;

/// ECMA-262 WhiteSpace and LineTerminator code points (ES2023 §12.2, §12.3), as a class body.
const WS: &str = "\\x{9}\\x{A}\\x{B}\\x{C}\\x{D}\\x{20}\\x{A0}\\x{1680}\\x{2000}-\\x{200A}\\x{2028}\\x{2029}\\x{202F}\\x{205F}\\x{3000}\\x{FEFF}";
const DIGIT: &str = "0-9";
const WORD: &str = "0-9A-Za-z_";
/// `.`: anything but the line terminators LF, CR, LS and PS.
const DOT: &str = "[^\\x{A}\\x{D}\\x{2028}\\x{2029}]";

/// A compiled pattern.
#[derive(Debug, Clone)]
pub struct Pattern {
    source: String,
    re: Regex,
}

impl Pattern {
    /// Translates and compiles `source`; the error says why it is refused.
    pub fn compile(source: &str) -> Result<Pattern, String> {
        if source.len() > MAX_PATTERN_BYTES {
            return Err(format!("pattern longer than {MAX_PATTERN_BYTES} bytes"));
        }
        let translated = translate(source)?;
        let re = RegexBuilder::new(&translated)
            .size_limit(REGEX_SIZE_LIMIT)
            .dfa_size_limit(REGEX_DFA_LIMIT)
            .nest_limit(REGEX_NEST_LIMIT)
            .build()
            .map_err(|e| match e {
                regex::Error::CompiledTooBig(_) => "pattern exceeds the compiled size limit".to_string(),
                _ => "pattern is outside the supported ECMA-262 subset".to_string(),
            })?;
        Ok(Pattern { source: source.to_string(), re })
    }

    /// Whether the pattern matches anywhere in `text` (ECMA-262 `RegExp.prototype.test`).
    pub fn is_match(&self, text: &str) -> bool {
        self.re.is_match(text)
    }

    /// The pattern as written.
    pub fn source(&self) -> &str {
        &self.source
    }
}

fn hex(c: u32) -> String {
    format!("\\x{{{c:X}}}")
}

fn unsupported(what: &str) -> String {
    format!("pattern uses {what}, which is outside the supported ECMA-262 subset")
}

/// One item of a class body before ranges are formed.
enum Item {
    Char(u32),
    Set(&'static str),
    Dash,
}

fn hex_digits(chars: &[char], at: usize, n: usize) -> Option<u32> {
    let s: String = chars.get(at..at + n)?.iter().collect();
    if s.len() != n || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(&s, 16).ok()
}

/// Reads the escape after the backslash at `chars[i - 1]`; returns the item and the
/// index after it. `in_class` selects the class meaning of `\b` (refused) and refuses the
/// negated shorthand sets.
fn escape(chars: &[char], i: usize, in_class: bool) -> Result<(Item, usize), String> {
    let Some(&c) = chars.get(i) else { return Err(unsupported("a trailing backslash")) };
    let item = match c {
        'd' => Item::Set(DIGIT),
        'w' => Item::Set(WORD),
        's' => Item::Set(WS),
        'D' | 'W' | 'S' if in_class => return Err(unsupported("a negated class escape inside a class")),
        'D' => Item::Set("^0-9"),
        'W' => Item::Set("^0-9A-Za-z_"),
        'S' => Item::Set("^WS"),
        't' => Item::Char(0x9),
        'n' => Item::Char(0xA),
        'v' => Item::Char(0xB),
        'f' => Item::Char(0xC),
        'r' => Item::Char(0xD),
        '0' => {
            if chars.get(i + 1).is_some_and(char::is_ascii_digit) {
                return Err(unsupported("an octal escape"));
            }
            Item::Char(0)
        }
        'x' => {
            let v = hex_digits(chars, i + 1, 2).ok_or_else(|| unsupported("a malformed \\x escape"))?;
            return Ok((Item::Char(v), i + 3));
        }
        'u' => {
            let v = hex_digits(chars, i + 1, 4).ok_or_else(|| unsupported("a malformed \\u escape"))?;
            if (0xD800..=0xDFFF).contains(&v) {
                return Err(unsupported("a surrogate \\u escape"));
            }
            return Ok((Item::Char(v), i + 5));
        }
        '^' | '$' | '\\' | '.' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '/' | '-' => {
            Item::Char(u32::from(c))
        }
        // Inside a class `\b` is the backspace character (ES2023 §22.2.2.9).
        'b' if in_class => Item::Char(0x8),
        'b' | 'B' => return Err(unsupported("a word-boundary escape")),
        '1'..='9' | 'k' => return Err(unsupported("a backreference")),
        _ => return Err(unsupported("an unsupported escape")),
    };
    Ok((item, i + 1))
}

fn set_outside(set: &'static str) -> String {
    match set {
        "^WS" => format!("[^{WS}]"),
        s => format!("[{s}]"),
    }
}

/// Translates a class starting after `[` at `chars[i]`; returns the class and the index
/// after the closing `]`.
fn class(chars: &[char], mut i: usize) -> Result<(String, usize), String> {
    let negated = chars.get(i) == Some(&'^');
    if negated {
        i += 1;
    }
    let mut items = Vec::new();
    loop {
        let Some(&c) = chars.get(i) else { return Err(unsupported("an unterminated class")) };
        match c {
            ']' => {
                i += 1;
                break;
            }
            '\\' => {
                let (item, next) = escape(chars, i + 1, true)?;
                items.push(item);
                i = next;
            }
            '-' => {
                items.push(Item::Dash);
                i += 1;
            }
            other => {
                items.push(Item::Char(u32::from(other)));
                i += 1;
            }
        }
    }
    if items.is_empty() {
        return Err(unsupported("an empty class"));
    }
    let mut body = String::new();
    let mut k = 0;
    while k < items.len() {
        match (items.get(k), items.get(k + 1), items.get(k + 2)) {
            (Some(Item::Char(a)), Some(Item::Dash), Some(Item::Char(b))) => {
                if a > b {
                    return Err(unsupported("a class range out of order"));
                }
                body.push_str(&format!("{}-{}", hex(*a), hex(*b)));
                k += 3;
            }
            (Some(Item::Set(_)), Some(Item::Dash), Some(Item::Char(_) | Item::Set(_)))
            | (Some(Item::Char(_)), Some(Item::Dash), Some(Item::Set(_))) => {
                return Err(unsupported("a class range with a class escape"));
            }
            (Some(Item::Char(a)), _, _) => {
                body.push_str(&hex(*a));
                k += 1;
            }
            (Some(Item::Set(s)), _, _) => {
                body.push_str(s);
                k += 1;
            }
            (Some(Item::Dash), _, _) => {
                body.push_str(&hex(u32::from('-')));
                k += 1;
            }
            (None, _, _) => break,
        }
    }
    Ok((format!("[{}{}]", if negated { "^" } else { "" }, body), i))
}

/// Reads `{n}`, `{n,}` or `{n,m}` at `chars[i]` (`{`); returns it and the index after it.
fn quantifier(chars: &[char], i: usize) -> Option<(String, usize)> {
    let mut j = i + 1;
    let start = j;
    while chars.get(j).is_some_and(char::is_ascii_digit) {
        j += 1;
    }
    if j == start {
        return None;
    }
    if chars.get(j) == Some(&',') {
        j += 1;
        while chars.get(j).is_some_and(char::is_ascii_digit) {
            j += 1;
        }
    }
    if chars.get(j) != Some(&'}') {
        return None;
    }
    Some((chars.get(i..=j)?.iter().collect(), j + 1))
}

/// Translates an ECMA-262 pattern of the supported subset into `regex` crate syntax.
pub fn translate(source: &str) -> Result<String, String> {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        match c {
            '\\' => {
                let (item, next) = escape(&chars, i + 1, false)?;
                match item {
                    Item::Char(v) => out.push_str(&hex(v)),
                    Item::Set(s) => out.push_str(&set_outside(s)),
                    Item::Dash => out.push_str(&hex(u32::from('-'))),
                }
                i = next;
            }
            '.' => {
                out.push_str(DOT);
                i += 1;
            }
            '[' => {
                let (cls, next) = class(&chars, i + 1)?;
                out.push_str(&cls);
                i = next;
            }
            '(' => {
                if chars.get(i + 1) == Some(&'?') {
                    match (chars.get(i + 2), chars.get(i + 3)) {
                        (Some(':'), _) => {
                            out.push_str("(?:");
                            i += 3;
                        }
                        (Some('<'), Some(n)) if *n != '=' && *n != '!' => {
                            let mut j = i + 3;
                            let name_start = j;
                            while chars.get(j).is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_') {
                                j += 1;
                            }
                            let name: String = chars.get(name_start..j).unwrap_or(&[]).iter().collect();
                            let valid = name.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
                            if !valid || chars.get(j) != Some(&'>') {
                                return Err(unsupported("a malformed group name"));
                            }
                            out.push_str(&format!("(?<{name}>"));
                            i = j + 1;
                        }
                        _ => return Err(unsupported("lookaround or an inline flag")),
                    }
                } else {
                    out.push('(');
                    i += 1;
                }
            }
            '{' => {
                let (q, next) = quantifier(&chars, i).ok_or_else(|| unsupported("a lone `{`"))?;
                out.push_str(&q);
                i = next;
            }
            '}' => return Err(unsupported("a lone `}`")),
            ']' => return Err(unsupported("a lone `]`")),
            ')' | '|' | '^' | '$' | '*' | '+' | '?' => {
                out.push(c);
                i += 1;
            }
            c if c.is_ascii_alphanumeric() => {
                out.push(c);
                i += 1;
            }
            other => {
                out.push_str(&hex(u32::from(other)));
                i += 1;
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn m(p: &str, s: &str) -> bool {
        Pattern::compile(p).unwrap_or_else(|e| panic!("{p}: {e}")).is_match(s)
    }

    fn refused(p: &str) -> bool {
        Pattern::compile(p).is_err()
    }

    /// Expected values follow ECMA-262 (ES2023 §22.2) by hand.
    #[test]
    fn ecma_meanings() {
        assert!(m("^[a-z]+$", "abc") && !m("^[a-z]+$", "abC"));
        assert!(m("b", "abc"), "search, not a full match");
        assert!(m("^\\d{3}$", "123") && !m("^\\d$", "\u{0663}"), "\\d is ASCII");
        assert!(!m("^\\w$", "\u{e9}"), "\\w is ASCII");
        assert!(m("^\\s$", "\u{feff}") && m("^\\s$", "\u{2028}") && !m("^\\s$", "\u{85}"));
        assert!(!m("^.$", "\r") && !m("^.$", "\u{2028}") && m("^.$", "\u{1f600}"));
        assert!(!m("^a$", "a\n"), "$ is the end of input");
        assert!(m("^[\\d.-]+$", "1.5-2") && m("^[-a]$", "-") && m("^[a-]$", "-"));
        assert!(m("^\\x41\\u00e9$", "A\u{e9}") && m("^a\\/b$", "a/b") && m("^[\\b]$", "\u{8}"));
        assert!(m("^(?:ab)+$", "abab") && m("^(?<y>\\d{4})$", "2026") && m("^a{2,3}?$", "aa"));
        assert!(m("^[^\\s]+$", "ab") && !m("^\\S+$", "a b") && m("^\\D\\W$", "a-"));
        assert!(m("^[&~]+$", "&~"), "no set operations inside classes");
        assert!(m("^#a b$", "#a b"), "no verbose mode");
    }

    #[test]
    fn refusals() {
        for p in [
            "(?=a)", "(?!a)", "(?<=a)", "(?<!a)", "(a)\\1", "\\k<n>", "\\bword", "\\B", "\\p{L}", "\\cA", "(?i)a",
            "[]", "[^]", "a{", "a}", "]", "[a", "\\", "\\q", "[\\D]", "[z-a]", "[\\d-z]", "\\ud800", "\\012", "\\x4",
        ] {
            assert!(refused(p), "{p} should be refused");
        }
        assert!(refused(&"a".repeat(MAX_PATTERN_BYTES + 1)));
        assert!(refused(&format!("{}a{}", "(".repeat(40), ")".repeat(40))), "nest limit");
        assert!(matches!(Pattern::compile("(?:\\w{1000}){1000}"), Err(e) if e.contains("size limit")), "size limit");
    }

    #[test]
    fn linear_on_classic_redos() {
        // `(a+)+$` backtracks exponentially in a backtracking engine; here it is linear.
        let p = Pattern::compile("^(a+)+$").unwrap();
        let s = format!("{}!", "a".repeat(100_000));
        assert!(!p.is_match(&s));
    }
}
