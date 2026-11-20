//! No SIMD in the kernel artifact (spec node KRN.MAN.REPRO, the FMA guard): [`simd_opcodes`]
//! decodes every function body of the code section operator by operator and reports every
//! `0xFD`-prefixed (fixed-width or relaxed SIMD) instruction. The reproducible build disables
//! SIMD and relaxed SIMD, so any hit means a flag, a toolchain or a dependency brought it back;
//! relaxed multiply-add is the FMA this guards against, and plain SIMD is refused with it so the
//! guard is an allowlist (scalar instructions only) rather than a denylist.
//!
//! The decoder is the one of `KRN.NUM.LINT.FLAGS` (`fid-knp1-lint`, `flags/wasm.rs`), copied
//! here because both crates are in the `tool` layer and may not depend on each other. Because it
//! decodes, an `0xFD` byte inside an immediate is never mistaken for an instruction. It knows the
//! core instruction set and the sign-extension, non-trapping float-to-int, bulk-memory,
//! reference-types, multi-value, multi-memory, tail-call, fixed-width SIMD and relaxed-SIMD
//! proposals; anything else, an unknown opcode, or a malformed or truncated body is an error
//! (fail closed). Run it on a module [`crate::module::parse`] accepts.

use std::fmt;

/// Most function bodies in one module.
pub const MAX_FUNCTIONS: u32 = 1_000_000;
/// Deepest block nesting accepted in one function body.
pub const MAX_BLOCK_DEPTH: u32 = 100_000;

/// Sub-opcodes inside `0x00 ..= 0xFF` that the fixed-width SIMD proposal leaves unassigned.
const SIMD_UNASSIGNED: [u32; 20] = [
    0x9a, 0xa2, 0xa5, 0xa6, 0xaf, 0xb0, 0xb2, 0xb3, 0xb4, 0xbb, 0xc2, 0xc5, 0xc6, 0xcf, 0xd0, 0xd2, 0xd3, 0xd4, 0xe2,
    0xee,
];
/// Relaxed-SIMD sub-opcodes.
const RELAXED: std::ops::RangeInclusive<u32> = 0x100..=0x113;

/// One SIMD instruction: its byte offset in the module, the index of its function body in the
/// code section (from 0, imports not counted) and its sub-opcode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimdHit {
    pub offset: usize,
    pub func: u32,
    pub sub: u32,
}

impl fmt::Display for SimdHit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = if RELAXED.contains(&self.sub) { "relaxed SIMD" } else { "SIMD" };
        write!(f, "SIMD 0xfd {:#x} ({kind}) in function {} at byte {}", self.sub, self.func, self.offset)
    }
}

/// The code section cannot be decoded; `offset` is where decoding stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeError {
    pub offset: usize,
    pub message: String,
}

impl fmt::Display for CodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "code not decodable at byte {}: {}", self.offset, self.message)
    }
}

impl std::error::Error for CodeError {}

type R<T> = Result<T, CodeError>;

struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
    end: usize,
}

impl Reader<'_> {
    fn err<T>(&self, m: impl Into<String>) -> R<T> {
        Err(CodeError { offset: self.pos, message: m.into() })
    }

    fn byte(&mut self) -> R<u8> {
        if self.pos >= self.end {
            return self.err("unexpected end of data");
        }
        match self.b.get(self.pos).copied() {
            Some(v) => {
                self.pos += 1;
                Ok(v)
            }
            None => self.err("unexpected end of data"),
        }
    }

    fn skip(&mut self, n: usize) -> R<()> {
        match self.pos.checked_add(n) {
            Some(p) if p <= self.end => {
                self.pos = p;
                Ok(())
            }
            _ => self.err("unexpected end of data"),
        }
    }

    /// Unsigned LEB128 of at most `bits` bits.
    fn uleb(&mut self, bits: u32) -> R<u64> {
        let max_bytes = bits.div_ceil(7);
        let mut result: u64 = 0;
        let mut shift: u32 = 0;
        for i in 0..max_bytes {
            let b = self.byte()?;
            let low = u64::from(b & 0x7f);
            result |= low << shift;
            if b & 0x80 == 0 {
                if i + 1 == max_bytes && bits < 64 {
                    let used = bits - shift;
                    if used < 7 && (low >> used) != 0 {
                        return self.err("LEB128 value out of range");
                    }
                }
                return Ok(result);
            }
            shift += 7;
        }
        self.err("LEB128 value too long")
    }

    fn u32(&mut self) -> R<u32> {
        let v = self.uleb(32)?;
        u32::try_from(v).or_else(|_| self.err("LEB128 value out of range"))
    }

    /// Signed LEB128 of at most `bits` bits; only the sign of 33-bit block types is used.
    fn sleb(&mut self, bits: u32) -> R<i64> {
        let max_bytes = bits.div_ceil(7);
        let mut result: i64 = 0;
        let mut shift: u32 = 0;
        for _ in 0..max_bytes {
            let b = self.byte()?;
            if shift < 64 {
                result |= i64::from(b & 0x7f) << shift;
            }
            shift += 7;
            if b & 0x80 == 0 {
                if shift < 64 && b & 0x40 != 0 {
                    result |= -1i64 << shift;
                }
                return Ok(result);
            }
        }
        self.err("LEB128 value too long")
    }
}

fn is_valtype(b: u8) -> bool {
    matches!(b, 0x7f | 0x7e | 0x7d | 0x7c | 0x7b | 0x70 | 0x6f)
}

fn blocktype(r: &mut Reader<'_>) -> R<()> {
    let at = r.pos;
    match r.b.get(at).copied() {
        Some(0x40) => r.skip(1),
        Some(v) if is_valtype(v) => r.skip(1),
        Some(_) => {
            if r.sleb(33)? < 0 {
                r.pos = at;
                return r.err("unsupported block type");
            }
            Ok(())
        }
        None => r.err("unexpected end of data"),
    }
}

fn memarg(r: &mut Reader<'_>) -> R<()> {
    let align = r.u32()?;
    if align & 0x40 != 0 {
        r.u32()?; // multi-memory: explicit memory index
    }
    r.uleb(64)?;
    Ok(())
}

/// Decodes one `0xFD` instruction after its prefix; returns the sub-opcode.
fn simd(r: &mut Reader<'_>) -> R<u32> {
    let sub = r.u32()?;
    if SIMD_UNASSIGNED.contains(&sub) {
        return r.err(format!("unassigned SIMD opcode 0xfd {sub:#x}"));
    }
    match sub {
        0x00..=0x0b | 0x5c | 0x5d => memarg(r)?,
        0x0c | 0x0d => r.skip(16)?,
        0x15..=0x22 => r.skip(1)?,
        0x54..=0x5b => {
            memarg(r)?;
            r.skip(1)?;
        }
        0x0e..=0x14 | 0x23..=0x53 | 0x5e..=0xff | 0x100..=0x113 => {}
        _ => return r.err(format!("unknown SIMD opcode 0xfd {sub:#x}")),
    }
    Ok(sub)
}

/// Decodes one `0xFC` instruction after its prefix.
fn misc(r: &mut Reader<'_>) -> R<()> {
    let sub = r.u32()?;
    match sub {
        0..=7 => {}
        8 | 10 | 12 | 14 => {
            r.u32()?;
            r.u32()?;
        }
        9 | 11 | 13 | 15 | 16 | 17 => {
            r.u32()?;
        }
        _ => return r.err(format!("unknown opcode 0xfc {sub:#x}")),
    }
    Ok(())
}

fn body(r: &mut Reader<'_>, func: u32, hits: &mut Vec<SimdHit>) -> R<()> {
    let groups = r.u32()?;
    for _ in 0..groups {
        r.u32()?;
        let t = r.byte()?;
        if !is_valtype(t) {
            return r.err(format!("unsupported local type {t:#x}"));
        }
    }
    let mut depth: u32 = 1;
    while depth > 0 {
        let start = r.pos;
        let op = r.byte()?;
        match op {
            0x00 | 0x01 | 0x05 | 0x0f | 0x1a | 0x1b | 0x45..=0xc4 | 0xd1 => {}
            0x02..=0x04 => {
                blocktype(r)?;
                depth += 1;
                if depth > MAX_BLOCK_DEPTH {
                    return r.err("block nesting too deep");
                }
            }
            0x0b => depth -= 1,
            0x0c | 0x0d | 0x10 | 0x12 | 0x20..=0x26 | 0x3f | 0x40 | 0xd2 => {
                r.u32()?;
            }
            0x0e => {
                let n = r.u32()?;
                if (n as usize) > r.end.saturating_sub(r.pos) {
                    return r.err("br_table longer than the body");
                }
                for _ in 0..=n {
                    r.u32()?;
                }
            }
            0x11 | 0x13 => {
                r.u32()?;
                r.u32()?;
            }
            0x1c => {
                let n = r.u32()?;
                for _ in 0..n {
                    let t = r.byte()?;
                    if !is_valtype(t) {
                        return r.err(format!("unsupported value type {t:#x}"));
                    }
                }
            }
            0x28..=0x3e => memarg(r)?,
            0x41 => {
                r.sleb(32)?;
            }
            0x42 => {
                r.sleb(64)?;
            }
            0x43 => r.skip(4)?,
            0x44 => r.skip(8)?,
            0xd0 => {
                r.sleb(33)?;
            }
            0xfc => misc(r)?,
            0xfd => {
                let sub = simd(r)?;
                hits.push(SimdHit { offset: start, func, sub });
            }
            _ => {
                r.pos = start;
                return r.err(format!("unknown or unsupported opcode {op:#04x}"));
            }
        }
    }
    if r.pos != r.end {
        return r.err("function body continues after its final `end`");
    }
    Ok(())
}

/// Every SIMD instruction in the module's code section, in byte order (empty when there is
/// none). Fails when the module or a body cannot be decoded.
pub fn simd_opcodes(bytes: &[u8]) -> Result<Vec<SimdHit>, CodeError> {
    crate::module::parse(bytes).map_err(|e| CodeError { offset: e.offset, message: e.to_string() })?;
    let mut r = Reader { b: bytes, pos: 8, end: bytes.len() };
    let mut hits = Vec::new();
    while r.pos < r.end {
        let id = r.byte()?;
        let size = r.u32()? as usize;
        let start = r.pos;
        let Some(end) = start.checked_add(size).filter(|e| *e <= r.end) else {
            return r.err(format!("section {id} runs past the end of the module"));
        };
        if id == 10 {
            let mut s = Reader { b: bytes, pos: start, end };
            let count = s.u32()?;
            if count > MAX_FUNCTIONS {
                return s.err("too many function bodies");
            }
            for func in 0..count {
                let len = s.u32()? as usize;
                let bstart = s.pos;
                let Some(bend) = bstart.checked_add(len).filter(|e| *e <= end) else {
                    return s.err(format!("function body {func} runs past the code section"));
                };
                let mut br = Reader { b: bytes, pos: bstart, end: bend };
                body(&mut br, func, &mut hits)?;
                s.pos = bend;
            }
            if s.pos != end {
                return s.err("code section has trailing bytes");
            }
        }
        r.pos = end;
    }
    Ok(hits)
}
