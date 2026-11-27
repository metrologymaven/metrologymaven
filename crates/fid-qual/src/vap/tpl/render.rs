//! Deterministic Markdown and ODF Text renderers for checked templates
//! (REQ-QUAL.VAP.TPL-003 and -005).
//!
//! Both renderers walk one block list built by [`blocks`], so the two files always hold the
//! same sections, fields and rows in the same order. The block list is:
//!
//! 1. heading 1: the title;
//! 2. `Document: <id>, template version <version>`;
//! 3. the interpretation notice ([`INTERPRETATION_NOTICE`], D-045);
//! 4. `Pre-filled for intended uses: <sorted selected IDs, ", "-joined, or none>`;
//! 5. the licence notice;
//! 6. when the template has framework references: heading 2 `Framework references` and a
//!    table `Designator | Edition | Clause`;
//! 7. per section: heading 2, the optional paragraph, then per field:
//!    - text: `<label>: <value>` when its rule names a selected use, followed by
//!      `(pre-filled for: <matching uses>)`; otherwise `<label>: (to be completed)`;
//!    - choice: the same with `(choose one)`, then `(options: <a> / <b> ...)`;
//!    - signature: `<label>: name ______, signature ______, date ______`;
//!    - table: the label, then the table. Static cells always show; pre-fill cells show only
//!      when the row's rule names a selected use; customer cells are blank. A table with a
//!      pre-fill column gets a last column `Pre-filled for` naming the matching uses.
//!
//! Markdown: `#`/`##` headings, labelled lines as `**<label>:** <value>`, the pre-fill mark in
//! `_..._`, a table label as `**<label>**`, pipe tables with a `| --- |` separator; blocks are
//! separated by one blank line and the file ends with one newline. Every character of
//! `` \`*_[]<>|#& `` that comes from the template or the fixed phrases is backslash-escaped.
//!
//! ODF: `content.xml` with one element per line (`text:h`, `text:p`, `table:table` with a
//! header-row group, tables named `Table1`, `Table2`, ... in order), `&`, `<` and `>`
//! escaped; packaged by [`super::odf::package`].

use super::{FieldBody, Fill, Selection, Template, TplError};

/// The interpretation notice every rendered document carries (D-045).
pub const INTERPRETATION_NOTICE: &str = "Interpretation: provisional. The framework references and judgements in \
this template have not been reviewed by a qualified person; check each one before relying on it.";

const TO_COMPLETE: &str = "(to be completed)";
const CHOOSE_ONE: &str = "(choose one)";
const SIGNATURE: &str = "name ______, signature ______, date ______";

/// One rendered block, shared by both output formats.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Heading { level: u8, text: String },
    Para(String),
    Labelled { label: String, value: String, mark: Option<String>, options: Option<Vec<String>> },
    Table { label: Option<String>, headers: Vec<String>, rows: Vec<Vec<String>> },
}

/// Builds the block list of `template` rendered for `selection`.
pub fn blocks(template: &Template, selection: &Selection) -> Vec<Block> {
    let mut out = Vec::new();
    out.push(Block::Heading { level: 1, text: template.title().to_string() });
    out.push(Block::Para(format!("Document: {}, template version {}", template.id(), template.version())));
    out.push(Block::Para(INTERPRETATION_NOTICE.to_string()));
    let sel = if selection.ids().is_empty() { "none".to_string() } else { selection.ids().join(", ") };
    out.push(Block::Para(format!("Pre-filled for intended uses: {sel}")));
    out.push(Block::Para(template.licence_notice().to_string()));
    if !template.refs().is_empty() {
        out.push(Block::Heading { level: 2, text: "Framework references".to_string() });
        out.push(Block::Table {
            label: None,
            headers: vec!["Designator".to_string(), "Edition".to_string(), "Clause".to_string()],
            rows: template
                .refs()
                .iter()
                .map(|r| vec![r.designator.clone(), r.edition.clone(), r.clause.clone()])
                .collect(),
        });
    }
    for s in template.sections() {
        out.push(Block::Heading { level: 2, text: s.heading.clone() });
        if let Some(t) = &s.text {
            out.push(Block::Para(t.clone()));
        }
        for f in &s.fields {
            match &f.body {
                FieldBody::Text { prefill } | FieldBody::Choice { prefill, .. } => {
                    let options = match &f.body {
                        FieldBody::Choice { options, .. } => Some(options.clone()),
                        _ => None,
                    };
                    let placeholder = if options.is_some() { CHOOSE_ONE } else { TO_COMPLETE };
                    let (value, mark) = match prefill {
                        Some(p) => {
                            let m = selection.matching(&p.uses);
                            if m.is_empty() {
                                (placeholder.to_string(), None)
                            } else {
                                (p.value.clone(), Some(m.join(", ")))
                            }
                        }
                        None => (placeholder.to_string(), None),
                    };
                    out.push(Block::Labelled { label: f.label.clone(), value, mark, options });
                }
                FieldBody::Signature => out.push(Block::Labelled {
                    label: f.label.clone(),
                    value: SIGNATURE.to_string(),
                    mark: None,
                    options: None,
                }),
                FieldBody::Table { columns, rows } => {
                    let marked = columns.iter().any(|c| c.fill == Fill::Prefill);
                    let mut headers: Vec<String> = columns.iter().map(|c| c.heading.clone()).collect();
                    if marked {
                        headers.push("Pre-filled for".to_string());
                    }
                    let mut body = Vec::with_capacity(rows.len());
                    for r in rows {
                        let m = r.prefill.as_ref().map(|u| selection.matching(u)).unwrap_or_default();
                        let mut cells: Vec<String> = columns
                            .iter()
                            .map(|c| {
                                let show = match c.fill {
                                    Fill::Static => true,
                                    Fill::Prefill => !m.is_empty(),
                                    Fill::Customer => false,
                                };
                                if show {
                                    r.cells.get(&c.id).cloned().unwrap_or_default()
                                } else {
                                    String::new()
                                }
                            })
                            .collect();
                        if marked {
                            cells.push(m.join(", "));
                        }
                        body.push(cells);
                    }
                    out.push(Block::Table { label: Some(f.label.clone()), headers, rows: body });
                }
            }
        }
    }
    out
}

fn md_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        if matches!(c, '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '|' | '#' | '&') {
            o.push('\\');
        }
        o.push(c);
    }
    o
}

fn md_row(cells: &[String]) -> String {
    let inner: Vec<String> = cells.iter().map(|c| md_escape(c.as_str())).collect();
    format!("| {} |", inner.join(" | "))
}

/// Renders `template` for `selection` as Markdown (UTF-8, `\n` line ends).
pub fn markdown(template: &Template, selection: &Selection) -> String {
    let mut parts: Vec<String> = Vec::new();
    for b in blocks(template, selection) {
        match b {
            Block::Heading { level, text } => {
                let hashes = if level == 1 { "#" } else { "##" };
                parts.push(format!("{hashes} {}", md_escape(&text)));
            }
            Block::Para(t) => parts.push(md_escape(&t)),
            Block::Labelled { label, value, mark, options } => {
                let mut line = format!("**{}:** {}", md_escape(&label), md_escape(&value));
                if let Some(m) = mark {
                    line.push_str(&format!(" _(pre-filled for: {})_", md_escape(&m)));
                }
                if let Some(o) = options {
                    line.push_str(&format!(" (options: {})", md_escape(&o.join(" / "))));
                }
                parts.push(line);
            }
            Block::Table { label, headers, rows } => {
                if let Some(l) = label {
                    parts.push(format!("**{}**", md_escape(&l)));
                }
                let mut lines = vec![md_row(&headers), format!("|{}", " --- |".repeat(headers.len()))];
                lines.extend(rows.iter().map(|r| md_row(r.as_slice())));
                parts.push(lines.join("\n"));
            }
        }
    }
    let mut s = parts.join("\n\n");
    s.push('\n');
    s
}

fn xml_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            _ => o.push(c),
        }
    }
    o
}

fn xml_row(cells: &[String]) -> String {
    let mut o = String::from("<table:table-row>");
    for c in cells {
        o.push_str("<table:table-cell office:value-type=\"string\"><text:p>");
        o.push_str(&xml_escape(c));
        o.push_str("</text:p></table:table-cell>");
    }
    o.push_str("</table:table-row>");
    o
}

/// The ODF `content.xml` of `template` rendered for `selection`.
pub fn odt_content_xml(template: &Template, selection: &Selection) -> String {
    let mut lines: Vec<String> = vec![
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>".to_string(),
        "<office:document-content xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" \
         xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" \
         xmlns:table=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\" office:version=\"1.3\">"
            .to_string(),
        "<office:body>".to_string(),
        "<office:text>".to_string(),
    ];
    let mut tables = 0u32;
    for b in blocks(template, selection) {
        match b {
            Block::Heading { level, text } => {
                lines.push(format!("<text:h text:outline-level=\"{level}\">{}</text:h>", xml_escape(&text)));
            }
            Block::Para(t) => lines.push(format!("<text:p>{}</text:p>", xml_escape(&t))),
            Block::Labelled { label, value, mark, options } => {
                let mut t = format!("{label}: {value}");
                if let Some(m) = mark {
                    t.push_str(&format!(" (pre-filled for: {m})"));
                }
                if let Some(o) = options {
                    t.push_str(&format!(" (options: {})", o.join(" / ")));
                }
                lines.push(format!("<text:p>{}</text:p>", xml_escape(&t)));
            }
            Block::Table { label, headers, rows } => {
                if let Some(l) = label {
                    lines.push(format!("<text:p>{}</text:p>", xml_escape(&l)));
                }
                tables = tables.saturating_add(1);
                lines.push(format!("<table:table table:name=\"Table{tables}\">"));
                lines.push(format!("<table:table-column table:number-columns-repeated=\"{}\"/>", headers.len()));
                lines.push("<table:table-header-rows>".to_string());
                lines.push(xml_row(&headers));
                lines.push("</table:table-header-rows>".to_string());
                lines.extend(rows.iter().map(|r| xml_row(r.as_slice())));
                lines.push("</table:table>".to_string());
            }
        }
    }
    lines.push("</office:text>".to_string());
    lines.push("</office:body>".to_string());
    lines.push("</office:document-content>".to_string());
    let mut s = lines.join("\n");
    s.push('\n');
    s
}

/// Renders `template` for `selection` as an ODF Text package (`.odt` bytes).
pub fn odt(template: &Template, selection: &Selection) -> Result<Vec<u8>, TplError> {
    super::odf::package(&odt_content_xml(template, selection))
}
