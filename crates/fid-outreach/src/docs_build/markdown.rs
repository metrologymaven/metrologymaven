//! Bounded CommonMark + GFM tables + fenced code renderer for `docs-build`
//! (spec node `OUTR.DOCS.FRAME.NAV`).
//!
//! Security and determinism rules:
//! - No raw HTML passthrough: every HTML tag and non-capability comment is HTML-escaped.
//! - Exception (per spec): well-formed capability-tag comments (`<!-- cap: ... -->` and
//!   `<!-- cap-planned: ... -->`) are preserved verbatim both at block level and inline.
//! - Fixed heading anchor slug rule with per-page deduplication (seeded with reserved frame IDs).
//! - Internal links are rewritten to relative paths ending in a file name (`.md` -> `.html`,
//!   trailing `/` -> `index.html`, root-relative `/...` -> relative from current page).
//! - Pure Rust, bounded nesting depth, `BTreeSet` only, no `unsafe`, no panics.

use std::collections::BTreeSet;

/// Deepest block or inline nesting rendered recursively.
const MAX_NESTING_DEPTH: usize = 16;
/// Longest capability comment inner body accepted for passthrough.
const MAX_CAP_COMMENT_LEN: usize = 256;
/// Frame element IDs that heading slugs must not collide with.
const RESERVED_IDS: [&str; 1] = ["main-content"];

/// Result of rendering one Markdown document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedMarkdown {
    /// Rendered HTML body fragment (LF line endings).
    pub html: String,
    /// `title:` extracted from optional YAML front matter, if present and non-empty.
    pub front_matter_title: Option<String>,
    /// Plain text of the first level-1 heading, if present.
    pub first_h1: Option<String>,
    /// Headings in document order: `(level, id_slug, plain_text)`.
    pub headings: Vec<(u8, String, String)>,
}

/// Escapes `&`, `<`, `>`, `"` and `'` for HTML text and attribute values.
pub fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Percent-encodes path characters outside `[A-Za-z0-9._~@/-]` (matching `search_index::url_of`).
pub fn encode_rel_path(path: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(path.len());
    for &b in path.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'~' | b'@' | b'/' | b'-') {
            out.push(b as char);
        } else {
            out.push('%');
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 0x0f) as usize] as char);
        }
    }
    out
}

/// Computes a relative URL from `from_page` (e.g. `book/units.html`) to `to_page`
/// (e.g. `ref/cli/demo.read_item@1.html`), always ending in a file name.
pub fn relative_href(from_page: &str, to_page: &str) -> String {
    let from_dirs: Vec<&str> = match from_page.rsplit_once('/') {
        Some((dir, _)) if !dir.is_empty() => dir.split('/').filter(|s| !s.is_empty()).collect(),
        _ => Vec::new(),
    };
    let (to_dirs, to_file): (Vec<&str>, &str) = match to_page.rsplit_once('/') {
        Some((dir, file)) => (dir.split('/').filter(|s| !s.is_empty()).collect(), file),
        None => (Vec::new(), to_page),
    };
    let common = from_dirs.iter().zip(to_dirs.iter()).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<&str> = vec![".."; from_dirs.len().saturating_sub(common)];
    for seg in to_dirs.iter().skip(common) {
        parts.push(seg);
    }
    parts.push(to_file);
    encode_rel_path(&parts.join("/"))
}

/// Normalises a relative path (resolving `.` and `..` segments cleanly) relative to `base_dirs`.
/// Returns `None` if `..` escapes above the edition root.
fn resolve_relative_to_root(base_dirs: &[&str], rel: &str) -> Option<String> {
    let mut stack: Vec<&str> = base_dirs.to_vec();
    for seg in rel.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                stack.pop()?;
            }
            other => stack.push(other),
        }
    }
    Some(stack.join("/"))
}

/// Rewrites a Markdown link or image destination so every internal link is relative and ends in
/// a file name (never absolute, never protocol-relative, never ending in `/`).
/// Dangerous schemes (`javascript:`, `vbscript:`, `data:`) and protocol-relative URLs (`//...`)
/// are neutralised to `#`.
pub fn rewrite_link(raw_dest: &str, from_page: &str, is_image: bool) -> String {
    let dest = raw_dest.trim();
    if dest.is_empty() {
        return "#".to_string();
    }
    // Pure fragment link stays as-is (unless empty `#`).
    if dest.starts_with('#') {
        return dest.to_string();
    }
    // Protocol-relative URLs are forbidden (AC-2 / C6).
    if dest.starts_with("//") {
        return "#".to_string();
    }
    // Check for explicit scheme (RFC 3986: ALPHA *(ALPHA / DIGIT / "+" / "-" / ".") ":").
    if let Some((scheme, _)) = dest.split_once(':') {
        let valid_scheme = scheme.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
            && scheme.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'));
        if valid_scheme && !scheme.contains('/') {
            let lower = scheme.to_ascii_lowercase();
            if matches!(lower.as_str(), "http" | "https" | "mailto") {
                return dest.to_string();
            }
            // Disallow javascript:, vbscript:, data:, file:, etc.
            return "#".to_string();
        }
    }

    // Split off optional `#fragment` and `?query`.
    let (before_frag, frag) = match dest.split_once('#') {
        Some((p, f)) => (p, Some(f)),
        None => (dest, None),
    };
    let (path_part, query) = match before_frag.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (before_frag, None),
    };

    let mut path = path_part.replace('\\', "/");
    let is_root_relative = path.starts_with('/');
    if is_root_relative {
        path = path.trim_start_matches('/').to_string();
    }
    if path.is_empty() || path.ends_with('/') {
        if !is_image {
            path.push_str("index.html");
        } else {
            return "#".to_string();
        }
    } else if !is_image {
        if let Some(stem) = path.strip_suffix(".md") {
            path = format!("{stem}.html");
        } else if let Some(stem) = path.strip_suffix(".MD") {
            path = format!("{stem}.html");
        }
    }

    let from_dirs: Vec<&str> = match from_page.rsplit_once('/') {
        Some((dir, _)) if !dir.is_empty() => dir.split('/').filter(|s| !s.is_empty()).collect(),
        _ => Vec::new(),
    };

    let target_from_root = if is_root_relative {
        resolve_relative_to_root(&[], &path)
    } else {
        resolve_relative_to_root(&from_dirs, &path)
    };

    let mut out = match target_from_root {
        Some(norm) if !norm.is_empty() => relative_href(from_page, &norm),
        _ => {
            // Link attempted to escape above root; keep cleaned relative path without leading `/`.
            let cleaned = path.trim_start_matches("./");
            if cleaned.is_empty() {
                "index.html".to_string()
            } else {
                encode_rel_path(cleaned)
            }
        }
    };

    if let Some(q) = query {
        out.push('?');
        out.push_str(q);
    }
    if let Some(f) = frag {
        out.push('#');
        out.push_str(f);
    }
    out
}

/// Deterministic heading anchor slug rule:
/// lowercase ASCII, keep alphanumeric Unicode chars, collapse any separator/punctuation run to
/// `-`, trim leading/trailing `-`, fallback `"section"`, and deduplicate with `-2`, `-3`, ...
pub fn slugify(text: &str, used: &mut BTreeSet<String>) -> String {
    let mut base = String::new();
    let mut pending_dash = false;
    for c in text.chars() {
        let lower = c.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() || (!lower.is_ascii() && lower.is_alphanumeric()) {
            if pending_dash && !base.is_empty() {
                base.push('-');
            }
            pending_dash = false;
            base.push(lower);
        } else if !base.is_empty() {
            pending_dash = true;
        }
    }
    if base.is_empty() {
        base.push_str("section");
    }
    if used.insert(base.clone()) {
        return base;
    }
    let mut n: u32 = 2;
    loop {
        let candidate = format!("{base}-{n}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
        n = n.saturating_add(1);
    }
}

/// Returns true iff `inner` (the slice between `<!--` and `-->`) is a valid capability-tag
/// comment: `cap: <id>` or `cap-planned: <id>`, single-line, bounded, and free of `<`, `>`, `--`.
pub fn is_capability_comment(inner: &str) -> bool {
    if inner.len() > MAX_CAP_COMMENT_LEN || inner.contains('\n') || inner.contains('\r') {
        return false;
    }
    if inner.contains('<') || inner.contains('>') || inner.contains("--") {
        return false;
    }
    let trimmed = inner.trim();
    let rest = if let Some(r) = trimmed.strip_prefix("cap:") {
        r
    } else if let Some(r) = trimmed.strip_prefix("cap-planned:") {
        r
    } else {
        return false;
    };
    !rest.trim().is_empty()
}

/// Strips optional UTF-8 BOM, normalises CRLF/CR to LF, and strips optional leading YAML
/// front matter (`---` ... `---`). Returns `(body_lines, front_matter_title)`.
fn strip_front_matter(input: &str) -> (Vec<String>, Option<String>) {
    let text = input.strip_prefix('\u{feff}').unwrap_or(input);
    let normalised = text.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<String> = normalised.lines().map(String::from).collect();
    if lines.first().map(|l| l.trim_end()) == Some("---") {
        if let Some(end_idx) = lines.iter().enumerate().skip(1).find_map(|(idx, l)| {
            let t = l.trim_end();
            (t == "---" || t == "...").then_some(idx)
        }) {
            let mut title: Option<String> = None;
            for line in &lines[1..end_idx] {
                if let Some(rest) = line.trim_start().strip_prefix("title:") {
                    let v = unquote_yaml(rest.trim());
                    if !v.is_empty() {
                        title = Some(v);
                    }
                }
            }
            return (lines[end_idx + 1..].to_vec(), title);
        }
    }
    (lines, None)
}

fn unquote_yaml(s: &str) -> String {
    if s.len() >= 2 {
        if let Some(inner) = s.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
            return inner.replace("\\\"", "\"").replace("\\\\", "\\");
        }
        if let Some(inner) = s.strip_prefix('\'').and_then(|r| r.strip_suffix('\'')) {
            return inner.replace("''", "'");
        }
    }
    s.to_string()
}

/// Renders one Markdown document into an HTML body fragment.
pub fn render_markdown(input: &str, page_path: &str) -> RenderedMarkdown {
    let (lines, front_matter_title) = strip_front_matter(input);
    let mut ctx = RenderCtx {
        page_path,
        used_ids: RESERVED_IDS.iter().map(|s| (*s).to_string()).collect(),
        first_h1: None,
        headings: Vec::new(),
    };
    let html = render_blocks(&lines, &mut ctx, 0);
    RenderedMarkdown { html, front_matter_title, first_h1: ctx.first_h1, headings: ctx.headings }
}

struct RenderCtx<'a> {
    page_path: &'a str,
    used_ids: BTreeSet<String>,
    first_h1: Option<String>,
    headings: Vec<(u8, String, String)>,
}

fn render_blocks(lines: &[String], ctx: &mut RenderCtx<'_>, depth: usize) -> String {
    let mut out = String::new();
    let mut i = 0;
    let n = lines.len();
    while i < n {
        let line = &lines[i];
        if line.trim().is_empty() {
            i += 1;
            continue;
        }

        // Standalone capability comment line.
        if let Some(comment) = parse_standalone_cap_comment(line) {
            out.push_str(comment);
            out.push('\n');
            i += 1;
            continue;
        }

        // Fenced code block.
        if let Some((fence_char, fence_len, info)) = parse_fence_open(line) {
            i += 1;
            let mut code = String::new();
            while i < n {
                if is_fence_close(&lines[i], fence_char, fence_len) {
                    i += 1;
                    break;
                }
                code.push_str(&lines[i]);
                code.push('\n');
                i += 1;
            }
            let lang = info.split_whitespace().next().unwrap_or("");
            if lang.is_empty() {
                out.push_str("<pre><code>");
            } else {
                out.push_str("<pre><code class=\"language-");
                out.push_str(&escape_html(lang));
                out.push_str("\">");
            }
            out.push_str(&escape_html(&code));
            out.push_str("</code></pre>\n");
            continue;
        }

        // ATX heading.
        if let Some((level, raw_text)) = parse_atx_heading(line) {
            emit_heading(level, raw_text, ctx, &mut out);
            i += 1;
            continue;
        }

        // Thematic break.
        if is_thematic_break(line) {
            out.push_str("<hr>\n");
            i += 1;
            continue;
        }

        // Blockquote (bounded recursion).
        if depth < MAX_NESTING_DEPTH && is_blockquote_line(line) {
            let mut inner_lines: Vec<String> = Vec::new();
            while i < n && is_blockquote_line(&lines[i]) {
                inner_lines.push(strip_blockquote_prefix(&lines[i]).to_string());
                i += 1;
            }
            out.push_str("<blockquote>\n");
            out.push_str(&render_blocks(&inner_lines, ctx, depth + 1));
            out.push_str("</blockquote>\n");
            continue;
        }

        // GFM pipe table (header row + delimiter row).
        if i + 1 < n {
            if let Some(aligns) = parse_table_delimiter(&lines[i + 1]) {
                let header_cells = split_table_row(line);
                if header_cells.len() == aligns.len() && !header_cells.is_empty() {
                    out.push_str("<table>\n<thead>\n<tr>");
                    for (cell, align) in header_cells.iter().zip(aligns.iter()) {
                        out.push_str("<th");
                        emit_align_attr(*align, &mut out);
                        out.push('>');
                        out.push_str(&render_inline(cell.trim(), ctx.page_path, 0));
                        out.push_str("</th>");
                    }
                    out.push_str("</tr>\n</thead>\n");
                    i += 2;
                    let mut body_rows: Vec<Vec<String>> = Vec::new();
                    while i < n {
                        let row_line = &lines[i];
                        if row_line.trim().is_empty()
                            || parse_fence_open(row_line).is_some()
                            || parse_atx_heading(row_line).is_some()
                            || is_blockquote_line(row_line)
                            || is_thematic_break(row_line)
                            || !row_line.contains('|')
                        {
                            break;
                        }
                        body_rows.push(split_table_row(row_line));
                        i += 1;
                    }
                    if !body_rows.is_empty() {
                        out.push_str("<tbody>\n");
                        for row in body_rows {
                            out.push_str("<tr>");
                            for (col_idx, align) in aligns.iter().enumerate() {
                                let cell = row.get(col_idx).map(String::as_str).unwrap_or("");
                                out.push_str("<td");
                                emit_align_attr(*align, &mut out);
                                out.push('>');
                                out.push_str(&render_inline(cell.trim(), ctx.page_path, 0));
                                out.push_str("</td>");
                            }
                            out.push_str("</tr>\n");
                        }
                        out.push_str("</tbody>\n");
                    }
                    out.push_str("</table>\n");
                    continue;
                }
            }
        }

        // Lists (unordered or ordered, bounded nesting).
        if depth < MAX_NESTING_DEPTH {
            if let Some(first_marker) = parse_list_marker(line) {
                let ordered = first_marker.ordered;
                let start = first_marker.start;
                if ordered {
                    if start == 1 {
                        out.push_str("<ol>\n");
                    } else {
                        out.push_str(&format!("<ol start=\"{start}\">\n"));
                    }
                } else {
                    out.push_str("<ul>\n");
                }
                while i < n {
                    let Some(marker) = parse_list_marker(&lines[i]) else { break };
                    if marker.ordered != ordered || marker.indent != first_marker.indent {
                        break;
                    }
                    let mut item_lines: Vec<String> = vec![marker.rest.to_string()];
                    i += 1;
                    while i < n {
                        let next = &lines[i];
                        if next.trim().is_empty() {
                            // Keep blank line only if followed by indented continuation.
                            if i + 1 < n && leading_spaces(&lines[i + 1]) >= marker.content_indent {
                                item_lines.push(String::new());
                                i += 1;
                                continue;
                            }
                            break;
                        }
                        if let Some(next_m) = parse_list_marker(next) {
                            if next_m.indent <= first_marker.indent {
                                break;
                            }
                        }
                        if leading_spaces(next) >= marker.content_indent {
                            let stripped = strip_leading_spaces(next, marker.content_indent);
                            item_lines.push(stripped.to_string());
                            i += 1;
                        } else if parse_fence_open(next).is_none()
                            && parse_atx_heading(next).is_none()
                            && !is_thematic_break(next)
                            && !is_blockquote_line(next)
                            && parse_list_marker(next).is_none()
                        {
                            // Lazy continuation line.
                            item_lines.push(next.trim_start().to_string());
                            i += 1;
                        } else {
                            break;
                        }
                    }
                    out.push_str("<li>");
                    let has_nested_block = item_lines.iter().any(|l| l.trim().is_empty())
                        || item_lines.iter().skip(1).any(|l| {
                            parse_list_marker(l).is_some() || parse_fence_open(l).is_some() || is_blockquote_line(l)
                        });
                    if has_nested_block {
                        out.push('\n');
                        out.push_str(&render_blocks(&item_lines, ctx, depth + 1));
                    } else {
                        let joined = item_lines.join("\n");
                        out.push_str(&render_inline(joined.trim(), ctx.page_path, 0));
                    }
                    out.push_str("</li>\n");
                }
                if ordered {
                    out.push_str("</ol>\n");
                } else {
                    out.push_str("</ul>\n");
                }
                continue;
            }
        }

        // Paragraph (or Setext heading if underlined by === or ---).
        let mut para_lines: Vec<&str> = Vec::new();
        let mut setext_level: Option<u8> = None;
        while i < n {
            let cur = &lines[i];
            if cur.trim().is_empty() {
                break;
            }
            if !para_lines.is_empty() {
                if let Some(lvl) = parse_setext_underline(cur) {
                    setext_level = Some(lvl);
                    i += 1;
                    break;
                }
                if parse_fence_open(cur).is_some()
                    || parse_atx_heading(cur).is_some()
                    || is_thematic_break(cur)
                    || is_blockquote_line(cur)
                    || parse_standalone_cap_comment(cur).is_some()
                    || parse_list_marker(cur).is_some()
                {
                    break;
                }
                // Check if cur starts a table (cur + next is delimiter).
                if i + 1 < n && parse_table_delimiter(&lines[i + 1]).is_some() && cur.contains('|') {
                    break;
                }
            }
            para_lines.push(cur.trim_start());
            i += 1;
        }
        let joined = para_lines.join("\n");
        if let Some(lvl) = setext_level {
            emit_heading(lvl, joined.trim(), ctx, &mut out);
        } else if !joined.trim().is_empty() {
            out.push_str("<p>");
            out.push_str(&render_inline(joined.trim(), ctx.page_path, 0));
            out.push_str("</p>\n");
        }
    }
    out
}

fn emit_heading(level: u8, raw_text: &str, ctx: &mut RenderCtx<'_>, out: &mut String) {
    let plain = heading_plain_text(raw_text);
    let slug = slugify(&plain, &mut ctx.used_ids);
    if level == 1 && ctx.first_h1.is_none() && !plain.is_empty() {
        ctx.first_h1 = Some(plain.clone());
    }
    ctx.headings.push((level, slug.clone(), plain));
    out.push_str(&format!("<h{level} id=\"{}\">", escape_html(&slug)));
    out.push_str(&render_inline(raw_text, ctx.page_path, 0));
    out.push_str(&format!("</h{level}>\n"));
}

fn parse_standalone_cap_comment(line: &str) -> Option<&str> {
    let t = line.trim();
    let inner = t.strip_prefix("<!--")?.strip_suffix("-->")?;
    is_capability_comment(inner).then_some(t)
}

fn leading_spaces(s: &str) -> usize {
    s.bytes().take_while(|&b| b == b' ').count()
}

fn strip_leading_spaces(s: &str, max: usize) -> &str {
    let take = leading_spaces(s).min(max);
    &s[take..]
}

fn parse_fence_open(line: &str) -> Option<(u8, usize, &str)> {
    let spaces = leading_spaces(line);
    if spaces > 3 {
        return None;
    }
    let rest = &line[spaces..];
    let first = *rest.as_bytes().first()?;
    if first != b'`' && first != b'~' {
        return None;
    }
    let len = rest.bytes().take_while(|&b| b == first).count();
    if len < 3 {
        return None;
    }
    let info = rest[len..].trim();
    if first == b'`' && info.contains('`') {
        return None;
    }
    Some((first, len, info))
}

fn is_fence_close(line: &str, fence_char: u8, min_len: usize) -> bool {
    let spaces = leading_spaces(line);
    if spaces > 3 {
        return false;
    }
    let rest = &line[spaces..];
    let len = rest.bytes().take_while(|&b| b == fence_char).count();
    len >= min_len && rest[len..].trim().is_empty()
}

fn parse_atx_heading(line: &str) -> Option<(u8, &str)> {
    let spaces = leading_spaces(line);
    if spaces > 3 {
        return None;
    }
    let rest = &line[spaces..];
    let hashes = rest.bytes().take_while(|&b| b == b'#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let after = &rest[hashes..];
    if !after.is_empty() && !after.starts_with(' ') && !after.starts_with('\t') {
        return None;
    }
    let mut content = after.trim();
    // Optional closing `#` sequence preceded by whitespace.
    let trimmed_end = content.trim_end_matches('#');
    if trimmed_end.len() < content.len() {
        if trimmed_end.is_empty() {
            content = "";
        } else if trimmed_end.ends_with(' ') || trimmed_end.ends_with('\t') {
            content = trimmed_end.trim_end();
        }
    }
    Some((hashes as u8, content))
}

fn parse_setext_underline(line: &str) -> Option<u8> {
    let spaces = leading_spaces(line);
    if spaces > 3 {
        return None;
    }
    let rest = line[spaces..].trim_end();
    if rest.is_empty() {
        return None;
    }
    if rest.bytes().all(|b| b == b'=') {
        return Some(1);
    }
    if rest.bytes().all(|b| b == b'-') {
        return Some(2);
    }
    None
}

fn is_thematic_break(line: &str) -> bool {
    let spaces = leading_spaces(line);
    if spaces > 3 {
        return false;
    }
    let mut marker: Option<u8> = None;
    let mut count = 0usize;
    for b in line[spaces..].bytes() {
        if b == b' ' || b == b'\t' {
            continue;
        }
        if matches!(b, b'-' | b'*' | b'_') {
            match marker {
                None => marker = Some(b),
                Some(m) if m != b => return false,
                Some(_) => {}
            }
            count += 1;
        } else {
            return false;
        }
    }
    count >= 3
}

fn is_blockquote_line(line: &str) -> bool {
    let spaces = leading_spaces(line);
    spaces <= 3 && line[spaces..].starts_with('>')
}

fn strip_blockquote_prefix(line: &str) -> &str {
    let spaces = leading_spaces(line);
    let after = line[spaces..].strip_prefix('>').unwrap_or(line);
    after.strip_prefix(' ').unwrap_or(after)
}

#[derive(Debug, Clone, Copy)]
enum Align {
    None,
    Left,
    Center,
    Right,
}

fn emit_align_attr(align: Align, out: &mut String) {
    match align {
        Align::None => {}
        Align::Left => out.push_str(" class=\"align-left\""),
        Align::Center => out.push_str(" class=\"align-center\""),
        Align::Right => out.push_str(" class=\"align-right\""),
    }
}

fn parse_table_delimiter(line: &str) -> Option<Vec<Align>> {
    if !line.contains('|') && !line.contains('-') {
        return None;
    }
    let cells = split_table_row(line);
    if cells.is_empty() {
        return None;
    }
    let mut aligns = Vec::with_capacity(cells.len());
    for raw in cells {
        let c = raw.trim();
        if c.is_empty() {
            return None;
        }
        let left = c.starts_with(':');
        let right = c.ends_with(':');
        let mid = c.trim_start_matches(':').trim_end_matches(':');
        if mid.is_empty() || !mid.bytes().all(|b| b == b'-') {
            return None;
        }
        let align = match (left, right) {
            (true, true) => Align::Center,
            (true, false) => Align::Left,
            (false, true) => Align::Right,
            (false, false) => Align::None,
        };
        aligns.push(align);
    }
    Some(aligns)
}

fn split_table_row(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    let trimmed = trimmed.strip_prefix('|').unwrap_or(trimmed);
    let trimmed = if trimmed.ends_with('|') && !trimmed.ends_with("\\|") {
        &trimmed[..trimmed.len().saturating_sub(1)]
    } else {
        trimmed
    };
    let mut cells = Vec::new();
    let mut cur = String::new();
    let bytes = trimmed.as_bytes();
    let mut i = 0;
    let mut in_code: usize = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'`' {
            let run = bytes[i..].iter().take_while(|&&c| c == b'`').count();
            for _ in 0..run {
                cur.push('`');
            }
            if in_code == 0 {
                in_code = run;
            } else if in_code == run {
                in_code = 0;
            }
            i += run;
            continue;
        }
        if in_code == 0 && b == b'\\' && bytes.get(i + 1) == Some(&b'|') {
            cur.push('|');
            i += 2;
            continue;
        }
        if in_code == 0 && b == b'|' {
            cells.push(std::mem::take(&mut cur));
            i += 1;
            continue;
        }
        if let Some(ch) = trimmed[i..].chars().next() {
            cur.push(ch);
            i += ch.len_utf8();
        } else {
            break;
        }
    }
    cells.push(cur);
    cells
}

struct ListMarker<'a> {
    indent: usize,
    content_indent: usize,
    ordered: bool,
    start: u64,
    rest: &'a str,
}

fn parse_list_marker(line: &str) -> Option<ListMarker<'_>> {
    let indent = leading_spaces(line);
    if indent > 3 {
        return None;
    }
    let rest = &line[indent..];
    let bytes = rest.as_bytes();
    let first = *bytes.first()?;
    if matches!(first, b'-' | b'*' | b'+') {
        if bytes.get(1).is_some_and(|&b| b == b' ' || b == b'\t') {
            let after = &rest[2..];
            return Some(ListMarker { indent, content_indent: indent + 2, ordered: false, start: 1, rest: after });
        }
        return None;
    }
    if first.is_ascii_digit() {
        let digits = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
        if digits > 9 {
            return None;
        }
        let delim = *bytes.get(digits)?;
        if delim != b'.' && delim != b')' {
            return None;
        }
        if bytes.get(digits + 1).is_some_and(|&b| b == b' ' || b == b'\t') {
            let start = rest[..digits].parse::<u64>().ok()?;
            let after = &rest[digits + 2..];
            return Some(ListMarker { indent, content_indent: indent + digits + 2, ordered: true, start, rest: after });
        }
    }
    None
}

/// Extracts plain text of a heading for anchor slugification and `<title>`.
fn heading_plain_text(s: &str) -> String {
    let html = render_inline(s, "index.html", 0);
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut chars = html.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '<' {
            in_tag = true;
            continue;
        }
        if in_tag {
            if c == '>' {
                in_tag = false;
            }
            continue;
        }
        if c == '&' {
            let mut ent = String::new();
            while let Some(&nc) = chars.peek() {
                if nc == ';' {
                    chars.next();
                    break;
                }
                if ent.len() > 10 || !nc.is_ascii_alphanumeric() && nc != '#' {
                    break;
                }
                ent.push(nc);
                chars.next();
            }
            match ent.as_str() {
                "amp" => out.push('&'),
                "lt" => out.push('<'),
                "gt" => out.push('>'),
                "quot" => out.push('"'),
                "#39" | "apos" => out.push('\''),
                _ => {
                    out.push('&');
                    out.push_str(&ent);
                }
            }
            continue;
        }
        out.push(c);
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Renders inline Markdown elements with bounded recursion depth.
fn render_inline(s: &str, page_path: &str, depth: usize) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Capability comment passthrough (`<!-- cap: ... -->` / `<!-- cap-planned: ... -->`).
        if s[i..].starts_with("<!--") {
            if let Some(rel_end) = s[i + 4..].find("-->") {
                let inner = &s[i + 4..i + 4 + rel_end];
                if is_capability_comment(inner) {
                    out.push_str(&s[i..i + 4 + rel_end + 3]);
                    i += 4 + rel_end + 3;
                    continue;
                }
            }
        }

        let b = bytes[i];

        // Backslash escape of ASCII punctuation or hard line break.
        if b == b'\\' {
            if let Some(&next) = bytes.get(i + 1) {
                if next == b'\n' {
                    out.push_str("<br>\n");
                    i += 2;
                    continue;
                }
                if next.is_ascii_punctuation() {
                    out.push_str(&escape_html(&(next as char).to_string()));
                    i += 2;
                    continue;
                }
            }
            out.push('\\');
            i += 1;
            continue;
        }

        // Hard line break (two or more spaces before `\n`).
        if b == b' ' {
            let spaces = bytes[i..].iter().take_while(|&&c| c == b' ').count();
            if spaces >= 2 && bytes.get(i + spaces) == Some(&b'\n') {
                out.push_str("<br>\n");
                i += spaces + 1;
                continue;
            }
        }

        // Inline code span.
        if b == b'`' {
            let ticks = bytes[i..].iter().take_while(|&&c| c == b'`').count();
            if let Some(close_pos) = find_closing_ticks(bytes, i + ticks, ticks) {
                let mut inner = s[i + ticks..close_pos].replace('\n', " ");
                if inner.len() >= 2
                    && inner.starts_with(' ')
                    && inner.ends_with(' ')
                    && !inner.bytes().all(|c| c == b' ')
                {
                    inner = inner[1..inner.len() - 1].to_string();
                }
                out.push_str("<code>");
                out.push_str(&escape_html(&inner));
                out.push_str("</code>");
                i = close_pos + ticks;
                continue;
            }
            // No matching closing ticks: emit literally.
            for _ in 0..ticks {
                out.push('`');
            }
            i += ticks;
            continue;
        }

        // Image `![alt](dest "title")` or link `[text](dest "title")`.
        if depth < MAX_NESTING_DEPTH && ((b == b'!' && bytes.get(i + 1) == Some(&b'[')) || b == b'[') {
            let is_image = b == b'!';
            let bracket_start = if is_image { i + 1 } else { i };
            if let Some((label, dest, title, end_idx)) = parse_inline_link(s, bracket_start) {
                let rewritten = rewrite_link(dest, page_path, is_image);
                if is_image {
                    out.push_str("<img src=\"");
                    out.push_str(&escape_html(&rewritten));
                    out.push_str("\" alt=\"");
                    out.push_str(&escape_html(label));
                    out.push('"');
                    if let Some(t) = title {
                        out.push_str(" title=\"");
                        out.push_str(&escape_html(t));
                        out.push('"');
                    }
                    out.push('>');
                } else {
                    out.push_str("<a href=\"");
                    out.push_str(&escape_html(&rewritten));
                    out.push('"');
                    if let Some(t) = title {
                        out.push_str(" title=\"");
                        out.push_str(&escape_html(t));
                        out.push('"');
                    }
                    out.push('>');
                    out.push_str(&render_inline(label, page_path, depth + 1));
                    out.push_str("</a>");
                }
                i = end_idx;
                continue;
            }
        }

        // Autolink `<https://...>` or `<http://...>`.
        if b == b'<' {
            if let Some(close) = s[i + 1..].find('>') {
                let cand = &s[i + 1..i + 1 + close];
                if (cand.starts_with("https://") || cand.starts_with("http://"))
                    && !cand.chars().any(char::is_whitespace)
                    && !cand.contains('<')
                {
                    let esc = escape_html(cand);
                    out.push_str("<a href=\"");
                    out.push_str(&esc);
                    out.push_str("\">");
                    out.push_str(&esc);
                    out.push_str("</a>");
                    i += 1 + close + 1;
                    continue;
                }
            }
            out.push_str("&lt;");
            i += 1;
            continue;
        }

        // Strong (`**` / `__`) and emphasis (`*` / `_`).
        if depth < MAX_NESTING_DEPTH && (b == b'*' || b == b'_') {
            if let Some((tag, inner, next_i)) = parse_emphasis(s, i, b) {
                out.push('<');
                out.push_str(tag);
                out.push('>');
                out.push_str(&render_inline(inner, page_path, depth + 1));
                out.push_str("</");
                out.push_str(tag);
                out.push('>');
                i = next_i;
                continue;
            }
        }

        // Ordinary character (HTML-escaped).
        if let Some(ch) = s[i..].chars().next() {
            match ch {
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                '"' => out.push_str("&quot;"),
                '\'' => out.push_str("&#39;"),
                _ => out.push(ch),
            }
            i += ch.len_utf8();
        } else {
            break;
        }
    }
    out
}

fn find_closing_ticks(bytes: &[u8], start: usize, count: usize) -> Option<usize> {
    let mut i = start;
    while i < bytes.len() {
        if bytes[i] == b'`' {
            let run = bytes[i..].iter().take_while(|&&c| c == b'`').count();
            if run == count {
                return Some(i);
            }
            i += run;
        } else {
            i += 1;
        }
    }
    None
}

fn parse_inline_link(s: &str, bracket_start: usize) -> Option<(&str, &str, Option<&str>, usize)> {
    let bytes = s.as_bytes();
    if bytes.get(bracket_start) != Some(&b'[') {
        return None;
    }
    // Find matching `]`, respecting nested brackets and inline code spans.
    let mut i = bracket_start + 1;
    let mut nest = 0usize;
    let mut close_bracket = None;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'`' => {
                let ticks = bytes[i..].iter().take_while(|&&c| c == b'`').count();
                if let Some(end) = find_closing_ticks(bytes, i + ticks, ticks) {
                    i = end + ticks;
                } else {
                    i += ticks;
                }
            }
            b'[' => {
                nest += 1;
                i += 1;
            }
            b']' => {
                if nest == 0 {
                    close_bracket = Some(i);
                    break;
                }
                nest -= 1;
                i += 1;
            }
            _ => i += 1,
        }
    }
    let close_bracket = close_bracket?;
    if bytes.get(close_bracket + 1) != Some(&b'(') {
        return None;
    }
    // Parse destination and optional `"title"` inside `(...)`.
    let mut j = close_bracket + 2;
    let mut paren_depth = 0usize;
    let mut in_quote: Option<u8> = None;
    let mut close_paren = None;
    while j < bytes.len() {
        let c = bytes[j];
        if let Some(q) = in_quote {
            if c == b'\\' {
                j += 2;
                continue;
            }
            if c == q {
                in_quote = None;
            }
            j += 1;
            continue;
        }
        match c {
            b'\\' => j += 2,
            b'"' | b'\'' => {
                in_quote = Some(c);
                j += 1;
            }
            b'(' => {
                paren_depth += 1;
                j += 1;
            }
            b')' => {
                if paren_depth == 0 {
                    close_paren = Some(j);
                    break;
                }
                paren_depth -= 1;
                j += 1;
            }
            b'\n' => return None,
            _ => j += 1,
        }
    }
    let close_paren = close_paren?;
    let label = &s[bracket_start + 1..close_bracket];
    let inside = s[close_bracket + 2..close_paren].trim();
    let (dest, title) = split_dest_and_title(inside);
    Some((label, dest, title, close_paren + 1))
}

fn split_dest_and_title(inside: &str) -> (&str, Option<&str>) {
    if let Some(after_lt) = inside.strip_prefix('<') {
        if let Some(gt) = after_lt.find('>') {
            let dest = &after_lt[..gt];
            let rest = after_lt[gt + 1..].trim();
            return (dest, strip_quotes(rest));
        }
    }
    if let Some(ws) = inside.find(char::is_whitespace) {
        let dest = &inside[..ws];
        let rest = inside[ws..].trim();
        if let Some(t) = strip_quotes(rest) {
            return (dest, Some(t));
        }
    }
    (inside, None)
}

fn strip_quotes(s: &str) -> Option<&str> {
    if s.len() >= 2 {
        if let Some(t) = s.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
            return Some(t);
        }
        if let Some(t) = s.strip_prefix('\'').and_then(|r| r.strip_suffix('\'')) {
            return Some(t);
        }
    }
    None
}

fn is_word_char(b: u8) -> bool {
    b.is_ascii_alphanumeric()
}

fn parse_emphasis(s: &str, start: usize, delim: u8) -> Option<(&'static str, &str, usize)> {
    let bytes = s.as_bytes();
    let double = bytes.get(start + 1) == Some(&delim);
    let len = if double { 2 } else { 1 };
    let tag = if double { "strong" } else { "em" };
    // For `_`, do not open emphasis inside a word (e.g. `demo.read_item`).
    if delim == b'_' && start > 0 && is_word_char(bytes[start - 1]) {
        return None;
    }
    let content_start = start + len;
    // Opening delimiter cannot be followed by whitespace or the same delimiter.
    let first = *bytes.get(content_start)?;
    if first.is_ascii_whitespace() || first == delim {
        return None;
    }
    let mut i = content_start;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'`' => {
                let ticks = bytes[i..].iter().take_while(|&&c| c == b'`').count();
                if let Some(end) = find_closing_ticks(bytes, i + ticks, ticks) {
                    i = end + ticks;
                } else {
                    i += ticks;
                }
            }
            c if c == delim => {
                let matches = if double { bytes.get(i + 1) == Some(&delim) } else { true };
                if matches && i > content_start && !bytes[i - 1].is_ascii_whitespace() {
                    let after = i + len;
                    // For `_`, do not close emphasis inside a word.
                    if delim == b'_' && bytes.get(after).is_some_and(|&b| is_word_char(b)) {
                        i += 1;
                        continue;
                    }
                    return Some((tag, &s[content_start..i], after));
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    None
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn relative_href_never_starts_or_ends_with_slash() {
        assert_eq!(relative_href("book/index.html", "book/units.html"), "units.html");
        assert_eq!(relative_href("book/index.html", "contributor/guide.html"), "../contributor/guide.html");
        assert_eq!(relative_href("ref/cli/demo.read_item@1.html", "book/index.html"), "../../book/index.html");
        assert_eq!(relative_href("ref/cli/demo.read_item@1.html", "frame.css"), "../../frame.css");
        assert_eq!(relative_href("index.html", "book/index.html"), "book/index.html");
    }

    #[test]
    fn link_rewriting_enforces_relative_file_names() {
        assert_eq!(rewrite_link("units.md", "book/index.html", false), "units.html");
        assert_eq!(rewrite_link("./units.md#si", "book/index.html", false), "units.html#si");
        assert_eq!(rewrite_link("/book/units.md", "contributor/guide.html", false), "../book/units.html");
        assert_eq!(rewrite_link("/book/", "contributor/guide.html", false), "../book/index.html");
        assert_eq!(rewrite_link("../book/", "contributor/guide.html", false), "../book/index.html");
        assert_eq!(rewrite_link("#local", "book/index.html", false), "#local");
        assert_eq!(rewrite_link("javascript:alert(1)", "book/index.html", false), "#");
        assert_eq!(rewrite_link("//evil.example/x", "book/index.html", false), "#");
    }

    #[test]
    fn capability_comments_kept_and_other_html_escaped() {
        let md = "# Title\n\n<!-- cap: GOM.ALN.01 -->\n\nText <!-- cap-planned: GOM.ALN.02 --> and <!-- edition-notice --> and <script>x</script>.\n";
        let r = render_markdown(md, "book/index.html");
        assert!(r.html.contains("<!-- cap: GOM.ALN.01 -->"));
        assert!(r.html.contains("<!-- cap-planned: GOM.ALN.02 -->"));
        assert!(!r.html.contains("<!-- edition-notice -->"));
        assert!(r.html.contains("&lt;!-- edition-notice --&gt;"));
        assert!(!r.html.contains("<script>"));
        assert!(r.html.contains("&lt;script&gt;x&lt;/script&gt;"));
    }

    #[test]
    fn heading_slugs_deduplicate_and_avoid_reserved_ids() {
        let md = "# Main Content\n\n## Details\n\n## Details\n";
        let r = render_markdown(md, "book/index.html");
        assert!(r.html.contains("<h1 id=\"main-content-2\">Main Content</h1>"));
        assert!(r.html.contains("<h2 id=\"details\">Details</h2>"));
        assert!(r.html.contains("<h2 id=\"details-2\">Details</h2>"));
    }

    #[test]
    fn tables_fenced_code_and_intraword_underscores() {
        let md = "# Units\n\nCall `demo.read_item` or demo.read_item_two.\n\n| Name | Unit |\n| --- | ---: |\n| length | m |\n\n```sh run\nfid doctor\n```\n";
        let r = render_markdown(md, "book/units.html");
        assert!(r.html.contains("demo.read_item_two"), "{}", r.html);
        assert!(!r.html.contains("<em>item</em>"));
        assert!(r
            .html
            .contains("<table>\n<thead>\n<tr><th>Name</th><th class=\"align-right\">Unit</th></tr>\n</thead>"));
        assert!(r.html.contains("<pre><code class=\"language-sh\">fid doctor\n</code></pre>"));
    }
}
