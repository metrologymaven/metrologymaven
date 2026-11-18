//! The comparator and its first-difference report (REQ-IO.QIF.NORM-003).

use super::stream::{render_pi, render_start, CAttr, Item, ItemKind, Stream};
use super::{NormError, NormRules};
use fid_io_reg::{BoundedSource, Budget, UntrustedText};

/// The first difference between two documents under a qif-norm version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diff {
    /// XPath-like location with canonical prefixes and 1-based positions among siblings
    /// of the same name, e.g. `/q:Doc[1]/q:Value[2]`. For text it is the parent element;
    /// for an attribute it ends in `/@name`. Element names come from the documents.
    pub path: UntrustedText,
    /// Line in the first document where the differing item starts, if it has one there.
    pub line_a: Option<u32>,
    /// Line in the second document.
    pub line_b: Option<u32>,
    /// The value in the first document: the text or attribute value, or the canonical
    /// form of the differing tag; `None` when the first document has nothing there.
    pub a: Option<UntrustedText>,
    /// The value in the second document.
    pub b: Option<UntrustedText>,
}

/// The result of a comparison.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Equality {
    /// The documents are equal under the rules.
    Equal,
    /// The first difference in document order.
    Different(Diff),
}

/// Compares two documents under `rules`, each read under its own budget. Both documents
/// are read to their end even after a difference, so a malformed input is always an
/// error, wherever the difference is.
pub fn compare_with(
    a: &[u8],
    b: &[u8],
    budget_a: Budget,
    budget_b: Budget,
    rules: &NormRules,
) -> Result<Equality, NormError> {
    let mut sa = Stream::new(BoundedSource::new(a, budget_a), a.len() as u64, rules, true);
    let mut sb = Stream::new(BoundedSource::new(b, budget_b), b.len() as u64, rules, true);
    let mut found: Option<Diff> = None;
    loop {
        let ia = sa.next_item()?;
        let ib = sb.next_item()?;
        match (ia, ib) {
            (None, None) => break,
            (Some(x), Some(y)) if x.kind == y.kind => {}
            (x, y) => {
                found = Some(first_difference(x, y));
                break;
            }
        }
    }
    if found.is_some() {
        while sa.next_item()?.is_some() {}
        while sb.next_item()?.is_some() {}
    }
    Ok(found.map_or(Equality::Equal, Equality::Different))
}

fn first_difference(x: Option<Item>, y: Option<Item>) -> Diff {
    match (x, y) {
        (Some(x), Some(y)) => {
            match (&x.kind, &y.kind) {
                (ItemKind::Text(ta), ItemKind::Text(tb)) => {
                    return diff(&x.path, x.line, y.line, Some(ta.as_str()), Some(tb.as_str()));
                }
                (ItemKind::Start { name: na, attrs: aa, .. }, ItemKind::Start { name: nb, attrs: ab, .. })
                    if na == nb =>
                {
                    if let Some(d) = attr_difference(&x, &y, aa, ab) {
                        return d;
                    }
                }
                _ => {}
            }
            diff(&x.path, x.line, y.line, Some(render(&x.kind).as_str()), Some(render(&y.kind).as_str()))
        }
        (Some(x), None) => diff(&x.path, x.line, None, Some(render(&x.kind).as_str()), None),
        (None, Some(y)) => diff(&y.path, None, y.line, None, Some(render(&y.kind).as_str())),
        (None, None) => diff("", None, None, None, None),
    }
}

/// The first attribute, in canonical order, that is missing on one side or has a
/// different value.
fn attr_difference(x: &Item, y: &Item, aa: &[CAttr], ab: &[CAttr]) -> Option<Diff> {
    let (mut i, mut j) = (0, 0);
    loop {
        match (aa.get(i), ab.get(j)) {
            (None, None) => return None,
            (Some(p), Some(q)) => match (&p.uri, &p.local).cmp(&(&q.uri, &q.local)) {
                std::cmp::Ordering::Equal => {
                    if p.value != q.value {
                        let path = format!("{}/@{}", x.path, p.name);
                        return Some(diff(&path, x.line, y.line, Some(p.value.as_str()), Some(q.value.as_str())));
                    }
                    i += 1;
                    j += 1;
                }
                std::cmp::Ordering::Less => return Some(missing(x, y, Some(p), None)),
                std::cmp::Ordering::Greater => return Some(missing(x, y, None, Some(q))),
            },
            (Some(p), None) => return Some(missing(x, y, Some(p), None)),
            (None, Some(q)) => return Some(missing(x, y, None, Some(q))),
        }
    }
}

fn missing(x: &Item, y: &Item, p: Option<&CAttr>, q: Option<&CAttr>) -> Diff {
    let name = p.or(q).map_or("", |a| a.name.as_str());
    let path = format!("{}/@{}", x.path, name);
    diff(&path, x.line, y.line, p.map(|a| a.value.as_str()), q.map(|a| a.value.as_str()))
}

fn diff(path: &str, line_a: Option<u32>, line_b: Option<u32>, a: Option<&str>, b: Option<&str>) -> Diff {
    Diff { path: UntrustedText::new(path), line_a, line_b, a: a.map(UntrustedText::new), b: b.map(UntrustedText::new) }
}

/// The canonical form of one item, without namespace declarations.
fn render(kind: &ItemKind) -> String {
    let mut out = Vec::new();
    match kind {
        ItemKind::Start { .. } => render_start(&mut out, kind, false),
        ItemKind::End { name } => return format!("</{name}>"),
        ItemKind::Text(t) => return t.clone(),
        ItemKind::Pi { .. } => render_pi(&mut out, kind),
    }
    String::from_utf8_lossy(&out).into_owned()
}
