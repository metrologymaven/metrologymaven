//! Bounded, fail-closed WebAssembly binary reader, validator and compiler for the sandbox.
//!
//! Accepts the core-1.0 binary format plus the post-MVP features that `rustc` emits for
//! `wasm32-wasip1` without SIMD or threads: sign-extension operators, non-trapping
//! float-to-int conversions, `memory.copy`/`memory.fill`, multi-value block types and the
//! LEB table index of `call_indirect`. Everything else (SIMD, threads, reference-type
//! instructions, passive segments, tags, 64-bit or shared memories, more than one memory or
//! table) is refused with `sandbox.malformed_module`.
//!
//! Every function body is type-checked with the validation algorithm of the core
//! specification (appendix "Validation Algorithm") and compiled into a flat [`Op`] array with
//! resolved branch targets, so the interpreter never interprets structured control flow.
//!
//! Imports are checked as soon as the import section is read: only the function imports of
//! [`super::IMPORT_ALLOWLIST`] and [`super::TRAPPED_IMPORTS`], with their exact WASI
//! signatures, pass; anything else is `sandbox.forbidden_import`.

use super::{ErrorCode, SandboxError, IMPORT_ALLOWLIST, TRAPPED_IMPORTS};
use std::collections::{BTreeMap, BTreeSet};

/// WebAssembly page size in bytes.
pub(crate) const PAGE: u64 = 65_536;
/// Largest memory a 32-bit module can address, in pages.
pub(crate) const MAX_PAGES: u32 = 65_536;

const MAX_TYPES: u32 = 100_000;
const MAX_PARAMS: u32 = 1_000;
const MAX_IMPORTS: u32 = 4_096;
const MAX_FUNCS: u32 = 1_000_000;
const MAX_GLOBALS: u32 = 1_000_000;
const MAX_EXPORTS: u32 = 100_000;
const MAX_SEGMENTS: u32 = 100_000;
const MAX_TABLE: u32 = 1 << 20;
const MAX_LOCALS: u64 = 50_000;
const MAX_BODY_BYTES: u32 = 7_654_321;
const MAX_NAME_BYTES: u32 = 1_024;
/// Cap on compiled operations over all functions (16 bytes each).
const MAX_OPS: usize = 1 << 24;
/// Cap on the nesting depth of blocks inside one function.
const MAX_NESTING: usize = 10_000;

/// Load work allowed per artifact byte. Together with [`WORK_BASE`] this bounds everything
/// [`parse`] does, linearly in the artifact size.
pub(crate) const WORK_PER_BYTE: u64 = 64;
/// Load work allowed for any module on top of [`WORK_PER_BYTE`].
pub(crate) const WORK_BASE: u64 = 1 << 20;
/// Largest work one validation step may do before the budget is checked again: one
/// `br_table` target (copying and checking at most 2 x [`MAX_PARAMS`] label types), one call
/// (popping and pushing at most 2 x [`MAX_PARAMS`] operands) or one block end, plus slack.
#[cfg(test)]
pub(crate) const WORK_MAX_STEP: u64 = 4 * MAX_PARAMS as u64 + 64;

/// Work done while loading a module, and its cap. Every parse step charges it: one unit per
/// section byte read, the sorted-set work of type and export deduplication, one unit per
/// declared local, and the operand-stack work of validation. Loading stops with
/// `sandbox.malformed_module` as soon as `used` exceeds `cap`, so a hostile module cannot make
/// loading superlinear in its size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Work {
    pub used: u64,
    pub cap: u64,
}

impl Work {
    /// The budget for an artifact of `len` bytes.
    pub(crate) fn for_module(len: usize) -> Self {
        Work { used: 0, cap: WORK_PER_BYTE.saturating_mul(len as u64).saturating_add(WORK_BASE) }
    }

    fn spend(&mut self, n: u64) -> Result<(), SandboxError> {
        self.used = self.used.saturating_add(n);
        if self.used > self.cap {
            return Err(too_expensive());
        }
        Ok(())
    }
}

fn too_expensive() -> SandboxError {
    malformed("limit_exceeded: module too expensive to validate")
}

/// Cost of one insertion into a sorted set of `n` entries whose key has `len` parts.
fn sorted_insert_cost(len: usize, n: u32) -> u64 {
    let depth = u64::from(32 - n.leading_zeros()) + 1;
    (len as u64 + 1).saturating_mul(depth)
}

/// Number types. Vector and reference types are refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ValType {
    I32,
    I64,
    F32,
    F64,
}

/// A function type.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct FuncType {
    pub params: Vec<ValType>,
    pub results: Vec<ValType>,
}

/// The host function an allowlisted import is bound to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HostFn {
    FdRead,
    FdWrite,
    ProcExit,
    EnvironGet,
    EnvironSizesGet,
    /// A sealed clock import (`fid_trap`): calling it traps.
    Trap,
}

/// A resolved branch: jump to `pc`, keeping the top `keep` values and dropping `drop` below.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Target {
    pub pc: u32,
    pub keep: u32,
    pub drop: u32,
}

/// One compiled operation. Structured control flow is resolved to jumps; `block`, `loop`,
/// `nop` and inner `end` produce no operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Op {
    Unreachable,
    Br(Target),
    BrIf(Target),
    /// `if`: pop the condition and jump to `pc` when it is zero.
    BrUnless(u32),
    /// End of the `then` arm of an `if` with an `else`.
    Jump(u32),
    /// `br_table`: targets `start..start+len` of the function's table; the last is the default.
    BrTable {
        start: u32,
        len: u32,
    },
    Return,
    Call(u32),
    CallIndirect(u32),
    Drop,
    Select,
    LocalGet(u32),
    LocalSet(u32),
    LocalTee(u32),
    GlobalGet(u32),
    GlobalSet(u32),
    Load {
        op: u8,
        offset: u32,
    },
    Store {
        op: u8,
        offset: u32,
    },
    MemorySize,
    MemoryGrow,
    Const(u64),
    /// Numeric operator with opcode `0x45..=0xC4`.
    Num(u8),
    /// Saturating truncation `0xFC 0..=7`.
    TruncSat(u8),
    MemoryCopy,
    MemoryFill,
}

/// A defined function, compiled.
#[derive(Clone, Debug)]
pub(crate) struct Func {
    pub type_idx: u32,
    /// Number of declared locals (excluding parameters); all are zero-initialised.
    pub locals: u32,
    /// Largest operand-stack height inside the body (excluding locals).
    pub max_height: u32,
    pub code: Vec<Op>,
    pub table: Vec<Target>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Global {
    pub ty: ValType,
    pub mutable: bool,
    pub init: u64,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Limits {
    pub min: u32,
    pub max: Option<u32>,
}

#[derive(Clone, Debug)]
pub(crate) struct ElemSeg {
    pub offset: u32,
    pub funcs: Vec<u32>,
}

#[derive(Clone, Debug)]
pub(crate) struct DataSeg {
    pub offset: u32,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExportKind {
    Func,
    Table,
    Memory,
    Global,
}

#[derive(Clone, Debug)]
pub(crate) struct Export {
    pub name: String,
    pub kind: ExportKind,
    pub index: u32,
}

/// A validated, compiled module.
#[derive(Clone, Debug, Default)]
pub(crate) struct Module {
    pub types: Vec<FuncType>,
    /// For each type, the smallest index of a structurally equal type (signature identity).
    pub canon_types: Vec<u32>,
    /// Host function and type index of each imported function, in import order.
    pub imports: Vec<(HostFn, u32)>,
    pub funcs: Vec<Func>,
    pub table: Option<Limits>,
    pub memory: Option<Limits>,
    pub globals: Vec<Global>,
    pub exports: Vec<Export>,
    pub start: Option<u32>,
    pub elems: Vec<ElemSeg>,
    pub datas: Vec<DataSeg>,
}

impl Module {
    /// Type index of function `f` (imports first), if it exists.
    pub fn func_type_idx(&self, f: u32) -> Option<u32> {
        let n = self.imports.len();
        let i = f as usize;
        if i < n {
            self.imports.get(i).map(|(_, t)| *t)
        } else {
            self.funcs.get(i - n).map(|d| d.type_idx)
        }
    }

    /// Type of function `f`.
    pub fn func_type(&self, f: u32) -> Option<&FuncType> {
        self.func_type_idx(f).and_then(|t| self.types.get(t as usize))
    }

    /// Index of the exported function `name`.
    pub fn export_func(&self, name: &str) -> Option<u32> {
        self.exports.iter().find(|e| e.kind == ExportKind::Func && e.name == name).map(|e| e.index)
    }
}

fn malformed(detail: impl Into<String>) -> SandboxError {
    SandboxError::new(ErrorCode::MalformedModule, detail)
}

fn forbidden(detail: impl Into<String>) -> SandboxError {
    SandboxError::new(ErrorCode::ForbiddenImport, detail)
}

// ------------------------------------------------------------------ byte reader

struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(b: &'a [u8]) -> Self {
        Reader { b, pos: 0 }
    }

    fn eof(&self) -> bool {
        self.pos >= self.b.len()
    }

    fn remaining(&self) -> usize {
        self.b.len().saturating_sub(self.pos)
    }

    fn byte(&mut self) -> Result<u8, SandboxError> {
        let v = self.b.get(self.pos).copied().ok_or_else(|| malformed("truncated: unexpected end of input"))?;
        self.pos += 1;
        Ok(v)
    }

    fn bytes(&mut self, n: usize) -> Result<&'a [u8], SandboxError> {
        let end = self.pos.checked_add(n).ok_or_else(|| malformed("truncated: length overflow"))?;
        let s = self.b.get(self.pos..end).ok_or_else(|| malformed("truncated: unexpected end of input"))?;
        self.pos = end;
        Ok(s)
    }

    /// Unsigned LEB128 `u32`: at most 5 bytes, no bits above bit 31.
    fn u32(&mut self) -> Result<u32, SandboxError> {
        let mut result: u64 = 0;
        for i in 0..5u32 {
            let b = self.byte()?;
            if i == 4 && b & 0xf0 != 0 {
                return Err(malformed("bad_leb: u32 out of range"));
            }
            result |= u64::from(b & 0x7f) << (7 * i);
            if b & 0x80 == 0 {
                return u32::try_from(result).map_err(|_| malformed("bad_leb: u32 out of range"));
            }
        }
        Err(malformed("bad_leb: u32 longer than 5 bytes"))
    }

    /// Signed LEB128 of `bits` (32 or 33) bits, sign-extended into `i64`.
    fn s_small(&mut self, bits: u32) -> Result<i64, SandboxError> {
        let mut result: i64 = 0;
        let mut shift = 0u32;
        loop {
            let b = self.byte()?;
            result |= i64::from(b & 0x7f) << shift;
            shift += 7;
            if b & 0x80 == 0 {
                if b & 0x40 != 0 {
                    result |= -1i64 << shift;
                }
                break;
            }
            if shift >= 35 {
                return Err(malformed("bad_leb: signed integer too long"));
            }
        }
        let lo = -(1i64 << (bits - 1));
        let hi = (1i64 << (bits - 1)) - 1;
        if !(lo..=hi).contains(&result) {
            return Err(malformed("bad_leb: signed integer out of range"));
        }
        Ok(result)
    }

    /// Signed LEB128 `i64`: at most 10 bytes; the 10th must be `0x00` or `0x7f`.
    fn s64(&mut self) -> Result<i64, SandboxError> {
        let mut result: i64 = 0;
        let mut shift = 0u32;
        loop {
            let b = self.byte()?;
            if shift == 63 {
                if b != 0x00 && b != 0x7f {
                    return Err(malformed("bad_leb: i64 out of range"));
                }
                result |= i64::from(b & 1) << 63;
                return Ok(result);
            }
            result |= i64::from(b & 0x7f) << shift;
            shift += 7;
            if b & 0x80 == 0 {
                if shift < 64 && b & 0x40 != 0 {
                    result |= -1i64 << shift;
                }
                return Ok(result);
            }
        }
    }

    fn name(&mut self) -> Result<String, SandboxError> {
        let n = self.u32()?;
        if n > MAX_NAME_BYTES {
            return Err(malformed("limit_exceeded: name longer than 1024 bytes"));
        }
        let raw = self.bytes(n as usize)?;
        String::from_utf8(raw.to_vec()).map_err(|_| malformed("bad_utf8: name is not UTF-8"))
    }

    /// A vector count, bounded by `max` and by the bytes left (every entry takes a byte).
    fn count(&mut self, max: u32, what: &str) -> Result<u32, SandboxError> {
        let n = self.u32()?;
        if n > max || n as usize > self.remaining() {
            return Err(malformed(format!("limit_exceeded: {n} {what}")));
        }
        Ok(n)
    }

    fn val_type(&mut self) -> Result<ValType, SandboxError> {
        val_type_of(self.byte()?)
    }

    fn limits(&mut self, max_allowed: u32, what: &str) -> Result<Limits, SandboxError> {
        let flags = self.byte()?;
        let min = self.u32()?;
        let max = match flags {
            0 => None,
            1 => Some(self.u32()?),
            _ => return Err(malformed(format!("bad_limits: {what} flags {flags:#x} (shared or 64-bit refused)"))),
        };
        if min > max_allowed || max.is_some_and(|m| m > max_allowed || m < min) {
            return Err(malformed(format!("bad_limits: {what} limits out of range")));
        }
        Ok(Limits { min, max })
    }
}

fn val_type_of(b: u8) -> Result<ValType, SandboxError> {
    match b {
        0x7f => Ok(ValType::I32),
        0x7e => Ok(ValType::I64),
        0x7d => Ok(ValType::F32),
        0x7c => Ok(ValType::F64),
        _ => Err(malformed(format!("unsupported value type {b:#04x}"))),
    }
}

// ------------------------------------------------------------------ imports

/// WASI preview-1 signature of an allowlisted or trapped import.
fn import_signature(module: &str, name: &str) -> Option<(HostFn, &'static [ValType], &'static [ValType])> {
    use ValType::{I32, I64};
    let host = match (module, name) {
        ("wasi_snapshot_preview1", "fd_read") => (HostFn::FdRead, &[I32, I32, I32, I32][..], &[I32][..]),
        ("wasi_snapshot_preview1", "fd_write") => (HostFn::FdWrite, &[I32, I32, I32, I32][..], &[I32][..]),
        ("wasi_snapshot_preview1", "proc_exit") => (HostFn::ProcExit, &[I32][..], &[][..]),
        ("wasi_snapshot_preview1", "environ_get") => (HostFn::EnvironGet, &[I32, I32][..], &[I32][..]),
        ("wasi_snapshot_preview1", "environ_sizes_get") => (HostFn::EnvironSizesGet, &[I32, I32][..], &[I32][..]),
        ("fid_trap", "clock_time_get") => (HostFn::Trap, &[I32, I64, I32][..], &[I32][..]),
        ("fid_trap", "clock_res_get") => (HostFn::Trap, &[I32, I32][..], &[I32][..]),
        _ => return None,
    };
    let listed = IMPORT_ALLOWLIST.contains(&(module, name)) || TRAPPED_IMPORTS.contains(&(module, name));
    listed.then_some(host)
}

// ------------------------------------------------------------------ module

/// Section order rank of the core specification (`None` for unknown or refused ids).
fn section_rank(id: u8) -> Option<u8> {
    match id {
        1 => Some(1),
        2 => Some(2),
        3 => Some(3),
        4 => Some(4),
        5 => Some(5),
        6 => Some(7),
        7 => Some(8),
        8 => Some(9),
        9 => Some(10),
        12 => Some(11),
        10 => Some(12),
        11 => Some(13),
        _ => None,
    }
}

/// Parses, checks imports, validates and compiles `bytes` within [`Work::for_module`].
pub(crate) fn parse(bytes: &[u8]) -> Result<Module, SandboxError> {
    parse_counted(bytes, &mut Work::for_module(bytes.len()))
}

/// [`parse`] charging `work`, which holds the work done when it returns, also on refusal.
pub(crate) fn parse_counted(bytes: &[u8], work: &mut Work) -> Result<Module, SandboxError> {
    let mut r = Reader::new(bytes);
    if r.bytes(4)? != b"\0asm" {
        return Err(malformed("bad_magic"));
    }
    if r.bytes(4)? != [1, 0, 0, 0] {
        return Err(malformed("bad_version: only core modules of version 1"));
    }
    let mut m = Module::default();
    let mut func_types: Vec<u32> = Vec::new();
    let mut data_count: Option<u32> = None;
    let mut seen_code = false;
    let mut last_rank = 0u8;
    let mut ops_total = 0usize;
    while !r.eof() {
        let id = r.byte()?;
        let size = r.u32()? as usize;
        let body = r.bytes(size)?;
        work.spend(size as u64 + 1)?;
        if id == 0 {
            // Custom section: the name must be well formed; the contents are ignored.
            let mut s = Reader::new(body);
            s.name()?;
            continue;
        }
        let rank = section_rank(id).ok_or_else(|| malformed(format!("unknown_section: id {id}")))?;
        if rank <= last_rank {
            return Err(malformed(format!("section_order: section {id} out of order or repeated")));
        }
        last_rank = rank;
        let mut s = Reader::new(body);
        match id {
            1 => read_types(&mut s, &mut m, work)?,
            2 => read_imports(&mut s, &mut m)?,
            3 => {
                let n = s.count(MAX_FUNCS, "functions")?;
                for _ in 0..n {
                    let t = s.u32()?;
                    if t as usize >= m.types.len() {
                        return Err(malformed("bad_type_index: function type"));
                    }
                    func_types.push(t);
                }
            }
            4 => {
                let n = s.count(1, "tables")?;
                if n == 1 {
                    if s.byte()? != 0x70 {
                        return Err(malformed("unsupported table element type (only funcref)"));
                    }
                    m.table = Some(s.limits(MAX_TABLE, "table")?);
                }
            }
            5 => {
                let n = s.count(1, "memories")?;
                if n == 1 {
                    m.memory = Some(s.limits(MAX_PAGES, "memory")?);
                }
            }
            6 => {
                let n = s.count(MAX_GLOBALS, "globals")?;
                for _ in 0..n {
                    let t = s.val_type()?;
                    let mutable = match s.byte()? {
                        0 => false,
                        1 => true,
                        _ => return Err(malformed("bad global mutability")),
                    };
                    let init = const_expr(&mut s, t)?;
                    m.globals.push(Global { ty: t, mutable, init });
                }
            }
            7 => read_exports(&mut s, &mut m, func_types.len(), work)?,
            8 => {
                let f = s.u32()?;
                if f as usize >= m.imports.len() + func_types.len() {
                    return Err(malformed("bad start function index"));
                }
                m.start = Some(f);
            }
            9 => read_elems(&mut s, &mut m, func_types.len())?,
            12 => data_count = Some(s.u32()?),
            10 => {
                seen_code = true;
                let n = s.u32()?;
                if n as usize != func_types.len() {
                    return Err(malformed("function and code section counts differ"));
                }
                // Signatures of every function must be known before bodies reference them.
                work.spend(r.pos as u64)?;
                let mut skeleton = m.clone();
                skeleton.funcs = func_types
                    .iter()
                    .map(|&t| Func { type_idx: t, locals: 0, max_height: 0, code: Vec::new(), table: Vec::new() })
                    .collect();
                for &t in &func_types {
                    let size = s.u32()?;
                    if size > MAX_BODY_BYTES {
                        return Err(malformed("limit_exceeded: function body too large"));
                    }
                    let body = s.bytes(size as usize)?;
                    let f = compile_body(&skeleton, t, body, work)?;
                    ops_total = ops_total.saturating_add(f.code.len()).saturating_add(f.table.len());
                    if ops_total > MAX_OPS {
                        return Err(malformed("limit_exceeded: code too large"));
                    }
                    m.funcs.push(f);
                }
            }
            11 => {
                let n = s.count(MAX_SEGMENTS, "data segments")?;
                if data_count.is_some_and(|c| c != n) {
                    return Err(malformed("data count section disagrees with data section"));
                }
                for _ in 0..n {
                    let flags = s.u32()?;
                    match flags {
                        0 => {}
                        2 => {
                            if s.u32()? != 0 {
                                return Err(malformed("data segment for a memory other than 0"));
                            }
                        }
                        _ => return Err(malformed("unsupported data segment kind (passive segments refused)")),
                    }
                    if m.memory.is_none() {
                        return Err(malformed("data segment without memory"));
                    }
                    let offset = const_expr(&mut s, ValType::I32)? as u32;
                    let len = s.u32()?;
                    let bytes = s.bytes(len as usize)?.to_vec();
                    m.datas.push(DataSeg { offset, bytes });
                }
            }
            _ => return Err(malformed(format!("unknown_section: id {id}"))),
        }
        if !s.eof() {
            return Err(malformed(format!("section_size_mismatch: section {id}")));
        }
    }
    if !seen_code && !func_types.is_empty() {
        return Err(malformed("function section without code section"));
    }
    if data_count.is_some_and(|c| c as usize != m.datas.len()) {
        return Err(malformed("data count section disagrees with data section"));
    }
    if let Some(f) = m.start {
        let ok = m.func_type(f).is_some_and(|t| t.params.is_empty() && t.results.is_empty());
        if !ok {
            return Err(malformed("start function must have type [] -> []"));
        }
    }
    Ok(m)
}

fn read_types(s: &mut Reader<'_>, m: &mut Module, work: &mut Work) -> Result<(), SandboxError> {
    let n = s.count(MAX_TYPES, "types")?;
    // First index of each distinct type: signature identity in O(n log n), not O(n^2).
    let mut first: BTreeMap<FuncType, u32> = BTreeMap::new();
    for i in 0..n {
        if s.byte()? != 0x60 {
            return Err(malformed("bad_type: expected function type 0x60"));
        }
        let np = s.count(MAX_PARAMS, "params")?;
        let params = (0..np).map(|_| s.val_type()).collect::<Result<Vec<_>, _>>()?;
        let nr = s.count(MAX_PARAMS, "results")?;
        let results = (0..nr).map(|_| s.val_type()).collect::<Result<Vec<_>, _>>()?;
        let ft = FuncType { params, results };
        work.spend(sorted_insert_cost(ft.params.len() + ft.results.len(), i))?;
        let canon = *first.entry(ft.clone()).or_insert(i);
        m.types.push(ft);
        m.canon_types.push(canon);
    }
    Ok(())
}

fn read_imports(s: &mut Reader<'_>, m: &mut Module) -> Result<(), SandboxError> {
    let n = s.count(MAX_IMPORTS, "imports")?;
    for _ in 0..n {
        let module = s.name()?;
        let name = s.name()?;
        let kind = s.byte()?;
        if kind != 0 {
            let what = match kind {
                1 => "table",
                2 => "memory",
                3 => "global",
                4 => "tag",
                _ => "unknown",
            };
            return Err(forbidden(format!("{module}.{name}: {what} import (only allowlisted functions)")));
        }
        let t = s.u32()?;
        let ty = m.types.get(t as usize).ok_or_else(|| malformed("bad_type_index: import type"))?;
        let Some((host, params, results)) = import_signature(&module, &name) else {
            return Err(forbidden(format!("{module}.{name}: not in the kernel import allowlist")));
        };
        if ty.params != params || ty.results != results {
            return Err(forbidden(format!("{module}.{name}: signature differs from WASI preview 1")));
        }
        if m.imports.len() >= MAX_IMPORTS as usize {
            return Err(malformed("limit_exceeded: imports"));
        }
        m.imports.push((host, t));
    }
    Ok(())
}

fn read_exports(s: &mut Reader<'_>, m: &mut Module, defined: usize, work: &mut Work) -> Result<(), SandboxError> {
    let n = s.count(MAX_EXPORTS, "exports")?;
    let mut names: BTreeSet<String> = BTreeSet::new();
    for i in 0..n {
        let name = s.name()?;
        let kind = match s.byte()? {
            0 => ExportKind::Func,
            1 => ExportKind::Table,
            2 => ExportKind::Memory,
            3 => ExportKind::Global,
            _ => return Err(malformed("bad export kind")),
        };
        let index = s.u32()?;
        let in_range = match kind {
            ExportKind::Func => (index as usize) < m.imports.len() + defined,
            ExportKind::Table => index == 0 && m.table.is_some(),
            ExportKind::Memory => index == 0 && m.memory.is_some(),
            ExportKind::Global => (index as usize) < m.globals.len(),
        };
        if !in_range {
            return Err(malformed(format!("export {name:?} index out of range")));
        }
        work.spend(sorted_insert_cost(name.len(), i))?;
        if !names.insert(name.clone()) {
            return Err(malformed(format!("duplicate export {name:?}")));
        }
        m.exports.push(Export { name, kind, index });
    }
    Ok(())
}

fn read_elems(s: &mut Reader<'_>, m: &mut Module, defined: usize) -> Result<(), SandboxError> {
    let n = s.count(MAX_SEGMENTS, "element segments")?;
    for _ in 0..n {
        let flags = s.u32()?;
        match flags {
            0 => {}
            2 => {
                if s.u32()? != 0 {
                    return Err(malformed("element segment for a table other than 0"));
                }
            }
            _ => return Err(malformed("unsupported element segment kind")),
        }
        if m.table.is_none() {
            return Err(malformed("element segment without table"));
        }
        let offset = const_expr(s, ValType::I32)? as u32;
        if flags == 2 && s.byte()? != 0x00 {
            return Err(malformed("unsupported element kind"));
        }
        let k = s.count(MAX_TABLE, "element entries")?;
        let mut funcs = Vec::new();
        for _ in 0..k {
            let f = s.u32()?;
            if f as usize >= m.imports.len() + defined {
                return Err(malformed("element function index out of range"));
            }
            funcs.push(f);
        }
        m.elems.push(ElemSeg { offset, funcs });
    }
    Ok(())
}

/// A constant expression of type `t`: one `*.const` followed by `end`.
fn const_expr(s: &mut Reader<'_>, t: ValType) -> Result<u64, SandboxError> {
    let op = s.byte()?;
    let (ty, v) = match op {
        0x41 => (ValType::I32, u64::from(s.s_small(32)? as i32 as u32)),
        0x42 => (ValType::I64, s.s64()? as u64),
        0x43 => (ValType::F32, u64::from(le_u32(s.bytes(4)?))),
        0x44 => (ValType::F64, le_u64(s.bytes(8)?)),
        _ => return Err(malformed(format!("unsupported constant expression opcode {op:#04x}"))),
    };
    if ty != t {
        return Err(malformed("constant expression type mismatch"));
    }
    if s.byte()? != 0x0b {
        return Err(malformed("constant expression must be a single constant"));
    }
    Ok(v)
}

fn le_u32(b: &[u8]) -> u32 {
    let mut a = [0u8; 4];
    for (d, s) in a.iter_mut().zip(b) {
        *d = *s;
    }
    u32::from_le_bytes(a)
}

fn le_u64(b: &[u8]) -> u64 {
    let mut a = [0u8; 8];
    for (d, s) in a.iter_mut().zip(b) {
        *d = *s;
    }
    u64::from_le_bytes(a)
}

// ------------------------------------------------------------------ function bodies

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Block,
    Loop,
    If,
    Else,
    Func,
}

#[derive(Clone, Copy, Debug)]
enum Fixup {
    Op(usize),
    Table(usize),
}

struct Ctrl {
    kind: Kind,
    params: Vec<ValType>,
    results: Vec<ValType>,
    height: usize,
    unreachable: bool,
    start_pc: u32,
    if_op: Option<usize>,
    fixups: Vec<Fixup>,
}

impl Ctrl {
    fn labels(&self) -> &[ValType] {
        if self.kind == Kind::Loop {
            &self.params
        } else {
            &self.results
        }
    }
}

/// `None` stands for the unknown type of the polymorphic stack in unreachable code.
type Opd = Option<ValType>;

struct Validator<'m> {
    m: &'m Module,
    locals: Vec<ValType>,
    vals: Vec<Opd>,
    ctrls: Vec<Ctrl>,
    code: Vec<Op>,
    table: Vec<Target>,
    max_height: usize,
    /// Module load work so far, including this body's operand pushes, pops and checks;
    /// checked against `budget` after every instruction and every `br_table` target, so a
    /// hostile body cannot make validation quadratic.
    work: u64,
    budget: u64,
}

fn type_err(detail: &str) -> SandboxError {
    malformed(format!("type mismatch: {detail}"))
}

impl Validator<'_> {
    /// Refuses the module once the load work exceeds its budget.
    fn check_budget(&self) -> Result<(), SandboxError> {
        if self.work > self.budget {
            return Err(too_expensive());
        }
        Ok(())
    }

    fn push(&mut self, t: Opd) {
        self.work += 1;
        self.vals.push(t);
        if self.vals.len() > self.max_height {
            self.max_height = self.vals.len();
        }
    }

    fn pop(&mut self) -> Result<Opd, SandboxError> {
        self.work += 1;
        let c = self.ctrls.last().ok_or_else(|| type_err("no frame"))?;
        if self.vals.len() == c.height {
            if c.unreachable {
                return Ok(None);
            }
            return Err(type_err("operand stack underflow"));
        }
        self.vals.pop().ok_or_else(|| type_err("operand stack underflow"))
    }

    fn pop_expect(&mut self, t: ValType) -> Result<(), SandboxError> {
        match self.pop()? {
            Some(a) if a != t => Err(type_err("unexpected operand type")),
            _ => Ok(()),
        }
    }

    fn pop_all(&mut self, ts: &[ValType]) -> Result<(), SandboxError> {
        for &t in ts.iter().rev() {
            self.pop_expect(t)?;
        }
        Ok(())
    }

    fn push_all(&mut self, ts: &[ValType]) {
        for &t in ts {
            self.push(Some(t));
        }
    }

    /// Checks that the top operands match `ts` without consuming them.
    fn check_top(&mut self, ts: &[ValType]) -> Result<(), SandboxError> {
        let c = self.ctrls.last().ok_or_else(|| type_err("no frame"))?;
        let (height, unreachable) = (c.height, c.unreachable);
        let len = self.vals.len();
        for (i, &t) in ts.iter().rev().enumerate() {
            self.work += 1;
            match len.checked_sub(i + 1).filter(|&p| p >= height) {
                Some(p) => {
                    if let Some(Some(a)) = self.vals.get(p) {
                        if *a != t {
                            return Err(type_err("unexpected operand type"));
                        }
                    }
                }
                None if unreachable => {}
                None => return Err(type_err("operand stack underflow")),
            }
        }
        Ok(())
    }

    fn set_unreachable(&mut self) -> Result<(), SandboxError> {
        let c = self.ctrls.last_mut().ok_or_else(|| type_err("no frame"))?;
        self.vals.truncate(c.height);
        c.unreachable = true;
        Ok(())
    }

    fn emit(&mut self, op: Op) -> Result<(), SandboxError> {
        if self.code.len() >= u32::MAX as usize / 2 {
            return Err(malformed("limit_exceeded: function too large"));
        }
        self.code.push(op);
        Ok(())
    }

    fn pc(&self) -> u32 {
        self.code.len() as u32
    }

    fn push_ctrl(&mut self, kind: Kind, params: Vec<ValType>, results: Vec<ValType>) -> Result<(), SandboxError> {
        if self.ctrls.len() >= MAX_NESTING {
            return Err(malformed("limit_exceeded: blocks nested too deeply"));
        }
        let height = self.vals.len();
        let start_pc = self.pc();
        self.push_all(&params);
        self.ctrls.push(Ctrl {
            kind,
            params,
            results,
            height,
            unreachable: false,
            start_pc,
            if_op: None,
            fixups: Vec::new(),
        });
        Ok(())
    }

    /// Checks the end of the innermost block and removes it.
    fn pop_ctrl(&mut self) -> Result<Ctrl, SandboxError> {
        let results = self.ctrls.last().map(|c| c.results.clone()).ok_or_else(|| type_err("no frame"))?;
        self.pop_all(&results)?;
        let c = self.ctrls.pop().ok_or_else(|| type_err("no frame"))?;
        if self.vals.len() != c.height {
            return Err(type_err("values left on the stack at the end of a block"));
        }
        Ok(c)
    }

    /// The branch to label `depth` from the current operand-stack height.
    fn target(&mut self, depth: u32) -> Result<(usize, Target, Vec<ValType>), SandboxError> {
        let n = self.ctrls.len();
        let idx = n.checked_sub(1 + depth as usize).ok_or_else(|| malformed("branch label out of range"))?;
        let c = self.ctrls.get(idx).ok_or_else(|| malformed("branch label out of range"))?;
        let labels = c.labels().to_vec();
        self.work += labels.len() as u64 + 1;
        let keep = labels.len();
        let drop = self.vals.len().saturating_sub(c.height).saturating_sub(keep);
        let pc = if c.kind == Kind::Loop { c.start_pc } else { 0 };
        let t = Target { pc, keep: keep as u32, drop: drop as u32 };
        Ok((idx, t, labels))
    }

    fn add_fixup(&mut self, ctrl: usize, fix: Fixup) -> Result<(), SandboxError> {
        let c = self.ctrls.get_mut(ctrl).ok_or_else(|| malformed("branch label out of range"))?;
        if c.kind != Kind::Loop {
            c.fixups.push(fix);
        }
        Ok(())
    }

    fn block_type(&mut self, r: &mut Reader<'_>) -> Result<(Vec<ValType>, Vec<ValType>), SandboxError> {
        let first = *r.b.get(r.pos).ok_or_else(|| malformed("truncated block type"))?;
        if first == 0x40 {
            r.pos += 1;
            return Ok((Vec::new(), Vec::new()));
        }
        if matches!(first, 0x7c..=0x7f) {
            r.pos += 1;
            return Ok((Vec::new(), vec![val_type_of(first)?]));
        }
        let idx = r.s_small(33)?;
        let t = usize::try_from(idx)
            .ok()
            .and_then(|i| self.m.types.get(i))
            .ok_or_else(|| malformed("unsupported or out-of-range block type"))?;
        self.work += (t.params.len() + t.results.len()) as u64;
        Ok((t.params.clone(), t.results.clone()))
    }

    fn memarg(&mut self, r: &mut Reader<'_>, natural: u32) -> Result<u32, SandboxError> {
        let align = r.u32()?;
        let offset = r.u32()?;
        if self.m.memory.is_none() {
            return Err(malformed("memory instruction without memory"));
        }
        if align > natural {
            return Err(malformed("alignment larger than natural"));
        }
        Ok(offset)
    }

    fn mem_index_zero(&mut self, r: &mut Reader<'_>) -> Result<(), SandboxError> {
        if r.u32()? != 0 {
            return Err(malformed("memory index other than 0"));
        }
        if self.m.memory.is_none() {
            return Err(malformed("memory instruction without memory"));
        }
        Ok(())
    }
}

/// Load opcode -> (natural alignment log2, result type).
fn load_info(op: u8) -> Option<(u32, ValType)> {
    use ValType::*;
    Some(match op {
        0x28 => (2, I32),
        0x29 => (3, I64),
        0x2a => (2, F32),
        0x2b => (3, F64),
        0x2c | 0x2d => (0, I32),
        0x2e | 0x2f => (1, I32),
        0x30 | 0x31 => (0, I64),
        0x32 | 0x33 => (1, I64),
        0x34 | 0x35 => (2, I64),
        _ => return None,
    })
}

/// Store opcode -> (natural alignment log2, operand type).
fn store_info(op: u8) -> Option<(u32, ValType)> {
    use ValType::*;
    Some(match op {
        0x36 => (2, I32),
        0x37 => (3, I64),
        0x38 => (2, F32),
        0x39 => (3, F64),
        0x3a => (0, I32),
        0x3b => (1, I32),
        0x3c => (0, I64),
        0x3d => (1, I64),
        0x3e => (2, I64),
        _ => return None,
    })
}

/// Numeric opcode `0x45..=0xC4` -> (operand types, result type).
pub(crate) fn num_sig(op: u8) -> Option<(&'static [ValType], ValType)> {
    use ValType::*;
    const I: &[ValType] = &[I32];
    const II: &[ValType] = &[I32, I32];
    const L: &[ValType] = &[I64];
    const LL: &[ValType] = &[I64, I64];
    const F: &[ValType] = &[F32];
    const FF: &[ValType] = &[F32, F32];
    const D: &[ValType] = &[F64];
    const DD: &[ValType] = &[F64, F64];
    Some(match op {
        0x45 => (I, I32),
        0x46..=0x4f => (II, I32),
        0x50 => (L, I32),
        0x51..=0x5a => (LL, I32),
        0x5b..=0x60 => (FF, I32),
        0x61..=0x66 => (DD, I32),
        0x67..=0x69 => (I, I32),
        0x6a..=0x78 => (II, I32),
        0x79..=0x7b => (L, I64),
        0x7c..=0x8a => (LL, I64),
        0x8b..=0x91 => (F, F32),
        0x92..=0x98 => (FF, F32),
        0x99..=0x9f => (D, F64),
        0xa0..=0xa6 => (DD, F64),
        0xa7 => (L, I32),
        0xa8 | 0xa9 => (F, I32),
        0xaa | 0xab => (D, I32),
        0xac | 0xad => (I, I64),
        0xae | 0xaf => (F, I64),
        0xb0 | 0xb1 => (D, I64),
        0xb2 | 0xb3 => (I, F32),
        0xb4 | 0xb5 => (L, F32),
        0xb6 => (D, F32),
        0xb7 | 0xb8 => (I, F64),
        0xb9 | 0xba => (L, F64),
        0xbb => (F, F64),
        0xbc => (F, I32),
        0xbd => (D, I64),
        0xbe => (I, F32),
        0xbf => (L, F64),
        0xc0 | 0xc1 => (I, I32),
        0xc2..=0xc4 => (L, I64),
        _ => return None,
    })
}

/// Saturating truncation `0xFC n` -> (operand type, result type).
pub(crate) fn trunc_sat_sig(n: u32) -> Option<(ValType, ValType)> {
    use ValType::*;
    Some(match n {
        0 | 1 => (F32, I32),
        2 | 3 => (F64, I32),
        4 | 5 => (F32, I64),
        6 | 7 => (F64, I64),
        _ => return None,
    })
}

fn compile_body(m: &Module, type_idx: u32, body: &[u8], work: &mut Work) -> Result<Func, SandboxError> {
    let ft = m.types.get(type_idx as usize).ok_or_else(|| malformed("bad_type_index"))?;
    let mut r = Reader::new(body);
    let mut locals = ft.params.clone();
    let groups = r.count(MAX_LOCALS as u32, "local groups")?;
    let mut declared: u64 = 0;
    for _ in 0..groups {
        let n = r.u32()?;
        let t = r.val_type()?;
        // `locals` already holds the parameters and the earlier groups.
        if locals.len() as u64 + u64::from(n) > MAX_LOCALS {
            return Err(malformed("limit_exceeded: too many locals"));
        }
        work.spend(u64::from(n))?;
        declared += u64::from(n);
        locals.extend(std::iter::repeat_n(t, n as usize));
    }
    let mut v = Validator {
        m,
        locals,
        vals: Vec::new(),
        ctrls: Vec::new(),
        code: Vec::new(),
        table: Vec::new(),
        max_height: 0,
        work: work.used,
        budget: work.cap,
    };
    v.ctrls.push(Ctrl {
        kind: Kind::Func,
        params: Vec::new(),
        results: ft.results.clone(),
        height: 0,
        unreachable: false,
        start_pc: 0,
        if_op: None,
        fixups: Vec::new(),
    });
    let result = compile_steps(&mut v, &mut r);
    work.used = v.work;
    result?;
    if !r.eof() {
        return Err(malformed("bytes after the end of a function body"));
    }
    Ok(Func { type_idx, locals: declared as u32, max_height: v.max_height as u32, code: v.code, table: v.table })
}

/// Validates and compiles instructions until the function body's final `end`.
fn compile_steps(v: &mut Validator<'_>, r: &mut Reader<'_>) -> Result<(), SandboxError> {
    while !v.ctrls.is_empty() {
        let op = r.byte()?;
        step(v, r, op)?;
        v.check_budget()?;
    }
    Ok(())
}

fn local_type(v: &Validator<'_>, i: u32) -> Result<ValType, SandboxError> {
    v.locals.get(i as usize).copied().ok_or_else(|| malformed("local index out of range"))
}

/// Validates and compiles one instruction.
fn step(v: &mut Validator<'_>, r: &mut Reader<'_>, op: u8) -> Result<(), SandboxError> {
    use ValType::*;
    match op {
        0x00 => {
            v.emit(Op::Unreachable)?;
            v.set_unreachable()?;
        }
        0x01 => {}
        0x02 | 0x03 => {
            let (params, results) = v.block_type(r)?;
            v.pop_all(&params)?;
            let kind = if op == 0x02 { Kind::Block } else { Kind::Loop };
            v.push_ctrl(kind, params, results)?;
        }
        0x04 => {
            let (params, results) = v.block_type(r)?;
            v.pop_expect(I32)?;
            v.pop_all(&params)?;
            let at = v.code.len();
            v.emit(Op::BrUnless(0))?;
            v.push_ctrl(Kind::If, params, results)?;
            if let Some(c) = v.ctrls.last_mut() {
                c.if_op = Some(at);
            }
        }
        0x05 => {
            let c = v.pop_ctrl()?;
            if c.kind != Kind::If {
                return Err(malformed("else without if"));
            }
            let jump_at = v.code.len();
            v.emit(Op::Jump(0))?;
            let else_pc = v.pc();
            if let Some(Op::BrUnless(pc)) = c.if_op.and_then(|i| v.code.get_mut(i)) {
                *pc = else_pc;
            }
            let mut fixups = c.fixups;
            fixups.push(Fixup::Op(jump_at));
            v.push_ctrl(Kind::Else, c.params, c.results)?;
            if let Some(n) = v.ctrls.last_mut() {
                n.fixups = fixups;
            }
        }
        0x0b => {
            let c = v.pop_ctrl()?;
            v.work += (c.fixups.len() + c.params.len()) as u64;
            if c.kind == Kind::If && c.params != c.results {
                return Err(type_err("if without else must not change the stack type"));
            }
            if c.kind == Kind::Func {
                v.emit(Op::Return)?;
            }
            // Branches to a block land after its end; to the function body, on its return.
            let end_pc = if c.kind == Kind::Func { v.pc() - 1 } else { v.pc() };
            if let Some(Op::BrUnless(pc)) = c.if_op.and_then(|i| v.code.get_mut(i)) {
                *pc = end_pc;
            }
            for f in c.fixups {
                let slot = match f {
                    Fixup::Op(i) => match v.code.get_mut(i) {
                        Some(Op::Br(t) | Op::BrIf(t)) => Some(&mut t.pc),
                        Some(Op::Jump(pc)) => Some(pc),
                        _ => None,
                    },
                    Fixup::Table(i) => v.table.get_mut(i).map(|t| &mut t.pc),
                };
                let slot = slot.ok_or_else(|| malformed("internal: bad branch fixup"))?;
                *slot = end_pc;
            }
            v.push_all(&c.results);
        }
        0x0c => {
            let depth = r.u32()?;
            let (idx, t, labels) = v.target(depth)?;
            v.pop_all(&labels)?;
            v.add_fixup(idx, Fixup::Op(v.code.len()))?;
            v.emit(Op::Br(t))?;
            v.set_unreachable()?;
        }
        0x0d => {
            let depth = r.u32()?;
            v.pop_expect(I32)?;
            let (idx, t, labels) = v.target(depth)?;
            v.pop_all(&labels)?;
            v.push_all(&labels);
            v.add_fixup(idx, Fixup::Op(v.code.len()))?;
            v.emit(Op::BrIf(t))?;
        }
        0x0e => {
            let n = r.count(MAX_TABLE, "br_table targets")?;
            v.pop_expect(I32)?;
            let start = v.table.len();
            let mut arity: Option<usize> = None;
            for _ in 0..=n {
                let depth = r.u32()?;
                let (idx, t, labels) = v.target(depth)?;
                if arity.is_some_and(|a| a != labels.len()) {
                    return Err(type_err("br_table targets of different arity"));
                }
                arity = Some(labels.len());
                // Type-check against each label without consuming the operands.
                v.check_top(&labels)?;
                v.add_fixup(idx, Fixup::Table(v.table.len()))?;
                v.table.push(t);
                // One instruction may hold a million targets: check the budget per target.
                v.check_budget()?;
            }
            v.emit(Op::BrTable { start: start as u32, len: n + 1 })?;
            v.set_unreachable()?;
        }
        0x0f => {
            let results = v.ctrls.first().map(|c| c.results.clone()).unwrap_or_default();
            v.pop_all(&results)?;
            v.emit(Op::Return)?;
            v.set_unreachable()?;
        }
        0x10 => {
            let f = r.u32()?;
            let t = v.m.func_type(f).ok_or_else(|| malformed("call to unknown function"))?.clone();
            v.pop_all(&t.params)?;
            v.push_all(&t.results);
            v.emit(Op::Call(f))?;
        }
        0x11 => {
            let ti = r.u32()?;
            if r.u32()? != 0 || v.m.table.is_none() {
                return Err(malformed("call_indirect needs table 0"));
            }
            let t = v.m.types.get(ti as usize).ok_or_else(|| malformed("bad_type_index: call_indirect"))?.clone();
            v.pop_expect(I32)?;
            v.pop_all(&t.params)?;
            v.push_all(&t.results);
            v.emit(Op::CallIndirect(ti))?;
        }
        0x1a => {
            v.pop()?;
            v.emit(Op::Drop)?;
        }
        0x1b | 0x1c => {
            let want = if op == 0x1c {
                if r.u32()? != 1 {
                    return Err(malformed("typed select needs exactly one type"));
                }
                Some(r.val_type()?)
            } else {
                None
            };
            v.pop_expect(I32)?;
            let a = v.pop()?;
            let b = v.pop()?;
            let t = match (a, b) {
                (Some(x), Some(y)) if x != y => return Err(type_err("select operands differ")),
                (Some(x), _) | (None, Some(x)) => Some(x),
                (None, None) => None,
            };
            if let (Some(w), Some(t)) = (want, t) {
                if w != t {
                    return Err(type_err("select operand type"));
                }
            }
            v.push(want.or(t));
            v.emit(Op::Select)?;
        }
        0x20 => {
            let i = r.u32()?;
            let t = local_type(v, i)?;
            v.push(Some(t));
            v.emit(Op::LocalGet(i))?;
        }
        0x21 | 0x22 => {
            let i = r.u32()?;
            let t = local_type(v, i)?;
            v.pop_expect(t)?;
            if op == 0x22 {
                v.push(Some(t));
                v.emit(Op::LocalTee(i))?;
            } else {
                v.emit(Op::LocalSet(i))?;
            }
        }
        0x23 | 0x24 => {
            let i = r.u32()?;
            let g = *v.m.globals.get(i as usize).ok_or_else(|| malformed("global index out of range"))?;
            let t = g.ty;
            if op == 0x23 {
                v.push(Some(t));
                v.emit(Op::GlobalGet(i))?;
            } else {
                if !g.mutable {
                    return Err(malformed("global.set of an immutable global"));
                }
                v.pop_expect(t)?;
                v.emit(Op::GlobalSet(i))?;
            }
        }
        0x28..=0x35 => {
            let (natural, t) = load_info(op).ok_or_else(|| malformed("bad load"))?;
            let offset = v.memarg(r, natural)?;
            v.pop_expect(I32)?;
            v.push(Some(t));
            v.emit(Op::Load { op, offset })?;
        }
        0x36..=0x3e => {
            let (natural, t) = store_info(op).ok_or_else(|| malformed("bad store"))?;
            let offset = v.memarg(r, natural)?;
            v.pop_expect(t)?;
            v.pop_expect(I32)?;
            v.emit(Op::Store { op, offset })?;
        }
        0x3f => {
            v.mem_index_zero(r)?;
            v.push(Some(I32));
            v.emit(Op::MemorySize)?;
        }
        0x40 => {
            v.mem_index_zero(r)?;
            v.pop_expect(I32)?;
            v.push(Some(I32));
            v.emit(Op::MemoryGrow)?;
        }
        0x41 => {
            let x = r.s_small(32)? as i32 as u32;
            v.push(Some(I32));
            v.emit(Op::Const(u64::from(x)))?;
        }
        0x42 => {
            let x = r.s64()? as u64;
            v.push(Some(I64));
            v.emit(Op::Const(x))?;
        }
        0x43 => {
            let x = le_u32(r.bytes(4)?);
            v.push(Some(F32));
            v.emit(Op::Const(u64::from(x)))?;
        }
        0x44 => {
            let x = le_u64(r.bytes(8)?);
            v.push(Some(F64));
            v.emit(Op::Const(x))?;
        }
        0x45..=0xc4 => {
            let (params, result) = num_sig(op).ok_or_else(|| malformed("bad numeric opcode"))?;
            v.pop_all(params)?;
            v.push(Some(result));
            v.emit(Op::Num(op))?;
        }
        0xfc => {
            let sub = r.u32()?;
            match sub {
                0..=7 => {
                    let (a, b) = trunc_sat_sig(sub).ok_or_else(|| malformed("bad opcode"))?;
                    v.pop_expect(a)?;
                    v.push(Some(b));
                    v.emit(Op::TruncSat(sub as u8))?;
                }
                10 => {
                    v.mem_index_zero(r)?;
                    v.mem_index_zero(r)?;
                    v.pop_all(&[I32, I32, I32])?;
                    v.emit(Op::MemoryCopy)?;
                }
                11 => {
                    v.mem_index_zero(r)?;
                    v.pop_all(&[I32, I32, I32])?;
                    v.emit(Op::MemoryFill)?;
                }
                _ => return Err(malformed(format!("unsupported opcode 0xfc {sub}"))),
            }
        }
        _ => return Err(malformed(format!("unsupported opcode {op:#04x}"))),
    }
    Ok(())
}
