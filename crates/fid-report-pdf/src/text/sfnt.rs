//! Bounded TrueType (glyf outline) parser. Font files are hostile input (CONSTITUTION §8):
//! every read is bounds-checked, every count is bounded by the bytes that hold it, and
//! nothing is executed (hinting programs are only copied).

/// Reads a big-endian u16.
pub(crate) fn rd16(d: &[u8], o: usize) -> Option<u16> {
    let b = d.get(o..o.checked_add(2)?)?;
    Some(u16::from_be_bytes([b[0], b[1]]))
}

/// Reads a big-endian i16.
pub(crate) fn rdi16(d: &[u8], o: usize) -> Option<i16> {
    rd16(d, o).map(|v| v as i16)
}

/// Reads a big-endian u32.
pub(crate) fn rd32(d: &[u8], o: usize) -> Option<u32> {
    let b = d.get(o..o.checked_add(4)?)?;
    Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// A table location in the font file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Span {
    pub off: usize,
    pub len: usize,
}

impl Span {
    pub(crate) fn of<'a>(&self, d: &'a [u8]) -> &'a [u8] {
        d.get(self.off..self.off.saturating_add(self.len)).unwrap_or(&[])
    }
}

/// The character map used for lookups.
#[derive(Debug, Clone)]
pub(crate) enum Cmap {
    /// Format 4: subtable location and segment count (end codes strictly increasing).
    F4 { sub: Span, seg_count: usize },
    /// Format 12: groups (start, end, start glyph), sorted and non-overlapping.
    F12 { groups: Vec<(u32, u32, u32)> },
}

/// What the text module needs from a font.
#[derive(Debug, Clone)]
pub(crate) struct Parsed {
    pub upem: u16,
    pub num_glyphs: u16,
    /// (advance width, left side bearing) per glyph, in font units.
    pub metrics: Vec<(u16, i16)>,
    /// Glyph offsets into `glyf`, `num_glyphs + 1` entries, non-decreasing.
    pub loca: Vec<u32>,
    pub glyf: Span,
    pub head: Span,
    pub hhea: Span,
    pub maxp: Span,
    /// `cvt `, `fpgm`, `prep` when present: copied into subsets unchanged.
    pub hinting: Vec<([u8; 4], Span)>,
    pub cmap: Cmap,
    /// Font bounding box (xMin, yMin, xMax, yMax) from `head`.
    pub bbox: [i16; 4],
    pub ascender: i16,
    pub descender: i16,
}

const MAX_TABLES: usize = 256;

type R<T> = Result<T, &'static str>;

fn need<T>(v: Option<T>, why: &'static str) -> R<T> {
    v.ok_or(why)
}

/// Parses and validates a TrueType font.
pub(crate) fn parse(d: &[u8]) -> R<Parsed> {
    let version = need(rd32(d, 0), "truncated")?;
    match version {
        0x0001_0000 | 0x7472_7565 => {}
        0x4F54_544F => return Err("cff_outlines_unsupported"),
        _ => return Err("not_truetype"),
    }
    let n = usize::from(need(rd16(d, 4), "truncated")?);
    if n == 0 || n > MAX_TABLES {
        return Err("table_count");
    }
    let mut tables: Vec<([u8; 4], Span)> = Vec::with_capacity(n);
    for i in 0..n {
        let rec = 12 + 16 * i;
        let tag_b = need(d.get(rec..rec + 4), "truncated")?;
        let tag = [tag_b[0], tag_b[1], tag_b[2], tag_b[3]];
        let off = need(rd32(d, rec + 8), "truncated")? as usize;
        let len = need(rd32(d, rec + 12), "truncated")? as usize;
        let end = need(off.checked_add(len), "table_bounds")?;
        if end > d.len() {
            return Err("table_bounds");
        }
        if tables.iter().any(|(t, _)| *t == tag) {
            return Err("duplicate_table");
        }
        tables.push((tag, Span { off, len }));
    }
    let find = |t: &[u8; 4]| tables.iter().find(|(x, _)| x == t).map(|(_, s)| *s);
    let head = need(find(b"head"), "missing_head")?;
    let hhea = need(find(b"hhea"), "missing_hhea")?;
    let maxp = need(find(b"maxp"), "missing_maxp")?;
    let hmtx = need(find(b"hmtx"), "missing_hmtx")?;
    let loca_s = need(find(b"loca"), "missing_loca")?;
    let glyf = need(find(b"glyf"), "missing_glyf")?;
    let cmap_s = need(find(b"cmap"), "missing_cmap")?;

    let hd = head.of(d);
    if hd.len() < 54 || rd32(hd, 12) != Some(0x5F0F_3CF5) {
        return Err("bad_head");
    }
    let upem = need(rd16(hd, 18), "bad_head")?;
    if !(16..=16384).contains(&upem) {
        return Err("bad_units_per_em");
    }
    let bbox = [
        need(rdi16(hd, 36), "bad_head")?,
        need(rdi16(hd, 38), "bad_head")?,
        need(rdi16(hd, 40), "bad_head")?,
        need(rdi16(hd, 42), "bad_head")?,
    ];
    let loca_long = match need(rdi16(hd, 50), "bad_head")? {
        0 => false,
        1 => true,
        _ => return Err("bad_loca_format"),
    };

    let mp = maxp.of(d);
    if mp.len() < 6 {
        return Err("bad_maxp");
    }
    let num_glyphs = need(rd16(mp, 4), "bad_maxp")?;
    if num_glyphs == 0 {
        return Err("no_glyphs");
    }
    let ng = usize::from(num_glyphs);

    let hh = hhea.of(d);
    if hh.len() < 36 {
        return Err("bad_hhea");
    }
    let ascender = need(rdi16(hh, 4), "bad_hhea")?;
    let descender = need(rdi16(hh, 6), "bad_hhea")?;
    let nhm = usize::from(need(rd16(hh, 34), "bad_hhea")?);
    if nhm == 0 || nhm > ng {
        return Err("bad_hmetric_count");
    }
    let hm = hmtx.of(d);
    if hm.len() < 4 * nhm + 2 * (ng - nhm) {
        return Err("bad_hmtx");
    }
    let mut metrics = Vec::with_capacity(ng);
    let mut last_adv = 0u16;
    for g in 0..ng {
        if g < nhm {
            last_adv = need(rd16(hm, 4 * g), "bad_hmtx")?;
            metrics.push((last_adv, need(rdi16(hm, 4 * g + 2), "bad_hmtx")?));
        } else {
            metrics.push((last_adv, need(rdi16(hm, 4 * nhm + 2 * (g - nhm)), "bad_hmtx")?));
        }
    }

    let lo = loca_s.of(d);
    let entry = if loca_long { 4 } else { 2 };
    if lo.len() < entry * (ng + 1) {
        return Err("bad_loca");
    }
    let mut loca = Vec::with_capacity(ng + 1);
    for g in 0..=ng {
        let v = if loca_long {
            need(rd32(lo, 4 * g), "bad_loca")?
        } else {
            u32::from(need(rd16(lo, 2 * g), "bad_loca")?) * 2
        };
        if let Some(&prev) = loca.last() {
            if v < prev {
                return Err("bad_loca");
            }
        }
        if v as usize > glyf.len {
            return Err("bad_loca");
        }
        loca.push(v);
    }

    let cmap = parse_cmap(d, cmap_s)?;
    let mut hinting = Vec::new();
    for t in [b"cvt ", b"fpgm", b"prep"] {
        if let Some(s) = find(t) {
            hinting.push((*t, s));
        }
    }
    Ok(Parsed { upem, num_glyphs, metrics, loca, glyf, head, hhea, maxp, hinting, cmap, bbox, ascender, descender })
}

fn parse_cmap(d: &[u8], s: Span) -> R<Cmap> {
    let c = s.of(d);
    let n = usize::from(need(rd16(c, 2), "bad_cmap")?);
    // (rank, subtable offset within the cmap table); lower rank is preferred.
    let mut best: Option<(u8, usize)> = None;
    for i in 0..n {
        let rec = 4 + 8 * i;
        let platform = need(rd16(c, rec), "bad_cmap")?;
        let encoding = need(rd16(c, rec + 2), "bad_cmap")?;
        let off = need(rd32(c, rec + 4), "bad_cmap")? as usize;
        let format = need(rd16(c, off), "bad_cmap")?;
        let rank = match (platform, encoding, format) {
            (3, 10, 12) => 0,
            (0, 4, 12) | (0, 6, 12) => 1,
            (3, 1, 4) => 2,
            (0, 3, 4) => 3,
            (0, e, 4) if e <= 2 => 4,
            _ => continue,
        };
        if best.map_or(true, |(r, _)| rank < r) {
            best = Some((rank, off));
        }
    }
    let (rank, off) = need(best, "no_unicode_cmap")?;
    if rank <= 1 {
        let len = need(rd32(c, off.saturating_add(4)), "bad_cmap12")? as usize;
        let sub = need(c.get(off..need(off.checked_add(len), "bad_cmap12")?), "bad_cmap12")?;
        let count = need(rd32(sub, 12), "bad_cmap12")? as usize;
        if count > (sub.len().saturating_sub(16)) / 12 {
            return Err("bad_cmap12");
        }
        let mut groups = Vec::with_capacity(count);
        let mut prev_end: Option<u32> = None;
        for g in 0..count {
            let b = 16 + 12 * g;
            let (start, end, gid) = (
                need(rd32(sub, b), "bad_cmap12")?,
                need(rd32(sub, b + 4), "bad_cmap12")?,
                need(rd32(sub, b + 8), "bad_cmap12")?,
            );
            if start > end || end > 0x10_FFFF || prev_end.is_some_and(|p| start <= p) {
                return Err("bad_cmap12");
            }
            prev_end = Some(end);
            groups.push((start, end, gid));
        }
        Ok(Cmap::F12 { groups })
    } else {
        let len = usize::from(need(rd16(c, off.saturating_add(2)), "bad_cmap4")?);
        // Some fonts store a length past the table end; clamp to the bytes present.
        let avail = c.len().saturating_sub(off);
        let len = len.min(avail);
        let sub = need(c.get(off..off + len), "bad_cmap4")?;
        let seg_x2 = usize::from(need(rd16(sub, 6), "bad_cmap4")?);
        if seg_x2 == 0 || seg_x2 % 2 != 0 || 16 + 4 * seg_x2 > sub.len() {
            return Err("bad_cmap4");
        }
        let seg_count = seg_x2 / 2;
        let mut prev: Option<u16> = None;
        for i in 0..seg_count {
            let end = need(rd16(sub, 14 + 2 * i), "bad_cmap4")?;
            let start = need(rd16(sub, 16 + seg_x2 + 2 * i), "bad_cmap4")?;
            if start > end || prev.is_some_and(|p| end <= p) {
                return Err("bad_cmap4");
            }
            prev = Some(end);
        }
        Ok(Cmap::F4 { sub: Span { off: s.off + off, len }, seg_count })
    }
}

impl Parsed {
    /// The glyph for a code point, or 0 when the font has none.
    pub(crate) fn glyph_for(&self, d: &[u8], cp: u32) -> u16 {
        let g = match &self.cmap {
            Cmap::F12 { groups } => {
                let i = groups.partition_point(|&(_, end, _)| end < cp);
                match groups.get(i) {
                    Some(&(start, _, gid)) if start <= cp => gid.checked_add(cp - start).unwrap_or(0),
                    _ => 0,
                }
            }
            Cmap::F4 { sub, seg_count } => lookup4(sub.of(d), *seg_count, cp).map_or(0, u32::from),
        };
        if g < u32::from(self.num_glyphs) {
            g as u16
        } else {
            0
        }
    }

    /// The outline bytes of a glyph (empty for a glyph without outline).
    pub(crate) fn glyph_data<'a>(&self, d: &'a [u8], gid: u16) -> &'a [u8] {
        let g = usize::from(gid);
        match (self.loca.get(g), self.loca.get(g + 1)) {
            (Some(&a), Some(&b)) => self.glyf.of(d).get(a as usize..b as usize).unwrap_or(&[]),
            _ => &[],
        }
    }
}

fn lookup4(sub: &[u8], seg_count: usize, cp: u32) -> Option<u16> {
    if cp > 0xFFFF {
        return None;
    }
    let c = cp as u16;
    let ends = 14;
    let starts = 16 + 2 * seg_count;
    let deltas = starts + 2 * seg_count;
    let ranges = deltas + 2 * seg_count;
    // First segment whose end code is >= c (end codes are strictly increasing).
    let (mut lo, mut hi) = (0usize, seg_count);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if rd16(sub, ends + 2 * mid)? < c {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    if lo >= seg_count {
        return None;
    }
    let i = lo;
    let start = rd16(sub, starts + 2 * i)?;
    if start > c {
        return None;
    }
    let delta = rd16(sub, deltas + 2 * i)?;
    let ro = usize::from(rd16(sub, ranges + 2 * i)?);
    if ro == 0 {
        return Some(c.wrapping_add(delta));
    }
    let addr = ranges + 2 * i + ro + 2 * usize::from(c - start);
    let g = rd16(sub, addr)?;
    if g == 0 {
        None
    } else {
        Some(g.wrapping_add(delta))
    }
}

/// Component references of a composite glyph: (byte offset of the glyph-index field within
/// the glyph data, component glyph ID). Empty for a simple or empty glyph.
pub(crate) fn components(g: &[u8]) -> R<Vec<(usize, u16)>> {
    if g.len() < 10 || rdi16(g, 0).is_some_and(|n| n >= 0) {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    let mut p = 10usize;
    loop {
        let flags = need(rd16(g, p), "bad_composite")?;
        let gid = need(rd16(g, p + 2), "bad_composite")?;
        out.push((p + 2, gid));
        p += 4;
        p += if flags & 0x0001 != 0 { 4 } else { 2 };
        if flags & 0x0008 != 0 {
            p += 2;
        } else if flags & 0x0040 != 0 {
            p += 4;
        } else if flags & 0x0080 != 0 {
            p += 8;
        }
        if p > g.len() {
            return Err("bad_composite");
        }
        if flags & 0x0020 == 0 {
            break;
        }
        if out.len() > g.len() / 6 {
            return Err("bad_composite");
        }
    }
    Ok(out)
}
