//! Section-level comparison of two WebAssembly modules (spec node KRN.MAN.REPRO,
//! REQ-KRN.MAN.REPRO-004): when two builds of the kernel artifact differ, CI reports which
//! sections differ.
//!
//! Both modules must pass the structural reader ([`crate::module::parse`]). Sections are matched
//! by label, not by position, so a section that only one build has does not shift the others:
//! a non-custom section is labelled by its kind (`type`, `import`, .., `data`), a custom
//! section by `custom:<name>`, and a label that occurs more than once is numbered in order
//! (`custom:name#2`); a custom name containing `#` is refused (`bad_section_name`) so labels
//! never collide. Each section is compared by the SHA-256 of its whole encoding (id, size
//! and payload).
//!
//! The report lists every label in the first module's order, then labels only the second has,
//! one line each:
//!
//! ```text
//! same <label> <size>
//! DIFFERS <label> <size a> <size b> sha256:<a> sha256:<b>
//! ONLY_A <label> <size>
//! ONLY_B <label> <size>
//! ```
//!
//! after a first line `identical sha256:<hex>` or `differ sha256:<a> sha256:<b>`, and, when they
//! differ, a last line `differing sections: <label>,..` (or `differing sections: none` when only
//! the section order differs).

use crate::module::{self, WasmError, WasmErrorKind};
use fid_kci::kci::sha256_digest;

/// One section: its label and its whole encoding's byte range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub label: String,
    pub range: (usize, usize),
}

fn kind(id: u8) -> &'static str {
    match id {
        1 => "type",
        2 => "import",
        3 => "function",
        4 => "table",
        5 => "memory",
        6 => "global",
        7 => "export",
        8 => "start",
        9 => "element",
        10 => "code",
        11 => "data",
        12 => "datacount",
        13 => "tag",
        _ => "custom",
    }
}

fn uleb(bytes: &[u8], pos: &mut usize) -> Result<usize, WasmError> {
    let start = *pos;
    let mut value: usize = 0;
    for i in 0..5u32 {
        let b = *bytes.get(*pos).ok_or(WasmError { kind: WasmErrorKind::Truncated, offset: *pos })?;
        *pos += 1;
        value |= usize::from(b & 0x7f) << (7 * i);
        if b & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(WasmError { kind: WasmErrorKind::BadLeb, offset: start })
}

/// The sections of a module that [`module::parse`] accepts, labelled (see the module docs).
pub fn sections(bytes: &[u8]) -> Result<Vec<Section>, WasmError> {
    module::parse(bytes)?;
    let mut out: Vec<Section> = Vec::new();
    // Exact occurrence counter per base label; names never contain `#`, so `base#n` is unique.
    let mut counts: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut pos = 8;
    while pos < bytes.len() {
        let at = pos;
        let id = *bytes.get(pos).ok_or(WasmError { kind: WasmErrorKind::Truncated, offset: pos })?;
        pos += 1;
        let size = uleb(bytes, &mut pos)?;
        let end = pos + size;
        let mut base = kind(id).to_string();
        if id == 0 {
            let mut p = pos;
            let len = uleb(bytes, &mut p)?;
            let name = bytes.get(p..p + len).ok_or(WasmError { kind: WasmErrorKind::Truncated, offset: p })?;
            if name.contains(&b'#') {
                return Err(WasmError { kind: WasmErrorKind::BadSectionName, offset: p });
            }
            base = format!("custom:{}", String::from_utf8_lossy(name));
        }
        let seen = counts.entry(base.clone()).or_insert(0usize);
        *seen += 1;
        let label = if *seen == 1 { base } else { format!("{base}#{seen}") };
        out.push(Section { label, range: (at, end) });
        pos = end;
    }
    Ok(out)
}

/// Compares two modules section by section. Returns the report lines (see the module docs) and
/// whether the modules are byte-identical.
pub fn diff(a: &[u8], b: &[u8]) -> Result<(Vec<String>, bool), WasmError> {
    let sa = sections(a)?;
    let sb = sections(b)?;
    let (da, db) = (sha256_digest(a), sha256_digest(b));
    if a == b {
        let mut lines = vec![format!("identical {da}")];
        lines.extend(sa.iter().map(|s| format!("same {} {}", s.label, s.range.1 - s.range.0)));
        return Ok((lines, true));
    }
    let mut lines = vec![format!("differ {da} {db}")];
    let mut differing = Vec::new();
    let slice = |m: &[u8], s: &Section| m.get(s.range.0..s.range.1).map(<[u8]>::to_vec).unwrap_or_default();
    for x in &sa {
        let size_a = x.range.1 - x.range.0;
        match sb.iter().find(|y| y.label == x.label) {
            Some(y) => {
                let (ba, bb) = (slice(a, x), slice(b, y));
                if ba == bb {
                    lines.push(format!("same {} {size_a}", x.label));
                } else {
                    lines.push(format!(
                        "DIFFERS {} {size_a} {} {} {}",
                        x.label,
                        bb.len(),
                        sha256_digest(&ba),
                        sha256_digest(&bb)
                    ));
                    differing.push(x.label.clone());
                }
            }
            None => {
                lines.push(format!("ONLY_A {} {size_a}", x.label));
                differing.push(x.label.clone());
            }
        }
    }
    for y in sb.iter().filter(|y| !sa.iter().any(|x| x.label == y.label)) {
        lines.push(format!("ONLY_B {} {}", y.label, y.range.1 - y.range.0));
        differing.push(y.label.clone());
    }
    let list = if differing.is_empty() { "none".to_string() } else { differing.join(",") };
    lines.push(format!("differing sections: {list}"));
    Ok((lines, false))
}
