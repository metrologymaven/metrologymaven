//! Fuel-metered interpreter over the compiled [`Op`] arrays of a validated [`Module`].
//!
//! - Values are untyped 64-bit slots (`i32`/`f32` in the low 32 bits); validation guarantees
//!   their types, and every stack or memory access is still checked, so a fault is an
//!   `engine_fault` trap, never a panic.
//! - Calls use an explicit frame stack (no host recursion); depth and slot limits give
//!   `call_stack_exhausted`.
//! - Linear memory is a table of 64 KiB pages allocated on first write, so `memory.grow`
//!   costs no host memory until pages are touched; every allocation is fallible and a refused
//!   one is `sandbox.memory_limit`.
//! - Floating-point results follow IEEE 754 with every NaN produced by an arithmetic
//!   operator canonicalised (KRN.MAN.WASM `nan_canonicalization = true`); `abs`, `neg`,
//!   `copysign`, loads, stores and reinterpretations keep NaN payloads bit for bit.
//! - Each executed operation costs one unit of fuel, and bulk byte moves (`memory.copy`,
//!   `memory.fill`, WASI reads and writes) one more unit per 1 KiB; fuel is per call (start
//!   function and `_start` together). The wall-clock watchdog is polled whenever another
//!   65 536 units are spent and only ever stops a call.

use super::module::{DataSeg, ElemSeg, HostFn, Module, Op, Target, MAX_PAGES, PAGE};
use super::{ErrorCode, SandboxError, TrapKind};
use std::time::Instant;

const CANON_F32: u64 = 0x7fc0_0000;
const CANON_F64: u64 = 0x7ff8_0000_0000_0000;
const PAGE_USIZE: usize = PAGE as usize;
const WATCHDOG_INTERVAL: u64 = 0x1_0000;
/// Bytes moved per extra unit of fuel by bulk operations.
const BYTES_PER_FUEL: u64 = 1024;

/// WASI errno values used by the stubs.
const ERRNO_SUCCESS: u32 = 0;
const ERRNO_BADF: u32 = 8;
const ERRNO_FAULT: u32 = 21;

pub(crate) fn trap(kind: TrapKind, detail: impl Into<String>) -> SandboxError {
    SandboxError::trap(kind, detail)
}

fn fault(detail: &str) -> SandboxError {
    trap(TrapKind::EngineFault, detail)
}

fn oob() -> SandboxError {
    trap(TrapKind::OutOfBoundsMemoryAccess, "out of bounds memory access")
}

fn mem_limit(detail: impl Into<String>) -> SandboxError {
    SandboxError::new(ErrorCode::MemoryLimit, detail)
}

// ------------------------------------------------------------------ linear memory

/// Paged linear memory. `None` pages read as zero and are allocated on first write.
pub(crate) struct Memory {
    pages: Vec<Option<Vec<u8>>>,
    max_pages: u32,
}

impl Memory {
    fn new(min: u32, max_pages: u32) -> Result<Self, SandboxError> {
        if min > max_pages {
            return Err(mem_limit(format!("initial memory of {min} pages exceeds the cap of {max_pages} pages")));
        }
        let mut pages = Vec::new();
        pages.try_reserve_exact(min as usize).map_err(|_| mem_limit("host refused the memory page table"))?;
        pages.resize(min as usize, None);
        Ok(Memory { pages, max_pages })
    }

    fn size_pages(&self) -> u32 {
        self.pages.len() as u32
    }

    fn size_bytes(&self) -> u64 {
        self.pages.len() as u64 * PAGE
    }

    /// `memory.grow`; any failure is `sandbox.memory_limit` (`grow_failure = "trap"`).
    fn grow(&mut self, delta: u32) -> Result<u32, SandboxError> {
        let old = self.size_pages();
        let new = u64::from(old) + u64::from(delta);
        if new > u64::from(self.max_pages) {
            return Err(mem_limit(format!("memory.grow to {new} pages exceeds the cap of {} pages", self.max_pages)));
        }
        self.pages.try_reserve(delta as usize).map_err(|_| mem_limit("host refused the memory page table"))?;
        self.pages.resize(new as usize, None);
        Ok(old)
    }

    fn check(&self, addr: u64, len: u64) -> Result<(), SandboxError> {
        match addr.checked_add(len) {
            Some(end) if end <= self.size_bytes() => Ok(()),
            _ => Err(oob()),
        }
    }

    fn page_mut(&mut self, p: usize) -> Result<&mut Vec<u8>, SandboxError> {
        let slot = self.pages.get_mut(p).ok_or_else(oob)?;
        if slot.is_none() {
            let mut v = Vec::new();
            v.try_reserve_exact(PAGE_USIZE).map_err(|_| mem_limit("host refused a memory page"))?;
            v.resize(PAGE_USIZE, 0);
            *slot = Some(v);
        }
        slot.as_mut().ok_or_else(|| fault("page allocation"))
    }

    /// Reads `out.len()` bytes at `addr` (bounds already checked by the caller or here).
    pub(crate) fn read(&self, addr: u64, out: &mut [u8]) -> Result<(), SandboxError> {
        self.check(addr, out.len() as u64)?;
        let mut a = addr as usize;
        let mut done = 0;
        while done < out.len() {
            let (p, off) = (a / PAGE_USIZE, a % PAGE_USIZE);
            let n = (PAGE_USIZE - off).min(out.len() - done);
            let dst = out.get_mut(done..done + n).ok_or_else(|| fault("read"))?;
            match self.pages.get(p) {
                Some(Some(page)) => dst.copy_from_slice(page.get(off..off + n).ok_or_else(|| fault("read"))?),
                Some(None) => dst.fill(0),
                None => return Err(oob()),
            }
            done += n;
            a += n;
        }
        Ok(())
    }

    pub(crate) fn write(&mut self, addr: u64, data: &[u8]) -> Result<(), SandboxError> {
        self.check(addr, data.len() as u64)?;
        let mut a = addr as usize;
        let mut done = 0;
        while done < data.len() {
            let (p, off) = (a / PAGE_USIZE, a % PAGE_USIZE);
            let n = (PAGE_USIZE - off).min(data.len() - done);
            let src = data.get(done..done + n).ok_or_else(|| fault("write"))?;
            let page = self.page_mut(p)?;
            page.get_mut(off..off + n).ok_or_else(|| fault("write"))?.copy_from_slice(src);
            done += n;
            a += n;
        }
        Ok(())
    }

    fn fill(&mut self, addr: u64, val: u8, len: u64) -> Result<(), SandboxError> {
        self.check(addr, len)?;
        let mut a = addr as usize;
        let mut left = len as usize;
        while left > 0 {
            let (p, off) = (a / PAGE_USIZE, a % PAGE_USIZE);
            let n = (PAGE_USIZE - off).min(left);
            let untouched = matches!(self.pages.get(p), Some(None));
            if !untouched || val != 0 {
                let page = self.page_mut(p)?;
                page.get_mut(off..off + n).ok_or_else(|| fault("fill"))?.fill(val);
            }
            left -= n;
            a += n;
        }
        Ok(())
    }

    /// `memory.copy` with overlapping ranges handled as if through a temporary buffer.
    fn copy(&mut self, dst: u64, src: u64, len: u64) -> Result<(), SandboxError> {
        self.check(src, len)?;
        self.check(dst, len)?;
        let mut buf = vec![0u8; (len as usize).min(PAGE_USIZE)];
        let mut done = 0u64;
        while done < len {
            let n = (len - done).min(PAGE);
            // Copy backwards when the destination is above an overlapping source.
            let at = if dst > src { len - done - n } else { done };
            let chunk = buf.get_mut(..n as usize).ok_or_else(|| fault("copy"))?;
            self.read(src + at, chunk)?;
            self.write(dst + at, chunk)?;
            done += n;
        }
        Ok(())
    }

    fn load<const N: usize>(&self, addr: u64) -> Result<[u8; N], SandboxError> {
        let mut b = [0u8; N];
        let a = addr as usize;
        let off = a % PAGE_USIZE;
        if off + N <= PAGE_USIZE && addr + N as u64 <= self.size_bytes() {
            match self.pages.get(a / PAGE_USIZE) {
                Some(Some(page)) => b.copy_from_slice(page.get(off..off + N).ok_or_else(|| fault("load"))?),
                Some(None) => {}
                None => return Err(oob()),
            }
            return Ok(b);
        }
        self.read(addr, &mut b)?;
        Ok(b)
    }

    pub(crate) fn read_u32(&self, addr: u64) -> Result<u32, SandboxError> {
        Ok(u32::from_le_bytes(self.load::<4>(addr)?))
    }
}

// ------------------------------------------------------------------ host state

/// Deterministic WASI host state of one call: the call bytes on stdin, captured stdout.
pub(crate) struct Host<'a> {
    pub stdin: &'a [u8],
    pub cursor: usize,
    pub stdout: Vec<u8>,
    pub stderr_bytes: u64,
    pub max_output: u64,
}

/// How a run ended without an error.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum End {
    /// The entry function returned these values.
    Returned(Vec<u64>),
    /// The guest called `proc_exit`.
    Exit(u32),
}

/// Resource limits of one run.
pub(crate) struct RunLimits {
    pub max_depth: u32,
    pub max_slots: u64,
    pub deadline: Option<Instant>,
}

struct Frame {
    /// Defined-function index (not counting imports).
    func: u32,
    /// Where the caller resumes.
    ret_pc: usize,
    /// Stack index of the first parameter.
    base: usize,
}

/// One instantiation of a module: fresh memory, globals and table.
pub(crate) struct Instance<'m> {
    m: &'m Module,
    pub memory: Memory,
    globals: Vec<u64>,
    table: Vec<Option<u32>>,
    stack: Vec<u64>,
    fuel: u64,
    /// Poll the watchdog when `fuel` drops to this value.
    next_check: u64,
}

impl<'m> Instance<'m> {
    /// Instantiates `m` with linear memory capped at `cap_pages` pages: allocates memory,
    /// globals and table, and applies element and data segments (bounds-checked first, as
    /// the core specification requires). The start function is not run here. `fuel` is the
    /// budget of every invocation on this instance together.
    pub(crate) fn new(m: &'m Module, cap_pages: u32, fuel: u64) -> Result<Self, SandboxError> {
        let memory = match m.memory {
            Some(l) => {
                let max = l.max.unwrap_or(MAX_PAGES).min(cap_pages);
                Memory::new(l.min, max)?
            }
            None => Memory::new(0, 0)?,
        };
        let table_len = m.table.map_or(0, |t| t.min) as usize;
        let mut inst = Instance {
            m,
            memory,
            globals: m.globals.iter().map(|g| g.init).collect(),
            table: vec![None; table_len],
            stack: Vec::new(),
            fuel,
            next_check: fuel.saturating_sub(WATCHDOG_INTERVAL),
        };
        for ElemSeg { offset, funcs } in &m.elems {
            let start = *offset as usize;
            let slots = start
                .checked_add(funcs.len())
                .and_then(|end| inst.table.get_mut(start..end))
                .ok_or_else(|| trap(TrapKind::TableOutOfBounds, "element segment does not fit the table"))?;
            for (slot, f) in slots.iter_mut().zip(funcs) {
                *slot = Some(*f);
            }
        }
        for DataSeg { offset, bytes } in &m.datas {
            inst.memory.write(u64::from(*offset), bytes)?;
        }
        Ok(inst)
    }

    /// Runs function `func` (which must take no parameters) to completion.
    pub(crate) fn invoke(&mut self, func: u32, host: &mut Host<'_>, lim: &RunLimits) -> Result<End, SandboxError> {
        self.stack.clear();
        let n_imports = self.m.imports.len() as u32;
        let def = func.checked_sub(n_imports).ok_or_else(|| fault("entry is an import"))?;
        let mut frames: Vec<Frame> = Vec::new();
        self.enter(&mut frames, def, 0, lim)?;
        let mut pc = 0usize;
        let mut cur = self.m.funcs.get(def as usize).ok_or_else(|| fault("entry"))?;
        loop {
            if self.fuel == 0 {
                return Err(SandboxError::new(ErrorCode::BudgetExhausted, "fuel budget of the call exhausted"));
            }
            self.fuel -= 1;
            if self.fuel <= self.next_check {
                self.next_check = self.fuel.saturating_sub(WATCHDOG_INTERVAL);
                if let Some(d) = lim.deadline {
                    if Instant::now() >= d {
                        return Err(SandboxError::new(ErrorCode::BudgetExhausted, "wall-clock budget exhausted"));
                    }
                }
            }
            let op = *cur.code.get(pc).ok_or_else(|| fault("pc out of range"))?;
            pc += 1;
            match op {
                Op::Unreachable => return Err(trap(TrapKind::Unreachable, "unreachable executed")),
                Op::Br(t) => pc = self.branch(t)?,
                Op::BrIf(t) => {
                    if self.pop()? as u32 != 0 {
                        pc = self.branch(t)?;
                    }
                }
                Op::BrUnless(to) => {
                    if self.pop()? as u32 == 0 {
                        pc = to as usize;
                    }
                }
                Op::Jump(to) => pc = to as usize,
                Op::BrTable { start, len } => {
                    let i = self.pop()? as u32;
                    let k = if i < len { i } else { len.saturating_sub(1) };
                    let idx = start as usize + k as usize;
                    let t = *cur.table.get(idx).ok_or_else(|| fault("br_table"))?;
                    pc = self.branch(t)?;
                }
                Op::Return => {
                    let f = frames.pop().ok_or_else(|| fault("frame"))?;
                    let ty = self.m.func_type(f.func + n_imports).ok_or_else(|| fault("type"))?;
                    let n = ty.results.len();
                    let len = self.stack.len();
                    let src = len.checked_sub(n).filter(|s| *s >= f.base).ok_or_else(|| fault("return"))?;
                    self.stack.copy_within(src..len, f.base);
                    self.stack.truncate(f.base + n);
                    match frames.last() {
                        None => return Ok(End::Returned(std::mem::take(&mut self.stack))),
                        Some(caller) => {
                            cur = self.m.funcs.get(caller.func as usize).ok_or_else(|| fault("frame"))?;
                            pc = f.ret_pc;
                        }
                    }
                }
                Op::Call(f) | Op::CallIndirect(f) => {
                    let callee = if let Op::CallIndirect(ty) = op {
                        let i = self.pop()? as u32;
                        let slot = self
                            .table
                            .get(i as usize)
                            .ok_or_else(|| trap(TrapKind::TableOutOfBounds, "call_indirect index outside the table"))?;
                        let callee = slot.ok_or_else(|| trap(TrapKind::IndirectCallToNull, "null table entry"))?;
                        let want = self.m.canon_types.get(ty as usize);
                        let got = self.m.func_type_idx(callee).and_then(|t| self.m.canon_types.get(t as usize));
                        if want.is_none() || want != got {
                            return Err(trap(TrapKind::BadSignature, "call_indirect signature mismatch"));
                        }
                        callee
                    } else {
                        f
                    };
                    if callee < n_imports {
                        if let Some(code) = self.host_call(callee, host)? {
                            return Ok(End::Exit(code));
                        }
                    } else {
                        let def = callee - n_imports;
                        let nparams = self.m.func_type(callee).map_or(0, |t| t.params.len());
                        let base = self.stack.len().checked_sub(nparams).ok_or_else(|| fault("call"))?;
                        self.enter(&mut frames, def, base, lim)?;
                        if let Some(fr) = frames.last_mut() {
                            fr.ret_pc = pc;
                        }
                        cur = self.m.funcs.get(def as usize).ok_or_else(|| fault("callee"))?;
                        pc = 0;
                    }
                }
                Op::Drop => {
                    self.pop()?;
                }
                Op::Select => {
                    let c = self.pop()? as u32;
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.stack.push(if c != 0 { a } else { b });
                }
                Op::LocalGet(i) => {
                    let base = frames.last().map_or(0, |f| f.base);
                    let v = *self.stack.get(base + i as usize).ok_or_else(|| fault("local"))?;
                    self.stack.push(v);
                }
                Op::LocalSet(i) => {
                    let v = self.pop()?;
                    self.set_local(&frames, i, v)?;
                }
                Op::LocalTee(i) => {
                    let v = *self.stack.last().ok_or_else(|| fault("stack"))?;
                    self.set_local(&frames, i, v)?;
                }
                Op::GlobalGet(i) => {
                    let v = *self.globals.get(i as usize).ok_or_else(|| fault("global"))?;
                    self.stack.push(v);
                }
                Op::GlobalSet(i) => {
                    let v = self.pop()?;
                    *self.globals.get_mut(i as usize).ok_or_else(|| fault("global"))? = v;
                }
                Op::Load { op, offset } => {
                    let a = (self.pop()? as u32) as u64 + u64::from(offset);
                    let v = self.load(op, a)?;
                    self.stack.push(v);
                }
                Op::Store { op, offset } => {
                    let v = self.pop()?;
                    let a = (self.pop()? as u32) as u64 + u64::from(offset);
                    self.store(op, a, v)?;
                }
                Op::MemorySize => self.stack.push(u64::from(self.memory.size_pages())),
                Op::MemoryGrow => {
                    let d = self.pop()? as u32;
                    let old = self.memory.grow(d)?;
                    self.stack.push(u64::from(old));
                }
                Op::Const(c) => self.stack.push(c),
                Op::Num(code) => num(&mut self.stack, code)?,
                Op::TruncSat(code) => trunc_sat(&mut self.stack, code)?,
                Op::MemoryCopy => {
                    let n = self.pop()? as u32;
                    let s = self.pop()? as u32;
                    let d = self.pop()? as u32;
                    self.charge(u64::from(n) / BYTES_PER_FUEL)?;
                    self.memory.copy(u64::from(d), u64::from(s), u64::from(n))?;
                }
                Op::MemoryFill => {
                    let n = self.pop()? as u32;
                    let val = self.pop()? as u8;
                    let d = self.pop()? as u32;
                    self.charge(u64::from(n) / BYTES_PER_FUEL)?;
                    self.memory.fill(u64::from(d), val, u64::from(n))?;
                }
            }
        }
    }

    /// Pushes a frame for defined function `def` whose parameters start at `base`.
    fn enter(&mut self, frames: &mut Vec<Frame>, def: u32, base: usize, lim: &RunLimits) -> Result<(), SandboxError> {
        let f = self.m.funcs.get(def as usize).ok_or_else(|| fault("callee"))?;
        let need = base as u64
            + self.m.func_type(def + self.m.imports.len() as u32).map_or(0, |t| t.params.len() as u64)
            + u64::from(f.locals)
            + u64::from(f.max_height);
        if frames.len() >= lim.max_depth as usize || need > lim.max_slots {
            return Err(trap(TrapKind::CallStackExhausted, "call stack exhausted"));
        }
        self.stack.extend(std::iter::repeat_n(0u64, f.locals as usize));
        frames.push(Frame { func: def, ret_pc: 0, base });
        Ok(())
    }

    fn set_local(&mut self, frames: &[Frame], i: u32, v: u64) -> Result<(), SandboxError> {
        let base = frames.last().map_or(0, |f| f.base);
        *self.stack.get_mut(base + i as usize).ok_or_else(|| fault("local"))? = v;
        Ok(())
    }

    fn pop(&mut self) -> Result<u64, SandboxError> {
        self.stack.pop().ok_or_else(|| fault("operand stack underflow"))
    }

    fn branch(&mut self, t: Target) -> Result<usize, SandboxError> {
        if t.drop > 0 {
            let len = self.stack.len();
            let keep = t.keep as usize;
            let src = len.checked_sub(keep).ok_or_else(|| fault("branch"))?;
            let dst = src.checked_sub(t.drop as usize).ok_or_else(|| fault("branch"))?;
            self.stack.copy_within(src..len, dst);
            self.stack.truncate(dst + keep);
        }
        Ok(t.pc as usize)
    }

    fn load(&self, op: u8, a: u64) -> Result<u64, SandboxError> {
        let mem = &self.memory;
        Ok(match op {
            0x28 | 0x2a => u64::from(u32::from_le_bytes(mem.load::<4>(a)?)),
            0x29 | 0x2b => u64::from_le_bytes(mem.load::<8>(a)?),
            0x2c => u64::from(i8::from_le_bytes(mem.load::<1>(a)?) as i32 as u32),
            0x2d => u64::from(u8::from_le_bytes(mem.load::<1>(a)?)),
            0x2e => u64::from(i16::from_le_bytes(mem.load::<2>(a)?) as i32 as u32),
            0x2f => u64::from(u16::from_le_bytes(mem.load::<2>(a)?)),
            0x30 => i8::from_le_bytes(mem.load::<1>(a)?) as i64 as u64,
            0x31 => u64::from(u8::from_le_bytes(mem.load::<1>(a)?)),
            0x32 => i16::from_le_bytes(mem.load::<2>(a)?) as i64 as u64,
            0x33 => u64::from(u16::from_le_bytes(mem.load::<2>(a)?)),
            0x34 => i32::from_le_bytes(mem.load::<4>(a)?) as i64 as u64,
            0x35 => u64::from(u32::from_le_bytes(mem.load::<4>(a)?)),
            _ => return Err(fault("load opcode")),
        })
    }

    fn store(&mut self, op: u8, a: u64, v: u64) -> Result<(), SandboxError> {
        let bytes = v.to_le_bytes();
        let n = match op {
            0x36 | 0x38 | 0x3e => 4,
            0x37 | 0x39 => 8,
            0x3a | 0x3c => 1,
            0x3b | 0x3d => 2,
            _ => return Err(fault("store opcode")),
        };
        self.memory.write(a, bytes.get(..n).ok_or_else(|| fault("store"))?)
    }

    fn charge(&mut self, units: u64) -> Result<(), SandboxError> {
        if self.fuel < units {
            self.fuel = 0;
            return Err(SandboxError::new(ErrorCode::BudgetExhausted, "fuel budget of the call exhausted"));
        }
        self.fuel -= units;
        Ok(())
    }

    fn arg(&mut self) -> Result<u32, SandboxError> {
        Ok(self.pop()? as u32)
    }

    /// Calls host function `idx`. Returns `Some(code)` when the guest exits.
    fn host_call(&mut self, idx: u32, host: &mut Host<'_>) -> Result<Option<u32>, SandboxError> {
        let (h, _) = *self.m.imports.get(idx as usize).ok_or_else(|| fault("import"))?;
        match h {
            HostFn::Trap => Err(trap(TrapKind::HostTrap, "call to a sealed clock import")),
            HostFn::ProcExit => Ok(Some(self.arg()?)),
            HostFn::EnvironSizesGet => {
                let size_ptr = self.arg()?;
                let count_ptr = self.arg()?;
                let ok = self.memory.write(u64::from(count_ptr), &[0; 4]).is_ok()
                    && self.memory.write(u64::from(size_ptr), &[0; 4]).is_ok();
                self.stack.push(u64::from(if ok { ERRNO_SUCCESS } else { ERRNO_FAULT }));
                Ok(None)
            }
            HostFn::EnvironGet => {
                self.arg()?;
                self.arg()?;
                self.stack.push(u64::from(ERRNO_SUCCESS));
                Ok(None)
            }
            HostFn::FdRead | HostFn::FdWrite => {
                let out_ptr = self.arg()?;
                let iovs_len = self.arg()?;
                let iovs = self.arg()?;
                let fd = self.arg()?;
                let errno = if h == HostFn::FdRead {
                    self.fd_read(fd, iovs, iovs_len, out_ptr, host)?
                } else {
                    self.fd_write(fd, iovs, iovs_len, out_ptr, host)?
                };
                self.stack.push(u64::from(errno));
                Ok(None)
            }
        }
    }

    fn iovec(&self, iovs: u32, i: u32) -> Option<(u64, u64)> {
        let at = u64::from(iovs) + 8 * u64::from(i);
        let ptr = self.memory.read_u32(at).ok()?;
        let len = self.memory.read_u32(at + 4).ok()?;
        Some((u64::from(ptr), u64::from(len)))
    }

    /// `fd_read`: only fd 0, served from the call bytes; any other fd is `EBADF`.
    fn fd_read(&mut self, fd: u32, iovs: u32, n: u32, out: u32, host: &mut Host<'_>) -> Result<u32, SandboxError> {
        if fd != 0 {
            return Ok(ERRNO_BADF);
        }
        let mut total: u64 = 0;
        for i in 0..n {
            self.charge(1)?;
            let Some((ptr, len)) = self.iovec(iovs, i) else { return Ok(ERRNO_FAULT) };
            self.charge(len / BYTES_PER_FUEL)?;
            let rest = host.stdin.get(host.cursor..).unwrap_or_default();
            let k = (len as usize).min(rest.len());
            let chunk = rest.get(..k).unwrap_or_default();
            if self.memory.write(ptr, chunk).is_err() {
                return Ok(ERRNO_FAULT);
            }
            host.cursor += k;
            total += k as u64;
            if (k as u64) < len {
                break;
            }
        }
        if self.memory.write(u64::from(out), &(total as u32).to_le_bytes()).is_err() {
            return Ok(ERRNO_FAULT);
        }
        Ok(ERRNO_SUCCESS)
    }

    /// `fd_write`: fd 1 is captured, fd 2 is counted and discarded, any other fd is `EBADF`.
    fn fd_write(&mut self, fd: u32, iovs: u32, n: u32, out: u32, host: &mut Host<'_>) -> Result<u32, SandboxError> {
        if fd != 1 && fd != 2 {
            return Ok(ERRNO_BADF);
        }
        let mut total: u64 = 0;
        for i in 0..n {
            self.charge(1)?;
            let Some((ptr, len)) = self.iovec(iovs, i) else { return Ok(ERRNO_FAULT) };
            if self.memory.check(ptr, len).is_err() {
                return Ok(ERRNO_FAULT);
            }
            self.charge(len / BYTES_PER_FUEL)?;
            if fd == 1 {
                if host.stdout.len() as u64 + len > host.max_output {
                    return Err(mem_limit("kernel output exceeds max_output_bytes"));
                }
                let start = host.stdout.len();
                host.stdout.try_reserve(len as usize).map_err(|_| mem_limit("host refused output buffer"))?;
                host.stdout.resize(start + len as usize, 0);
                let dst = host.stdout.get_mut(start..).ok_or_else(|| fault("stdout"))?;
                self.memory.read(ptr, dst)?;
            } else {
                host.stderr_bytes = host.stderr_bytes.saturating_add(len);
            }
            total += len;
        }
        if self.memory.write(u64::from(out), &(total as u32).to_le_bytes()).is_err() {
            return Ok(ERRNO_FAULT);
        }
        Ok(ERRNO_SUCCESS)
    }
}

// ------------------------------------------------------------------ numeric operators

fn f32v(x: u64) -> f32 {
    f32::from_bits(x as u32)
}

fn f64v(x: u64) -> f64 {
    f64::from_bits(x)
}

/// Canonicalising result of an `f32` arithmetic operator.
fn c32(x: f32) -> u64 {
    if x.is_nan() {
        CANON_F32
    } else {
        u64::from(x.to_bits())
    }
}

/// Canonicalising result of an `f64` arithmetic operator.
fn c64(x: f64) -> u64 {
    if x.is_nan() {
        CANON_F64
    } else {
        x.to_bits()
    }
}

fn b(x: bool) -> u64 {
    u64::from(x)
}

fn fmin32(a: f32, b: f32) -> u64 {
    if a.is_nan() || b.is_nan() {
        CANON_F32
    } else if a == 0.0 && b == 0.0 {
        u64::from(a.to_bits() | b.to_bits())
    } else {
        c32(if a < b { a } else { b })
    }
}

fn fmax32(a: f32, b: f32) -> u64 {
    if a.is_nan() || b.is_nan() {
        CANON_F32
    } else if a == 0.0 && b == 0.0 {
        u64::from(a.to_bits() & b.to_bits())
    } else {
        c32(if a > b { a } else { b })
    }
}

fn fmin64(a: f64, b: f64) -> u64 {
    if a.is_nan() || b.is_nan() {
        CANON_F64
    } else if a == 0.0 && b == 0.0 {
        a.to_bits() | b.to_bits()
    } else {
        c64(if a < b { a } else { b })
    }
}

fn fmax64(a: f64, b: f64) -> u64 {
    if a.is_nan() || b.is_nan() {
        CANON_F64
    } else if a == 0.0 && b == 0.0 {
        a.to_bits() & b.to_bits()
    } else {
        c64(if a > b { a } else { b })
    }
}

fn div0() -> SandboxError {
    trap(TrapKind::IntegerDivideByZero, "integer divide by zero")
}

fn overflow() -> SandboxError {
    trap(TrapKind::IntegerOverflow, "integer overflow")
}

/// Trapping float-to-integer truncation: NaN is `bad_conversion_to_integer`, a value whose
/// truncation lies outside `[lo, hi)` (infinities included) is `integer_overflow`.
fn trunc_check(x: f64, nan: bool, lo: f64, hi: f64) -> Result<f64, SandboxError> {
    if nan {
        return Err(trap(TrapKind::BadConversionToInteger, "invalid conversion to integer"));
    }
    let t = x.trunc();
    if (lo..hi).contains(&t) {
        Ok(t)
    } else {
        Err(overflow())
    }
}

const TWO31: f64 = 2_147_483_648.0;
const TWO32: f64 = 4_294_967_296.0;
const TWO63: f64 = 9_223_372_036_854_775_808.0;
const TWO64: f64 = 18_446_744_073_709_551_616.0;

/// Executes numeric opcode `op` (`0x45..=0xC4`) on the operand stack.
fn num(s: &mut Vec<u64>, op: u8) -> Result<(), SandboxError> {
    let binary = matches!(op, 0x46..=0x4f | 0x51..=0x66 | 0x6a..=0x78 | 0x7c..=0x8a | 0x92..=0x98 | 0xa0..=0xa6);
    let y = s.pop().ok_or_else(|| fault("operand stack underflow"))?;
    let x = if binary { s.pop().ok_or_else(|| fault("operand stack underflow"))? } else { y };
    let (a32, b32) = (x as u32, y as u32);
    let (sa32, sb32) = (a32 as i32, b32 as i32);
    let (sa64, sb64) = (x as i64, y as i64);
    let r: u64 = match op {
        // i32 comparisons
        0x45 => b(a32 == 0),
        0x46 => b(a32 == b32),
        0x47 => b(a32 != b32),
        0x48 => b(sa32 < sb32),
        0x49 => b(a32 < b32),
        0x4a => b(sa32 > sb32),
        0x4b => b(a32 > b32),
        0x4c => b(sa32 <= sb32),
        0x4d => b(a32 <= b32),
        0x4e => b(sa32 >= sb32),
        0x4f => b(a32 >= b32),
        // i64 comparisons
        0x50 => b(x == 0),
        0x51 => b(x == y),
        0x52 => b(x != y),
        0x53 => b(sa64 < sb64),
        0x54 => b(x < y),
        0x55 => b(sa64 > sb64),
        0x56 => b(x > y),
        0x57 => b(sa64 <= sb64),
        0x58 => b(x <= y),
        0x59 => b(sa64 >= sb64),
        0x5a => b(x >= y),
        // f32 comparisons
        0x5b => b(f32v(x) == f32v(y)),
        0x5c => b(f32v(x) != f32v(y)),
        0x5d => b(f32v(x) < f32v(y)),
        0x5e => b(f32v(x) > f32v(y)),
        0x5f => b(f32v(x) <= f32v(y)),
        0x60 => b(f32v(x) >= f32v(y)),
        // f64 comparisons
        0x61 => b(f64v(x) == f64v(y)),
        0x62 => b(f64v(x) != f64v(y)),
        0x63 => b(f64v(x) < f64v(y)),
        0x64 => b(f64v(x) > f64v(y)),
        0x65 => b(f64v(x) <= f64v(y)),
        0x66 => b(f64v(x) >= f64v(y)),
        // i32 arithmetic
        0x67 => u64::from(a32.leading_zeros()),
        0x68 => u64::from(a32.trailing_zeros()),
        0x69 => u64::from(a32.count_ones()),
        0x6a => u64::from(a32.wrapping_add(b32)),
        0x6b => u64::from(a32.wrapping_sub(b32)),
        0x6c => u64::from(a32.wrapping_mul(b32)),
        0x6d => {
            if sb32 == 0 {
                return Err(div0());
            }
            u64::from(sa32.checked_div(sb32).ok_or_else(overflow)? as u32)
        }
        0x6e => u64::from(a32.checked_div(b32).ok_or_else(div0)?),
        0x6f => {
            if sb32 == 0 {
                return Err(div0());
            }
            u64::from(sa32.wrapping_rem(sb32) as u32)
        }
        0x70 => u64::from(a32.checked_rem(b32).ok_or_else(div0)?),
        0x71 => u64::from(a32 & b32),
        0x72 => u64::from(a32 | b32),
        0x73 => u64::from(a32 ^ b32),
        0x74 => u64::from(a32.wrapping_shl(b32)),
        0x75 => u64::from(sa32.wrapping_shr(b32) as u32),
        0x76 => u64::from(a32.wrapping_shr(b32)),
        0x77 => u64::from(a32.rotate_left(b32 % 32)),
        0x78 => u64::from(a32.rotate_right(b32 % 32)),
        // i64 arithmetic
        0x79 => u64::from(x.leading_zeros()),
        0x7a => u64::from(x.trailing_zeros()),
        0x7b => u64::from(x.count_ones()),
        0x7c => x.wrapping_add(y),
        0x7d => x.wrapping_sub(y),
        0x7e => x.wrapping_mul(y),
        0x7f => {
            if sb64 == 0 {
                return Err(div0());
            }
            sa64.checked_div(sb64).ok_or_else(overflow)? as u64
        }
        0x80 => x.checked_div(y).ok_or_else(div0)?,
        0x81 => {
            if sb64 == 0 {
                return Err(div0());
            }
            sa64.wrapping_rem(sb64) as u64
        }
        0x82 => x.checked_rem(y).ok_or_else(div0)?,
        0x83 => x & y,
        0x84 => x | y,
        0x85 => x ^ y,
        0x86 => x.wrapping_shl(y as u32),
        0x87 => sa64.wrapping_shr(y as u32) as u64,
        0x88 => x.wrapping_shr(y as u32),
        0x89 => x.rotate_left((y % 64) as u32),
        0x8a => x.rotate_right((y % 64) as u32),
        // f32 arithmetic
        0x8b => u64::from(a32 & 0x7fff_ffff),
        0x8c => u64::from(a32 ^ 0x8000_0000),
        0x8d => c32(f32v(x).ceil()),
        0x8e => c32(f32v(x).floor()),
        0x8f => c32(f32v(x).trunc()),
        0x90 => c32(f32v(x).round_ties_even()),
        0x91 => c32(f32v(x).sqrt()),
        0x92 => c32(f32v(x) + f32v(y)),
        0x93 => c32(f32v(x) - f32v(y)),
        0x94 => c32(f32v(x) * f32v(y)),
        0x95 => c32(f32v(x) / f32v(y)),
        0x96 => fmin32(f32v(x), f32v(y)),
        0x97 => fmax32(f32v(x), f32v(y)),
        0x98 => u64::from((a32 & 0x7fff_ffff) | (b32 & 0x8000_0000)),
        // f64 arithmetic
        0x99 => x & 0x7fff_ffff_ffff_ffff,
        0x9a => x ^ 0x8000_0000_0000_0000,
        0x9b => c64(f64v(x).ceil()),
        0x9c => c64(f64v(x).floor()),
        0x9d => c64(f64v(x).trunc()),
        0x9e => c64(f64v(x).round_ties_even()),
        0x9f => c64(f64v(x).sqrt()),
        0xa0 => c64(f64v(x) + f64v(y)),
        0xa1 => c64(f64v(x) - f64v(y)),
        0xa2 => c64(f64v(x) * f64v(y)),
        0xa3 => c64(f64v(x) / f64v(y)),
        0xa4 => fmin64(f64v(x), f64v(y)),
        0xa5 => fmax64(f64v(x), f64v(y)),
        0xa6 => (x & 0x7fff_ffff_ffff_ffff) | (y & 0x8000_0000_0000_0000),
        // conversions
        0xa7 => u64::from(x as u32),
        0xa8 => {
            let v = f32v(x);
            u64::from(trunc_check(f64::from(v), v.is_nan(), -TWO31, TWO31)? as i32 as u32)
        }
        0xa9 => {
            let v = f32v(x);
            u64::from(trunc_check(f64::from(v), v.is_nan(), 0.0, TWO32)? as u32)
        }
        0xaa => {
            let v = f64v(x);
            u64::from(trunc_check(v, v.is_nan(), -TWO31, TWO31)? as i32 as u32)
        }
        0xab => {
            let v = f64v(x);
            u64::from(trunc_check(v, v.is_nan(), 0.0, TWO32)? as u32)
        }
        0xac => sa32 as i64 as u64,
        0xad => u64::from(a32),
        0xae => {
            let v = f32v(x);
            trunc_check(f64::from(v), v.is_nan(), -TWO63, TWO63)? as i64 as u64
        }
        0xaf => {
            let v = f32v(x);
            trunc_check(f64::from(v), v.is_nan(), 0.0, TWO64)? as u64
        }
        0xb0 => {
            let v = f64v(x);
            trunc_check(v, v.is_nan(), -TWO63, TWO63)? as i64 as u64
        }
        0xb1 => {
            let v = f64v(x);
            trunc_check(v, v.is_nan(), 0.0, TWO64)? as u64
        }
        0xb2 => u64::from((sa32 as f32).to_bits()),
        0xb3 => u64::from((a32 as f32).to_bits()),
        0xb4 => u64::from((sa64 as f32).to_bits()),
        0xb5 => u64::from((x as f32).to_bits()),
        0xb6 => c32(f64v(x) as f32),
        0xb7 => f64::from(sa32).to_bits(),
        0xb8 => f64::from(a32).to_bits(),
        0xb9 => (sa64 as f64).to_bits(),
        0xba => (x as f64).to_bits(),
        0xbb => c64(f64::from(f32v(x))),
        // reinterpretations keep the bits
        0xbc | 0xbe => u64::from(a32),
        0xbd | 0xbf => x,
        // sign extension
        0xc0 => u64::from(a32 as i8 as i32 as u32),
        0xc1 => u64::from(a32 as i16 as i32 as u32),
        0xc2 => x as i8 as i64 as u64,
        0xc3 => x as i16 as i64 as u64,
        0xc4 => x as i32 as i64 as u64,
        _ => return Err(fault("numeric opcode")),
    };
    s.push(r);
    Ok(())
}

/// Saturating truncation `0xFC code`: Rust's `as` casts have the WebAssembly semantics
/// (NaN to 0, out-of-range values saturate).
fn trunc_sat(s: &mut Vec<u64>, code: u8) -> Result<(), SandboxError> {
    let x = s.pop().ok_or_else(|| fault("operand stack underflow"))?;
    let r = match code {
        0 => u64::from(f32v(x) as i32 as u32),
        1 => u64::from(f32v(x) as u32),
        2 => u64::from(f64v(x) as i32 as u32),
        3 => u64::from(f64v(x) as u32),
        4 => f32v(x) as i64 as u64,
        5 => f32v(x) as u64,
        6 => f64v(x) as i64 as u64,
        7 => f64v(x) as u64,
        _ => return Err(fault("saturating truncation opcode")),
    };
    s.push(r);
    Ok(())
}
