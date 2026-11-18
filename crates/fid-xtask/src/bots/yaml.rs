//! A bounded reader for the YAML subset that bot configuration files use: block mappings,
//! block sequences (including `- key: value` items and sequences at their key's indent),
//! one-line flow sequences `[a, "b"]`, plain, single-quoted and double-quoted scalars, and
//! `#` comments. Everything else (anchors, aliases, tags, block scalars, flow mappings,
//! multiple documents, tab indentation) is refused, so a file the reader does not fully
//! understand fails the check instead of passing it.

use super::Node;
use std::collections::BTreeMap;

struct Line {
    no: usize,
    indent: usize,
    text: String,
}

/// Parses `src`; `max_depth` bounds the nesting of block collections.
pub fn parse(src: &str, max_depth: usize) -> Result<Node, String> {
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);
    check_breaks(src)?;
    let mut lines = Vec::new();
    for (i, raw) in src.lines().enumerate() {
        let no = i + 1;
        let body = strip_comment(raw).trim_end();
        let content = body.trim_start_matches(' ');
        if content.is_empty() {
            continue;
        }
        if content.starts_with('\t') {
            return Err(format!("line {no}: tab indentation is not supported"));
        }
        let indent = body.len() - content.len();
        if indent == 0 && content == "---" {
            if lines.is_empty() {
                continue;
            }
            return Err(format!("line {no}: multiple documents are not supported"));
        }
        if indent == 0 && (content == "..." || content.starts_with('%')) {
            return Err(format!("line {no}: directives and document markers are not supported"));
        }
        lines.push(Line { no, indent, text: content.to_string() });
    }
    let Some(first) = lines.first() else {
        return Ok(Node::Other);
    };
    let indent = first.indent;
    let mut pos = 0;
    let node = block(&mut lines, &mut pos, indent, 0, max_depth)?;
    if let Some(l) = lines.get(pos) {
        return Err(format!("line {}: unexpected indentation or structure", l.no));
    }
    Ok(node)
}

/// Line breaks are LF or CRLF only. YAML parsers also break lines on a lone CR (and YAML 1.1
/// parsers on U+0085, U+2028 and U+2029), so a file containing one could be read differently
/// by the bot than by this check; such files, and any other control character except tab,
/// are refused.
fn check_breaks(src: &str) -> Result<(), String> {
    let mut line = 1usize;
    let mut chars = src.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\n' => line += 1,
            '\t' => {}
            '\r' if chars.peek() == Some(&'\n') => {}
            '\r' => return Err(format!("line {line}: carriage return without line feed")),
            '\u{2028}' | '\u{2029}' => return Err(format!("line {line}: Unicode line or paragraph separator")),
            c if c.is_control() => return Err(format!("line {line}: control character U+{:04X}", u32::from(c))),
            _ => {}
        }
    }
    Ok(())
}

/// Cuts a `#` comment that starts the line or follows whitespace, outside quotes.
fn strip_comment(line: &str) -> &str {
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut prev: Option<char> = None;
    for (i, c) in line.char_indices() {
        match quote {
            Some(q) => {
                if escaped {
                    escaped = false;
                } else if q == '"' && c == '\\' {
                    escaped = true;
                } else if c == q {
                    quote = None;
                }
            }
            None => {
                let after_space = match prev {
                    None => true,
                    Some(p) => p.is_whitespace(),
                };
                if c == '#' && after_space {
                    return line.get(..i).unwrap_or("");
                }
                let opens = match prev {
                    None => true,
                    Some(p) => p.is_whitespace() || "[,{:-".contains(p) || (c == '\'' && p == '\''),
                };
                if (c == '"' || c == '\'') && opens {
                    quote = Some(c);
                }
            }
        }
        prev = Some(c);
    }
    line
}

fn is_seq_item(t: &str) -> bool {
    t == "-" || t.starts_with("- ")
}

fn block(lines: &mut [Line], pos: &mut usize, indent: usize, depth: usize, max: usize) -> Result<Node, String> {
    if depth > max {
        return Err(format!("nesting deeper than {max} levels"));
    }
    match lines.get(*pos) {
        None => Ok(Node::Other),
        Some(l) if is_seq_item(&l.text) => seq(lines, pos, indent, depth, max),
        Some(_) => map(lines, pos, indent, depth, max),
    }
}

fn seq(lines: &mut [Line], pos: &mut usize, indent: usize, depth: usize, max: usize) -> Result<Node, String> {
    let mut items = Vec::new();
    while let Some(l) = lines.get(*pos) {
        if l.indent < indent || (l.indent == indent && !is_seq_item(&l.text)) {
            break;
        }
        if l.indent > indent {
            return Err(format!("line {}: unexpected indentation", l.no));
        }
        let (no, text) = (l.no, l.text.clone());
        let rest = text.get(1..).unwrap_or("").trim_start_matches(' ');
        if rest.is_empty() {
            *pos += 1;
            match lines.get(*pos) {
                Some(n) if n.indent > indent => {
                    let ni = n.indent;
                    items.push(block(lines, pos, ni, depth + 1, max)?);
                }
                _ => items.push(Node::Other),
            }
        } else if is_seq_item(rest) || split_key(rest, no)?.is_some() {
            // A compact nested collection: read the rest of the line as if it started a
            // block at its own column.
            let column = indent + (text.len() - rest.len());
            let rest = rest.to_string();
            if let Some(m) = lines.get_mut(*pos) {
                m.indent = column;
                m.text = rest;
            }
            items.push(block(lines, pos, column, depth + 1, max)?);
        } else {
            items.push(value(rest, no)?);
            *pos += 1;
        }
    }
    Ok(Node::Seq(items))
}

fn map(lines: &mut [Line], pos: &mut usize, indent: usize, depth: usize, max: usize) -> Result<Node, String> {
    let mut out = BTreeMap::new();
    while let Some(l) = lines.get(*pos) {
        if l.indent < indent || (l.indent == indent && is_seq_item(&l.text)) {
            break;
        }
        if l.indent > indent {
            return Err(format!("line {}: unexpected indentation", l.no));
        }
        let (no, text) = (l.no, l.text.clone());
        let Some((key, rest)) = split_key(&text, no)? else {
            return Err(format!("line {no}: expected `key: value`"));
        };
        if key.is_empty() || key.starts_with(['&', '*', '!', '?', '|', '>', '{', '[', '%', '@', '`']) {
            return Err(format!("line {no}: unsupported key {key}"));
        }
        *pos += 1;
        let v = if rest.is_empty() {
            match lines.get(*pos) {
                Some(n) if n.indent > indent => {
                    let ni = n.indent;
                    block(lines, pos, ni, depth + 1, max)?
                }
                Some(n) if n.indent == indent && is_seq_item(&n.text) => seq(lines, pos, indent, depth + 1, max)?,
                _ => Node::Other,
            }
        } else {
            value(&rest, no)?
        };
        if out.insert(key.clone(), v).is_some() {
            return Err(format!("line {no}: duplicate key {key}"));
        }
    }
    Ok(Node::Map(out))
}

/// Splits `key: value` (or `key:`); `None` when the text is not a mapping entry.
fn split_key(t: &str, no: usize) -> Result<Option<(String, String)>, String> {
    if t.starts_with('"') || t.starts_with('\'') {
        let (key, after) = quoted(t, no)?;
        let after = after.trim_start_matches(' ');
        return Ok(match after.strip_prefix(':') {
            Some(r) if r.is_empty() || r.starts_with(' ') => Some((key, r.trim().to_string())),
            _ => None,
        });
    }
    if t.starts_with('[') || t.starts_with('{') {
        return Ok(None);
    }
    let idx = match t.find(": ") {
        Some(i) => Some(i),
        None if t.ends_with(':') => Some(t.len() - 1),
        None => None,
    };
    Ok(idx.map(|i| {
        let key = t.get(..i).unwrap_or("").trim_end().to_string();
        let rest = t.get(i + 1..).unwrap_or("").trim().to_string();
        (key, rest)
    }))
}

/// Reads a quoted scalar at the start of `t`; returns it and the text after the closing quote.
fn quoted(t: &str, no: usize) -> Result<(String, &str), String> {
    let mut chars = t.char_indices();
    let Some((_, q)) = chars.next() else {
        return Err(format!("line {no}: empty quoted string"));
    };
    let mut out = String::new();
    while let Some((i, c)) = chars.next() {
        let after = t.get(i + c.len_utf8()..).unwrap_or("");
        if q == '\'' {
            if c == '\'' {
                if after.starts_with('\'') {
                    out.push('\'');
                    chars.next();
                    continue;
                }
                return Ok((out, after));
            }
            out.push(c);
        } else {
            match c {
                '"' => return Ok((out, after)),
                '\\' => {
                    let Some((_, e)) = chars.next() else { break };
                    out.push(match e {
                        '\\' => '\\',
                        '"' => '"',
                        '/' => '/',
                        ' ' => ' ',
                        'n' => '\n',
                        't' => '\t',
                        _ => return Err(format!("line {no}: unsupported escape \\{e}")),
                    });
                }
                _ => out.push(c),
            }
        }
    }
    Err(format!("line {no}: unterminated quoted string"))
}

/// A value on the same line as its key or dash: a flow sequence or a scalar.
fn value(s: &str, no: usize) -> Result<Node, String> {
    let s = s.trim();
    if let Some(inner) = s.strip_prefix('[') {
        let inner =
            inner.strip_suffix(']').ok_or_else(|| format!("line {no}: a flow sequence must close on its line"))?;
        return flow_items(inner, no).map(Node::Seq);
    }
    if s == "{}" {
        return Ok(Node::Map(BTreeMap::new()));
    }
    scalar(s, no).map(Node::Str)
}

fn scalar(s: &str, no: usize) -> Result<String, String> {
    if s.starts_with('"') || s.starts_with('\'') {
        let (v, rest) = quoted(s, no)?;
        if !rest.trim().is_empty() {
            return Err(format!("line {no}: text after a quoted string"));
        }
        return Ok(v);
    }
    if s.starts_with(['&', '*', '!', '|', '>', '{', '}', '[', ']', '%', '@', '`', '?']) {
        return Err(format!("line {no}: unsupported YAML construct {s}"));
    }
    if s.contains(": ") || s.ends_with(':') || is_seq_item(s) {
        return Err(format!("line {no}: unexpected collection in a scalar"));
    }
    Ok(s.to_string())
}

fn flow_items(inner: &str, no: usize) -> Result<Vec<Node>, String> {
    let mut items = Vec::new();
    let mut rest = inner.trim();
    while !rest.is_empty() {
        let (item, after) = if rest.starts_with('"') || rest.starts_with('\'') {
            let (v, after) = quoted(rest, no)?;
            (v, after.trim_start())
        } else {
            let end = rest.find(',').unwrap_or(rest.len());
            let raw = rest.get(..end).unwrap_or("").trim();
            if raw.is_empty() {
                return Err(format!("line {no}: empty item in a flow sequence"));
            }
            (scalar(raw, no)?, rest.get(end..).unwrap_or(""))
        };
        items.push(Node::Str(item));
        rest = match after.strip_prefix(',') {
            Some(r) => r.trim_start(),
            None if after.is_empty() => "",
            None => return Err(format!("line {no}: expected , between flow sequence items")),
        };
    }
    Ok(items)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn s(v: &str) -> Node {
        Node::Str(v.to_string())
    }

    fn m(entries: Vec<(&str, Node)>) -> Node {
        Node::Map(entries.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
    }

    #[test]
    fn nested_maps_and_sequences() {
        let src = "# head\n---\na:\n  b:\n    - \"x/**\"   # c\n    - 'it''s'\n  c: [p, \"q, r\", 's']\nd:\n- one\n- two\ne: plain value\n";
        let want = m(vec![
            (
                "a",
                m(vec![
                    ("b", Node::Seq(vec![s("x/**"), s("it's")])),
                    ("c", Node::Seq(vec![s("p"), s("q, r"), s("s")])),
                ]),
            ),
            ("d", Node::Seq(vec![s("one"), s("two")])),
            ("e", s("plain value")),
        ]);
        assert_eq!(parse(src, 64).unwrap(), want);
    }

    #[test]
    fn compact_items() {
        let src = "tools:\n  - name: a\n    on: true\n  - name: \"b # x\"\n  -\n    - deep\n  - - nested\n";
        let want = m(vec![(
            "tools",
            Node::Seq(vec![
                m(vec![("name", s("a")), ("on", s("true"))]),
                m(vec![("name", s("b # x"))]),
                Node::Seq(vec![s("deep")]),
                Node::Seq(vec![s("nested")]),
            ]),
        )]);
        assert_eq!(parse(src, 64).unwrap(), want);
    }

    #[test]
    fn escapes_and_empties() {
        assert_eq!(parse("a: \"q\\\"#x\"\n", 64).unwrap(), m(vec![("a", s("q\"#x"))]));
        assert_eq!(
            parse("a:\nb: []\nc: {}\n", 64).unwrap(),
            m(vec![("a", Node::Other), ("b", Node::Seq(vec![])), ("c", Node::Map(BTreeMap::new()))])
        );
        assert_eq!(parse("# only a comment\n", 64).unwrap(), Node::Other);
        assert_eq!(parse("url: http://x.invalid/a#b\n", 64).unwrap(), m(vec![("url", s("http://x.invalid/a#b"))]));
    }

    #[test]
    fn crlf_and_tabs_in_text_are_accepted() {
        assert_eq!(parse("a: 1\r\nb: [x]\r\n", 64).unwrap(), m(vec![("a", s("1")), ("b", Node::Seq(vec![s("x")]))]));
        assert_eq!(parse("a: x\ty\n", 64).unwrap(), m(vec![("a", s("x\ty"))]));
    }

    #[test]
    fn refused() {
        for src in [
            "a: &x 1\n",
            "a: *x\n",
            "a:\n  - **/x\n",
            "a: !tag v\n",
            "a: |\n  text\n",
            "a: >\n  text\n",
            "a: {b: 1}\n",
            "a: [b\n",
            "a: 1\na: 2\n",
            "a:\n\t- x\n",
            "a: 1\n---\nb: 2\n",
            "a: b: c\n",
            "a: \"open\n",
            "a: 'x' y\n",
            "  a: 1\nb: 2\n",
            "a:\n  b: 1\n   c: 2\n",
            "- a\nb: 1\n",
            "a: \"\\q\"\n",
            "just a scalar line\nb: 1\n",
            // Line breaks other parsers honour but `str::lines` does not (review of PR #81).
            "paths: #\r  exclude: []\r  other:\n    exclude: [\"standards/**\"]\n",
            "a: 1\rb: 2\n",
            "a: 1\u{85}b: 2\n",
            "a: 1\u{2028}b: 2\n",
            "a: 1\u{2029}b: 2\n",
            "a: x\u{0}y\n",
            "a: x\u{1b}y\n",
            "a: x\u{7f}y\n",
        ] {
            assert!(parse(src, 64).is_err(), "{src:?} should be refused");
        }
    }

    #[test]
    fn depth_is_bounded() {
        let mut src = String::new();
        for i in 0..80 {
            src.push_str(&" ".repeat(i));
            src.push_str("k:\n");
        }
        assert!(parse(&src, 64).unwrap_err().contains("nesting"));
        assert!(parse(&src, 100).is_ok());
    }
}
