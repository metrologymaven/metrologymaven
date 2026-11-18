//! Per-invocation budget counters (REQ-IO.REG.LIMITS-001, -002).

use crate::error::{IoError, Position};
use crate::limits::{LimitKind, LimitRequest, Limits, Principal};
use std::fmt;
use std::ops::{Deref, DerefMut};
use std::time::Instant;

/// The wall clock is read once every this many charges (of any kind).
pub const WALL_CHECK_EVERY: u64 = 4096;

/// A monotonic millisecond clock. Injected so tests can drive wall-time expiry.
pub trait Clock: Send {
    /// Milliseconds since an arbitrary fixed origin; never decreases.
    fn now_ms(&mut self) -> u64;
}

/// The default [`Clock`], backed by [`std::time::Instant`].
#[derive(Debug, Clone, Copy)]
pub struct MonotonicClock {
    origin: Instant,
}

impl MonotonicClock {
    /// A clock whose origin is now.
    pub fn new() -> Self {
        MonotonicClock { origin: Instant::now() }
    }
}

impl Default for MonotonicClock {
    fn default() -> Self {
        MonotonicClock::new()
    }
}

impl Clock for MonotonicClock {
    fn now_ms(&mut self) -> u64 {
        u64::try_from(self.origin.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}

/// The counters of one reader invocation. Every `charge_*` either succeeds and adds to
/// its counter, or returns `limit_exceeded` and leaves the counter unchanged. Allocation
/// is tracked by explicit [`Budget::charge_alloc`] calls before a reader grows a buffer;
/// there is no global allocator hook.
pub struct Budget {
    limits: Limits,
    bytes: u64,
    elements: u64,
    depth: u32,
    max_depth: u32,
    alloc: u64,
    charges: u64,
    clock: Box<dyn Clock>,
    start_ms: u64,
}

impl fmt::Debug for Budget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Budget")
            .field("limits", &self.limits)
            .field("bytes", &self.bytes)
            .field("elements", &self.elements)
            .field("depth", &self.depth)
            .field("alloc", &self.alloc)
            .field("charges", &self.charges)
            .finish_non_exhaustive()
    }
}

impl Budget {
    /// A budget for `principal` with `request` applied to the defaults (see
    /// [`Limits::resolve`]), timed by a [`MonotonicClock`].
    pub fn new(principal: Principal, request: &LimitRequest) -> Result<Budget, IoError> {
        Budget::with_clock(principal, request, Box::new(MonotonicClock::new()))
    }

    /// As [`Budget::new`] with an explicit clock.
    pub fn with_clock(principal: Principal, request: &LimitRequest, clock: Box<dyn Clock>) -> Result<Budget, IoError> {
        let limits = Limits::resolve(principal, request)?;
        let mut clock = clock;
        let start_ms = clock.now_ms();
        Ok(Budget { limits, bytes: 0, elements: 0, depth: 0, max_depth: 0, alloc: 0, charges: 0, clock, start_ms })
    }

    /// The effective limits.
    pub fn limits(&self) -> &Limits {
        &self.limits
    }
    /// Bytes charged so far (the byte offset reached by a [`crate::BoundedSource`]).
    pub fn bytes_used(&self) -> u64 {
        self.bytes
    }
    /// Elements charged so far.
    pub fn elements_used(&self) -> u64 {
        self.elements
    }
    /// Current nesting depth.
    pub fn depth(&self) -> u32 {
        self.depth
    }
    /// Deepest nesting reached so far.
    pub fn max_depth(&self) -> u32 {
        self.max_depth
    }
    /// Tracked allocation charged so far, in bytes.
    pub fn alloc_used(&self) -> u64 {
        self.alloc
    }
    /// Number of charges so far (of any kind).
    pub fn charges(&self) -> u64 {
        self.charges
    }

    fn exceeded(&self, limit: LimitKind, at: Position) -> IoError {
        IoError::LimitExceeded { limit, value: self.limits.value(limit), at }
    }

    /// Counts one charge and reads the clock every [`WALL_CHECK_EVERY`] charges.
    fn tick(&mut self, at: Position) -> Result<(), IoError> {
        self.charges = self.charges.saturating_add(1);
        if self.charges % WALL_CHECK_EVERY == 0 {
            self.check_wall(at)?;
        }
        Ok(())
    }

    /// Reads the clock now and fails with `limit_exceeded{wall}` (determinism `N`) if
    /// the wall-time limit has passed.
    pub fn check_wall(&mut self, at: Position) -> Result<(), IoError> {
        let elapsed = self.clock.now_ms().saturating_sub(self.start_ms);
        if elapsed > self.limits.wall_ms() {
            return Err(self.exceeded(LimitKind::Wall, at));
        }
        Ok(())
    }

    /// Charges `n` input bytes. On failure `at` is the byte offset reached before this
    /// charge.
    pub fn charge_bytes(&mut self, n: u64) -> Result<(), IoError> {
        let at = Position::byte(self.bytes);
        let next = self.bytes.saturating_add(n);
        if next > self.limits.bytes() {
            return Err(self.exceeded(LimitKind::Bytes, at));
        }
        self.bytes = next;
        self.tick(Position::byte(next))
    }

    /// Charges `n` elements produced at input position `at`.
    pub fn charge_elements(&mut self, n: u64, at: Position) -> Result<(), IoError> {
        let next = self.elements.saturating_add(n);
        if next > self.limits.elements() {
            return Err(self.exceeded(LimitKind::Elements, at));
        }
        self.elements = next;
        self.tick(at)
    }

    /// Charges `n` bytes of buffer growth at input position `at`. Call it before the
    /// growth happens.
    pub fn charge_alloc(&mut self, n: u64, at: Position) -> Result<(), IoError> {
        let next = self.alloc.saturating_add(n);
        if next > self.limits.alloc() {
            return Err(self.exceeded(LimitKind::Alloc, at));
        }
        self.alloc = next;
        self.tick(at)
    }

    /// Enters one nesting level opened at input position `at`. The returned guard is the
    /// scope token: it dereferences to this budget, so a recursive reader passes it down,
    /// and leaving the scope (drop or [`DepthGuard::leave`]) restores the depth.
    pub fn enter(&mut self, at: Position) -> Result<DepthGuard<'_>, IoError> {
        if self.depth >= self.limits.depth() {
            return Err(self.exceeded(LimitKind::Depth, at));
        }
        self.tick(at)?;
        self.depth = self.depth.saturating_add(1);
        if self.depth > self.max_depth {
            self.max_depth = self.depth;
        }
        Ok(DepthGuard { budget: self })
    }
}

/// Scope token of one nesting level; see [`Budget::enter`].
#[derive(Debug)]
pub struct DepthGuard<'a> {
    budget: &'a mut Budget,
}

impl DepthGuard<'_> {
    /// Leaves the nesting level now (same as dropping the guard).
    pub fn leave(self) {}
}

impl Deref for DepthGuard<'_> {
    type Target = Budget;
    fn deref(&self) -> &Budget {
        self.budget
    }
}

impl DerefMut for DepthGuard<'_> {
    fn deref_mut(&mut self) -> &mut Budget {
        self.budget
    }
}

impl Drop for DepthGuard<'_> {
    fn drop(&mut self) {
        self.budget.depth = self.budget.depth.saturating_sub(1);
    }
}
