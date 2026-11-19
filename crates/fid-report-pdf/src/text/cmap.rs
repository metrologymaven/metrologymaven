//! ToUnicode CMaps (PDF 32000-1 §9.10.3): map subset glyph IDs back to the characters they
//! show, so extracting text returns the shaped string (REQ-RPT.OUTPUT.PDF.TEXT-006).

/// A ToUnicode CMap: entries (subset glyph ID, character) sorted by glyph ID, and the CMap
/// program bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CMap {
    entries: Vec<(u16, char)>,
    bytes: Vec<u8>,
}

/// Most `bfchar` entries per block (the PostScript CMap limit).
const BLOCK: usize = 100;

impl CMap {
    /// Builds the CMap. `entries` must be sorted by glyph ID with no duplicates (the subset
    /// builder guarantees it). The program text is fixed byte for byte:
    ///
    /// ```text
    /// /CIDInit /ProcSet findresource begin
    /// 12 dict begin
    /// begincmap
    /// /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def
    /// /CMapName /Adobe-Identity-UCS def
    /// /CMapType 2 def
    /// 1 begincodespacerange
    /// <0000> <FFFF>
    /// endcodespacerange
    /// <n> beginbfchar            (blocks of at most 100 entries)
    /// <GGGG> <UUUU>              (glyph ID; UTF-16BE, upper-case hex)
    /// endbfchar
    /// endcmap
    /// CMapName currentdict /CMap defineresource pop
    /// end
    /// end
    /// ```
    pub(crate) fn build(entries: Vec<(u16, char)>) -> CMap {
        let mut s = String::new();
        s.push_str("/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n");
        s.push_str("/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n");
        s.push_str("/CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n");
        s.push_str("1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n");
        for block in entries.chunks(BLOCK) {
            s.push_str(&format!("{} beginbfchar\n", block.len()));
            for &(g, c) in block {
                s.push_str(&format!("<{g:04X}> <"));
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    s.push_str(&format!("{unit:04X}"));
                }
                s.push_str(">\n");
            }
            s.push_str("endbfchar\n");
        }
        s.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
        CMap { entries, bytes: s.into_bytes() }
    }

    /// The (subset glyph ID, character) pairs, sorted by glyph ID.
    pub fn entries(&self) -> &[(u16, char)] {
        &self.entries
    }

    /// The CMap program bytes (the content of the PDF `/ToUnicode` stream).
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}
