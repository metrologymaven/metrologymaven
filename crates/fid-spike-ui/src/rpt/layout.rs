//! The zoom transform: pages stacked top to bottom in one scrollable column, millimetre
//! space mapped to logical pixels, and the CPU hit test from a pointer position back to a
//! page, a position in dmm and a block.

use super::canvas::{Canvas, Rect};

/// Logical pixels per millimetre at 100 % zoom (96 dpi).
pub const PX_PER_MM: f32 = 96.0 / 25.4;
/// Padding around the page column, in logical pixels (not zoomed).
pub const PAD_PX: f32 = 16.0;
/// Gap between pages, in logical pixels (not zoomed).
pub const GAP_PX: f32 = 24.0;

/// A page's rectangle in content pixels (origin at the content's top-left corner).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PxRect {
    /// Left.
    pub x: f32,
    /// Top.
    pub y: f32,
    /// Width.
    pub w: f32,
    /// Height.
    pub h: f32,
}

/// Page positions at one zoom.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    scale: f32,
    pages: Vec<PxRect>,
}

impl Layout {
    /// The layout of `canvas` at `zoom_percent` (clamped to 1..=10 000).
    pub fn new(canvas: &Canvas, zoom_percent: f32) -> Layout {
        let zoom = if zoom_percent.is_nan() { 100.0 } else { zoom_percent.clamp(1.0, 10_000.0) };
        let scale = PX_PER_MM * zoom / 100.0 / 10.0;
        let mut y = PAD_PX;
        let mut pages = Vec::with_capacity(canvas.pages().len());
        for p in canvas.pages() {
            let (w, h) = p.size();
            let r = PxRect { x: PAD_PX, y, w: w as f32 * scale, h: h as f32 * scale };
            y += r.h + GAP_PX;
            pages.push(r);
        }
        Layout { scale, pages }
    }

    /// Logical pixels per dmm.
    pub fn scale(&self) -> f32 {
        self.scale
    }

    /// Every page's rectangle, page 1 first.
    pub fn pages(&self) -> &[PxRect] {
        &self.pages
    }

    /// Width and height of the whole content, padding included.
    pub fn content_size(&self) -> (f32, f32) {
        let w = self.pages.iter().map(|p| p.w).fold(0.0, f32::max);
        let h = self.pages.last().map_or(0.0, |p| p.y + p.h);
        (w + 2.0 * PAD_PX, h + PAD_PX)
    }

    /// A page's rectangle.
    pub fn page(&self, page: u32) -> Option<PxRect> {
        self.pages.get((page as usize).checked_sub(1)?).copied()
    }

    /// A rectangle on `page` (dmm) in content pixels.
    pub fn to_px(&self, page: u32, r: Rect) -> Option<PxRect> {
        let p = self.page(page)?;
        let s = self.scale;
        Some(PxRect { x: p.x + r.x as f32 * s, y: p.y + r.y as f32 * s, w: r.w as f32 * s, h: r.h as f32 * s })
    }

    /// Pages that intersect the vertical span `[top, bottom)` of content pixels.
    pub fn visible_pages(&self, top: f32, bottom: f32) -> Vec<u32> {
        let mut out = Vec::new();
        for (i, p) in self.pages.iter().enumerate() {
            if p.y < bottom && p.y + p.h > top {
                out.push(i as u32 + 1);
            }
        }
        out
    }

    /// The page under a content-pixel position and the position on it in dmm (rounded
    /// down). Positions in the padding and the gaps hit no page.
    pub fn locate(&self, x: f32, y: f32) -> Option<(u32, i32, i32)> {
        if !(x.is_finite() && y.is_finite()) {
            return None;
        }
        for (i, p) in self.pages.iter().enumerate() {
            if x >= p.x && x < p.x + p.w && y >= p.y && y < p.y + p.h {
                let dx = ((x - p.x) / self.scale).floor() as i32;
                let dy = ((y - p.y) / self.scale).floor() as i32;
                return Some((i as u32 + 1, dx, dy));
            }
        }
        None
    }

    /// CPU hit test from a content-pixel position to the topmost block under it.
    pub fn hit(&self, canvas: &Canvas, x: f32, y: f32) -> Option<u32> {
        let (page, dx, dy) = self.locate(x, y)?;
        canvas.hit(page, dx, dy)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    const TWO: &str = "page\t1\tA4\npage\t2\tLetter\n\
                       block\t1\t1\ttext\t100\t200\t400\t100\tText 1\tabc\n\
                       block\t2\t2\ttext\t0\t0\t2159\t2794\tText 2\tall\n";

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() <= 1e-4 * b.abs().max(1.0)
    }

    #[test]
    fn transform_hand_labelled() {
        let c = Canvas::parse(TWO).unwrap();
        // At 254 % one mm is 9.6 px, one dmm 0.96 px.
        let l = Layout::new(&c, 254.0);
        assert!(close(l.scale(), 0.96));
        let p1 = l.page(1).unwrap();
        assert!(close(p1.x, 16.0) && close(p1.y, 16.0) && close(p1.w, 2016.0) && close(p1.h, 2851.2));
        let p2 = l.page(2).unwrap();
        assert!(close(p2.y, 16.0 + 2851.2 + 24.0) && close(p2.w, 2072.64));
        let b = l.to_px(1, c.get(1).unwrap().rect).unwrap();
        assert!(close(b.x, 16.0 + 96.0) && close(b.y, 16.0 + 192.0) && close(b.w, 384.0) && close(b.h, 96.0));
        let (w, h) = l.content_size();
        assert!(close(w, 2072.64 + 32.0) && close(h, p2.y + p2.h + 16.0));
        assert_eq!(l.page(0), None);
        assert_eq!(l.page(3), None);
    }

    #[test]
    fn locate_and_hit() {
        let c = Canvas::parse(TWO).unwrap();
        let l = Layout::new(&c, 254.0);
        // 112 px = page x 96 px = 100 dmm; 208 px = 200 dmm: block 1's corner.
        assert_eq!(l.locate(112.5, 208.5), Some((1, 100, 200)));
        assert_eq!(l.hit(&c, 112.5, 208.5), Some(1));
        assert_eq!(l.hit(&c, 111.0, 208.5), None);
        // Padding and the gap between pages.
        assert_eq!(l.locate(5.0, 100.0), None);
        assert_eq!(l.locate(100.0, 16.0 + 2851.2 + 10.0), None);
        assert_eq!(l.hit(&c, 100.0, 16.0 + 2851.2 + 30.0), Some(2));
        assert_eq!(l.locate(f32::NAN, 1.0), None);
    }

    #[test]
    fn visible_pages_and_zoom_range() {
        let c = Canvas::parse(TWO).unwrap();
        let l = Layout::new(&c, 254.0);
        assert_eq!(l.visible_pages(0.0, 100.0), [1]);
        assert_eq!(l.visible_pages(2800.0, 2900.0), [1, 2]);
        assert_eq!(l.visible_pages(2868.0, 2890.0), Vec::<u32>::new());
        for z in [25.0, 400.0] {
            let l = Layout::new(&c, z);
            assert!(close(l.page(1).unwrap().w, 210.0 * PX_PER_MM * z / 100.0));
        }
        assert_eq!(Layout::new(&c, f32::NAN), Layout::new(&c, 100.0));
    }
}
