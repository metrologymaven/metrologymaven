//! A bounded, fail-closed reader for the structure and import section of a WebAssembly module
//! (binary format of the WebAssembly Core Specification 2.0, section 5).
//!
//! It checks what the import check depends on and refuses everything it cannot verify:
//!
//! - the header is `\0asm` and binary version 1 exactly (a component or any other layer is
//!   `bad_version`);
//! - every section has a known id (0 to 13), its declared size lies inside the input, and the
//!   non-custom sections appear at most once each and in the specification's order (type,
//!   import, function, table, memory, tag, global, export, start, element, data count, code,
//!   data); the input ends exactly after the last section;
//! - LEB128 integers follow the specification: at most 5 bytes for a `u32` and no bits set
//!   above bit 31 (non-minimal encodings within 5 bytes are valid and accepted);
//! - the type section holds only function types (`0x60`) over the number, vector and
//!   reference value types;
//! - every import is fully decoded (module and field names are UTF-8; descriptor kinds
//!   function, table, memory, global and tag; limits with flags 0 or 1 only, so shared and
//!   64-bit memories are refused); a function or tag import's type index exists; the import
//!   section and the type section end exactly at their declared size.
//!
//! The bodies of the other sections are not interpreted: a module reaches the host only
//! through its imports, so the import section is the whole of what this check certifies.
//! Counts and names are capped ([`MAX_IMPORTS`], [`MAX_TYPES`], [`MAX_NAME_BYTES`],
//! [`MAX_TYPE_ARITY`]) and the input is at most [`MAX_MODULE_BYTES`]; nothing is allocated in
//! proportion to a declared count before the bytes for it have been read.

use std::fmt;

/// Largest accepted module, in bytes.
pub const MAX_MODULE_BYTES: usize = 64 * 1024 * 1024;
/// Most imports one module may declare.
pub const MAX_IMPORTS: u32 = 4096;
/// Most entries in the type section.
pub const MAX_TYPES: u32 = 65_536;
/// Longest module, field or custom-section name, in bytes.
pub const MAX_NAME_BYTES: u32 = 1024;
/// Most parameters, and most results, of one function type.
pub const MAX_TYPE_ARITY: u32 = 1000;

/// The binary header: magic `\0asm` and version 1.
const MAGIC: [u8; 4] = [0x00, 0x61, 0x73, 0x6d];
const VERSION_1: [u8; 4] = [0x01, 0x00, 0x00, 0x00];

/// What an import provides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ImportKind {
    Func,
    Table,
    Memory,
    Global,
    Tag,
}

impl ImportKind {
    /// Lowercase name.
    pub fn as_str(self) -> &'static str {
        match self {
            ImportKind::Func => "func",
            ImportKind::Table => "table",
            ImportKind::Memory => "memory",
            ImportKind::Global => "global",
            ImportKind::Tag => "tag",
        }
    }
}

/// One import, in module order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    pub module: String,
    pub name: String,
    pub kind: ImportKind,
}

/// What the reader established about a module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    /// Every import, in the order the module declares them.
    pub imports: Vec<Import>,
    /// Number of entries in the type section (0 when there is none).
    pub type_count: u32,
}

/// Why a module was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmErrorKind {
    TooLarge,
    BadMagic,
    BadVersion,
    Truncated,
    BadLeb,
    SectionOverrun,
    SectionSizeMismatch,
    UnknownSection,
    SectionOrder,
    BadUtf8,
    BadImportKind,
    BadType,
    BadLimits,
    BadTypeIndex,
    LimitExceeded,
}

/// A refusal and the byte offset where it was detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WasmError {
    pub kind: WasmErrorKind,
    pub offset: usize,
}

impl WasmError {
    /// Stable code.
    pub fn code(&self) -> &'static str {
        match self.kind {
            WasmErrorKind::TooLarge => "too_large",
            WasmErrorKind::BadMagic => "bad_magic",
            WasmErrorKind::BadVersion => "bad_version",
            WasmErrorKind::Truncated => "truncated",
            WasmErrorKind::BadLeb => "bad_leb",
            WasmErrorKind::SectionOverrun => "section_overrun",
            WasmErrorKind::SectionSizeMismatch => "section_size_mismatch",
            WasmErrorKind::UnknownSection => "unknown_section",
            WasmErrorKind::SectionOrder => "section_order",
            WasmErrorKind::BadUtf8 => "bad_utf8",
            WasmErrorKind::BadImportKind => "bad_import_kind",
            WasmErrorKind::BadType => "bad_type",
            WasmErrorKind::BadLimits => "bad_limits",
            WasmErrorKind::BadTypeIndex => "bad_type_index",
            WasmErrorKind::LimitExceeded => "limit_exceeded",
        }
    }
}

impl fmt::Display for WasmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at byte {}", self.code(), self.offset)
    }
}

impl std::error::Error for WasmError {}

fn fail<T>(kind: WasmErrorKind, offset: usize) -> Result<T, WasmError> {
    Err(WasmError { kind, offset })
}

/// A cursor over `buf[pos..end]`; offsets are absolute in the module.
struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
    end: usize,
}

impl<'a> Reader<'a> {
    fn byte(&mut self) -> Result<u8, WasmError> {
        if self.pos >= self.end {
            return fail(WasmErrorKind::Truncated, self.pos);
        }
        let b =
            self.buf.get(self.pos).copied().ok_or(WasmError { kind: WasmErrorKind::Truncated, offset: self.pos })?;
        self.pos += 1;
        Ok(b)
    }

    fn bytes(&mut self, n: usize) -> Result<&'a [u8], WasmError> {
        let stop = match self.pos.checked_add(n) {
            Some(s) if s <= self.end => s,
            _ => return fail(WasmErrorKind::Truncated, self.pos),
        };
        let out = self.buf.get(self.pos..stop).ok_or(WasmError { kind: WasmErrorKind::Truncated, offset: self.pos })?;
        self.pos = stop;
        Ok(out)
    }

    /// Unsigned LEB128 `u32`: at most 5 bytes, nothing above bit 31.
    fn u32(&mut self) -> Result<u32, WasmError> {
        let start = self.pos;
        let mut value: u32 = 0;
        for i in 0..5u32 {
            let b = self.byte()?;
            if i == 4 && b & 0xf0 != 0 {
                return fail(WasmErrorKind::BadLeb, start);
            }
            value |= u32::from(b & 0x7f) << (7 * i);
            if b & 0x80 == 0 {
                return Ok(value);
            }
        }
        fail(WasmErrorKind::BadLeb, start)
    }

    /// A count capped at `max`.
    fn count(&mut self, max: u32) -> Result<u32, WasmError> {
        let at = self.pos;
        let n = self.u32()?;
        if n > max {
            return fail(WasmErrorKind::LimitExceeded, at);
        }
        Ok(n)
    }

    fn name(&mut self) -> Result<String, WasmError> {
        let len = self.count(MAX_NAME_BYTES)?;
        let at = self.pos;
        let raw = self.bytes(len as usize)?;
        match std::str::from_utf8(raw) {
            Ok(s) => Ok(s.to_string()),
            Err(_) => fail(WasmErrorKind::BadUtf8, at),
        }
    }

    fn finish(&self) -> Result<(), WasmError> {
        if self.pos != self.end {
            return fail(WasmErrorKind::SectionSizeMismatch, self.pos);
        }
        Ok(())
    }
}

/// Position of a non-custom section id in the specification's order, or `None` if unknown.
fn rank(id: u8) -> Option<u8> {
    Some(match id {
        1 => 1,   // type
        2 => 2,   // import
        3 => 3,   // function
        4 => 4,   // table
        5 => 5,   // memory
        13 => 6,  // tag
        6 => 7,   // global
        7 => 8,   // export
        8 => 9,   // start
        9 => 10,  // element
        12 => 11, // data count
        10 => 12, // code
        11 => 13, // data
        _ => return None,
    })
}

/// Number and vector value types, and the two reference types.
fn is_valtype(b: u8) -> bool {
    matches!(b, 0x7f | 0x7e | 0x7d | 0x7c | 0x7b | 0x70 | 0x6f)
}

fn valtype(r: &mut Reader<'_>) -> Result<(), WasmError> {
    let at = r.pos;
    if !is_valtype(r.byte()?) {
        return fail(WasmErrorKind::BadType, at);
    }
    Ok(())
}

fn types(r: &mut Reader<'_>) -> Result<u32, WasmError> {
    let n = r.count(MAX_TYPES)?;
    for _ in 0..n {
        let at = r.pos;
        if r.byte()? != 0x60 {
            return fail(WasmErrorKind::BadType, at);
        }
        for _ in 0..2 {
            let arity = r.count(MAX_TYPE_ARITY)?;
            for _ in 0..arity {
                valtype(r)?;
            }
        }
    }
    r.finish()?;
    Ok(n)
}

fn limits(r: &mut Reader<'_>) -> Result<(), WasmError> {
    let at = r.pos;
    match r.byte()? {
        0x00 => {
            r.u32()?;
        }
        0x01 => {
            let min = r.u32()?;
            let max = r.u32()?;
            if max < min {
                return fail(WasmErrorKind::BadLimits, at);
            }
        }
        _ => return fail(WasmErrorKind::BadLimits, at),
    }
    Ok(())
}

fn type_index(r: &mut Reader<'_>, type_count: u32) -> Result<(), WasmError> {
    let at = r.pos;
    if r.u32()? >= type_count {
        return fail(WasmErrorKind::BadTypeIndex, at);
    }
    Ok(())
}

fn imports(r: &mut Reader<'_>, type_count: u32) -> Result<Vec<Import>, WasmError> {
    let n = r.count(MAX_IMPORTS)?;
    let mut out = Vec::new();
    for _ in 0..n {
        let module = r.name()?;
        let name = r.name()?;
        let at = r.pos;
        let kind = match r.byte()? {
            0x00 => {
                type_index(r, type_count)?;
                ImportKind::Func
            }
            0x01 => {
                let t = r.pos;
                if !matches!(r.byte()?, 0x70 | 0x6f) {
                    return fail(WasmErrorKind::BadType, t);
                }
                limits(r)?;
                ImportKind::Table
            }
            0x02 => {
                limits(r)?;
                ImportKind::Memory
            }
            0x03 => {
                valtype(r)?;
                let m = r.pos;
                if r.byte()? > 1 {
                    return fail(WasmErrorKind::BadType, m);
                }
                ImportKind::Global
            }
            0x04 => {
                let a = r.pos;
                if r.byte()? != 0 {
                    return fail(WasmErrorKind::BadType, a);
                }
                type_index(r, type_count)?;
                ImportKind::Tag
            }
            _ => return fail(WasmErrorKind::BadImportKind, at),
        };
        out.push(Import { module, name, kind });
    }
    r.finish()?;
    Ok(out)
}

/// Reads a module's structure and imports. Bounded and fail-closed (see the module docs).
pub fn parse(bytes: &[u8]) -> Result<Module, WasmError> {
    if bytes.len() > MAX_MODULE_BYTES {
        return fail(WasmErrorKind::TooLarge, MAX_MODULE_BYTES);
    }
    let mut r = Reader { buf: bytes, pos: 0, end: bytes.len() };
    if r.bytes(4)? != MAGIC {
        return fail(WasmErrorKind::BadMagic, 0);
    }
    if r.bytes(4)? != VERSION_1 {
        return fail(WasmErrorKind::BadVersion, 4);
    }
    let mut last_rank = 0u8;
    let mut type_count = 0u32;
    let mut found = Vec::new();
    while r.pos < r.end {
        let id_at = r.pos;
        let id = r.byte()?;
        let size = r.u32()? as usize;
        let start = r.pos;
        let end = match start.checked_add(size) {
            Some(e) if e <= bytes.len() => e,
            _ => return fail(WasmErrorKind::SectionOverrun, id_at),
        };
        let mut s = Reader { buf: bytes, pos: start, end };
        if id == 0 {
            s.name()?;
        } else {
            let Some(k) = rank(id) else {
                return fail(WasmErrorKind::UnknownSection, id_at);
            };
            if k <= last_rank {
                return fail(WasmErrorKind::SectionOrder, id_at);
            }
            last_rank = k;
            match id {
                1 => type_count = types(&mut s)?,
                2 => found = imports(&mut s, type_count)?,
                _ => {}
            }
        }
        r.pos = end;
    }
    Ok(Module { imports: found, type_count })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn leb(bytes: &[u8]) -> Result<u32, WasmError> {
        Reader { buf: bytes, pos: 0, end: bytes.len() }.u32()
    }

    #[test]
    fn leb128_bounds() {
        assert_eq!(leb(&[0x00]).unwrap(), 0);
        assert_eq!(leb(&[0x7f]).unwrap(), 127);
        assert_eq!(leb(&[0x80, 0x01]).unwrap(), 128);
        assert_eq!(leb(&[0xff, 0xff, 0xff, 0xff, 0x0f]).unwrap(), u32::MAX);
        assert_eq!(leb(&[0x80, 0x80, 0x80, 0x80, 0x00]).unwrap(), 0);
        assert_eq!(leb(&[0xff, 0xff, 0xff, 0xff, 0x1f]).unwrap_err().kind, WasmErrorKind::BadLeb);
        assert_eq!(leb(&[0x80, 0x80, 0x80, 0x80, 0x80]).unwrap_err().kind, WasmErrorKind::BadLeb);
        assert_eq!(leb(&[0x80, 0x80]).unwrap_err().kind, WasmErrorKind::Truncated);
    }

    #[test]
    fn ranks_are_a_strict_order_over_known_ids() {
        let mut r: Vec<u8> = (1..=13).filter_map(rank).collect();
        r.sort_unstable();
        assert_eq!(r, (1..=13).collect::<Vec<u8>>());
        assert_eq!(rank(0), None);
        assert_eq!(rank(14), None);
    }
}
