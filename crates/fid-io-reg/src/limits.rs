//! Limits, their defaults and the raise rule (REQ-IO.REG.LIMITS-001, -003).

use crate::error::IoError;
use std::fmt;

/// Default input byte limit: 64 GiB [unverified: to be tuned on the corpus].
pub const DEFAULT_BYTES: u64 = 64 * 1024 * 1024 * 1024;
/// Default element limit: 2^32 [unverified: to be tuned on the corpus].
pub const DEFAULT_ELEMENTS: u64 = 1 << 32;
/// Default nesting depth limit: 64 [unverified: to be tuned on the corpus].
pub const DEFAULT_DEPTH: u32 = 64;
/// Default tracked allocation limit: 8 GiB [unverified: to be tuned on the corpus].
pub const DEFAULT_ALLOC: u64 = 8 * 1024 * 1024 * 1024;
/// Default wall-time limit: 600 s, in milliseconds [unverified: to be tuned on the corpus].
pub const DEFAULT_WALL_MS: u64 = 600_000;

/// Which limit a [`IoError::LimitExceeded`] or [`IoError::LimitRaiseForbidden`] names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LimitKind {
    /// Input bytes read from the source.
    Bytes,
    /// Elements (records, entities, tokens) a reader produced.
    Elements,
    /// Nesting depth.
    Depth,
    /// Total tracked allocation, in bytes.
    Alloc,
    /// Wall time, in milliseconds.
    Wall,
}

impl LimitKind {
    /// Every kind, in declaration order.
    pub const ALL: &'static [LimitKind] =
        &[LimitKind::Bytes, LimitKind::Elements, LimitKind::Depth, LimitKind::Alloc, LimitKind::Wall];

    /// The stable wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            LimitKind::Bytes => "bytes",
            LimitKind::Elements => "elements",
            LimitKind::Depth => "depth",
            LimitKind::Alloc => "alloc",
            LimitKind::Wall => "wall",
        }
    }

    /// The unit of the limit's value.
    pub fn unit(self) -> &'static str {
        match self {
            LimitKind::Bytes | LimitKind::Alloc => "bytes",
            LimitKind::Elements => "elements",
            LimitKind::Depth => "levels",
            LimitKind::Wall => "ms",
        }
    }
}

impl fmt::Display for LimitKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Who asked for the read. Only a human may raise a limit above its default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Principal {
    /// A person at an interactive surface.
    Human,
    /// An AI agent (MCP or similar).
    Agent,
    /// A script or automation.
    Script,
}

impl Principal {
    /// The stable wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            Principal::Human => "human",
            Principal::Agent => "agent",
            Principal::Script => "script",
        }
    }
}

/// Per-call overrides. `None` keeps the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LimitRequest {
    /// Input byte limit.
    pub bytes: Option<u64>,
    /// Element limit.
    pub elements: Option<u64>,
    /// Nesting depth limit.
    pub depth: Option<u32>,
    /// Tracked allocation limit, in bytes.
    pub alloc: Option<u64>,
    /// Wall-time limit, in milliseconds.
    pub wall_ms: Option<u64>,
}

impl LimitRequest {
    /// Sets the byte limit.
    pub fn bytes(mut self, v: u64) -> Self {
        self.bytes = Some(v);
        self
    }
    /// Sets the element limit.
    pub fn elements(mut self, v: u64) -> Self {
        self.elements = Some(v);
        self
    }
    /// Sets the depth limit.
    pub fn depth(mut self, v: u32) -> Self {
        self.depth = Some(v);
        self
    }
    /// Sets the tracked allocation limit.
    pub fn alloc(mut self, v: u64) -> Self {
        self.alloc = Some(v);
        self
    }
    /// Sets the wall-time limit in milliseconds.
    pub fn wall_ms(mut self, v: u64) -> Self {
        self.wall_ms = Some(v);
        self
    }

    /// The requested value for `kind`, if any.
    pub fn get(&self, kind: LimitKind) -> Option<u64> {
        match kind {
            LimitKind::Bytes => self.bytes,
            LimitKind::Elements => self.elements,
            LimitKind::Depth => self.depth.map(u64::from),
            LimitKind::Alloc => self.alloc,
            LimitKind::Wall => self.wall_ms,
        }
    }
}

/// Effective limits of one invocation. Only built through [`Limits::resolve`] (or
/// [`Limits::DEFAULT`]), so the raise rule cannot be bypassed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    bytes: u64,
    elements: u64,
    depth: u32,
    alloc: u64,
    wall_ms: u64,
}

impl Limits {
    /// The documented defaults.
    pub const DEFAULT: Limits = Limits {
        bytes: DEFAULT_BYTES,
        elements: DEFAULT_ELEMENTS,
        depth: DEFAULT_DEPTH,
        alloc: DEFAULT_ALLOC,
        wall_ms: DEFAULT_WALL_MS,
    };

    /// Applies `request` to the defaults. A value above its default fails with
    /// `limit_raise_forbidden` unless `principal` is [`Principal::Human`]; lowering is
    /// always allowed.
    pub fn resolve(principal: Principal, request: &LimitRequest) -> Result<Limits, IoError> {
        let mut out = Limits::DEFAULT;
        for &kind in LimitKind::ALL {
            let requested = match request.get(kind) {
                Some(v) => v,
                None => continue,
            };
            let default = Limits::DEFAULT.value(kind);
            if requested > default && principal != Principal::Human {
                return Err(IoError::LimitRaiseForbidden { limit: kind, requested, default });
            }
            match kind {
                LimitKind::Bytes => out.bytes = requested,
                LimitKind::Elements => out.elements = requested,
                LimitKind::Depth => out.depth = u32::try_from(requested).unwrap_or(u32::MAX),
                LimitKind::Alloc => out.alloc = requested,
                LimitKind::Wall => out.wall_ms = requested,
            }
        }
        Ok(out)
    }

    /// The configured value of `kind`, in its [`LimitKind::unit`].
    pub fn value(&self, kind: LimitKind) -> u64 {
        match kind {
            LimitKind::Bytes => self.bytes,
            LimitKind::Elements => self.elements,
            LimitKind::Depth => u64::from(self.depth),
            LimitKind::Alloc => self.alloc,
            LimitKind::Wall => self.wall_ms,
        }
    }

    /// Input byte limit.
    pub fn bytes(&self) -> u64 {
        self.bytes
    }
    /// Element limit.
    pub fn elements(&self) -> u64 {
        self.elements
    }
    /// Nesting depth limit.
    pub fn depth(&self) -> u32 {
        self.depth
    }
    /// Tracked allocation limit, in bytes.
    pub fn alloc(&self) -> u64 {
        self.alloc
    }
    /// Wall-time limit, in milliseconds.
    pub fn wall_ms(&self) -> u64 {
        self.wall_ms
    }
}

impl Default for Limits {
    fn default() -> Self {
        Limits::DEFAULT
    }
}
