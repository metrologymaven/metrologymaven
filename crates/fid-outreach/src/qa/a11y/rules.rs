//! Page rules A01 to A11 (REQ-OUTR.SITE.QA.A11Y-001), evaluated on one parsed page.
//! The title-sharing part of A05 needs all pages and is done by the caller.

use super::html::{Doc, Element, Node, NodeId};
use super::vocab::{valid_bcp47, ARIA_ATTRS, ARIA_ROLES};
use std::collections::BTreeMap;

/// One rule finding on a page.
#[derive(Debug, Clone)]
pub struct Finding {
    pub line: u32,
    pub rule: &'static str,
    pub element: String,
    pub detail: String,
}

/// What the caller needs from a page besides its findings.
#[derive(Debug, Clone, Default)]
pub struct PageFacts {
    /// Normalised title text and its line, when the page has a non-empty title.
    pub title: Option<(String, u32)>,
    /// Contents of `<style>` elements with the line their text starts on.
    pub style_blocks: Vec<(String, u32)>,
    /// `style` attributes: value, line, element description.
    pub style_attrs: Vec<(String, u32, String)>,
}

fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn clip(s: &str) -> String {
    let c = collapse(s);
    if c.chars().count() > 60 {
        let mut t: String = c.chars().take(57).collect();
        t.push_str("...");
        t
    } else {
        c
    }
}

/// Short description of an element: tag plus identifying attributes.
pub fn describe(el: &Element) -> String {
    let mut s = format!("<{}", el.name);
    for k in ["id", "class", "name", "type", "href", "src", "role"] {
        if let Some(v) = el.attr(k) {
            s.push_str(&format!(" {k}=\"{}\"", clip(v)));
        }
    }
    s.push('>');
    s
}

fn roles(el: &Element) -> Vec<String> {
    el.attr("role").map(|r| r.split_whitespace().map(str::to_ascii_lowercase).collect()).unwrap_or_default()
}

fn has_role(el: &Element, wanted: &[&str]) -> bool {
    roles(el).iter().any(|r| wanted.contains(&r.as_str()))
}

struct Ctx<'a> {
    doc: &'a Doc,
    ids: BTreeMap<String, NodeId>,
}

impl Ctx<'_> {
    fn el(&self, id: NodeId) -> Option<&Element> {
        self.doc.element(id)
    }

    /// True when the element or an ancestor has `aria-hidden="true"`.
    fn hidden(&self, id: NodeId) -> bool {
        std::iter::once(id).chain(self.doc.ancestors(id)).any(|n| {
            self.el(n).and_then(|e| e.attr("aria-hidden")).is_some_and(|v| v.trim().eq_ignore_ascii_case("true"))
        })
    }

    fn labelledby(&self, el: &Element) -> String {
        let Some(refs) = el.attr("aria-labelledby") else { return String::new() };
        let parts: Vec<String> = refs
            .split_whitespace()
            .filter_map(|r| self.ids.get(r))
            .map(|n| collapse(&self.content_name(*n)))
            .filter(|t| !t.is_empty())
            .collect();
        parts.join(" ")
    }

    /// `aria-labelledby` then `aria-label`, collapsed; empty when neither gives a name.
    fn aria_name(&self, el: &Element) -> String {
        let by = self.labelledby(el);
        if !by.is_empty() {
            return by;
        }
        collapse(el.attr("aria-label").unwrap_or(""))
    }

    /// Name from content: text, `alt` of images, names of SVGs and labelled descendants;
    /// hidden subtrees and scripts are skipped.
    fn content_name(&self, id: NodeId) -> String {
        let mut out = String::new();
        let mut stack: Vec<NodeId> = match self.el(id) {
            Some(e) => e.children.iter().rev().copied().collect(),
            None => Vec::new(),
        };
        while let Some(n) = stack.pop() {
            match self.doc.nodes.get(n) {
                Some(Node::Text { text, .. }) => {
                    out.push(' ');
                    out.push_str(text);
                }
                Some(Node::Element(e)) => {
                    if e.attr("aria-hidden").is_some_and(|v| v.trim().eq_ignore_ascii_case("true")) {
                        continue;
                    }
                    let own = self.aria_name(e);
                    if !own.is_empty() {
                        out.push(' ');
                        out.push_str(&own);
                        continue;
                    }
                    match e.name.as_str() {
                        "img" => {
                            out.push(' ');
                            out.push_str(e.attr("alt").unwrap_or(""));
                        }
                        "svg" => {
                            out.push(' ');
                            out.push_str(&self.svg_title(n));
                        }
                        "style" | "script" | "template" => {}
                        _ => stack.extend(e.children.iter().rev().copied()),
                    }
                }
                None => {}
            }
        }
        collapse(&out)
    }

    /// Text of the first `<title>` child of an SVG.
    fn svg_title(&self, id: NodeId) -> String {
        let Some(e) = self.el(id) else { return String::new() };
        for c in &e.children {
            if self.el(*c).is_some_and(|t| t.name == "title") {
                return collapse(&self.doc.text_content(*c));
            }
        }
        String::new()
    }

    /// Simplified accessible name: aria-labelledby, aria-label, content, title.
    fn accessible_name(&self, id: NodeId) -> String {
        let Some(el) = self.el(id) else { return String::new() };
        let aria = self.aria_name(el);
        if !aria.is_empty() {
            return aria;
        }
        let content = self.content_name(id);
        if !content.is_empty() {
            return content;
        }
        collapse(el.attr("title").unwrap_or(""))
    }

    fn label_text(&self, el: &Element, id: NodeId) -> bool {
        if !self.aria_name(el).is_empty() || !collapse(el.attr("title").unwrap_or("")).is_empty() {
            return true;
        }
        let ancestor_label = self
            .doc
            .ancestors(id)
            .into_iter()
            .any(|a| self.el(a).is_some_and(|e| e.name == "label") && !self.content_name(a).is_empty());
        if ancestor_label {
            return true;
        }
        let Some(own) = el.attr("id").filter(|v| !v.is_empty()) else { return false };
        self.doc
            .elements()
            .any(|(lid, l)| l.name == "label" && l.attr("for") == Some(own) && !self.content_name(lid).is_empty())
    }
}

fn push(out: &mut Vec<Finding>, line: u32, rule: &'static str, element: String, detail: &str) {
    out.push(Finding { line, rule, element, detail: detail.to_string() });
}

/// Runs rules A01 to A11 and the per-page part of A05 on one page.
pub fn check_page(doc: &Doc) -> (Vec<Finding>, PageFacts) {
    let mut ids: BTreeMap<String, NodeId> = BTreeMap::new();
    for (id, el) in doc.elements() {
        if let Some(v) = el.attr("id") {
            ids.entry(v.to_string()).or_insert(id);
        }
    }
    let cx = Ctx { doc, ids };
    let mut out: Vec<Finding> = Vec::new();
    let mut facts = PageFacts::default();

    let html = doc.elements().find(|(_, e)| e.name == "html");
    let html_line = html.map_or(1, |(_, e)| e.line);
    let body_line = doc.elements().find(|(_, e)| e.name == "body").map_or(html_line, |(_, e)| e.line);

    // A07: lang on html.
    match html.map(|(_, e)| e) {
        None => push(&mut out, 1, "A07", "<html>".to_string(), "no html element"),
        Some(e) => match e.attr("lang").map(str::trim) {
            None | Some("") => push(&mut out, e.line, "A07", describe(e), "missing lang"),
            Some(l) if !valid_bcp47(l) => {
                push(&mut out, e.line, "A07", describe(e), &format!("invalid BCP 47 language tag \"{}\"", clip(l)))
            }
            Some(_) => {}
        },
    }

    // A05 (per page): missing or empty title. SVG titles do not count.
    let title = doc.elements().find(|(id, e)| {
        e.name == "title" && !doc.ancestors(*id).iter().any(|a| doc.element(*a).is_some_and(|p| p.name == "svg"))
    });
    match title {
        None => {
            let line = doc.elements().find(|(_, e)| e.name == "head").map_or(html_line, |(_, e)| e.line);
            push(&mut out, line, "A05", "<title>".to_string(), "missing title");
        }
        Some((id, e)) => {
            let t = collapse(&doc.text_content(id));
            if t.is_empty() {
                push(&mut out, e.line, "A05", "<title>".to_string(), "empty title");
            } else {
                facts.title = Some((t, e.line));
            }
        }
    }

    // A02: exactly one h1 and no skipped heading level.
    let mut h1 = 0u32;
    let mut prev = 0u32;
    let mut first_heading_line = None;
    for (_, e) in doc.elements() {
        let level = match e.name.as_str() {
            "h1" => 1,
            "h2" => 2,
            "h3" => 3,
            "h4" => 4,
            "h5" => 5,
            "h6" => 6,
            _ => continue,
        };
        if first_heading_line.is_none() {
            first_heading_line = Some(e.line);
        }
        if level == 1 {
            h1 += 1;
            if h1 > 1 {
                push(&mut out, e.line, "A02", describe(e), "more than one h1");
            }
        }
        if level > prev + 1 {
            push(&mut out, e.line, "A02", describe(e), &format!("heading level skipped: h{level} after h{prev}"));
        }
        prev = level;
    }
    if h1 == 0 {
        push(&mut out, first_heading_line.unwrap_or(body_line), "A02", "<h1>".to_string(), "no h1");
    }

    // A09: main landmark and a skip link to it.
    let main = doc.elements().find(|(_, e)| e.name == "main" || has_role(e, &["main"]));
    match main {
        None => push(&mut out, body_line, "A09", "<body>".to_string(), "no main landmark"),
        Some((_, m)) => {
            let target = m.attr("id").filter(|v| !v.is_empty()).map(|v| format!("#{v}"));
            let linked = target.as_deref().is_some_and(|t| {
                doc.elements().any(|(_, a)| a.name == "a" && a.attr("href").map(str::trim) == Some(t))
            });
            if !linked {
                push(&mut out, m.line, "A09", describe(m), "no skip link to the main landmark");
            }
        }
    }

    let mut seen_ids: BTreeMap<&str, u32> = BTreeMap::new();
    for (id, e) in doc.elements() {
        let name = e.name.as_str();
        let decorative = cx.hidden(id) || has_role(e, &["presentation", "none"]);

        // A01: text alternatives.
        if name == "img" && !decorative && e.attr("alt").is_none() && cx.aria_name(e).is_empty() {
            push(&mut out, e.line, "A01", describe(e), "img without alt");
        }
        if name == "input"
            && e.attr("type").is_some_and(|t| t.trim().eq_ignore_ascii_case("image"))
            && !decorative
            && collapse(e.attr("alt").unwrap_or("")).is_empty()
            && cx.aria_name(e).is_empty()
            && collapse(e.attr("title").unwrap_or("")).is_empty()
        {
            push(&mut out, e.line, "A01", describe(e), "image input without text alternative");
        }
        if name == "svg"
            && !decorative
            && !doc.ancestors(id).iter().any(|a| doc.element(*a).is_some_and(|p| p.name == "svg"))
            && cx.aria_name(e).is_empty()
            && cx.svg_title(id).is_empty()
        {
            push(&mut out, e.line, "A01", describe(e), "svg without accessible name or aria-hidden");
        }

        // A03: form controls need a label.
        let control = match name {
            "select" | "textarea" => true,
            "input" => !matches!(
                e.attr("type").map(|t| t.trim().to_ascii_lowercase()).as_deref(),
                Some("hidden" | "submit" | "reset" | "button" | "image")
            ),
            _ => false,
        };
        if control && !cx.hidden(id) && !cx.label_text(e, id) {
            push(&mut out, e.line, "A03", describe(e), "form control without a label");
        }

        // A04: data tables need header cells.
        if name == "table" && !has_role(e, &["presentation", "none"]) {
            let headers = doc
                .descendants(id)
                .into_iter()
                .any(|d| doc.element(d).is_some_and(|c| c.name == "th" || has_role(c, &["columnheader", "rowheader"])));
            if !headers {
                push(&mut out, e.line, "A04", describe(e), "table without header cells");
            }
        }

        // A06: links and buttons need a name.
        let is_link = name == "a" && e.has("href");
        let is_button = name == "button";
        if (is_link || is_button || has_role(e, &["link", "button"]))
            && !cx.hidden(id)
            && cx.accessible_name(id).is_empty()
        {
            let what = if is_button || has_role(e, &["button"]) { "button" } else { "link" };
            push(&mut out, e.line, "A06", describe(e), &format!("{what} without accessible name"));
        }

        // A08: duplicate ids, ARIA attributes and roles outside WAI-ARIA 1.2.
        if let Some(v) = e.attr("id") {
            if let Some(first) = seen_ids.get(v) {
                push(
                    &mut out,
                    e.line,
                    "A08",
                    describe(e),
                    &format!("duplicate id \"{}\" (first on line {first})", clip(v)),
                );
            } else {
                seen_ids.insert(v, e.line);
            }
        }
        for (k, _) in &e.attrs {
            if k.starts_with("aria-") && !ARIA_ATTRS.contains(&k.as_str()) {
                push(&mut out, e.line, "A08", describe(e), &format!("attribute {k} is not defined by WAI-ARIA 1.2"));
            }
        }
        for r in roles(e) {
            if !ARIA_ROLES.contains(&r.as_str()) {
                push(
                    &mut out,
                    e.line,
                    "A08",
                    describe(e),
                    &format!("role \"{}\" is not defined by WAI-ARIA 1.2", clip(&r)),
                );
            }
        }

        // A10: zoom must not be disabled.
        if name == "meta" && e.attr("name").is_some_and(|n| n.trim().eq_ignore_ascii_case("viewport")) {
            let content = e.attr("content").unwrap_or("").to_ascii_lowercase();
            let mut bad = Vec::new();
            for item in content.split([',', ';']) {
                let Some((k, v)) = item.split_once('=') else { continue };
                let (k, v) = (k.trim(), v.trim());
                if k == "user-scalable" && (v == "no" || v == "0") {
                    bad.push(format!("user-scalable={v}"));
                }
                if k == "maximum-scale" && v.parse::<f64>().is_ok_and(|s| s < 2.0) {
                    bad.push(format!("maximum-scale={v}"));
                }
            }
            if !bad.is_empty() {
                push(&mut out, e.line, "A10", describe(e), &format!("viewport disables zoom: {}", bad.join(", ")));
            }
        }

        // A11: autoplaying media must be muted and have controls.
        if (name == "audio" || name == "video") && e.has("autoplay") {
            let mut why = Vec::new();
            if !e.has("muted") {
                why.push("not muted");
            }
            if !e.has("controls") {
                why.push("no controls");
            }
            if !why.is_empty() {
                push(&mut out, e.line, "A11", describe(e), &format!("autoplay media {}", why.join(" and ")));
            }
        }

        // Inputs for the stylesheet rules.
        if name == "style" {
            facts.style_blocks.push((doc.text_content(id), doc.first_text_line(id)));
        }
        if let Some(st) = e.attr("style") {
            facts.style_attrs.push((st.to_string(), e.line, describe(e)));
        }
    }
    (out, facts)
}
