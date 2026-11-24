//! The synthetic report canvas of the leaf design (`Page { blocks: [Block { kind, rect_mm,
//! content }] }`), loaded from `fixtures/viz/report-canvas-200/canvas.tsv`, plus CPU hit
//! testing and the accessibility facts each block announces.
//!
//! Lengths are integers in tenths of a millimetre ("dmm") so that the Python reference
//! model and this crate compute snapping and clamping exactly.

use fid_hal_idl::UntrustedText;
use std::fmt;

/// Largest accepted canvas file in bytes.
pub const MAX_CANVAS_BYTES: usize = 4 * 1024 * 1024;
/// Largest accepted number of pages.
pub const MAX_PAGES: usize = 100;
/// Largest accepted number of blocks.
pub const MAX_BLOCKS: usize = 10_000;
/// Longest block name in bytes.
pub const MAX_NAME_BYTES: usize = 256;
/// Longest block text in characters (also the editing buffer limit).
pub const MAX_TEXT_CHARS: usize = 4096;
/// Largest number of table rows or columns.
pub const MAX_TABLE_SIDE: u32 = 50;
/// Smallest block width or height (5 mm).
pub const MIN_SIDE: i32 = 50;

/// Paper size of a page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Paper {
    /// ISO A4, 210 x 297 mm.
    A4,
    /// US Letter, 215.9 x 279.4 mm.
    Letter,
}

impl Paper {
    /// Width and height in dmm.
    pub fn size(self) -> (i32, i32) {
        match self {
            Paper::A4 => (2100, 2970),
            Paper::Letter => (2159, 2794),
        }
    }

    /// Name used in the fixture and the UI.
    pub fn name(self) -> &'static str {
        match self {
            Paper::A4 => "A4",
            Paper::Letter => "Letter",
        }
    }

    fn parse(s: &str) -> Option<Paper> {
        match s {
            "A4" => Some(Paper::A4),
            "Letter" => Some(Paper::Letter),
            _ => None,
        }
    }
}

/// What a block shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A page header or footer band with one line of text.
    Band,
    /// A rich-text block.
    Text,
    /// A table; its content is `<rows>x<columns>`.
    Table,
    /// The live wgpu 3D frame; its content is `-`.
    Frame3d,
}

impl Kind {
    /// Name used in the fixture.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Band => "band",
            Kind::Text => "text",
            Kind::Table => "table",
            Kind::Frame3d => "frame3d",
        }
    }

    /// Whether the block's text can be edited (bands and text blocks).
    pub fn editable(self) -> bool {
        matches!(self, Kind::Band | Kind::Text)
    }

    /// The type word a screen reader announces.
    pub fn word(self) -> &'static str {
        match self {
            Kind::Band => "Band",
            Kind::Text => "Text block",
            Kind::Table => "Table",
            Kind::Frame3d => "3D view",
        }
    }

    /// The accessibility role (mapped onto AccessKit roles by the example).
    pub fn role(self) -> &'static str {
        match self {
            Kind::Band => "group",
            Kind::Text => "paragraph",
            Kind::Table => "table",
            Kind::Frame3d => "image",
        }
    }

    fn parse(s: &str) -> Option<Kind> {
        match s {
            "band" => Some(Kind::Band),
            "text" => Some(Kind::Text),
            "table" => Some(Kind::Table),
            "frame3d" => Some(Kind::Frame3d),
            _ => None,
        }
    }
}

/// A rectangle on a page in dmm, origin at the page's top-left corner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    /// Left edge.
    pub x: i32,
    /// Top edge.
    pub y: i32,
    /// Width.
    pub w: i32,
    /// Height.
    pub h: i32,
}

impl Rect {
    /// Whether the point lies inside (left and top edges inclusive, right and bottom
    /// exclusive).
    pub fn contains(&self, px: i32, py: i32) -> bool {
        let (x, y) = (i64::from(px), i64::from(py));
        x >= i64::from(self.x)
            && y >= i64::from(self.y)
            && x < i64::from(self.x) + i64::from(self.w)
            && y < i64::from(self.y) + i64::from(self.h)
    }
}

/// One block on a page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// 1-based id; also the keyboard (Tab) order and the paint order.
    pub id: u32,
    /// 1-based page number.
    pub page: u32,
    /// Kind.
    pub kind: Kind,
    /// Position and size on the page.
    pub rect: Rect,
    /// Name, from the file: shown only escaped (CONSTITUTION C6).
    pub name: UntrustedText,
    /// Text (bands and text blocks), `<rows>x<columns>` (tables) or `-` (3D frame).
    pub content: String,
}

impl Block {
    /// Rows and columns of a table block.
    pub fn table_size(&self) -> Option<(u32, u32)> {
        if self.kind != Kind::Table {
            return None;
        }
        table_size(&self.content)
    }
}

fn table_size(s: &str) -> Option<(u32, u32)> {
    let (r, c) = s.split_once('x')?;
    let side = |t: &str| -> Option<u32> {
        let v: u32 = t.parse().ok()?;
        ((1..=MAX_TABLE_SIDE).contains(&v) && v.to_string() == t).then_some(v)
    };
    Some((side(r)?, side(c)?))
}

/// Whether the text holds no character [`UntrustedText::as_display`] would escape (control,
/// bidirectional, zero-width and line-separator characters), so it can be shown verbatim.
pub fn plain_text(s: &str) -> bool {
    s.chars().all(|c| {
        let u = c as u32;
        !(c.is_control()
            || (0x200b..=0x200f).contains(&u)
            || (0x2028..=0x202e).contains(&u)
            || (0x2060..=0x206f).contains(&u)
            || u == 0xfeff)
    })
}

/// Why a canvas file was rejected (1-based line numbers).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanvasError {
    /// Larger than [`MAX_CANVAS_BYTES`], [`MAX_PAGES`] or [`MAX_BLOCKS`].
    TooLarge,
    /// No page.
    NoPages,
    /// A line is malformed or breaks a rule.
    Line(usize),
}

impl fmt::Display for CanvasError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CanvasError::TooLarge => write!(f, "canvas file too large"),
            CanvasError::NoPages => write!(f, "canvas file has no page"),
            CanvasError::Line(l) => write!(f, "canvas file line {l}: malformed"),
        }
    }
}

impl std::error::Error for CanvasError {}

/// The accessibility facts of one block (REQ-VIZ.SHELL.SPKRPT-005): role and the label a
/// screen reader announces (type, name and position).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct A11y {
    /// Role ("group", "paragraph", "table" or "image").
    pub role: &'static str,
    /// `<type> <name>, page <p> of <n>, x <x> mm, y <y> mm, <w> by <h> mm`.
    pub label: String,
}

/// A length in dmm as millimetres with one decimal (`-0.5` for -5).
pub fn mm(v: i32) -> String {
    let a = i64::from(v).abs();
    let sign = if v < 0 { "-" } else { "" };
    format!("{sign}{}.{}", a / 10, a % 10)
}

/// Pages and blocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Canvas {
    pages: Vec<Paper>,
    blocks: Vec<Block>,
}

fn int(s: &str) -> Option<i32> {
    let v: i32 = s.parse().ok()?;
    (v.to_string() == s).then_some(v)
}

impl Canvas {
    /// Parses `canvas.tsv`: `#` comments, then `page<TAB>n<TAB>paper` lines (n = 1, 2, ...)
    /// before `block<TAB>id<TAB>page<TAB>kind<TAB>x<TAB>y<TAB>w<TAB>h<TAB>name<TAB>content`
    /// lines (id = 1, 2, ...). Every block lies inside its page with sides of at least
    /// [`MIN_SIDE`]; names and texts are plain (see [`plain_text`]).
    pub fn parse(text: &str) -> Result<Canvas, CanvasError> {
        if text.len() > MAX_CANVAS_BYTES {
            return Err(CanvasError::TooLarge);
        }
        let mut pages = Vec::new();
        let mut blocks: Vec<Block> = Vec::new();
        let body = text.strip_suffix('\n').unwrap_or(text);
        for (k, line) in body.split('\n').enumerate() {
            let ln = k + 1;
            let bad = CanvasError::Line(ln);
            if line.starts_with('#') {
                continue;
            }
            let f: Vec<&str> = line.split('\t').collect();
            match f.as_slice() {
                ["page", n, paper] => {
                    if !blocks.is_empty() || *n != (pages.len() + 1).to_string() {
                        return Err(bad);
                    }
                    if pages.len() >= MAX_PAGES {
                        return Err(CanvasError::TooLarge);
                    }
                    pages.push(Paper::parse(paper).ok_or(bad)?);
                }
                ["block", id, page, kind, x, y, w, h, name, content] => {
                    if blocks.len() >= MAX_BLOCKS {
                        return Err(CanvasError::TooLarge);
                    }
                    if *id != (blocks.len() + 1).to_string() {
                        return Err(bad);
                    }
                    let page_no = int(page).filter(|p| *p >= 1).ok_or(bad.clone())?;
                    let paper = pages.get((page_no - 1) as usize).copied().ok_or(bad.clone())?;
                    let kind = Kind::parse(kind).ok_or(bad.clone())?;
                    let rect = Rect {
                        x: int(x).ok_or(bad.clone())?,
                        y: int(y).ok_or(bad.clone())?,
                        w: int(w).ok_or(bad.clone())?,
                        h: int(h).ok_or(bad.clone())?,
                    };
                    if !fits(rect, paper.size()) {
                        return Err(bad);
                    }
                    if name.is_empty() || name.len() > MAX_NAME_BYTES || !plain_text(name) {
                        return Err(bad);
                    }
                    let content_ok = match kind {
                        Kind::Band | Kind::Text => content.chars().count() <= MAX_TEXT_CHARS && plain_text(content),
                        Kind::Table => table_size(content).is_some(),
                        Kind::Frame3d => *content == "-",
                    };
                    if !content_ok {
                        return Err(bad);
                    }
                    blocks.push(Block {
                        id: blocks.len() as u32 + 1,
                        page: page_no as u32,
                        kind,
                        rect,
                        name: UntrustedText::new(name),
                        content: (*content).to_string(),
                    });
                }
                _ => return Err(bad),
            }
        }
        if pages.is_empty() {
            return Err(CanvasError::NoPages);
        }
        Ok(Canvas { pages, blocks })
    }

    /// Paper of every page, page 1 first.
    pub fn pages(&self) -> &[Paper] {
        &self.pages
    }

    /// Every block in id order.
    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    /// The block with this id.
    pub fn get(&self, id: u32) -> Option<&Block> {
        self.blocks.get((id as usize).checked_sub(1)?)
    }

    pub(crate) fn get_mut(&mut self, id: u32) -> Option<&mut Block> {
        self.blocks.get_mut((id as usize).checked_sub(1)?)
    }

    /// Width and height of a page in dmm.
    pub fn page_size(&self, page: u32) -> Option<(i32, i32)> {
        let i = (page as usize).checked_sub(1)?;
        self.pages.get(i).map(|p| p.size())
    }

    /// The blocks on a page, in id order.
    pub fn on_page(&self, page: u32) -> impl Iterator<Item = &Block> {
        self.blocks.iter().filter(move |b| b.page == page)
    }

    /// Number of blocks of a kind.
    pub fn count(&self, kind: Kind) -> usize {
        self.blocks.iter().filter(|b| b.kind == kind).count()
    }

    /// CPU hit test: the topmost block (highest id, painted last) on `page` containing the
    /// point.
    pub fn hit(&self, page: u32, x: i32, y: i32) -> Option<u32> {
        self.blocks.iter().rev().find(|b| b.page == page && b.rect.contains(x, y)).map(|b| b.id)
    }

    /// Sets every page to `paper`, then moves (and if needed shrinks) each block back
    /// inside its page. Returns how many blocks changed. Not undoable.
    pub fn set_paper(&mut self, paper: Paper) -> usize {
        let (pw, ph) = paper.size();
        for p in &mut self.pages {
            *p = paper;
        }
        let mut changed = 0;
        for b in &mut self.blocks {
            let r = b.rect;
            let w = r.w.min(pw);
            let h = r.h.min(ph);
            let fixed = Rect { x: r.x.min(pw - w), y: r.y.min(ph - h), w, h };
            if fixed != r {
                b.rect = fixed;
                changed += 1;
            }
        }
        changed
    }

    /// What a screen reader announces for the block (REQ-VIZ.SHELL.SPKRPT-005).
    pub fn a11y(&self, id: u32) -> Option<A11y> {
        let b = self.get(id)?;
        let r = b.rect;
        let label = format!(
            "{} {}, page {} of {}, x {} mm, y {} mm, {} by {} mm",
            b.kind.word(),
            b.name.as_display(),
            b.page,
            self.pages.len(),
            mm(r.x),
            mm(r.y),
            mm(r.w),
            mm(r.h)
        );
        Some(A11y { role: b.kind.role(), label })
    }
}

/// Whether `r` lies inside a page of size `(pw, ph)` with sides of at least [`MIN_SIDE`].
pub fn fits(r: Rect, (pw, ph): (i32, i32)) -> bool {
    let (x, y, w, h) = (i64::from(r.x), i64::from(r.y), i64::from(r.w), i64::from(r.h));
    let min = i64::from(MIN_SIDE);
    x >= 0 && y >= 0 && w >= min && h >= min && x + w <= i64::from(pw) && y + h <= i64::from(ph)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    const SMALL: &str = "# c\npage\t1\tA4\npage\t2\tLetter\n\
                         block\t1\t1\tband\t150\t100\t1800\t150\tHeader 1\tTitle\n\
                         block\t2\t1\ttable\t100\t100\t500\t300\tTable 1\t3x2\n\
                         block\t3\t2\tframe3d\t0\t2644\t2159\t150\tViewport 1\t-\n";

    #[test]
    fn parses_and_hits_topmost() {
        let c = Canvas::parse(SMALL).unwrap();
        assert_eq!(c.pages(), [Paper::A4, Paper::Letter]);
        assert_eq!(c.blocks().len(), 3);
        assert_eq!(c.get(2).unwrap().table_size(), Some((3, 2)));
        assert_eq!(c.get(1).unwrap().table_size(), None);
        // (200, 200) lies in both 1 and 2; 2 is painted last.
        assert_eq!(c.hit(1, 200, 200), Some(2));
        assert_eq!(c.hit(1, 1000, 200), Some(1));
        // Right and bottom edges are exclusive.
        assert_eq!(c.hit(1, 1950, 200), None);
        // Block 2 spans x 100..600, y 100..400; block 1 ends at y 250.
        assert_eq!(c.hit(1, 600, 399), None);
        assert_eq!(c.hit(1, 599, 399), Some(2));
        assert_eq!(c.hit(1, 599, 400), None);
        assert_eq!(c.hit(2, 5, 2700), Some(3));
        assert_eq!(c.hit(3, 5, 2700), None);
        assert_eq!(c.get(0), None);
        assert_eq!(c.page_size(2), Some((2159, 2794)));
        assert_eq!(c.page_size(0), None);
    }

    #[test]
    fn rejects_bad_lines() {
        let head = "page\t1\tA4\n";
        for (bad, line) in [
            ("page\t2\tA4\n", 1),
            ("page\t1\tA3\n", 1),
            ("page\t1\tA4\npage\t1\tA4\n", 2),
            ("page\t1\tA4\nblock\t2\t1\ttext\t0\t0\t50\t50\tn\tt\n", 2),
            ("page\t1\tA4\nblock\t1\t2\ttext\t0\t0\t50\t50\tn\tt\n", 2),
            ("page\t1\tA4\nblock\t1\t1\tnote\t0\t0\t50\t50\tn\tt\n", 2),
            ("page\t1\tA4\nblock\t1\t1\ttext\t-1\t0\t50\t50\tn\tt\n", 2),
            ("page\t1\tA4\nblock\t1\t1\ttext\t2051\t0\t50\t50\tn\tt\n", 2),
            ("page\t1\tA4\nblock\t1\t1\ttext\t0\t0\t49\t50\tn\tt\n", 2),
            ("page\t1\tA4\nblock\t1\t1\ttext\t0\t0\t50\t50\t\tt\n", 2),
            ("page\t1\tA4\nblock\t1\t1\ttext\t0\t0\t50\t50\tn\u{202e}\tt\n", 2),
            ("page\t1\tA4\nblock\t1\t1\ttext\t0\t0\t50\t50\tn\tt\u{200b}\n", 2),
            ("page\t1\tA4\nblock\t1\t1\ttable\t0\t0\t50\t50\tn\t0x2\n", 2),
            ("page\t1\tA4\nblock\t1\t1\ttable\t0\t0\t50\t50\tn\t51x2\n", 2),
            ("page\t1\tA4\nblock\t1\t1\ttable\t0\t0\t50\t50\tn\t02x2\n", 2),
            ("page\t1\tA4\nblock\t1\t1\tframe3d\t0\t0\t50\t50\tn\tx\n", 2),
            ("page\t1\tA4\nblock\t1\t1\ttext\t+0\t0\t50\t50\tn\tt\n", 2),
            ("page\t1\tA4\nblock\t1\t1\ttext\t0\t0\t50\t50\tn\n", 2),
            ("page\t1\tA4\nblock\t1\t1\ttext\t0\t0\t50\t50\tn\tt\npage\t2\tA4\n", 3),
            ("page\t1\tA4\n\n", 2),
        ] {
            assert_eq!(Canvas::parse(bad), Err(CanvasError::Line(line)), "{bad:?}");
        }
        assert_eq!(Canvas::parse("# only\n"), Err(CanvasError::NoPages));
        assert!(Canvas::parse(head).is_ok());
        let big = "#".repeat(MAX_CANVAS_BYTES + 1);
        assert_eq!(Canvas::parse(&big), Err(CanvasError::TooLarge));
    }

    #[test]
    fn a11y_label_hand_labelled_and_escaped() {
        let c = Canvas::parse(SMALL).unwrap();
        let a = c.a11y(3).unwrap();
        assert_eq!(a.role, "image");
        assert_eq!(a.label, "3D view Viewport 1, page 2 of 2, x 0.0 mm, y 264.4 mm, 215.9 by 15.0 mm");
        assert_eq!(c.a11y(2).unwrap().role, "table");
        assert_eq!(c.a11y(9), None);
        let mut c = c;
        c.get_mut(1).unwrap().name = UntrustedText::new("a\\b\u{202e}");
        assert!(c.a11y(1).unwrap().label.starts_with("Band a\\\\b\\u{202e}, page 1 of 2"));
        assert_eq!((mm(0), mm(5), mm(-5), mm(2159)), ("0.0".into(), "0.5".into(), "-0.5".into(), "215.9".into()));
    }

    #[test]
    fn set_paper_keeps_blocks_on_the_page() {
        let mut c = Canvas::parse(SMALL).unwrap();
        // To A4: block 3 is 2159 wide, wider than 2100: shrunk to 2100 (x 0); y fits.
        assert_eq!(c.set_paper(Paper::A4), 1);
        assert_eq!(c.get(3).unwrap().rect, Rect { x: 0, y: 2644, w: 2100, h: 150 });
        // To Letter: nothing ends below 2794 or right of 2159.
        assert_eq!(c.set_paper(Paper::Letter), 0);
        assert!(c.blocks().iter().all(|b| fits(b.rect, Paper::Letter.size())));
    }
}
