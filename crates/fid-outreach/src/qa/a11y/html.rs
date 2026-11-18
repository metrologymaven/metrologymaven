//! Bounded HTML tokenizer and simplified tree builder.
//!
//! The tokenizer follows the shape of the WHATWG tokenizer for the constructs that occur in
//! static pages (tags, attributes, comments, doctype, raw-text and RCDATA elements, a small set
//! of character references). Tree construction is simplified: void elements, matching end tags
//! and the common implied end tags (`p`, `li`, `dt`/`dd`, `option`, table cells and rows).
//! It never fails on malformed markup; it fails only when nesting exceeds [`MAX_DEPTH`].

/// Deepest accepted element nesting.
pub const MAX_DEPTH: usize = 512;

/// Index of a node in [`Doc::nodes`]; node 0 is the synthetic document root.
pub type NodeId = usize;

/// Maps byte offsets to 1-based line numbers.
pub struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    /// Indexes the line starts of `b`.
    pub fn new(b: &[u8]) -> Self {
        let mut starts = vec![0];
        for (i, c) in b.iter().enumerate() {
            if *c == b'\n' {
                starts.push(i + 1);
            }
        }
        LineIndex { starts }
    }

    /// 1-based line of byte offset `off`.
    pub fn line_of(&self, off: usize) -> u32 {
        let n = self.starts.partition_point(|s| *s <= off);
        u32::try_from(n).unwrap_or(u32::MAX)
    }
}

/// An element with lowercase name and attribute names (first occurrence of an attribute wins).
#[derive(Debug, Clone)]
pub struct Element {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub line: u32,
    pub children: Vec<NodeId>,
    pub parent: NodeId,
}

impl Element {
    /// Attribute value by lowercase name.
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    /// True when the attribute is present (any value).
    pub fn has(&self, name: &str) -> bool {
        self.attrs.iter().any(|(k, _)| k == name)
    }
}

/// A node of the parsed page.
#[derive(Debug, Clone)]
pub enum Node {
    Element(Element),
    Text { text: String, line: u32 },
}

/// A parsed page. Nodes are stored in document order.
#[derive(Debug, Clone)]
pub struct Doc {
    pub nodes: Vec<Node>,
}

impl Doc {
    /// The element with this id, if the node is an element.
    pub fn element(&self, id: NodeId) -> Option<&Element> {
        match self.nodes.get(id) {
            Some(Node::Element(e)) => Some(e),
            _ => None,
        }
    }

    /// Every element except the document root, in document order.
    pub fn elements(&self) -> impl Iterator<Item = (NodeId, &Element)> + '_ {
        self.nodes.iter().enumerate().skip(1).filter_map(|(i, n)| match n {
            Node::Element(e) => Some((i, e)),
            Node::Text { .. } => None,
        })
    }

    /// Ancestors of `id`, nearest first, excluding the document root.
    pub fn ancestors(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut cur = id;
        while let Some(e) = self.element(cur) {
            if e.parent == 0 || e.parent == cur || out.len() > MAX_DEPTH {
                break;
            }
            out.push(e.parent);
            cur = e.parent;
        }
        out
    }

    /// Descendants of `id` in document order.
    pub fn descendants(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack: Vec<NodeId> = match self.element(id) {
            Some(e) => e.children.iter().rev().copied().collect(),
            None => Vec::new(),
        };
        while let Some(n) = stack.pop() {
            out.push(n);
            if let Some(e) = self.element(n) {
                stack.extend(e.children.iter().rev().copied());
            }
        }
        out
    }

    /// Concatenated text of all descendant text nodes (scripts excluded by the parser).
    pub fn text_content(&self, id: NodeId) -> String {
        let mut s = String::new();
        for n in self.descendants(id) {
            if let Some(Node::Text { text, .. }) = self.nodes.get(n) {
                s.push_str(text);
            }
        }
        s
    }

    /// Line of the first text child of `id` (for `<style>` contents), else the element line.
    pub fn first_text_line(&self, id: NodeId) -> u32 {
        let Some(e) = self.element(id) else { return 1 };
        for c in &e.children {
            if let Some(Node::Text { line, .. }) = self.nodes.get(*c) {
                return *line;
            }
        }
        e.line
    }
}

const VOID: &[&str] =
    &["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr"];

const CLOSES_P: &[&str] = &[
    "address",
    "article",
    "aside",
    "blockquote",
    "details",
    "div",
    "dl",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hr",
    "main",
    "nav",
    "ol",
    "p",
    "pre",
    "section",
    "table",
    "ul",
];

struct Builder {
    nodes: Vec<Node>,
    stack: Vec<NodeId>,
}

impl Builder {
    fn new() -> Self {
        let root =
            Element { name: "#document".to_string(), attrs: Vec::new(), line: 1, children: Vec::new(), parent: 0 };
        Builder { nodes: vec![Node::Element(root)], stack: vec![0] }
    }

    fn current(&self) -> NodeId {
        self.stack.last().copied().unwrap_or(0)
    }

    fn name_at(&self, pos: usize) -> &str {
        let id = self.stack.get(pos).copied().unwrap_or(0);
        match self.nodes.get(id) {
            Some(Node::Element(e)) => e.name.as_str(),
            _ => "",
        }
    }

    fn append(&mut self, node: Node) -> NodeId {
        let parent = self.current();
        let id = self.nodes.len();
        self.nodes.push(node);
        if let Some(Node::Element(p)) = self.nodes.get_mut(parent) {
            p.children.push(id);
        }
        id
    }

    /// Pops up to and including the nearest open element named in `targets`, unless an
    /// element named in `boundary` is reached first.
    fn close_in_scope(&mut self, targets: &[&str], boundary: &[&str]) {
        let mut pos = self.stack.len();
        while pos > 1 {
            pos -= 1;
            let name = self.name_at(pos);
            let hit = targets.contains(&name);
            let stop = boundary.contains(&name);
            if hit {
                self.stack.truncate(pos);
                return;
            }
            if stop {
                return;
            }
        }
    }

    fn in_foreign(&self) -> bool {
        (1..self.stack.len()).any(|p| matches!(self.name_at(p), "svg" | "math"))
    }

    fn start(
        &mut self,
        name: String,
        attrs: Vec<(String, String)>,
        line: u32,
        self_closing: bool,
    ) -> Result<(), String> {
        match name.as_str() {
            "li" => self.close_in_scope(&["li"], &["ul", "ol", "menu"]),
            "dt" | "dd" => self.close_in_scope(&["dt", "dd"], &["dl"]),
            "option" => self.close_in_scope(&["option"], &["select", "datalist", "optgroup"]),
            "optgroup" => self.close_in_scope(&["option", "optgroup"], &["select"]),
            "tr" => self.close_in_scope(&["tr"], &["table", "thead", "tbody", "tfoot"]),
            "td" | "th" => self.close_in_scope(&["td", "th"], &["tr", "table"]),
            "thead" | "tbody" | "tfoot" => self.close_in_scope(&["thead", "tbody", "tfoot"], &["table"]),
            _ => {}
        }
        if CLOSES_P.contains(&name.as_str()) {
            self.close_in_scope(&["p"], &["button", "table", "td", "th", "caption", "template"]);
        }
        let foreign = self.in_foreign() || name == "svg" || name == "math";
        let void = VOID.contains(&name.as_str());
        let parent = self.current();
        let id = self.append(Node::Element(Element { name, attrs, line, children: Vec::new(), parent }));
        if void || (self_closing && foreign) {
            return Ok(());
        }
        if self.stack.len() >= MAX_DEPTH {
            return Err(format!("element nesting deeper than {MAX_DEPTH} at line {line}"));
        }
        self.stack.push(id);
        Ok(())
    }

    fn end(&mut self, name: &str) {
        if VOID.contains(&name) {
            return;
        }
        let mut pos = self.stack.len();
        while pos > 1 {
            pos -= 1;
            if self.name_at(pos) == name {
                self.stack.truncate(pos);
                return;
            }
        }
    }

    fn text(&mut self, text: String, line: u32) {
        if !text.is_empty() {
            self.append(Node::Text { text, line });
        }
    }
}

fn slice(b: &[u8], from: usize, to: usize) -> &str {
    b.get(from..to).and_then(|x| std::str::from_utf8(x).ok()).unwrap_or("")
}

fn find(b: &[u8], from: usize, pat: &[u8]) -> Option<usize> {
    let hay = b.get(from..)?;
    if pat.is_empty() || hay.len() < pat.len() {
        return None;
    }
    hay.windows(pat.len()).position(|w| w == pat).map(|p| p + from)
}

fn find_ci(b: &[u8], from: usize, pat: &[u8]) -> Option<usize> {
    let hay = b.get(from..)?;
    if pat.is_empty() || hay.len() < pat.len() {
        return None;
    }
    hay.windows(pat.len()).position(|w| w.eq_ignore_ascii_case(pat)).map(|p| p + from)
}

fn is_ws(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | b'\x0c')
}

/// Decodes the character references `&amp; &lt; &gt; &quot; &apos; &nbsp;` and numeric
/// references; any other `&` is kept literally.
pub fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(p) = rest.find('&') {
        out.push_str(rest.get(..p).unwrap_or(""));
        let after = rest.get(p + 1..).unwrap_or("");
        match entity(after) {
            Some((ch, used)) => {
                out.push(ch);
                rest = after.get(used..).unwrap_or("");
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn entity(s: &str) -> Option<(char, usize)> {
    let semi = s.find(';')?;
    if semi == 0 || semi > 32 {
        return None;
    }
    let body = s.get(..semi)?;
    let ch = if let Some(num) = body.strip_prefix('#') {
        let v = match num.strip_prefix(['x', 'X']) {
            Some(h) => u32::from_str_radix(h, 16).ok()?,
            None => num.parse::<u32>().ok()?,
        };
        char::from_u32(v)?
    } else {
        match body {
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            "nbsp" => '\u{a0}',
            _ => return None,
        }
    };
    Some((ch, semi + 1))
}

type Attrs = Vec<(String, String)>;

/// Reads a start tag whose `<` is at `start`. Returns name, attributes, self-closing flag and
/// the offset just past the tag.
fn start_tag(b: &[u8], start: usize) -> (String, Attrs, bool, usize) {
    let n = b.len();
    let mut i = start + 1;
    let ns = i;
    while i < n && !is_ws(b[i]) && b[i] != b'/' && b[i] != b'>' {
        i += 1;
    }
    let name = slice(b, ns, i).to_ascii_lowercase();
    let mut attrs: Attrs = Vec::new();
    let mut self_closing = false;
    loop {
        while i < n && (is_ws(b[i]) || b[i] == b'/') {
            i += 1;
        }
        if i >= n {
            break;
        }
        if b[i] == b'>' {
            self_closing = i > 0 && b.get(i - 1) == Some(&b'/');
            i += 1;
            break;
        }
        let an = i;
        i += 1;
        while i < n && !is_ws(b[i]) && b[i] != b'/' && b[i] != b'>' && b[i] != b'=' {
            i += 1;
        }
        let aname = slice(b, an, i).to_ascii_lowercase();
        while i < n && is_ws(b[i]) {
            i += 1;
        }
        let mut value = String::new();
        if i < n && b[i] == b'=' {
            i += 1;
            while i < n && is_ws(b[i]) {
                i += 1;
            }
            if i < n && (b[i] == b'"' || b[i] == b'\'') {
                let q = b[i];
                i += 1;
                let vs = i;
                while i < n && b[i] != q {
                    i += 1;
                }
                value = decode_entities(slice(b, vs, i));
                if i < n {
                    i += 1;
                }
            } else {
                let vs = i;
                while i < n && !is_ws(b[i]) && b[i] != b'>' {
                    i += 1;
                }
                value = decode_entities(slice(b, vs, i));
            }
        }
        if !aname.is_empty() && !attrs.iter().any(|(k, _)| *k == aname) {
            attrs.push((aname, value));
        }
    }
    (name, attrs, self_closing, i)
}

/// Parses a page. Fails only when element nesting exceeds [`MAX_DEPTH`].
pub fn parse(src: &str) -> Result<Doc, String> {
    let b = src.as_bytes();
    let n = b.len();
    let lines = LineIndex::new(b);
    let mut builder = Builder::new();
    let mut i = 0usize;
    while i < n {
        if b[i] != b'<' {
            let end = find(b, i, b"<").unwrap_or(n);
            builder.text(decode_entities(slice(b, i, end)), lines.line_of(i));
            i = end;
            continue;
        }
        let next = b.get(i + 1).copied().unwrap_or(0);
        if b.get(i..i + 4) == Some(b"<!--".as_slice()) {
            // `<!-->` and `<!--->` are complete (empty) comments.
            i = if b.get(i + 4) == Some(&b'>') {
                i + 5
            } else if b.get(i + 4..i + 6) == Some(b"->".as_slice()) {
                i + 6
            } else {
                find(b, i + 4, b"-->").map_or(n, |p| p + 3)
            };
        } else if next == b'!' || next == b'?' {
            i = find(b, i + 2, b">").map_or(n, |p| p + 1);
        } else if next == b'/' {
            let after = b.get(i + 2).copied().unwrap_or(0);
            if after.is_ascii_alphabetic() {
                let ns = i + 2;
                let mut j = ns;
                while j < n && !is_ws(b[j]) && b[j] != b'/' && b[j] != b'>' {
                    j += 1;
                }
                let name = slice(b, ns, j).to_ascii_lowercase();
                builder.end(&name);
                i = find(b, j, b">").map_or(n, |p| p + 1);
            } else {
                i = find(b, i + 2, b">").map_or(n, |p| p + 1);
            }
        } else if next.is_ascii_alphabetic() {
            let line = lines.line_of(i);
            let (name, attrs, self_closing, after) = start_tag(b, i);
            i = after;
            let raw = matches!(name.as_str(), "script" | "style" | "title" | "textarea");
            let kind = name.clone();
            builder.start(name, attrs, line, self_closing)?;
            if raw {
                let close = format!("</{kind}");
                let end = find_ci(b, i, close.as_bytes()).unwrap_or(n);
                let content = slice(b, i, end);
                match kind.as_str() {
                    "style" => builder.text(content.to_string(), lines.line_of(i)),
                    "title" | "textarea" => builder.text(decode_entities(content), lines.line_of(i)),
                    _ => {}
                }
                i = end;
            }
        } else {
            builder.text("<".to_string(), lines.line_of(i));
            i += 1;
        }
    }
    Ok(Doc { nodes: builder.nodes })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn tags_lines_and_entities() {
        let d = parse("<p>a &amp; b\n<img alt='x &lt; y' src=a.png>\n<!-- <b> -->\n<P CLASS=k>c").unwrap();
        let els: Vec<_> = d.elements().map(|(_, e)| (e.name.clone(), e.line)).collect();
        assert_eq!(els, vec![("p".to_string(), 1), ("img".to_string(), 2), ("p".to_string(), 4)]);
        let img = d.elements().find(|(_, e)| e.name == "img").unwrap().1;
        assert_eq!(img.attr("alt"), Some("x < y"));
        assert_eq!(d.text_content(0).trim(), "a & b\n\n\nc");
    }

    #[test]
    fn raw_text_and_implied_ends() {
        let d = parse("<ul><li>a<li>b</ul><script>if (a<b) {}</script><style>p{}</style>").unwrap();
        let ul = d.elements().find(|(_, e)| e.name == "ul").unwrap().0;
        assert_eq!(d.element(ul).unwrap().children.len(), 2);
        assert!(d.elements().all(|(_, e)| e.name != "b"));
    }

    #[test]
    fn abrupt_empty_comments_close() {
        let d = parse("<!--><p>a</p><!---><i>b</i>").unwrap();
        let names: Vec<_> = d.elements().map(|(_, e)| e.name.clone()).collect();
        assert_eq!(names, vec!["p".to_string(), "i".to_string()]);
    }
}
