//! Bounded, streaming scan of a WebAssembly binary module for fused or relaxed multiply-add
//! opcodes (`KNP1-FLAG-WASMFMA`) and SIMD opcodes the lint config does not allow
//! (`KNP1-FLAG-WASMSIMD`). Part of spec node `KRN.NUM.LINT.FLAGS` (REQ-005).
//!
//! Every function body of the code section is decoded operator by operator, so opcode bytes
//! that happen to occur inside immediates are never mistaken for instructions. The decoder
//! knows the core instruction set plus the sign-extension, non-trapping float-to-int,
//! bulk-memory, reference-types, multi-value, multi-memory, tail-call, fixed-width SIMD and
//! relaxed-SIMD proposals. Anything else (exception handling, GC, threads, an unknown opcode,
//! a truncated or over-long section, a body that does not end on its final `end`) fails
//! closed: the scan returns `KNP1-FLAG-WASMPARSE` and nothing else.
//!
//! Relaxed-SIMD opcodes (prefix `0xFD`): `0x100 ..= 0x113`; of these `f32x4.relaxed_madd`
//! (`0x105`), `f32x4.relaxed_nmadd` (`0x106`), `f64x2.relaxed_madd` (`0x107`) and
//! `f64x2.relaxed_nmadd` (`0x108`) are the multiply-add forms.

/// Largest wasm module scanned, in bytes.
pub const MAX_WASM_BYTES: usize = 64 * 1024 * 1024;
/// Most function bodies in one module.
pub const MAX_FUNCTIONS: u32 = 1_000_000;
/// Deepest block nesting accepted in one function body.
pub const MAX_BLOCK_DEPTH: u32 = 100_000;

/// First relaxed-SIMD sub-opcode.
pub const RELAXED_FIRST: u32 = 0x100;
/// Last relaxed-SIMD sub-opcode.
pub const RELAXED_LAST: u32 = 0x113;
/// The relaxed multiply-add sub-opcodes and their names.
pub const RELAXED_MADD: [(u32, &str); 4] = [
    (0x105, "f32x4.relaxed_madd"),
    (0x106, "f32x4.relaxed_nmadd"),
    (0x107, "f64x2.relaxed_madd"),
    (0x108, "f64x2.relaxed_nmadd"),
];

/// Which SIMD (`0xFD`-prefixed) opcodes the module may contain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimdPolicy {
    /// No `allowed_simd` in the config: every fixed-width SIMD opcode is allowed, every
    /// relaxed-SIMD opcode is reported.
    Default,
    /// `allowed_simd = [...]`: only these sub-opcodes are allowed.
    Only(Vec<u32>),
}

impl SimdPolicy {
    fn allows(&self, sub: u32) -> bool {
        match self {
            SimdPolicy::Default => !(RELAXED_FIRST..=RELAXED_LAST).contains(&sub),
            SimdPolicy::Only(list) => list.contains(&sub),
        }
    }
}

/// One hit of the scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WasmHit {
    /// A relaxed multiply-add opcode.
    Fma { offset: usize, func: u32, sub: u32, name: &'static str, bytes: Vec<u8> },
    /// A SIMD opcode the policy does not allow.
    Simd { offset: usize, func: u32, sub: u32, bytes: Vec<u8> },
}

/// The module cannot be parsed; `offset` is the byte where decoding stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WasmParseError {
    pub offset: usize,
    pub message: String,
}

struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
    end: usize,
}

type R<T> = Result<T, WasmParseError>;

impl Reader<'_> {
    fn err<T>(&self, m: impl Into<String>) -> R<T> {
        Err(WasmParseError { offset: self.pos, message: m.into() })
    }

    fn byte(&mut self) -> R<u8> {
        if self.pos >= self.end {
            return self.err("unexpected end of data");
        }
        let v = self.b.get(self.pos).copied();
        match v {
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

    /// Signed LEB128 of at most `bits` bits (`bits` <= 64). Only the length is bounded; the
    /// value is returned sign-extended (wrapping for 64-bit inputs, which callers ignore).
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
    let b = r.b.get(at).copied();
    match b {
        Some(0x40) => r.skip(1),
        Some(v) if is_valtype(v) => r.skip(1),
        Some(_) => {
            // s33 type index; it must be non-negative.
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
    r.uleb(64)?; // offset (memory64 allows 64 bits)
    Ok(())
}

/// Decodes one `0xFD` instruction after its prefix; returns the sub-opcode.
fn simd(r: &mut Reader<'_>) -> R<u32> {
    let sub = r.u32()?;
    match sub {
        0x00..=0x0b | 0x5c | 0x5d => memarg(r)?,
        0x0c | 0x0d => r.skip(16)?,
        0x15..=0x22 => r.skip(1)?,
        0x54..=0x5b => {
            memarg(r)?;
            r.skip(1)?;
        }
        0x0e..=0x14 | 0x23..=0x53 | 0x5e..=0xff | RELAXED_FIRST..=RELAXED_LAST => {}
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

/// Walks one function body (`r` bounded to the body). Hits are appended to `hits`.
fn body(r: &mut Reader<'_>, func: u32, policy: &SimdPolicy, hits: &mut Vec<WasmHit>) -> R<()> {
    let groups = r.u32()?;
    for _ in 0..groups {
        r.u32()?; // count
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
                let bytes = r.b.get(start..r.pos).map(<[u8]>::to_vec).unwrap_or_default();
                if let Some((_, name)) = RELAXED_MADD.iter().find(|(s, _)| *s == sub) {
                    hits.push(WasmHit::Fma { offset: start, func, sub, name, bytes });
                } else if !policy.allows(sub) {
                    hits.push(WasmHit::Simd { offset: start, func, sub, bytes });
                }
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

/// Scans a whole module. `func` in each hit counts the bodies of the code section from 0
/// (imported functions are not counted). Hits are in byte order.
pub fn scan(bytes: &[u8], policy: &SimdPolicy) -> Result<Vec<WasmHit>, WasmParseError> {
    if bytes.len() > MAX_WASM_BYTES {
        return Err(WasmParseError { offset: 0, message: format!("module larger than {MAX_WASM_BYTES} bytes") });
    }
    let mut r = Reader { b: bytes, pos: 0, end: bytes.len() };
    if bytes.get(0..4) != Some(b"\0asm".as_slice()) {
        return r.err("missing `\\0asm` magic");
    }
    if bytes.get(4..8) != Some([1u8, 0, 0, 0].as_slice()) {
        r.pos = 4;
        return r.err("unsupported binary version (only version 1 modules are scanned)");
    }
    r.pos = 8;
    let mut hits = Vec::new();
    let mut seen_code = false;
    while r.pos < r.end {
        let id = r.byte()?;
        let size = r.u32()? as usize;
        let start = r.pos;
        let Some(end) = start.checked_add(size).filter(|e| *e <= r.end) else {
            return r.err(format!("section {id} runs past the end of the module"));
        };
        match id {
            0..=9 | 11 | 12 => {}
            10 => {
                if seen_code {
                    return r.err("second code section");
                }
                seen_code = true;
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
                    body(&mut br, func, policy, &mut hits)?;
                    s.pos = bend;
                }
                if s.pos != end {
                    return s.err("code section has trailing bytes");
                }
            }
            _ => return r.err(format!("unknown section id {id}")),
        }
        r.pos = end;
    }
    Ok(hits)
}
